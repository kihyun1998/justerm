import { describe, expect, it } from "vitest";
import { WheelScroller } from "../src/scroll-control";
import type { WheelLike } from "../src/scroll-control";

// WheelEvent.deltaMode constants (DOM): PIXEL=0, LINE=1, PAGE=2.
const PIXEL = 0;
const LINE = 1;
const PAGE = 2;

// A default context: 20px cells, dpr 1, 24-row viewport.
const ctx = { cellHeight: 20, dpr: 1, rows: 24 };

function wheel(p: Partial<WheelLike> & { deltaY: number; deltaMode: number }): WheelLike {
  return { shiftKey: false, altKey: false, ctrlKey: false, ...p };
}

describe("WheelScroller.consumeWheelEvent", () => {
  // DOM_DELTA_LINE: amount = deltaY × scrollSensitivity (default 1), after xterm's
  // CoreMouseService.consumeWheelEvent — a whole amount passes through unchanged (a
  // fractional one carries, #908). Sign follows deltaY (positive = scroll down/newer).
  it("returns line-mode deltaY directly at default sensitivity", () => {
    const s = new WheelScroller();

    const lines = s.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE }), ctx);

    expect(lines).toBe(3);
  });

  // #246: a held Alt fast-scrolls by fastScrollSensitivity (xterm default 5,
  // _applyScrollModifier). Shift is excluded — it already bails to 0 at the top.
  it("multiplies by the default fast sensitivity (5) when Alt is held", () => {
    const s = new WheelScroller();

    const lines = s.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE, altKey: true }), ctx);

    expect(lines).toBe(15); // 3 × 5
  });

  it("fast-scrolls on Ctrl too, and applies to the accumulated pixel path", () => {
    const s = new WheelScroller();
    expect(s.consumeWheelEvent(wheel({ deltaY: 2, deltaMode: LINE, ctrlKey: true }), ctx)).toBe(10);
    // Pixel mode (deltaY 100 ≥ 50, so no trackpad damping): 100 × 5 = 500 px;
    // 500 / (cellHeight 20 / dpr 1) = 25 whole lines (5× the un-modified 5).
    const s2 = new WheelScroller();
    expect(s2.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL, altKey: true }), ctx)).toBe(25);
  });

  it("honors a custom fastScrollSensitivity, composed with scrollSensitivity", () => {
    const s = new WheelScroller({ fastScrollSensitivity: 3 });
    expect(s.consumeWheelEvent(wheel({ deltaY: 2, deltaMode: LINE, altKey: true }), ctx)).toBe(6); // 2×3

    // xterm multiplies BOTH: amount × fastScrollSensitivity × scrollSensitivity.
    const s2 = new WheelScroller({ scrollSensitivity: 2, fastScrollSensitivity: 5 });
    expect(s2.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE, altKey: true }), ctx)).toBe(30); // 3×2×5
  });

  it("Alt+Shift+wheel still bails to 0 (the shift bail wins over fast-scroll)", () => {
    const s = new WheelScroller();
    expect(
      s.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE, altKey: true, shiftKey: true }), ctx),
    ).toBe(0);
  });

  // xterm bails on shiftKey (it's a horizontal scroll) and on a zero deltaY.
  it("ignores shift-wheel and zero-delta as no scroll", () => {
    const s = new WheelScroller();

    expect(s.consumeWheelEvent(wheel({ deltaY: 5, deltaMode: LINE, shiftKey: true }), ctx)).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: 0, deltaMode: LINE }), ctx)).toBe(0);
  });

  // PIXEL mode divides by the cell pixel height and only emits whole lines,
  // carrying the sub-line remainder to the next event (xterm's _wheelPartialScroll).
  // deltaY 50 ≥ 50 dodges the trackpad branch (that's a separate cycle).
  // 50/20 = 2.5 per event: 1st → floor 2 (rem .5); 2nd → .5+2.5=3.0 → 3.
  it("divides pixel deltas into whole lines and carries the remainder", () => {
    const s = new WheelScroller();
    const ev = wheel({ deltaY: 50, deltaMode: PIXEL });

    expect(s.consumeWheelEvent(ev, ctx)).toBe(2);
    expect(s.consumeWheelEvent(ev, ctx)).toBe(3);
  });

  // A small pixel delta (|deltaY| < 50) is a trackpad — xterm damps it ×0.3 so a
  // gentle swipe doesn't fly. 30/20×0.3 = 0.45/event: three swipes accrue to 1.
  it("damps trackpad-sized pixel deltas by 0.3", () => {
    const s = new WheelScroller();
    const ev = wheel({ deltaY: 30, deltaMode: PIXEL });

    expect([
      s.consumeWheelEvent(ev, ctx),
      s.consumeWheelEvent(ev, ctx),
      s.consumeWheelEvent(ev, ctx),
    ]).toEqual([0, 0, 1]);
  });

  // PAGE mode scrolls a viewport's worth of rows per notch (xterm × rows).
  it("scrolls a full page of rows in page mode", () => {
    const s = new WheelScroller();

    const lines = s.consumeWheelEvent(wheel({ deltaY: 1, deltaMode: PAGE }), ctx);

    expect(lines).toBe(24); // 1 × rows
  });

  // The count is a scrollback line delta that ends up in `onScroll`, and a consumer's scroll API
  // takes a whole line (#908). LINE and PAGE carry a fractional product forward, as PIXEL does.
  it("emits whole lines in line mode and carries a fractional sensitivity forward", () => {
    const s = new WheelScroller({ scrollSensitivity: 0.5 });
    const down = wheel({ deltaY: 3, deltaMode: LINE });

    expect(s.consumeWheelEvent(down, ctx)).toBe(1); // 1.5 → 1, carries .5
    expect(s.consumeWheelEvent(down, ctx)).toBe(2); // .5 + 1.5 → 2

    const up = wheel({ deltaY: -3, deltaMode: LINE });
    expect(s.consumeWheelEvent(up, ctx)).toBe(-1); // -1.5 → -1, carries -.5
    expect(s.consumeWheelEvent(up, ctx)).toBe(-2);
  });

  it("emits whole lines in page mode", () => {
    const s = new WheelScroller({ scrollSensitivity: 0.5 });
    const page = wheel({ deltaY: 1, deltaMode: PAGE });

    expect(s.consumeWheelEvent(page, { ...ctx, rows: 25 })).toBe(12); // 12.5 → 12
    expect(s.consumeWheelEvent(page, { ...ctx, rows: 25 })).toBe(13); // .5 + 12.5
  });

  it("drops a sub-line notch until the carried remainder reaches a whole line", () => {
    const s = new WheelScroller({ scrollSensitivity: 0.4 });
    const notch = wheel({ deltaY: 1, deltaMode: LINE });

    expect(s.consumeWheelEvent(notch, ctx)).toBe(0); // .4
    expect(s.consumeWheelEvent(notch, ctx)).toBe(0); // .8
    expect(s.consumeWheelEvent(notch, ctx)).toBe(1); // 1.2

    // Upward, a sub-line notch is still `0` — not `-0`, which `Object.is` tells apart.
    const up = new WheelScroller({ scrollSensitivity: 0.4 });
    expect(up.consumeWheelEvent(wheel({ deltaY: -1, deltaMode: LINE }), ctx)).toBe(0);
  });

  // A trackpad (PIXEL) and a mouse wheel (LINE) share the accumulator, so a remainder one device
  // left behind must not shorten or cancel the other's notch — a change of `deltaMode` starts clean.
  it("drops the remainder when the deltaMode changes", () => {
    const s = new WheelScroller();

    expect(s.consumeWheelEvent(wheel({ deltaY: 90, deltaMode: PIXEL }), ctx)).toBe(4); // carries .5
    expect(s.consumeWheelEvent(wheel({ deltaY: -1, deltaMode: LINE }), ctx)).toBe(-1); // not -0.5 → 0
    expect(s.consumeWheelEvent(wheel({ deltaY: -3, deltaMode: LINE }), ctx)).toBe(-3);

    const half = new WheelScroller({ scrollSensitivity: 0.5 });
    const rows25 = { ...ctx, rows: 25 };
    expect(half.consumeWheelEvent(wheel({ deltaY: 1, deltaMode: LINE }), rows25)).toBe(0); // carries .5
    expect(half.consumeWheelEvent(wheel({ deltaY: 1, deltaMode: PAGE }), rows25)).toBe(12); // 12.5, not .5 + 12.5
  });

  // reset() drops the carried remainder (xterm calls it on buffer activate, so
  // an alt-screen switch starts scroll accumulation clean). Without the reset the
  // third swipe would tip over to 1 (.90 + .45); after it, accumulation restarts.
  it("clears the partial-scroll remainder on reset", () => {
    const s = new WheelScroller();
    const ev = wheel({ deltaY: 30, deltaMode: PIXEL });

    s.consumeWheelEvent(ev, ctx); // .45
    s.consumeWheelEvent(ev, ctx); // .90
    s.reset();

    expect(s.consumeWheelEvent(ev, ctx)).toBe(0); // .45, not 1
  });
});

