/**
 * The `OSC 52` consumer half (#841) — an application inside the terminal asking to
 * write, or read, the user's clipboard.
 *
 * **The engine's half ends at the boundary (#828).** Core recognises the sequence,
 * decodes the base64, relays a store as an event and a query as its own, and
 * encodes a reply when — and only when — the consumer calls `report_clipboard`.
 * It holds no clipboard, carries no allow/deny knob, and never touches a platform
 * API. Everything on this side of that line is policy, which ADR-0017 assigns
 * here.
 *
 * **And this widget declines the policy too, one layer further out.** It holds no
 * clipboard and no permission model: it routes the request to a
 * {@link ClipboardProvider} the *embedder* supplies, and with no provider it does
 * nothing at all. That is the same-layer reference's answer — xterm.js's
 * `addon-clipboard` takes an `IClipboardProvider` in its constructor and has no
 * policy enum anywhere (`addons/addon-clipboard/src/ClipboardAddon.ts:12` @
 * `699f553`) — and it is what the port pattern in this package already does for
 * selection, search and markers.
 *
 * **The two directions are refused independently, and that asymmetry is 3-for-3
 * across the references**: alacritty ships `Osc52::OnlyCopy` as its `#[default]`,
 * commented as *"a compromise between entirely disabling it (the most secure) and
 * allowing `paste` (the less secure)"* (`alacritty_terminal/src/term/mod.rs:372`
 * @ `852e971`); ghostty ships `clipboard-write: allow` beside `clipboard-read:
 * ask` (`src/config/Config.zig:2379`, `:2380` @ `e6e26e1`); xterm.js's browser
 * provider does both but only once an embedder has registered the addon. Every
 * one of them **refuses by silence** — alacritty `debug!("Denied osc52 load");
 * return`, ghostty `log.info(...); return` — so a refused read sends no reply
 * rather than an empty one. A {@link ClipboardProvider} with no `readText` is how
 * that is spelled here.
 */
import type { ClipboardQueryEvent, ClipboardStoreEvent, ClipboardTarget, Terminator, TermEvent } from "./events";

/**
 * The embedder's clipboard. **Both halves are optional and are read
 * independently** — omitting `readText` refuses reads while writes keep working,
 * which is the split the security question actually has (a remote application
 * reading back what the user copied earlier is the sharper of the two risks).
 *
 * Either half may be synchronous or return a promise. **A rejection is a refusal,
 * not a crash**: the controller swallows it, where xterm.js's addon does not
 * (`ClipboardAddon.ts:44`, a bare `.then` — an unhandled rejection in the
 * embedder's page).
 *
 * **But rejection is not the shape a browser read actually takes, measured
 * 2026-09-10** in Chromium over `localhost` (a secure context) with
 * `navigator.userActivation.isActive === true`:
 *
 * | call | outcome |
 * |---|---|
 * | `navigator.clipboard.readText()` | **still pending after 2000 ms** — neither resolved nor rejected |
 * | `navigator.clipboard.writeText()` | resolved |
 * | `permissions.query({name:"clipboard-read"})` | `"prompt"` |
 * | `permissions.query({name:"clipboard-write"})` | `"granted"` |
 *
 * So a read hangs on the permission prompt rather than failing, and the refusal
 * reaches the application as silence either way — the same answer by a different
 * route than the one this was designed against. Two consequences that are not
 * obvious: {@link ClipboardController.handle}'s promise then never settles (it is
 * floated, never awaited, in `Terminal`), and a provider that wants a *bounded*
 * read owes its own timeout. **The widget deliberately does not impose one** — a
 * deadline is policy, and inventing one here is what this whole module declines
 * to do.
 *
 * Not measured: what a *denied* prompt does, and whether any browser other than
 * Chromium rejects where this one hangs. Both are gaps, not absences.
 */
export interface ClipboardProvider {
  /** Put `text` on `target`. An EMPTY `text` is the sequence's clear idiom, not a
   * no-op to filter — the controller passes it through unchanged. */
  writeText?(target: ClipboardTarget, text: string): void | Promise<void>;
  /** What is on `target`, or `null` to refuse this one call. Absent refuses every
   * read. */
  readText?(target: ClipboardTarget): string | null | Promise<string | null>;
}

