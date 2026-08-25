import { describe, expect, it } from "vitest";
import { FitController, proposeDimensions, StubResizePort } from "../src/fit";

/** A manual debounce timer (mirrors accessibility.test.ts): `setTimer` stashes the
 * latest callback, `flush()` fires it — no real time. */
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

/** A baseline fit input: 800×600 container, no padding, 8×16 cells, no scrollbar. */
function base() {
  return {
    parentWidth: 800,
    parentHeight: 600,
    padding: { top: 0, bottom: 0, left: 0, right: 0 },
    cellWidth: 8,
    cellHeight: 16,
    scrollbarWidth: 0,
    scrollback: 0,
  };
}

describe("proposeDimensions (#114 fit: px → cols/rows)", () => {
  // Tracer: cols = availWidth / cellWidth, rows = availHeight / cellHeight.
  // 800/8 = 100 cols; 600/16 = 37.5 → floor 37 rows.
  it("divides the available box by the cell size", () => {
    expect(proposeDimensions(base())).toEqual({ cols: 100, rows: 37 });
  });

  // Element padding is subtracted from the parent box first. Horizontal 10+10=20 →
  // availW 780 → 97 cols; vertical 8+8=16 → availH 584 → floor(36.5) = 36 rows.
  it("subtracts element padding from the available box", () => {
    expect(
      proposeDimensions({ ...base(), padding: { top: 8, bottom: 8, left: 10, right: 10 } }),
    ).toEqual({ cols: 97, rows: 36 });
  });

  // With scrollback, the scrollbar width is reserved from the width (fit couples with
  // the #112 scrollbar). 800−14 = 786 → floor(786/8) = 98 cols; height unaffected.
  it("subtracts the scrollbar width when there is scrollback", () => {
    expect(proposeDimensions({ ...base(), scrollbarWidth: 14, scrollback: 100 })).toEqual({
      cols: 98,
      rows: 37,
    });
  });

  // No scrollback → no scrollbar shows → its width is NOT reserved (consistent with the
  // #112 scrollbar, which hides at scrollback 0). So the full width fits: 800/8 = 100.
  it("does not reserve the scrollbar width when scrollback is 0", () => {
    expect(proposeDimensions({ ...base(), scrollbarWidth: 14, scrollback: 0 })).toEqual({
      cols: 100,
      rows: 37,
    });
  });

  // A box too small to hold even one cell still yields the floor grid xterm enforces:
  // MINIMUM_COLS = 2, MINIMUM_ROWS = 1. 8/8 = 1 → clamped up to 2 cols; 8/16 = 0 → 1 row.
  it("clamps to a minimum of 2 cols and 1 row", () => {
    expect(proposeDimensions({ ...base(), parentWidth: 8, parentHeight: 8 })).toEqual({
      cols: 2,
      rows: 1,
    });
  });

  // #810 — a box with no area is "not measured", exactly like a NaN one, and the floor above is what
  // used to turn it into a plausible wrong answer. The side condition is the test above: an 8x8 box
  // is measured and genuinely tiny, and still floors. What separates them is `<= 0`, not smallness.
  it("returns undefined for a box with no area, rather than flooring it (#810)", () => {
    expect(proposeDimensions({ ...base(), parentWidth: 0, parentHeight: 0 })).toBeUndefined();
  });

  // The predicate is `||`, not `&&`: a collapsed split has one axis at 0 and the other laid out, and
  // `display: none` zeroes both — so a guard written for the second misses the first entirely.
  it("returns undefined when EITHER axis has no area (#810)", () => {
    expect(proposeDimensions({ ...base(), parentWidth: 0 })).toBeUndefined();
    expect(proposeDimensions({ ...base(), parentHeight: 0 })).toBeUndefined();
  });

  // `<= 0` rather than `=== 0`. A negative box is not a small box; it is a measurement that cannot be
  // one, and flooring it would be the same wrong answer arrived at from the other side.
  it("returns undefined for a negative box (#810)", () => {
    expect(proposeDimensions({ ...base(), parentWidth: -1 })).toBeUndefined();
  });

  // The guard is on the PARENT box, not on the space left after padding. A parent that exists but
  // whose padding eats it is a layout the host chose and measured; an absent parent is not a layout
  // at all. Conflating them would change a case #810 never measured.
  it("still floors when padding eats a box that WAS measured (#810)", () => {
    expect(
      proposeDimensions({
        ...base(),
        parentWidth: 10,
        parentHeight: 10,
        padding: { top: 20, bottom: 20, left: 20, right: 20 },
      }),
    ).toEqual({ cols: 2, rows: 1 });
  });

  // A cell dimension of 0 means the renderer hasn't measured yet — fitting would divide
  // by zero (Infinity). Return undefined so the caller skips the resize (xterm's
  // `dims.css.cell.width === 0` guard).
  it("returns undefined when a cell dimension is 0", () => {
    expect(proposeDimensions({ ...base(), cellWidth: 0 })).toBeUndefined();
    expect(proposeDimensions({ ...base(), cellHeight: 0 })).toBeUndefined();
  });

  // A detached/unmeasured element gives non-finite box metrics (NaN, or Infinity from a
  // degenerate input); fitting them would propose a non-finite grid. Return undefined so
  // the caller skips (xterm's `isNaN(dims.cols)` guard, widened to all non-finite).
  it("returns undefined when the box metrics are non-finite", () => {
    expect(proposeDimensions({ ...base(), parentWidth: NaN })).toBeUndefined();
    expect(proposeDimensions({ ...base(), parentHeight: NaN })).toBeUndefined();
    expect(proposeDimensions({ ...base(), parentWidth: Infinity })).toBeUndefined();
  });
});

