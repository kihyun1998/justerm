/**
 * Screen-reader accessibility for the frame-mode widget (#119). Pure logic —
 * no DOM, no GPU, no IPC: the consumer injects DOM sinks ({@link A11yTreeSink},
 * {@link LiveRegionSink}) and the controller decides *what* the assistive
 * technology should see. beamterm's canvas is opaque to AT, so this drives a
 * hidden DOM mirror beside it.
 *
 * Two structures, both reading the *viewport* (xterm.js `AccessibilityManager`):
 * a navigable row tree for spatial review, and an aria-live region announcing
 * new output. The viewport row text is assembled by the caller (from
 * `CellMirror`) and handed in as plain strings — keeping this module free of
 * the renderer/wasm.
 */

/** Max new lines announced at once before falling back to a notice — a screen
 * reader can't usefully keep up with more (xterm `MAX_ROWS_TO_READ`). */
const MAX_ROWS_TO_READ = 20;

/** Shown instead of a flood past {@link MAX_ROWS_TO_READ}: the user reviews the
 * row tree manually (xterm `tooMuchOutput`). */
export const TOO_MUCH_OUTPUT = "Too much output to announce, navigate to rows manually to read";

/** Debounce (ms) before flushing accumulated output to the live region, so
 * streaming output coalesces into one announcement (xterm `TimeBasedDebouncer`). */
const ANNOUNCE_DEBOUNCE_MS = 200;

/** The viewport frame header fields the controller reads. A `DecodedFrame`
 * satisfies it structurally. */
export interface A11yFrame {
  readonly rows: number;
  /** `0` = Full (whole-viewport repaint — clear/resize/alt-switch), else
   * Partial/incremental. A Full frame reseeds the announce baseline but isn't
   * announced (it's not new output). Absent → treated as incremental. */
  readonly kind?: number;
  /** Lines scrolled up from the bottom (0 = following the latest output). */
  readonly displayOffset?: number;
  /** History lines above the viewport. */
  readonly scrollbackLen?: number;
  /** Scroll op applied this frame (mirrors `DecodedFrame`): rows
   * `[scrollTop, scrollBottom]` shifted by `scrollCount` (positive = up). The
   * diff shifts the previous rows to match, so moved content isn't "new". */
  readonly hasScroll?: boolean;
  readonly scrollTop?: number;
  readonly scrollBottom?: number;
  readonly scrollCount?: number;
  /** Whether the alternate screen is active (#149 wire bit). When set, announce
   * is suppressed (a full-screen TUI repaint isn't "new output"); absent →
   * treated as primary (graceful degrade until the bit lands). */
  readonly altScreen?: boolean;
}

/**
 * The hidden row-tree sink: one DOM `listitem` per viewport row. A thin DOM
 * wrapper satisfies it; tests pass a recorder.
 */
export interface A11yTreeSink {
  /** Grow/shrink the tree to `rows` items (viewport resize). */
  resize(rows: number): void;
  /** Set row `i`'s text and 1-based `aria-posinset`/`aria-setsize`. */
  setRow(i: number, text: string, posInSet: number, setSize: number): void;
  /** Move AT focus to row `i` (boundary-scroll re-focus). */
  focusRow(i: number): void;
}

/** The aria-live region sink: announces new output to the screen reader. */
export interface LiveRegionSink {
  /** Append `text` to the live region (the SR reads the delta). */
  announce(text: string): void;
  /** Empty the live region and reset its line budget. */
  clear(): void;
}

/**
 * Drives the screen-reader mirror against injected DOM sinks. Fed a frame plus
 * its viewport row text each cadence tick; mutates the row tree and (later)
 * announces new output.
 */