/**
 * The answer channel back to the engine — core `Engine::report_clipboard`.
 *
 * The first *request → answer* seam in this package: the six existing ports
 * (selection, search, marker, resize, accessible, command-nav) are one-way
 * commands, and `FrameSource.subscribeEvents` is one-way notification. Scoped to
 * the clipboard on purpose (#841, decided with the maintainer): core has five
 * `Query…` events and this wires one, because the other four are colour queries
 * whose "who owns the palette" question this slice has not measured.
 */
export interface ClipboardPort {
  /** Answer the query that carried `target` and `terminator`, handing both back
   * verbatim. The engine base64-encodes `text` and queues the reply; the consumer
   * drains it and writes it to the pty. */
  report(target: ClipboardTarget, text: string, terminator: Terminator): void;
}

/** One recorded {@link ClipboardPort} answer. */
export interface ClipboardReport {
  target: ClipboardTarget;
  text: string;
  terminator: Terminator;
}

/** A recording {@link ClipboardPort} for tests/demos (mirrors `StubSelectionPort`).
 * `reports` being EMPTY is the assertion that matters most here — it is what a
 * refused read looks like on the wire. */
export class StubClipboardPort implements ClipboardPort {
  readonly reports: ClipboardReport[] = [];
  report(target: ClipboardTarget, text: string, terminator: Terminator): void {
    this.reports.push({ target, text, terminator });
  }
}

/** What {@link ClipboardController} is wired with. Both absent is the default
 * posture: the widget does nothing in either direction. */
export interface ClipboardOptions {
  /** Where a query's answer goes. Without it the controller **never reads the
   * clipboard at all** — see {@link ClipboardController.handle}. */
  port?: ClipboardPort;
  /** The embedder's clipboard. Absent = both directions refused. */
  provider?: ClipboardProvider;
}

/**
 * Routes the `OSC 52` events off the shared event stream to the embedder's
 * clipboard, and answers a query on the port.
 *
 * Wired by `Terminal` on the same `subscribeEvents` subscription the notification
 * handlers use, because core produces one event stream and a backend has one
 * channel to push it down.
 */
export class ClipboardController {
  constructor(private readonly options: ClipboardOptions = {}) {}

  /**
   * Handle one event off the stream. Events that are not the clipboard pair are
   * ignored, so this can be fed the whole subscription.
   *
   * **Never rejects, and may never settle.** It resolves once the provider has
   * settled — but a browser `readText()` was measured *pending indefinitely* on
   * the permission prompt (see {@link ClipboardProvider}), so `Terminal` floats it
   * rather than awaiting. Tests await it because their providers settle.
   */
  async handle(event: TermEvent): Promise<void> {
    switch (event.type) {
      case "clipboardStore":
        return this.store(event);
      case "clipboardQuery":
        return this.query(event);
      default:
        return;
    }
  }

  private async store(event: ClipboardStoreEvent): Promise<void> {
    const write = this.options.provider?.writeText;
    if (!write) return; // no writeText = writes refused, and reads are untouched by that
    try {
      await write.call(this.options.provider, event.target, event.text);
    } catch {
      // A refusal, not a crash — see ClipboardProvider. Nothing is reported back
      // to the application either way: a store has no reply in the sequence.
    }
  }

  private async query(event: ClipboardQueryEvent): Promise<void> {
    const { port, provider } = this.options;
    const read = provider?.readText;
    // Both guards are refusals, and the port one is deliberately checked FIRST:
    // with nowhere to send an answer, reading the user's clipboard would disclose
    // it to no one and touch a permission-gated API for nothing.
    if (!port || !read) return;
    let text: string | null;
    try {
      text = await read.call(provider, event.target);
    } catch {
      return; // the browser denied a stream-driven read; answer nothing
    }
    if (text === null) return; // a per-call refusal
    port.report(event.target, text, event.terminator);
  }
}
