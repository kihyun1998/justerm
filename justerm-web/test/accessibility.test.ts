import { describe, expect, it } from "vitest";
import { AccessibilityController, TOO_MUCH_OUTPUT } from "../src/accessibility";

/** A recording {@link A11yTreeSink} — the simplest concrete sink behind the
 * seam (mirrors `StubSelectionPort`/`StubSearchPort`). */
class StubTree {
  rows = 0;
  /** Last text/posinset/setsize written per row index. */
  readonly set = new Map<number, { text: string; posInSet: number; setSize: number }>();
  readonly focused: number[] = [];
  /** Every `resize` argument, to assert it isn't called redundantly. */
  readonly resizes: number[] = [];
  /** How many times `setRow` was called (to assert no churn while inactive, #169). */
  setRowCalls = 0;
  resize(rows: number): void {
    this.rows = rows;
    this.resizes.push(rows);
  }
  setRow(i: number, text: string, posInSet: number, setSize: number): void {
    this.setRowCalls++;
    this.set.set(i, { text, posInSet, setSize });
  }
  focusRow(i: number): void {
    this.focused.push(i);
  }
}

/** A manual timer (mirrors search.test.ts): `setTimer` stashes the latest
 * callback, `flush()` fires it — debounce tests run without real time. */
class ManualScheduler {
  private fn: (() => void) | null = null;
  readonly setTimer = (fn: () => void): number => {
    this.fn = fn;
    return 1;
  };
  readonly clearTimer = (): void => {
    this.fn = null;
  };
  flush(): void {
    const fn = this.fn;
    this.fn = null;
    fn?.();
  }
}

/** A recording {@link LiveRegionSink}. */
class StubLive {
  announced: string[] = [];
  cleared = 0;
  announce(text: string): void {
    this.announced.push(text);
  }
  clear(): void {
    this.cleared++;
  }
}

describe("AccessibilityController — review row tree (W1)", () => {
  // The hidden row tree mirrors the viewport: each row gets its text plus
  // 1-based `aria-posinset`/`aria-setsize` so the screen reader announces
  // "row N of M". At the bottom (displayOffset 0, no scrollback) the first
  // viewport row is position 1 of `rows`.
  it("writes each viewport row's text with 1-based aria position", () => {
    const tree = new StubTree();
    const ctrl = new AccessibilityController({ tree, live: new StubLive() });

    ctrl.onFrame({ rows: 2, displayOffset: 0, scrollbackLen: 0 }, ["echo hi", "$ "]);

    expect(tree.set.get(0)).toEqual({ text: "echo hi", posInSet: 1, setSize: 2 });
    expect(tree.set.get(1)).toEqual({ text: "$ ", posInSet: 2, setSize: 2 });
  });

  // Scrolled up into history, a viewport row's position is its *absolute* buffer
  // line: top = scrollbackLen − displayOffset (xterm `buffer.ydisp`), and the
  // set size is the whole buffer (scrollback + viewport). With 100 scrollback
  // lines scrolled up by 10, the top viewport row is line 91 of 105.
  it("positions rows by absolute buffer line when scrolled into history", () => {
    const tree = new StubTree();
    const ctrl = new AccessibilityController({ tree, live: new StubLive() });

    ctrl.onFrame({ rows: 5, displayOffset: 10, scrollbackLen: 100 }, ["a", "b", "c", "d", "e"]);

    expect(tree.set.get(0)).toMatchObject({ posInSet: 91, setSize: 105 });
    expect(tree.set.get(4)).toMatchObject({ posInSet: 95, setSize: 105 });
  });

  // The tree is kept sized to the viewport. A row-count change (resize) grows or
  // shrinks it once; steady-state frames don't re-resize redundantly.
  it("resizes the tree only when the viewport row count changes", () => {
    const tree = new StubTree();
    const ctrl = new AccessibilityController({ tree, live: new StubLive() });

    ctrl.onFrame({ rows: 2 }, ["a", "b"]);
    ctrl.onFrame({ rows: 2 }, ["a", "c"]); // same height — no re-resize
    ctrl.onFrame({ rows: 3 }, ["a", "c", "d"]); // grew

    expect(tree.resizes).toEqual([2, 3]);
    expect(tree.rows).toBe(3);
  });
});

