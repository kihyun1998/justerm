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
