import { describe, expect, it } from "vitest";
import { treatGlyphAsBackgroundColor } from "../src/glyph-class";

// #226: xterm's treatGlyphAsBackgroundColor — the glyphs excluded from the
// minimumContrastRatio correction because a nudge would seam against neighbours.
// Ported from RendererUtils: powerline U+E0A4..U+E0D6, box/block U+2500..U+259F.
describe("treatGlyphAsBackgroundColor (#226)", () => {
  it("matches the powerline range U+E0A4..U+E0D6 (inclusive)", () => {
    expect(treatGlyphAsBackgroundColor(0xe0a4)).toBe(true); // first
    expect(treatGlyphAsBackgroundColor(0xe0b0)).toBe(true); // a common separator
    expect(treatGlyphAsBackgroundColor(0xe0d6)).toBe(true); // last
    expect(treatGlyphAsBackgroundColor(0xe0a3)).toBe(false); // just below
    expect(treatGlyphAsBackgroundColor(0xe0d7)).toBe(false); // just above
  });

  it("matches the box-drawing / block range U+2500..U+259F (inclusive)", () => {
    expect(treatGlyphAsBackgroundColor(0x2500)).toBe(true); // ─ first (box-drawing)
    expect(treatGlyphAsBackgroundColor(0x2588)).toBe(true); // █ full block
    expect(treatGlyphAsBackgroundColor(0x259f)).toBe(true); // last (block)
    expect(treatGlyphAsBackgroundColor(0x24ff)).toBe(false); // just below
    expect(treatGlyphAsBackgroundColor(0x25a0)).toBe(false); // just above
  });

  it("returns false for ordinary text glyphs", () => {
    expect(treatGlyphAsBackgroundColor(0x20)).toBe(false); // space
    expect(treatGlyphAsBackgroundColor("A".codePointAt(0)!)).toBe(false);
    expect(treatGlyphAsBackgroundColor("한".codePointAt(0)!)).toBe(false);
  });
});
