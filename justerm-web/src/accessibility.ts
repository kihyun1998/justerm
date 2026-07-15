/**
 * Screen-reader accessibility for the frame-mode widget (#119). Pure logic —
 * no DOM, no GPU, no IPC: the consumer injects DOM sinks ({@link A11yTreeSink},
 * {@link LiveRegionSink}) and the controller decides *what* the assistive
 * technology should see. the renderer's canvas is opaque to AT, so this drives a
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
/** Max time output may accumulate before a forced flush, so an unbroken sub-debounce
 * stream (`yes`, a long build) still announces periodically instead of re-arming the
 * debounce forever and staying silent until it stops (xterm `TimeBasedDebouncer`'s 1s
 * throttle). #153.
 *
 * NB — deliberate divergence from xterm: this KEEPS the 200ms coalescing debounce and
 * only *caps* the flood, rather than xterm's pure 1s throttle. So at moderate cadence
 * (200ms–1s inter-output gaps) justerm may announce more often than xterm's strict
 * "at most once per second" ceiling — an intentional responsiveness trade. The cap is
 * the flood worst-case bound. A leading edge (#215) fires the first output after an
 * idle gap of ≥ this cap immediately (matching xterm's synchronous leading refresh),
 * so a fresh command's first line — or a flood's — isn't held for the debounce/cap. */
