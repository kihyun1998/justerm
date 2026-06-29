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
  // DOM_DELTA_LINE: amount = deltaY × scrollSensitivity (default 1). xterm
  // CoreMouseService.consumeWheelEvent — the LINE branch returns the modified
  // amount as-is. Sign follows deltaY (positive = scroll down/newer).
  it("returns line-mode deltaY directly at default sensitivity", () => {
    const s = new WheelScroller();

    const lines = s.consumeWheelEvent(wheel({ deltaY: 3, deltaMode: LINE }), ctx);

    expect(lines).toBe(3);
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
