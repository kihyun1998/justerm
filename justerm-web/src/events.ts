/**
 * Fire-and-forget consumer notifications (#117) — the web side of core's
 * `drain_events()` (`TermEvent`). These are NOT frame state (they never ride the
 * frame wire); the consumer drains them from the engine and delivers them through
 * a side channel ({@link import("./types").FrameSource.subscribeEvents}). The
 * widget only maps them to consumer callbacks — transport-agnostic.
 *
 * Scope is the notification set (title/bell/cwd); the palette/query `TermEvent`s
 * (OSC 4/10/11, colour-scheme/column queries) are a different concern — the
 * consumer *applies* or *answers* those (#122/#85/#82), not the notification surface.
 */
export type TermEvent =
  | { type: "title"; title: string } // OSC 0/2 — xterm's onTitleChange
  | { type: "bell" } // BEL — xterm's onBell
  | { type: "cwd"; cwd: string }; // OSC 7 — a justerm extension (no xterm parity)

/** Consumer notification callbacks. All optional — an absent handler is a no-op. */
export interface EventHandlers {
  /** The window/icon title changed (OSC 0/2). */
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
  }
}