describe("AccessibilityController — boundary scroll (W1)", () => {
  // When AT navigation reaches the top row and there's history above, the
  // viewport scrolls up one line and focus moves to the new second row so the
  // user can keep walking upward (xterm `_handleBoundaryFocus`). Scroll sign
  // matches the existing scroll seam: negative = toward history.
  it("scrolls up and re-focuses the inner row at the top boundary", () => {
    const tree = new StubTree();
    const scrolled: number[] = [];
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      onScroll: (n) => scrolled.push(n),
    });
    // 100 lines of history, scrolled up by 5 → room to scroll further up.
    ctrl.onFrame({ rows: 4, displayOffset: 5, scrollbackLen: 100 }, ["a", "b", "c", "d"]);

    ctrl.onBoundaryFocus("top", true); // focus arrived from the inner neighbour

    expect(scrolled).toEqual([-1]);
    expect(tree.focused).toEqual([1]);
  });

  // At the very top of the buffer (no history above) the top boundary can't go
  // further, and at the bottom while following (displayOffset 0) the bottom
  // boundary can't either — both are no-ops (xterm's `posInSet === lastRowPos`
  // guard).
  it("does not scroll past the buffer edges", () => {
    const tree = new StubTree();
    const scrolled: number[] = [];
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      onScroll: (n) => scrolled.push(n),
    });
    // No scrollback, following the bottom: viewport spans the whole buffer.
    ctrl.onFrame({ rows: 4, displayOffset: 0, scrollbackLen: 0 }, ["a", "b", "c", "d"]);

    ctrl.onBoundaryFocus("top", true); // top is buffer line 1 → no-op
    ctrl.onBoundaryFocus("bottom", true); // bottom is the last line → no-op

    expect(scrolled).toEqual([]);
    expect(tree.focused).toEqual([]);
  });

  // The boundary scroll has a second guard (xterm `relatedTarget`): scroll only
  // when focus arrived from the *inner* neighbour (the user walking outward).
  // A click, Tab-in, or programmatic focus onto the edge row must NOT scroll.
  it("does not scroll when focus did not arrive from the inner row", () => {
    const tree = new StubTree();
    const scrolled: number[] = [];
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      onScroll: (n) => scrolled.push(n),
    });
    ctrl.onFrame({ rows: 4, displayOffset: 5, scrollbackLen: 100 }, ["a", "b", "c", "d"]);

    ctrl.onBoundaryFocus("top", false); // focus landed here directly (click/Tab-in)

    expect(scrolled).toEqual([]); // suppressed despite room to scroll
    expect(tree.focused).toEqual([]);
  });
});

