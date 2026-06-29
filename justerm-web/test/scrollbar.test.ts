import { describe, expect, it } from "vitest";
import { dragToDisplayOffset, scrollbarMetrics } from "../src/scrollbar";

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