const ANNOUNCE_MAX_WAIT_MS = 1000;

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
  /** Monotonic clock (ms) for the {@link ANNOUNCE_MAX_WAIT_MS} cap; injected for tests. */
  private readonly now: () => number;
  /** Whether a screen reader is active (#161/#169). While inactive, the per-frame
   * row-tree `setRow` churn is skipped (nobody reads the tree) — but bookkeeping
   * stays current, so {@link syncTree} on reactivation refreshes from the cached
   * frame with no cold rebuild (justerm's edge over xterm disposing the manager). */
  private readonly isActive: () => boolean;
  /** New output accumulated across frames, flushed on the debounce timer. */
  private pending: string[] = [];
  private timer: number | undefined;
  /** `now()` when the current pending batch started (first push after empty), for the
   * {@link ANNOUNCE_MAX_WAIT_MS} cap. Undefined ⇔ pending empty. */
  private firstPendingAt: number | undefined;
  /** `now()` of the last announce that actually emitted, for the {@link
   * ANNOUNCE_MAX_WAIT_MS} leading-edge idle test (#215). Undefined ⇔ nothing has been
   * announced yet (idle since forever), so the first output leads. */
  private lastFlushAt: number | undefined;
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
    /** Monotonic clock (ms) for the max-wait cap; defaults to `performance.now`. */
    now?: () => number;
    /** SR-active probe (#169). Defaults to always-active (pre-#169 behaviour). */
    isActive?: () => boolean;
  }) {
    this.tree = opts.tree;
    this.live = opts.live;
    this.onScroll = opts.onScroll ?? (() => {});
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms) as unknown as number);
    this.clearTimer = opts.clearTimer ?? ((h) => clearTimeout(h));
    this.now = opts.now ?? (() => performance.now());
    this.isActive = opts.isActive ?? (() => true);
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
    // Skip the per-frame tree DOM churn while inactive — nobody reads it (#169).
    // Bookkeeping above/below still runs, so `syncTree` refreshes instantly on
    // reactivation. The announce path is gated separately (#161 sink wrap).
    if (this.isActive()) {
      for (let i = 0; i < frame.rows; i++) {
        this.tree.setRow(i, rows[i] ?? "", this.top + i + 1, this.setSize);
      }
    }
    this.announceNewOutput(frame, rows);
    this.prevRows = rows;
  }

  /**
   * Re-render the row tree from the cached last frame (#169). Call on
   * reactivation (`setScreenReaderActive(true)`): the tree DOM and bookkeeping
   * were kept while inactive, so this refreshes immediately with no cold rebuild
   * and without waiting for the next frame (which may never come if idle). A
   * no-op before the first frame (no cached rows).
   */
  syncTree(): void {
    if (this.prevRows === null) return;
    for (let i = 0; i < this.treeRows; i++) {
      this.tree.setRow(i, this.prevRows[i] ?? "", this.top + i + 1, this.setSize);
    }
  }

  /**
   * Reactivate after the screen reader was inactive (#169). Two things at once:
   * drop any pending announce that accumulated across the inactive span so it is
   * NOT replayed (a screen reader starting reviews prior output via the freshly
   * synced tree, not a surprise burst — matches xterm disposing its manager), then
   * re-render the tree from the cached frame. Call from `setScreenReaderActive` on
   * a false→true transition.
   */
  reactivate(): void {
    this.cancelPending();
    // #215: clear the leading-edge idle clock too, mirroring xterm recreating a FRESH
    // TimeBasedDebouncer (`_lastRefreshMs = 0`) on SR re-activation — so the first line
    // after the screen reader is re-enabled leads immediately, even if the off→on toggle
    // happened < the cap after the last announce. (Same "fresh manager" rationale as the
    // `consume` reset below.)
    this.lastFlushAt = undefined;
    // #183: start echo-dedup fresh, matching xterm's freshly-created manager whose
    // `_charsToConsume` is empty. Keys typed during the inactive span (or un-echoed
    // before it began) must not swallow the first real output as a stale echo —
    // announce work was gated off, so there was no output to dedup them against.
    this.consume.length = 0;
    this.syncTree();
  }

  /**
   * Accumulate the output new since the previous frame for a debounced announce.
   * The signal is a *row-text diff* (not render damage — damage conflates cursor
   * moves and repaints with output): a row whose text changed is newly printed.
   * Skipped on the first frame (no baseline) so the initial paint stays silent.
   */
  private announceNewOutput(frame: A11yFrame, rows: string[]): void {
    // #183: while the screen reader is inactive nobody hears an announce, so skip
    // the WHOLE diff (shiftPrev + per-row compare + commonPrefixLen + dedup) and
    // the debounce arm — not just the gated flush (#161 no-ops the sink, but the
    // CPU is still wasted). `prevRows` is advanced by the caller *after* this, so
    // the #161 no-replay anchor holds and the first active frame diffs correctly.
    // The echo-dedup `consume` queue drains only here, so it is instead handled at
    // its source: `onKey` enqueues only while active + `reactivate` clears it
    // (mirrors xterm disposing/recreating its AccessibilityManager, whose
    // `_charsToConsume` listener is unregistered while off and empty on recreate).
    if (!this.isActive()) return;
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
    // Leading edge (#215): the first output after an idle window is announced
    // immediately, zero-latency, instead of waiting the 200ms debounce (isolated
    // output) or — during a flood — the full 1s cap. `firstPendingAt === undefined`
    // marks the first push into an empty batch; `idleForLeadingEdge()` requires the
    // batch to follow a quiet gap of at least the cap (or the very first output ever,
    // when nothing has flushed yet — the `yes`-right-after-login flood). Matches
    // xterm's TimeBasedDebouncer synchronous leading refresh; subsequent frames in the
    // active window still coalesce via the debounce+cap below.
    if (this.firstPendingAt === undefined && this.idleForLeadingEdge()) {
      this.flush();
      return;
    }
    // Coalesce streaming frames: (re)arm the 200ms quiet-period timer. But cap the
    // total wait at ANNOUNCE_MAX_WAIT_MS from the batch's first push (#153) — else an
    // unbroken sub-200ms stream (`yes`) re-arms forever and the SR stays silent until
    // it stops. When the cap is hit, flush now instead of re-arming (a periodic
    // announce mid-flood, xterm `TimeBasedDebouncer`).
    if (this.firstPendingAt === undefined) this.firstPendingAt = this.now();
    if (this.timer !== undefined) this.clearTimer(this.timer);
    const remaining = ANNOUNCE_MAX_WAIT_MS - (this.now() - this.firstPendingAt);
    if (remaining <= 0) {
      this.flush();
    } else {
      this.timer = this.setTimer(() => this.flush(), Math.min(ANNOUNCE_DEBOUNCE_MS, remaining));
    }
  }

  /** Announce the accumulated output (debounce expiry). A flood the screen
   * reader can't follow collapses to a manual-review notice. */
  private flush(): void {
    this.timer = undefined;
    this.firstPendingAt = undefined;
    if (this.pending.length === 0) return;
    // Record when we last emitted, for the #215 leading-edge idle test. Only on a
    // real announce — an empty flush (a timer that fired after a cancel) mustn't
    // reset the idle clock, so this sits below the empty guard.
    this.lastFlushAt = this.now();
    const text = this.pending.join("\n");
    this.pending = [];
    this.live.announce(text.split("\n").length > MAX_ROWS_TO_READ ? TOO_MUCH_OUTPUT : text);
  }

  /** Whether the current (about-to-start) batch follows a quiet gap of at least the
   * cap — the #215 leading-edge condition. True when nothing has been announced yet
   * (idle since forever) or the last announce was ≥ {@link ANNOUNCE_MAX_WAIT_MS} ago. */
  private idleForLeadingEdge(): boolean {
    return this.lastFlushAt === undefined || this.now() - this.lastFlushAt >= ANNOUNCE_MAX_WAIT_MS;
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
    // #183: enqueue for echo-dedup ONLY while active. While inactive the drain
    // (dedupTyped, inside the gated announceNewOutput) never runs, so an ungated
    // push would grow `consume` unbounded and swallow the first output after
    // reactivation. This mirrors xterm's disposed manager registering no char
    // listener at all. Control chars aren't echoed as text, so skip them.
    // Push per code point (#153 G9): `char` may be multi-unit (IME commit, a pasted
    // run, an emoji) but `dedupTyped` drains one code point per echoed output char, so
    // a single multi-code-point entry would mismatch and wrongly announce. Splitting
    // keeps `consume` code-point-granular. Control chars aren't echoed as text.
    if (this.isActive()) {
      for (const cp of char) {
        if (!/\p{Control}/u.test(cp)) this.consume.push(cp);
      }
    }
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
    this.firstPendingAt = undefined;
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