describe("AccessibilityController — announce new output (W2)", () => {
  // Announce is debounced (xterm `TimeBasedDebouncer`): the controller
  // accumulates new output across frames and flushes once the timer fires, so
  // tests inject a manual scheduler and `flush()` before asserting.
  function make(tree: StubTree = new StubTree()) {
    const live = new StubLive();
    const sched = new ManualScheduler();
    const ctrl = new AccessibilityController({
      tree,
      live,
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    return { ctrl, live, tree, flush: () => sched.flush() };
  }

  // The first frame is the initial paint, not "new output" — it seeds the diff
  // baseline silently (announcing the whole screen on load would be noise). A
  // later frame that prints a line announces just that line.
  it("announces a newly printed line, not the initial paint", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 3 }, ["$ ls", "", ""]); // baseline
    ctrl.onFrame({ rows: 3 }, ["$ ls", "file.txt", ""]);
    flush();

    expect(live.announced).toEqual(["file.txt"]);
  });

  // A Full frame (kind 0) is a whole-viewport repaint — a clear, resize, or
  // alt-screen switch, NOT incremental output. Announcing its diff would read
  // the whole screen aloud on every `clear`. So it reseeds the baseline silently
  // and a later Partial frame diffs against it.
  it("does not announce a full-frame repaint, but reseeds the baseline", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 2 }, ["a", "b"]); // baseline
    ctrl.onFrame({ rows: 2, kind: 0 }, ["X", "Y"]); // full repaint (clear/resize)
    flush();
    expect(live.announced).toEqual([]); // repaint not announced

    ctrl.onFrame({ rows: 2, kind: 1 }, ["X", "Z"]); // partial output
    flush();
    expect(live.announced).toEqual(["Z"]); // diffs against the repaint baseline
  });

  // #215: the first output after idle fires a leading edge (announced immediately,
  // zero latency), so only the REST of a rapid burst coalesces on the trailing flush.
  // (Before #215 the whole burst — its first line included — coalesced into one
  // announcement; the leading edge trades that for a responsive first line, matching
  // xterm's TimeBasedDebouncer synchronous leading refresh.)
  it("leads the first line of a burst, then coalesces the rest", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 4 }, ["a", "", "", ""]); // baseline
    ctrl.onFrame({ rows: 4 }, ["a", "b", "", ""]); // first output after idle → leads now
    expect(live.announced).toEqual(["b"]); // announced immediately, no flush needed
    ctrl.onFrame({ rows: 4 }, ["a", "b", "c", ""]); // within the active window → debounced
    ctrl.onFrame({ rows: 4 }, ["a", "b", "c", "d"]);
    flush();

    expect(live.announced).toEqual(["b", "c\nd"]); // the rest coalesced into one trailing flush
  });

  // When output scrolls, every viewport row's text shifts up — a naive text
  // diff would announce the whole screen. The scroll op says only the bottom
  // `scrollCount` rows are newly exposed, so only the genuinely new line is
  // announced. This is the over-announce trap the design calls out.
  it("announces only newly scrolled-in lines, not the shifted screen", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 3 }, ["line1", "line2", "line3"]); // baseline
    ctrl.onFrame(
      { rows: 3, hasScroll: true, scrollTop: 0, scrollBottom: 2, scrollCount: 1 },
      ["line2", "line3", "line4"],
    );
    flush();

    expect(live.announced).toEqual(["line4"]);
  });

  // A typed char is echoed back by the shell as output. The textarea already
  // announced the keystroke, so the echo must not be announced again (xterm
  // `_charsToConsume`). Here typing "y" appends "y" to the prompt row; the
  // diff's new text is just "y", which the consume queue swallows.
  it("does not announce the echo of a typed char", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 2 }, ["$ ", ""]); // baseline
    ctrl.onKey("y");
    ctrl.onFrame({ rows: 2 }, ["$ y", ""]); // echo
    flush();

    expect(live.announced).toEqual([]);
  });

  // #153 G9: a key can commit MULTIPLE code points at once (IME, a pasted run). The
  // echo-dedup drains one code point per echoed char, so a single multi-code-point
  // consume entry would mismatch and wrongly re-announce. `onKey` splits per code
  // point, so a multi-char commit's echo is fully swallowed like a single char.
  it("dedups a multi-code-point key echo, not just single chars", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 2 }, ["$ ", ""]); // baseline
    ctrl.onKey("ab"); // a two-code-point commit (IME/paste)
    ctrl.onFrame({ rows: 2 }, ["$ ab", ""]); // shell echoes both
    flush();

    expect(live.announced).toEqual([]); // both echoed chars deduped
  });

  // #153: a pure trailing debounce never flushes during an UNBROKEN sub-200ms stream
  // (`yes`, a long build) — the SR stays silent until output stops. A max-wait cap
  // force-flushes periodically (xterm `TimeBasedDebouncer` 1s throttle). Drives an
  // injected clock; the debounce timer is never fired manually (no quiet gap).
  it("force-flushes an unbroken stream at the max-wait, not just on stop", () => {
    let t = 0;
    const live = new StubLive();
    const sched = new ManualScheduler();
    const ctrl = new AccessibilityController({
      tree: new StubTree(),
      live,
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
      now: () => t,
    });

    ctrl.onFrame({ rows: 2 }, ["x", ""]); // baseline (t=0, silent)
    ctrl.onFrame({ rows: 2 }, ["x", "L0"]); // first output at t=0 → #215 leading edge
    expect(live.announced).toEqual(["L0"]); // announced immediately
    // An unbroken sub-200ms stream from t=100 — no 200ms quiet gap, so the trailing
    // debounce never fires and only the max-wait cap can flush it. The batch's first
    // push is L1 at t=100, so the cap lands at t=1100.
    for (let i = 1; i <= 10; i++) {
      t = i * 100;
      ctrl.onFrame({ rows: 2 }, ["x", `L${i}`]);
    }
    expect(live.announced).toEqual(["L0"]); // still just the leading line at t=1000 (< cap)

    t = 1100; // elapsed since the batch's first push (t=100) hits the 1s cap
    ctrl.onFrame({ rows: 2 }, ["x", "L11"]);
    expect(live.announced.length).toBeGreaterThan(1); // forced flush mid-stream
  });

  // #215: the first output after an idle window is announced with ZERO latency — a
  // leading edge — instead of waiting the 200ms debounce. No timer flush is needed.
  it("announces the first output immediately, without the debounce timer", () => {
    const { ctrl, live } = make(); // note: no flush() call anywhere

    ctrl.onFrame({ rows: 2 }, ["$ ", ""]); // baseline (silent)
    ctrl.onFrame({ rows: 2 }, ["$ ", "hi"]); // first real output

    expect(live.announced).toEqual(["hi"]); // led immediately — no flush() needed
  });

  // #215: the leading edge re-arms after a quiet gap of at least the cap, so a fresh
  // command's first line following an idle period is announced immediately again — not
  // only the very first output of the session.
  it("leads again after an idle gap of at least the max-wait", () => {
    let t = 0;
    const live = new StubLive();
    const sched = new ManualScheduler();
    const ctrl = new AccessibilityController({
      tree: new StubTree(),
      live,
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
      now: () => t,
    });

    ctrl.onFrame({ rows: 2 }, ["$ ", ""]); // baseline
    ctrl.onFrame({ rows: 2 }, ["$ ", "one"]); // t=0 → leads
    expect(live.announced).toEqual(["one"]);

    t = 1500; // quiet for > the 1s cap
    ctrl.onFrame({ rows: 2 }, ["$ ", "two"]); // fresh output after idle → leads again
    expect(live.announced).toEqual(["one", "two"]); // immediate, no flush
  });

  // #215 / xterm parity: on screen-reader re-activation xterm disposes + recreates the
  // whole AccessibilityManager — a FRESH debouncer (`_lastRefreshMs = 0`) whose first
  // refresh always leads. `reactivate()` must mirror that by clearing the idle clock, so
  // the first line after re-enabling the SR leads immediately even if the toggle happened
  // < the cap after the last announce (otherwise it'd be held for the debounce).
  it("leads the first output after reactivation, even within the cap", () => {
    let t = 0;
    const live = new StubLive();
    const sched = new ManualScheduler();
    const ctrl = new AccessibilityController({
      tree: new StubTree(),
      live,
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
      now: () => t,
    });

    ctrl.onFrame({ rows: 2 }, ["$ ", ""]); // baseline
    ctrl.onFrame({ rows: 2 }, ["$ ", "a"]); // t=0 → leads, lastFlushAt=0
    expect(live.announced).toEqual(["a"]);

    t = 500; // SR toggled off then back on, only 500ms later (< the 1s cap)
    ctrl.reactivate();
    t = 600;
    ctrl.onFrame({ rows: 2 }, ["$ ", "b"]); // first output post-reactivation

    expect(live.announced).toEqual(["a", "b"]); // led immediately, not held for the debounce
  });

  // In the alternate screen (vim, htop) every repaint damages the whole screen;
  // announcing it would read the editor aloud on every keystroke. So announce is
  // suppressed there — but the review row tree still updates so the user can
  // navigate it manually. (Gated on the #149 alt-screen bit; absent → primary.)
  it("suppresses announce in the alternate screen but still mirrors rows", () => {
    const { ctrl, live, tree, flush } = make();

    ctrl.onFrame({ rows: 2, altScreen: false }, ["$ vim", ""]); // baseline (primary)
    ctrl.onFrame({ rows: 2, altScreen: true }, ["~ VIM ~", "-- INSERT --"]); // alt repaint
    flush();

    expect(live.announced).toEqual([]); // firehose suppressed
    expect(tree.set.get(1)).toMatchObject({ text: "-- INSERT --" }); // tree still mirrored
  });

  // A burst of more than MAX_ROWS_TO_READ (20) new lines would flood the screen
  // reader, which can't keep up. Past the cap, announce a "navigate manually"
  // notice instead of the flood (xterm's `tooMuchOutput`).
  it("caps a flood of new lines with a too-much-output notice", () => {
    const { ctrl, live, flush } = make();
    const blank = Array.from({ length: 21 }, () => "");
    const filled = Array.from({ length: 21 }, (_, i) => `line ${i}`);

    ctrl.onFrame({ rows: 21 }, blank); // baseline
    ctrl.onFrame({ rows: 21 }, filled); // 21 new lines > 20 cap
    flush();

    expect(live.announced).toEqual([TOO_MUCH_OUTPUT]);
  });

  // Losing focus clears the live region (and any pending announcement) so a
  // stale announcement isn't left for the next focus (xterm `onBlur`).
  it("clears the live region on blur", () => {
    const { ctrl, live } = make();

    ctrl.onBlur();

    expect(live.cleared).toBe(1);
  });

  // The consume queue must drain at the output rate (xterm pops one per output
  // char), not stall at the first mismatch. A typed char that is never echoed
  // (e.g. `read -s`) must not linger and silently swallow a later genuine
  // identical line.
  it("drains a never-echoed typed char so it can't suppress later output", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 3 }, ["a", "", ""]); // baseline
    ctrl.onKey("x"); // typed, but the next output is NOT its echo
    ctrl.onFrame({ rows: 3 }, ["a", "b", ""]); // output "b" → "x" should drain here
    flush();
    ctrl.onFrame({ rows: 3 }, ["a", "b", "x"]); // a genuine "x" line later
    flush();

    expect(live.announced).toEqual(["b", "x"]); // not ["b"] — "x" not swallowed
  });

  // A keystroke cancels an in-flight (debounced) announcement and clears the
  // live region: the user is typing, so stale output mustn't be read over them
  // (xterm `_handleKey` → `_clearLiveRegion`).
  it("cancels a pending announcement on keypress", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 2 }, ["a", ""]); // baseline
    ctrl.onFrame({ rows: 2 }, ["a", "b"]); // first output → #215 leads → announces "b"
    ctrl.onFrame({ rows: 2 }, ["a", "bc"]); // follow-up within the window → pending
    ctrl.onKey("z"); // typing → wipe the pending read
    flush();

    expect(live.announced).toEqual(["b"]); // the led line stands; the pending "c" dropped
    expect(live.cleared).toBeGreaterThanOrEqual(1);
  });

  // dispose() tears the controller down: a pending debounce timer must not fire
  // afterwards (it would announce into a detached region). Mirrors the dispose
  // seam the sibling controllers expose.
  it("does not flush a pending announcement after dispose", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 2 }, ["a", ""]); // baseline
    ctrl.onFrame({ rows: 2 }, ["a", "b"]); // first output → #215 leads → announces "b"
    ctrl.onFrame({ rows: 2 }, ["a", "bc"]); // follow-up within the window → pending
    ctrl.dispose();
    flush();

    expect(live.announced).toEqual(["b"]); // the pending "c" never flushes after dispose
  });
});