// #675 — a non-finite intermediate does not merely produce one wrong answer here,
// it *latches*: `wheelPartialScroll` keeps it (`Infinity % 1` is `NaN`), so every
// later notch is `NaN` too, including after the geometry recovers. Measured in a
// real browser: an unmeasured cell killed the wheel until an alt-screen switch.
//
// The guard is on the **output**, not on the context at entry, and the LINE-mode
// case below is why: `deltaMode: LINE` never divides by the cell, so refusing a
// whole context because `cellHeight` is 0 would break a scroll that works today.
// Each guard is placed where it protects something: one before the accumulator,
// one before the return.
describe("WheelScroller — a degenerate context cannot poison the scroller (#675)", () => {
  const bad = { cellHeight: 0, dpr: 1, rows: 24 };

  it("returns no scroll instead of NaN when the cell is unmeasured", () => {
    const s = new WheelScroller();

    expect(s.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL }), bad)).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL }), { ...ctx, cellHeight: NaN })).toBe(0);
  });

  // The half that matters. Returning 0 once is worth nothing if the instance is
  // already ruined — this is the assertion that pins "recovers", and it is the
  // one the browser measurement showed failing.
  it("still scrolls correctly once a real cell arrives", () => {
    const s = new WheelScroller();

    s.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL }), bad);
    s.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL }), bad);

    // 100px / (20/1) = 5 whole lines, exactly as if the bad events never happened.
    expect(s.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL }), ctx)).toBe(5);
  });

  // `ev.deltaY === 0` is the existing bail and `NaN === 0` is false, so a
  // non-finite delta reaches the arithmetic on every branch.
  it("refuses a non-finite deltaY without poisoning the remainder", () => {
    const s = new WheelScroller();

    expect(s.consumeWheelEvent(wheel({ deltaY: NaN, deltaMode: PIXEL }), ctx)).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: Infinity, deltaMode: PIXEL }), ctx)).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: 100, deltaMode: PIXEL }), ctx)).toBe(5);
  });

  it("refuses a non-finite line-mode deltaY without poisoning the carried remainder", () => {
    const s = new WheelScroller({ scrollSensitivity: 0.5 });

    expect(s.consumeWheelEvent(wheel({ deltaY: NaN, deltaMode: LINE }), ctx)).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: -Infinity, deltaMode: LINE }), ctx)).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE }), ctx)).toBe(1);
  });

  // PAGE reaches the accumulator through `rows`, not the cell, so a cell-only guard
  // would leave this case broken while the pixel one is fixed — and the second call
  // is what shows the remainder was not poisoned by the first.
  it("refuses a page scroll against a non-finite row count", () => {
    const s = new WheelScroller();

    expect(s.consumeWheelEvent(wheel({ deltaY: 1, deltaMode: PAGE }), { ...ctx, rows: NaN })).toBe(0);
    expect(s.consumeWheelEvent(wheel({ deltaY: 1, deltaMode: PAGE }), ctx)).toBe(24);
  });

  // Discriminating control: LINE mode does not read the cell at all, so an
  // unmeasured cell must NOT stop it. An entry-level context guard would fail
  // here — which is why the guard checks the computed amount instead.
  it("still scrolls in line mode when the cell is unmeasured, because it never divides by it", () => {
    const s = new WheelScroller();

    expect(s.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE }), bad)).toBe(3);
  });
});
