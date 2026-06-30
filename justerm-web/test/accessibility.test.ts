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
  resize(rows: number): void {
    this.rows = rows;
    this.resizes.push(rows);
  }
  setRow(i: number, text: string, posInSet: number, setSize: number): void {
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

    ctrl.onBoundaryFocus("top");

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

    ctrl.onBoundaryFocus("top"); // top is buffer line 1 → no-op
    ctrl.onBoundaryFocus("bottom"); // bottom is the last line → no-op

    expect(scrolled).toEqual([]);
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

    ctrl.onFrame({ rows: 3, cursorRow: 0 }, ["$ ls", "", ""]); // baseline
    ctrl.onFrame({ rows: 3, cursorRow: 2 }, ["$ ls", "file.txt", ""]);
    flush();

    expect(live.announced).toEqual(["file.txt"]);
  });

  // A Full frame (kind 0) is a whole-viewport repaint — a clear, resize, or
  // alt-screen switch, NOT incremental output. Announcing its diff would read
  // the whole screen aloud on every `clear`. So it reseeds the baseline silently
  // and a later Partial frame diffs against it.
  it("does not announce a full-frame repaint, but reseeds the baseline", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 2, cursorRow: 0 }, ["a", "b"]); // baseline
    ctrl.onFrame({ rows: 2, kind: 0 }, ["X", "Y"]); // full repaint (clear/resize)
    flush();
    expect(live.announced).toEqual([]); // repaint not announced

    ctrl.onFrame({ rows: 2, kind: 1, cursorRow: 1 }, ["X", "Z"]); // partial output
    flush();
    expect(live.announced).toEqual(["Z"]); // diffs against the repaint baseline
  });

  // Rapid frames (streaming output) collapse to one announcement — the point of
  // the debounce. The accumulated new lines flush together.
  it("coalesces rapid frames into a single announcement", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 3, cursorRow: 0 }, ["a", "", ""]); // baseline
    ctrl.onFrame({ rows: 3, cursorRow: 1 }, ["a", "b", ""]);
    ctrl.onFrame({ rows: 3, cursorRow: 2 }, ["a", "b", "c"]);
    flush();

    expect(live.announced).toEqual(["b\nc"]);
  });

  // When output scrolls, every viewport row's text shifts up — a naive text
  // diff would announce the whole screen. The scroll op says only the bottom
  // `scrollCount` rows are newly exposed, so only the genuinely new line is
  // announced. This is the over-announce trap the design calls out.
  it("announces only newly scrolled-in lines, not the shifted screen", () => {
    const { ctrl, live, flush } = make();

    ctrl.onFrame({ rows: 3, cursorRow: 2 }, ["line1", "line2", "line3"]); // baseline
    ctrl.onFrame(
      { rows: 3, cursorRow: 2, hasScroll: true, scrollTop: 0, scrollBottom: 2, scrollCount: 1 },
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

    ctrl.onFrame({ rows: 2, cursorRow: 0 }, ["$ ", ""]); // baseline
    ctrl.onKey("y");
    ctrl.onFrame({ rows: 2, cursorRow: 0 }, ["$ y", ""]); // echo
    flush();

    expect(live.announced).toEqual([]);
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
});