describe("AccessibilityController — row-tree churn gate while SR inactive (#169)", () => {
  // While inactive (host knows no screen reader is attached), a frame updates the
  // controller's bookkeeping but does NOT rewrite the tree DOM — the per-frame
  // `setRow` churn (xterm's `_refreshRows`, which it avoids by disposing the whole
  // manager) is skipped. The tree structure (resize) is still maintained.
  it("skips the row-tree setRow loop while inactive", () => {
    const tree = new StubTree();
    let active = false;
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      isActive: () => active,
    });

    ctrl.onFrame({ rows: 2, displayOffset: 0, scrollbackLen: 0 }, ["a", "b"]);

    expect(tree.setRowCalls).toBe(0); // no per-frame DOM churn
    expect(tree.resizes).toEqual([2]); // structure kept, so reactivation is instant
  });

  // Reactivation re-syncs the tree from the CACHED last frame immediately — no
  // cold rebuild and no waiting for the next frame (justerm's edge over xterm's
  // dispose+createInstance).
  it("syncTree renders the cached last frame on reactivation", () => {
    const tree = new StubTree();
    let active = false;
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      isActive: () => active,
    });

    ctrl.onFrame({ rows: 2, displayOffset: 0, scrollbackLen: 0 }, ["hello", "$ "]);
    expect(tree.setRowCalls).toBe(0);

    active = true;
    ctrl.syncTree();

    expect(tree.set.get(0)).toEqual({ text: "hello", posInSet: 1, setSize: 2 });
    expect(tree.set.get(1)).toEqual({ text: "$ ", posInSet: 2, setSize: 2 });
  });

  // After reactivation, per-frame mirroring resumes normally.
  it("resumes per-frame mirroring after reactivation", () => {
    const tree = new StubTree();
    let active = false;
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      isActive: () => active,
    });

    ctrl.onFrame({ rows: 1, displayOffset: 0, scrollbackLen: 0 }, ["old"]);
    active = true;
    ctrl.syncTree();
    tree.setRowCalls = 0; // ignore the sync render; count only the next frame

    ctrl.onFrame({ rows: 1, displayOffset: 0, scrollbackLen: 0 }, ["new"]);

    expect(tree.setRowCalls).toBe(1);
    expect(tree.set.get(0)?.text).toBe("new");
  });

  // syncTree before any frame has no cached rows — a safe no-op.
  it("syncTree is a no-op before the first frame", () => {
    const tree = new StubTree();
    const ctrl = new AccessibilityController({ tree, live: new StubLive(), isActive: () => true });

    ctrl.syncTree();

    expect(tree.setRowCalls).toBe(0);
  });

  // Omitting isActive defaults to active — the pre-#169 always-mirror behaviour.
  it("defaults to active: mirrors every frame when isActive is omitted", () => {
    const tree = new StubTree();
    const ctrl = new AccessibilityController({ tree, live: new StubLive() });

    ctrl.onFrame({ rows: 1, displayOffset: 0, scrollbackLen: 0 }, ["x"]);

    expect(tree.setRowCalls).toBe(1);
  });

  // A resize (viewport row-count change) while inactive still tracks the tree
  // structure, so syncTree renders ALL the current rows on reactivation — the
  // cached prevRows and treeRows stay in lockstep.
  it("re-syncs the full tree after a resize while inactive", () => {
    const tree = new StubTree();
    let active = false;
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      isActive: () => active,
    });

    ctrl.onFrame({ rows: 2, displayOffset: 0, scrollbackLen: 0 }, ["a", "b"]);
    ctrl.onFrame({ rows: 3, displayOffset: 0, scrollbackLen: 0 }, ["x", "y", "z"]); // grew
    expect(tree.setRowCalls).toBe(0);
    expect(tree.resizes).toEqual([2, 3]); // structure tracked the resize

    active = true;
    ctrl.syncTree();

    expect(tree.setRowCalls).toBe(3);
    expect(tree.set.get(0)?.text).toBe("x");
    expect(tree.set.get(2)?.text).toBe("z");
  });

  // The very first frame arriving while inactive seeds the baseline (announce
  // diff) and skips the tree churn; reactivation then renders that cached frame.
  it("handles the first frame arriving while inactive", () => {
    const tree = new StubTree();
    let active = false;
    const ctrl = new AccessibilityController({
      tree,
      live: new StubLive(),
      isActive: () => active,
    });

    ctrl.onFrame({ rows: 1, displayOffset: 0, scrollbackLen: 0 }, ["boot"]);
    expect(tree.setRowCalls).toBe(0);

    active = true;
    ctrl.syncTree();

    expect(tree.set.get(0)?.text).toBe("boot");
  });

  // reactivate() drops any announce that accumulated while inactive (no surprise
  // backlog replay — the user reviews via the synced tree) AND re-syncs the tree.
  // Without the drop, the debounce tail would flush through the now-active gate.
  it("reactivate drops the pending announce backlog and syncs the tree", () => {
    const tree = new StubTree();
    const live = new StubLive();
    const sched = new ManualScheduler();
    let active = false;
    const ctrl = new AccessibilityController({
      tree,
      live,
      isActive: () => active,
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });

    ctrl.onFrame({ rows: 1, displayOffset: 0, scrollbackLen: 0 }, ["$ "]); // baseline
    ctrl.onFrame({ rows: 1, displayOffset: 0, scrollbackLen: 0 }, ["output"]); // arms pending

    active = true;
    ctrl.reactivate();
    sched.flush(); // the cancelled debounce timer would have fired here

    expect(live.announced).toEqual([]); // backlog NOT replayed
    expect(tree.set.get(0)?.text).toBe("output"); // tree synced to the cached frame
  });
});

