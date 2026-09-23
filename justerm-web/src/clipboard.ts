/**
 * The `OSC 52` consumer half — an application inside the terminal asking to
 * write, or read, the user's clipboard.
 *
 * **The engine's half ends at the boundary.** Core recognises the sequence,
 * decodes the base64, relays a store as an event and a query as its own, and
 * encodes a reply when — and only when — the consumer calls `report_clipboard`.
 * It holds no clipboard, carries no allow/deny knob, and never touches a platform
 * API. Everything on this side of that line is policy, which [ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md) assigns
 * here.
 *
 * **And this widget declines the policy too, one layer further out.** It holds no
 * clipboard and no permission model: it routes the request to a
 * {@link ClipboardProvider} the *embedder* supplies, and with no provider it does
 * nothing at all — both reads and writes refused. Read and write are refused
 * independently; focus is not a gate (an embedder can refuse on focus inside its
 * provider); and **a refusal is silence** — no "denied" reply, as at every reference.
 * Why each: `docs/map/territory/events-and-replies.md`.
 */
import type { ClipboardQueryEvent, ClipboardStoreEvent, ClipboardTarget, Terminator, TermEvent } from "./events";

/**
 * The embedder's clipboard. **Both halves are optional and are read
 * independently** — omitting `readText` refuses reads while writes keep working,
 * which is the split the security question actually has (a remote application
 * reading back what the user copied earlier is the sharper of the two risks).
 *
 * Either half may be synchronous or return a promise. **A rejection is a refusal,
 * not a crash**: the controller swallows it.
 *
 * **A browser `navigator.clipboard.readText()` from a stream-driven query may hang
 * rather than reject** (measured in Chromium). The application sees silence until it
 * settles, {@link ClipboardController.handle}'s promise may never settle (hence
 * {@link ClipboardController.dispose}), and **the widget imposes no deadline** — a
 * provider wanting a bounded read owes its own timeout.
 */
export interface ClipboardProvider {
  /** Put `text` on `target`. An EMPTY `text` is the sequence's clear idiom, not a
   * no-op to filter — the controller passes it through unchanged. */
  writeText?(target: ClipboardTarget, text: string): void | Promise<void>;
  /**
   * What is on `target`, or `null` to refuse this one call. Absent refuses every
   * read.
   *
   * **`""` and `null` are different answers, and the difference is the whole of
   * this signature.** An empty string means *the clipboard is empty* and still
   * produces a reply, because the application is blocked waiting for one; `null`
   * means *refused* and produces silence. Collapsing them — the natural
   * `if (!text) return` — would hang an application over an empty clipboard, which
   * is the ordinary case rather than an exotic one, since the obvious embedder
   * implementation is `() => navigator.clipboard.readText()` and that yields `""`.
   */
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
  /** Set by {@link dispose}. Checked at every point an answer could still land, so
   * a read that settles after teardown reports nothing. */
  private disposed = false;
  /** The outstanding write per target, so a query cannot overtake the store it
   * follows — see {@link query}. Keyed by target because the sequence's targets are
   * independent clipboards; a write to `primary` must not delay a read of
   * `clipboard`. */
  private readonly pendingWrites = new Map<ClipboardTarget, Promise<void>>();

  constructor(private readonly options: ClipboardOptions = {}) {}

  /**
   * End the controller: no answer lands after this, whenever the provider settles.
   *
   * **This exists because the widget composes the controller and is its only
   * holder**, and the map's `a layer ends what it exclusively holds` puts the
   * ending on the composer. Unsubscribing from the event channel is not enough: an
   * in-flight read is already past the subscription. The window is wide rather than
   * theoretical — a browser read was measured pending indefinitely on the
   * permission prompt ({@link ClipboardProvider}), so the realistic sequence is a
   * stream-driven query opening a prompt, the host unmounting the terminal, and the
   * user then clicking *Allow*: without this, the clipboard would be reported into a
   * reply channel whose engine the host has already torn down.
   *
   * A promise cannot be cancelled, so this **latches the landing** rather than
   * stopping the flight — the same shape `MarkerIndexCache` uses for its in-flight
   * pull.
   */
  dispose(): void {
    this.disposed = true;
    this.pendingWrites.clear();
  }

  /**
   * Handle one event off the stream. Events that are not the clipboard pair are
   * ignored, so this can be fed the whole subscription.
   *
   * **Never rejects, and may never settle** — a browser `readText()` was measured
   * pending indefinitely on the permission prompt (see {@link ClipboardProvider}),
   * so `Terminal` floats it rather than awaiting, and {@link dispose} is what makes
   * that safe. Tests await it because their providers settle.
   */
  async handle(event: TermEvent): Promise<void> {
    if (this.disposed) return;
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
    // Recorded before it is awaited, so a query arriving later in the SAME batch
    // already sees it.
    const flight = (async () => {
      try {
        await write.call(this.options.provider, event.target, event.text);
      } catch {
        // A refusal, not a crash — see ClipboardProvider. Nothing is reported back
        // to the application either way: a store has no reply in the sequence.
      }
    })();
    this.pendingWrites.set(event.target, flight);
    await flight;
    // Only clear our own flight; a later store to the same target owns the slot now.
    if (this.pendingWrites.get(event.target) === flight) this.pendingWrites.delete(event.target);
  }

  private async query(event: ClipboardQueryEvent): Promise<void> {
    const { port, provider } = this.options;
    // The port is checked before `readText` is even *reached* on the provider: with
    // nowhere to send an answer there is no reason to touch a permission-gated API,
    // or to raise a browser prompt, for a result nobody can receive. (The order of
    // the two guards below carries no behaviour on its own — both are property
    // tests. What carries it is that both precede the call, and that the provider is
    // not dereferenced for `readText` until the port is known to exist.)
    if (!port) return;
    const read = provider?.readText;
    if (!read) return;

    // A store and a query arrive back-to-back in one drained batch, and `Terminal`
    // floats each — so without this, a read issued while the write it follows is
    // still in flight answers with the PRE-store clipboard. Awaited per target, so a
    // hung read on one target cannot stall another.
    const outstanding = this.pendingWrites.get(event.target);
    if (outstanding) await outstanding;
    if (this.disposed) return;

    let text: string | null;
    try {
      text = await read.call(provider, event.target);
    } catch {
      return; // the read failed or was denied; answer nothing
    }
    // `null` refuses this one call; `""` is an EMPTY clipboard and still answers,
    // because the application is blocked waiting for a reply (see ClipboardProvider).
    if (text === null) return;
    if (this.disposed) return; // settled after teardown — see dispose()
    port.report(event.target, text, event.terminator);
  }
}
