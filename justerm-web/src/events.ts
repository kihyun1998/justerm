/**
 * The web side of core's consumer event channel (#117) —
 * `drain_events()` (`TermEvent`). These are NOT frame state (they never ride the
 * frame wire); the consumer drains them from the engine and delivers them through
 * a side channel ({@link import("./types").FrameSource.subscribeEvents}). The
 * widget only maps them to consumer callbacks — transport-agnostic.
 *
 * **Two surfaces, and the split is the point (#841).** {@link TermEvent} is the
 * *channel* — everything core's `drain_events()` produces travels it, because a
 * backend has exactly one stream to push. {@link EventHandlers} is the
 * *notification* surface, and it stays title/bell/cwd: those are things a consumer
 * is merely told about. An `OSC 52` clipboard event is not one — the consumer
 * *acts on* it and, for a query, owes the application a reply — so it rides this
 * union and is handled by {@link import("./clipboard").ClipboardController}
 * instead of by a callback here.
 *
 * The palette/query `TermEvent`s (OSC 4/10/11/12, colour-scheme/column queries)
 * are the same shape and are still unwired (#122/#85/#82) — a consumer *applies*
 * or *answers* those. They are not on this union yet, and the clipboard pair
 * deliberately did not generalise a channel for them (#841, decided with the
 * maintainer): four colour queries carry a palette-ownership question this slice
 * has no measurement for.
 */
export type TermEvent =
  | { type: "title"; title: string } // OSC 0/2, or an XTWINOPS pop — xterm's onTitleChange
  | { type: "bell" } // BEL — xterm's onBell
  | { type: "cwd"; cwd: string } // OSC 7 — a justerm extension (no xterm parity)
  | ClipboardStoreEvent
  | ClipboardQueryEvent;

/** Which clipboard the application named, **relayed and never resolved** — core
 * keeps `p` and `s` apart on purpose, and what `"selection"` (the `s` field, *"the
 * configurable primary/clipboard selection"*) means is a setting the consumer
 * owns. A consumer with no such setting should treat it as `"primary"`, which is
 * what xterm-as-shipped does (`selectToClipboard` defaults to false). A platform
 * with no primary selection may collapse `"primary"` onto `"clipboard"`; the
 * widget does not make either choice. */
export type ClipboardTarget = "clipboard" | "primary" | "selection";

/*
 * **Why the two types above are NOT in `docs/map/territory/published-surface.md`'s
 * hand-copied-roster list — maintainer's call, 2026-09-10, and theirs to reverse.**
 *
 * That note enumerates value spaces this package transcribes from core and
 * republishes. Both of its existing entries reach this package **through the
 * decoder's wire lane**, which is what makes `justerm-wasm-decode` their proper
 * home and their absence from it the defect. `TermEvent` crosses no decoder lane
 * at all: it arrives on a side channel the *embedder* implements, whose only type
 * declaration is this package's. So a string union is the right home here and the
 * class does not apply.
 *
 * The gap that survives that reasoning, recorded because it is a different one:
 * core's `ClipboardTarget` is `#[non_exhaustive]` and names `q` and the cut
 * buffers as members a later slice may add, while the union above is closed and
 * {@link import("./clipboard").ClipboardController} passes the target to the
 * provider unexamined — there is no decline arm. No reachable consequence until
 * core adds a target, and nothing here gates that day.
 */

/** The byte an OSC reply must end with — **the one carried back, not a default**
 * (#836). `drain_events` hands over a *batch*, so two queries can be outstanding
 * at once and answered in either order; a remembered terminator cannot say which
 * exchange it belongs to. Opaque to the widget: it arrives on the query and goes
 * back out on the answer unread. */
export type Terminator = "st" | "bel";

/** An application asked to PUT `text` on the clipboard (`OSC 52`, #828/#841).
 * `text` arrives already decoded — no consumer carries a base64 implementation.
 *
 * **An empty `text` means clear**, and is not a degenerate case to filter out:
 * it is the sequence's clear idiom, and core relays it through the ordinary path
 * for exactly that reason. */
export interface ClipboardStoreEvent {
  type: "clipboardStore";
  target: ClipboardTarget;
  text: string;
}

/** An application asked what is ON the clipboard (`OSC 52` with a `?` payload).
 * The application is waiting for a reply, so this is the one event on this
 * channel with a response obligation — and **declining to answer is how a read is
 * refused**, which is what all four references do: alacritty and ghostty log and
 * return, xterm(C) emits nothing because its whole reply block sits inside the
 * `AllowWindowOps` branch, and xterm.js has no refusal path short of leaving the
 * addon unloaded. **None of them sends a "denied" reply** — the tally is checked
 * at the sources in {@link import("./clipboard").ClipboardProvider}'s module.
 *
 * Named as a pair with {@link ClipboardStoreEvent} rather than mirroring core's
 * `QueryClipboard`: a consumer reads these two together, and `clipboardStore` /
 * `clipboardQuery` sort side by side where `clipboardStore` / `queryClipboard`
 * do not. */
export interface ClipboardQueryEvent {
  type: "clipboardQuery";
  target: ClipboardTarget;
  terminator: Terminator;
}

/** Consumer notification callbacks. All optional — an absent handler is a no-op. */
export interface EventHandlers {
  /**
   * The window title is now `title`.
   *
   * Emitted both when the application sets one (OSC 0/2) and when it *restores*
   * a saved one (XTWINOPS `CSI 23 t`), so read it as "the title is now this"
   * rather than "the application chose this". A restore may hand back the empty
   * string — applications push before they set a title — which means "go back
   * to your default", not "show a blank title".
   */
  onTitle?(title: string): void;
  /** The terminal bell rang (BEL). */
  onBell?(): void;
  /** The working directory was reported (OSC 7), e.g. `file://host/path`. */
  onCwd?(cwd: string): void;
}

/** Route a {@link TermEvent} to the matching {@link EventHandlers} callback. */
export function dispatchTermEvent(event: TermEvent, handlers: EventHandlers): void {
  switch (event.type) {
    case "title":
      handlers.onTitle?.(event.title);
      return;
    case "bell":
      handlers.onBell?.();
      return;
    case "cwd":
      handlers.onCwd?.(event.cwd);
      return;
    // The clipboard pair rides this channel but is not a notification — it goes to
    // `ClipboardController`, wired separately on the same subscription (#841).
    // Listed rather than left to the switch's fallthrough so that adding a third
    // clipboard event has to come past this comment.
    case "clipboardStore":
    case "clipboardQuery":
      return;
  }
}
