import { describe, expect, it } from "vitest";
import { dragToDisplayOffset, rulerMarkHeightPx, scrollbarMetrics } from "../src/scrollbar";
import type { RulerMark } from "../src/decorations";

describe("scrollbarMetrics", () => {
  // Thumb geometry from the frame's scroll position, mirroring xterm Viewport
  // (scrollHeight = total lines, thumb = viewport/total, top = ydisp/total).
  // total = scrollbackLen + rows. At the bottom (displayOffset 0) the thumb sits
  // at the end: 24-row viewport over 100 total → height 0.24, top 0.76.
  it("sizes and positions the thumb from scroll position", () => {
    const m = scrollbarMetrics({ displayOffset: 0, scrollbackLen: 76, rows: 24 });

    expect(m).toEqual({
      visible: true,
      thumbHeightRatio: 0.24, // rows / total
      thumbTopRatio: 0.76, // (scrollbackLen - displayOffset) / total
    });
  });

  it("puts the thumb at the top when fully scrolled up, and hides with no scrollback", () => {
    // displayOffset == scrollbackLen = fully scrolled up → thumb at the top
    expect(scrollbarMetrics({ displayOffset: 76, scrollbackLen: 76, rows: 24 }).thumbTopRatio).toBe(0);
    // no history → nothing to scroll → bar hidden (Auto visibility)
    expect(scrollbarMetrics({ displayOffset: 0, scrollbackLen: 0, rows: 24 }).visible).toBe(false);
  });
});

describe("rulerMarkHeightPx (#500 §2)", () => {
  const GUTTER: RulerMark["position"][] = ["left", "center", "right"];

  // The REASON this function exists, not an incidental value. ADR-0024 R3 partitions the ruler
  // by position class and paints `full` above the gutter classes; its recorded validity
  // condition is that the payoff "is gated on mark geometry" — class-last-wins is only visible
  // once the full mark is THINNER than the gutter one it overlaps. A flat height made R3 decide
  // nothing but which colour showed on an exact overlap. Assert the relation, so a future edit
  // that equalises the two fails here rather than silently voiding R3.
  it("makes a gutter mark strictly taller than a full one, which is what R3's layering needs", () => {
    for (const p of GUTTER) {
      expect(rulerMarkHeightPx(p)).toBeGreaterThan(rulerMarkHeightPx("full"));
    }
  });

  // `full` was already right and stays put: xterm's `drawHeight.full` is `round(2 * dpr)` DEVICE
  // px (`OverviewRulerRenderer.ts:124` @ 699f553), which is 2 CSS px — exactly what this file has
  // always written. §2 is "gutter marks should be taller", one class, not four. Pinned so the
  // slice cannot drift into changing the class that never diverged.
  it("leaves the full class at 2 CSS px", () => {
    expect(rulerMarkHeightPx("full")).toBe(2);
  });

  // All three gutter classes are one class geometrically — xterm gives them a single
  // `nonFullHeight` (`OverviewRulerRenderer.ts:128-131`), and they differ only in x.
  it("gives every gutter class the same height", () => {
    const [h] = GUTTER.map(rulerMarkHeightPx);
    for (const p of GUTTER) expect(rulerMarkHeightPx(p)).toBe(h);
  });

  // `rulerMarkX`'s `switch` renders anything unrecognised full-WIDTH. The two functions must not
  // disagree about which marks are the full-width ones, or a mark would be laid out as `full`
  // horizontally and as a gutter mark vertically. Unreachable from typed code (the union is
  // closed) — the cast is the point, mirroring the same note in `decorations.ts`.
  it("treats an unrecognised position as full, agreeing with rulerMarkX", () => {
    expect(rulerMarkHeightPx("nonsense" as RulerMark["position"])).toBe(rulerMarkHeightPx("full"));
  });
});

describe("dragToDisplayOffset", () => {
  // Dragging the thumb to a track ratio picks the viewport's top line, which maps
  // back to a display offset (clamped to [0, scrollbackLen]). The backend then
  // scrolls there. total = 100; top line = ratio × total; offset = scrollbackLen − topLine.
  it("converts a drag track ratio to a clamped display offset", () => {
    const pos = { displayOffset: 0, scrollbackLen: 76, rows: 24 };
    expect(dragToDisplayOffset(0, pos)).toBe(76); // top → fully scrolled up
    expect(dragToDisplayOffset(1, pos)).toBe(0); // bottom → following the screen
    expect(dragToDisplayOffset(0.5, pos)).toBe(26); // middle: 76 − 50
  });
});