export class AccessibilityController {
  private readonly tree: A11yTreeSink;
  private readonly live: LiveRegionSink;
  private readonly onScroll: (lines: number) => void;
  private readonly setTimer: (fn: () => void, ms: number) => number;
  private readonly clearTimer: (handle: number) => void;
  /** New output accumulated across frames, flushed on the debounce timer. */
  private pending: string[] = [];
  private timer: number | undefined;
  /** Current tree height, to resize only when the viewport changes. */
  private treeRows = 0;
  /** Last frame's geometry, so boundary scroll knows the buffer edges. */
  private top = 0;
  private setSize = 0;
  /** Previous frame's viewport row text, for the new-output diff. `null` until
   * the first frame seeds the baseline (so the initial paint isn't announced). */
  private prevRows: string[] | null = null;
  /** Typed chars awaiting their echo: a printed char equal to the queue head is
   * the shell echoing a keystroke the AT already announced, so it's dropped
   * (xterm `_charsToConsume`). Frame-level, so typing while output streams can
   * race — the same trade-off xterm accepts. */
  private readonly consume: string[] = [];

  constructor(opts: {
    tree: A11yTreeSink;
    live: LiveRegionSink;
    onScroll?: (lines: number) => void;
    setTimer?: (fn: () => void, ms: number) => number;
    clearTimer?: (handle: number) => void;
  }) {
    this.tree = opts.tree;
    this.live = opts.live;
    this.onScroll = opts.onScroll ?? (() => {});
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms) as unknown as number);
    this.clearTimer = opts.clearTimer ?? ((h) => clearTimeout(h));
  }

  /** A new frame arrived. Mirror its viewport rows into the review tree. */
  onFrame(frame: A11yFrame, rows: string[]): void {
    if (frame.rows !== this.treeRows) {
      this.tree.resize(frame.rows);
      this.treeRows = frame.rows;
    }
    const scrollbackLen = frame.scrollbackLen ?? 0;
    const displayOffset = frame.displayOffset ?? 0;
    // Absolute index of the viewport's top row (xterm `buffer.ydisp`): right
    // after all scrollback when following, less the scroll-up offset.
    this.top = scrollbackLen - displayOffset;
    this.setSize = scrollbackLen + frame.rows;
    for (let i = 0; i < frame.rows; i++) {
      this.tree.setRow(i, rows[i] ?? "", this.top + i + 1, this.setSize);
    }
    this.announceNewOutput(frame, rows);
    this.prevRows = rows;
  }

  /**
   * Accumulate the output new since the previous frame for a debounced announce.
   * The signal is a *row-text diff* (not render damage — damage conflates cursor
   * moves and repaints with output): a row whose text changed is newly printed.
   * Skipped on the first frame (no baseline) so the initial paint stays silent.
   */
  private announceNewOutput(frame: A11yFrame, rows: string[]): void {
    if (this.prevRows === null) return;
    // The alternate screen (vim/htop) repaints wholesale — announcing it is
    // noise. Suppress, but the row tree (updated above) still serves review.
    if (frame.altScreen) return;
    // A Full frame (kind 0) is a repaint (clear/resize/alt-switch), not output —
    // reseed the baseline (done by the caller after this) without announcing.
    if (frame.kind === 0) return;
    // Shift the previous rows by this frame's scroll op so moved content lines
    // up with where it landed — only genuinely new text then differs.
    const prev = this.shiftPrev(frame);
    const parts: string[] = [];
    for (let i = 0; i < frame.rows; i++) {
      const cur = rows[i] ?? "";
      const p = prev[i] ?? "";
      if (cur === p) continue;
      // Announce only the changed *suffix* (a one-char prompt edit reads as that
      // char, not the whole re-read line) — and it's what dedup matches against.
      parts.push(cur.slice(commonPrefixLen(p, cur)));
    }
    const fresh = this.dedupTyped(parts.join("\n"));
    if (fresh.length === 0) return;
    this.pending.push(fresh);
    // Coalesce streaming frames: (re)arm the timer; the flush announces the lot.
    if (this.timer !== undefined) this.clearTimer(this.timer);
    this.timer = this.setTimer(() => this.flush(), ANNOUNCE_DEBOUNCE_MS);
  }

  /** Announce the accumulated output (debounce expiry). A flood the screen
   * reader can't follow collapses to a manual-review notice. */
  private flush(): void {
    this.timer = undefined;
    if (this.pending.length === 0) return;
    const text = this.pending.join("\n");
    this.pending = [];
    this.live.announce(text.split("\n").length > MAX_ROWS_TO_READ ? TOO_MUCH_OUTPUT : text);
  }

  /** Strip echoed keystrokes from `fresh`. Drains the consume queue at the output
   * rate — one queued char per output char (xterm `_charsToConsume`), announcing
   * mismatches — so a never-echoed key (e.g. `read -s`) can't linger and swallow
   * a later genuine line. A leading match (the common echo) is dropped. */
  private dedupTyped(fresh: string): string {
    if (this.consume.length === 0) return fresh;
    let result = "";
    for (const ch of fresh) {
      if (this.consume.length === 0) {
        result += ch;
        continue;
      }
      const queued = this.consume.shift();
      if (queued !== ch) result += ch; // mismatch → announce; match → drop the echo
    }
    return result;
  }

  /** A key was typed. The keystroke takes precedence over any in-flight output
   * read — cancel the pending announce and wipe the live region so stale output
   * isn't read over the user (xterm `_handleKey` → `_clearLiveRegion`). Then
   * queue the char so the shell's echo of it isn't re-announced (control chars
   * aren't echoed as text, so skip them — xterm's `\p{Control}` guard). */
  onKey(char: string): void {
    this.cancelPending();
    this.live.clear();
    if (!/\p{Control}/u.test(char)) this.consume.push(char);
  }

  /** The widget lost focus. Drop any pending announcement and clear the live
   * region so nothing stale is left for the next focus (xterm `onBlur`). */
  onBlur(): void {
    this.cancelPending();
    this.live.clear();
  }

  /** Tear down: cancel any pending debounce so it can't flush into a detached
   * live region after the widget is gone (the sibling controllers' dispose
   * pattern). */
  dispose(): void {
    this.cancelPending();
  }

  /** Cancel the debounce timer and drop the accumulated announcement. */
  private cancelPending(): void {
    if (this.timer !== undefined) {
      this.clearTimer(this.timer);
      this.timer = undefined;
    }
    this.pending = [];
  }

  /** The previous viewport rows with this frame's scroll op applied, so the
   * diff sees shifted content as unchanged and newly-exposed rows as blank. */
  private shiftPrev(frame: A11yFrame): string[] {
    const prev = this.prevRows ?? [];
    if (!frame.hasScroll || !frame.scrollCount) return prev;
    const top = frame.scrollTop ?? 0;
    const bottom = frame.scrollBottom ?? frame.rows - 1;
    const count = frame.scrollCount;
    const shifted = prev.slice();
    for (let i = top; i <= bottom; i++) {
      const src = i + count; // upward scroll pulls the row below into slot i
      shifted[i] = src >= top && src <= bottom ? (prev[src] ?? "") : "";
    }
    return shifted;
  }

  /**
   * AT focus reached a viewport boundary row. Scroll one line toward it so the
   * user can keep walking past the edge, then re-focus the inner neighbour
   * (xterm `_handleBoundaryFocus`).
   *
   * `cameFromInner` is xterm's `relatedTarget` guard: scroll only when focus
   * arrived from the inner neighbour (the user walking *outward*) — a click,
   * Tab-in, or programmatic focus onto the edge row must not scroll. No-op too
   * when the buffer edge is already exposed, or the viewport is too short to
   * have a distinct inner row.
   */
  onBoundaryFocus(position: "top" | "bottom", cameFromInner: boolean): void {
    if (!cameFromInner || this.treeRows < 3) return;
    if (position === "top") {
      if (this.top === 0) return; // viewport top is the first buffer line
      this.onScroll(-1);
      this.tree.focusRow(1);
    } else {
      if (this.top + this.treeRows >= this.setSize) return; // already at the bottom
      this.onScroll(1);
      this.tree.focusRow(this.treeRows - 2);
    }
  }
}

/** Length of the shared leading run of two strings (where they first differ). */
function commonPrefixLen(a: string, b: string): number {
  const n = Math.min(a.length, b.length);
  let i = 0;
  while (i < n && a[i] === b[i]) i++;
  return i;
}