describe("FitController (#114 debounced resize intent)", () => {
  function make() {
    const port = new StubResizePort();
    const sched = new ManualScheduler();
    const ctrl = new FitController({ port, setTimer: sched.setTimer, clearTimer: sched.clearTimer });
    return { ctrl, port, flush: () => sched.flush() };
  }

  // Tracer: a fit drives the resize intent (backend `Engine::resize` + PTY SIGWINCH) with
  // the proposed grid, after the debounce fires.
  it("drives the resize port with the proposed grid", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit(base()); // 800×600, 8×16 → 100×37
    flush();
    expect(port.calls).toEqual([{ cols: 100, rows: 37 }]);
  });

  // A resize that doesn't change the grid (e.g. a sub-cell pixel wobble) must NOT re-issue
  // the intent — a redundant backend resize would reflow + repaint for nothing.
  it("does not re-issue an unchanged grid", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit(base());
    flush();
    ctrl.fit({ ...base(), parentWidth: 803 }); // 803/8 = 100.375 → still 100 cols
    flush();
    expect(port.calls).toEqual([{ cols: 100, rows: 37 }]); // one call, not two
  });

  // A burst of resizes before the debounce fires coalesces into ONE intent, using the
  // LATEST geometry (a drag emits many events; we resize the backend once, at the end).
  it("coalesces a burst into one intent using the latest geometry", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit({ ...base(), parentWidth: 800 }); // would be 100
    ctrl.fit({ ...base(), parentWidth: 400 }); // 50 — supersedes before the debounce
    flush();
    expect(port.calls).toEqual([{ cols: 50, rows: 37 }]);
  });

  // When the fit can't be proposed (cell not measured → undefined), no intent is issued —
  // the backend keeps its current size until a real geometry arrives.
  it("issues no intent when the fit is undefined", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit({ ...base(), cellWidth: 0 });
    flush();
    expect(port.calls).toEqual([]);
  });

  // dispose() cancels a pending debounce so a resize can't fire into a torn-down backend
  // after the widget unmounts (the sibling controllers' dispose pattern).
  it("does not emit a pending fit after dispose", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit(base()); // arms the debounce
    ctrl.dispose();
    flush();
    expect(port.calls).toEqual([]);
  });
});

/**
 * #632 — the dedupe was keyed on `cols`/`rows` alone, which is only HALF of what
 * `proposeDimensions` derives from: the same pair can come from different cells. So after a
 * cell-size change the remembered pair described a grid nobody held any more, and a later
 * genuine container resize that happened to propose it was dropped in silence.
 */
describe("FitController dedupes on the cell too, not just the grid (#632)", () => {
  function make() {
    const port = new StubResizePort();
    const sched = new ManualScheduler();
    const ctrl = new FitController({ port, setTimer: sched.setTimer, clearTimer: sched.clearTimer });
    return { ctrl, port, flush: () => sched.flush() };
  }

  // THE FILED DEFECT. 800x600 / 8x16 → 100x37. The cell then heightens to 24 out of band (the
  // #578 contract has the consumer call `renderer.resize()` directly), and a real container
  // resize to 800x900 proposes floor(900/24) = 37 rows — the very pair the controller
  // remembers. Under the old key that resize never reached the port, so the engine kept a grid
  // the box no longer wanted: the silent desync #547 describes.
  it("lets a real resize through when it proposes the pre-change pair under a new cell", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit(base()); // 100x37 at cell 8x16
    flush();
    ctrl.fit({ ...base(), parentHeight: 900, cellHeight: 24 }); // 100x37 again, at cell 8x24
    flush();
    expect(port.calls).toEqual([
      { cols: 100, rows: 37 },
      { cols: 100, rows: 37 },
    ]);
  });

  // The other half of #632: a cell change can move the cell while leaving the grid IDENTICAL —
  // guaranteed once the MINIMUM_COLS/MINIMUM_ROWS floors bind (#547), and common whenever the
  // new cell divides the same box into the same count. floor(600/16.2) is still 37.
  it("flushes a cell change that leaves the grid identical", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit(base());
    flush();
    ctrl.fit({ ...base(), cellHeight: 16.2 }); // same 100x37, taller cell
    flush();
    expect(port.calls).toHaveLength(2);
  });

  // A width-only cell move counts too — the key must carry BOTH cell axes, not just the one
  // the height case happens to exercise. floor(800/8.05) is still 99... so assert the pair.
  it("flushes a cell WIDTH change that leaves the grid identical", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit({ ...base(), cellWidth: 8.05 }); // floor(800/8.05) = 99 cols
    flush();
    ctrl.fit({ ...base(), cellWidth: 8.06 }); // floor(800/8.06) = 99 cols, wider cell
    flush();
    expect(port.calls).toEqual([
      { cols: 99, rows: 37 },
      { cols: 99, rows: 37 },
    ]);
  });

  // THE SIDE CONDITION — the optimisation this dedupe exists for must survive, and it must
  // survive *after* a cell change too: the new key has to be STORED, not merely compared.
  // Without the store, every later fit would compare against the pre-change cell and re-issue
  // for ever, which is a redundant backend reflow per ResizeObserver burst.
  it("still skips a pixel wobble at an unchanged cell, including after a cell change", () => {
    const { ctrl, port, flush } = make();
    ctrl.fit(base());
    flush();
    ctrl.fit({ ...base(), cellHeight: 16.2 }); // a real cell change → flushes (2nd call)
    flush();
    ctrl.fit({ ...base(), parentWidth: 803, cellHeight: 16.2 }); // wobble, same cell → skipped
    flush();
    expect(port.calls).toHaveLength(2);
  });
});