describe("AccessibilityController — announce-diff CPU gate while SR inactive (#183)", () => {
  // A controller with a mutable SR-active flag and a RAW (ungated) live sink, so
  // the ONLY thing that can silence an announce is the skipped diff (#183) — NOT
  // #161's sink gate (which these tests deliberately don't wire). Mirrors the #169
  // tree-gate setup + the W2 manual scheduler.
  function make() {
    const tree = new StubTree();
    const live = new StubLive();
    const sched = new ManualScheduler();
    let active = true;
    const ctrl = new AccessibilityController({
      tree,
      live,
      isActive: () => active,
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    return {
      ctrl,
      live,
      tree,
      flush: () => sched.flush(),
      setActive: (a: boolean) => {
        active = a;
      },
    };
  }

  // The perf goal: while inactive the whole diff (shiftPrev + per-row compare +
  // debounce arm) is skipped, not merely the flush. With a RAW sink, an inactive
  // new-output frame still announces NOTHING — proving the diff never ran. (Under
  // #161 alone the diff would run and only the flush would no-op at the gated
  // sink; here the sink is ungated, so silence can only be the skipped work.)
  it("skips the diff and arms no timer while inactive (raw sink stays silent)", () => {
    const { ctrl, live, flush, setActive } = make();
    ctrl.onFrame({ rows: 2 }, ["a", ""]); // baseline (active)
    setActive(false);
    ctrl.onFrame({ rows: 2 }, ["a", "new line"]); // genuinely new output, but inactive
    flush(); // no timer armed → nothing to fire
    expect(live.announced).toEqual([]);
  });

  // The #161 no-replay anchor survives the #183 gate: `prevRows` still advances on
  // every inactive frame (it lives OUTSIDE announceNewOutput), so after
  // reactivation the first frame diffs against the LAST inactive content — a
  // repeat is silent and only genuinely new text ("d") announces. If the baseline
  // had frozen (or inactive output had leaked), "b"/"c" would (re-)announce here.
  it("keeps advancing the baseline while inactive so the first active diff is correct", () => {
    const { ctrl, live, flush, setActive } = make();
    ctrl.onFrame({ rows: 3 }, ["a", "", ""]); // baseline (active)
    setActive(false);
    ctrl.onFrame({ rows: 3 }, ["a", "b", ""]); // inactive: advances prevRows, announces nothing
    ctrl.onFrame({ rows: 3 }, ["a", "b", "c"]); // still inactive
    setActive(true);
    ctrl.reactivate();
    ctrl.onFrame({ rows: 3 }, ["a", "b", "c"]); // active, identical to last → no new text
    flush();
    expect(live.announced).toEqual([]); // nothing re-announced; baseline was current
    ctrl.onFrame({ rows: 3 }, ["a", "b", "d"]); // active, genuine new text
    flush();
    expect(live.announced).toEqual(["d"]);
  });

  // The hidden-state catch — the whole reason #183 isn't folded into #169. The
  // echo-dedup `consume` queue drains ONLY inside announceNewOutput (via
  // dedupTyped). Gating that while inactive would let keystrokes pile up in
  // `consume` unbounded, and on reactivation the first real output would be
  // wrongly swallowed as stale echo. So onKey enqueues only while active (xterm's
  // disposed manager registers no char listener) and reactivate clears the queue
  // (xterm's fresh manager starts empty).
  it("does not swallow the first post-reactivation output with keys typed while inactive", () => {
    const { ctrl, live, flush, setActive } = make();
    ctrl.onFrame({ rows: 1 }, ["$ "]); // baseline (active)
    setActive(false);
    ctrl.onKey("l"); // typed while inactive — must NOT accumulate in consume
    ctrl.onKey("s");
    setActive(true);
    ctrl.reactivate();
    ctrl.onFrame({ rows: 1 }, ["ls"]); // first real output after reactivation
    flush();
    expect(live.announced).toEqual(["ls"]); // announced in full, not swallowed as echo
  });

  // While active, echo dedup is unchanged (#119/W2 behaviour intact): a typed
  // char whose echo lands in the next output is still swallowed. Guards against
  // the fix over-reaching and disabling dedup wholesale.
  it("still dedups an echo typed while active", () => {
    const { ctrl, live, flush } = make(); // active throughout
    ctrl.onFrame({ rows: 1 }, ["$ "]); // baseline
    ctrl.onKey("y");
    ctrl.onFrame({ rows: 1 }, ["$ y"]); // the echo of the keystroke
    flush();
    expect(live.announced).toEqual([]); // echo swallowed, not announced
  });
});
