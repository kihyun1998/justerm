import { describe, expect, it } from "vitest";
import type { DecorationRect } from "../src/decorations";
import { composeCellColors, decorationAt } from "../src/decoration-render";

/** A decoration rect on one viewport row, columns `left..=right` inclusive. */
function rect(
  row: number,
  left: number,
  right: number,
  layer: "bottom" | "top",
  extra: { bg?: number; fg?: number } = {},
): DecorationRect {
  return { row, left, right, layer, ...extra };
}

describe("decorationAt — decoration query at a cell (#120 S2)", () => {
  // Mirrors overlay.ts `highlightAt`: a decoration covers a cell when the row
  // matches and the column is within its inclusive `left..=right` span.
  it("returns the decoration covering the cell at the given layer", () => {
    const rects = [rect(2, 1, 4, "bottom", { bg: 0x111 })];
    expect(decorationAt(rects, 3, 2, "bottom")).toEqual(rects[0]);
  });

  it("returns null when no decoration covers the cell", () => {
    const rects = [rect(2, 1, 4, "bottom")];
    expect(decorationAt(rects, 5, 2, "bottom")).toBeNull(); // col past right
    expect(decorationAt(rects, 3, 0, "bottom")).toBeNull(); // wrong row
  });

  // Layer is part of the query: a `bottom` decoration is invisible to a `top`
  // lookup and vice-versa (the renderer queries each layer separately).
  it("filters by layer", () => {
    const rects = [rect(0, 0, 2, "bottom")];
    expect(decorationAt(rects, 1, 0, "top")).toBeNull();
    expect(decorationAt(rects, 1, 0, "bottom")).toEqual(rects[0]);
  });

  // When several decorations at the same layer overlap a cell, the last one wins
  // (later registration paints on top — xterm's draw order).
  it("last overlapping decoration at a layer wins", () => {
    const first = rect(0, 0, 5, "bottom", { bg: 0x111 });
    const second = rect(0, 2, 3, "bottom", { bg: 0x222 });
    expect(decorationAt([first, second], 3, 0, "bottom")).toEqual(second);
  });
});

describe("composeCellColors — layered cell colour (#120 S2)", () => {
  const base = { fg: 0xaaaaaa, bg: 0x111111 };

  // Back-to-front precedence: base < bottom-decoration < selection/match
  // highlight < top-decoration. Each layer overrides bg (and fg if it sets one).

  it("returns the base unchanged with no overlays", () => {
    expect(composeCellColors(base, null, null, null)).toEqual(base);
  });

  // #115: the selection/search highlight is an alpha blend over the cell's own
  // background (xterm CellColorResolver blends the selection colour at alpha
  // 0x80), NOT a solid fill — so a coloured cell shows through the highlight.
  // Independent check: white at ~50% over black is mid-grey.
  it("blends the highlight bg over the cell bg (alpha 0x80), not a solid fill", () => {
    const onBlack = { fg: 0xaaaaaa, bg: 0x000000 };
    expect(composeCellColors(onBlack, null, 0xffffff, null).bg).toBe(0x808080);
  });

  // A bottom decoration overrides the cell background beneath the glyph; the
  // glyph colour (fg) is unchanged, so text stays legible (AC: bottom = bg under text).
  it("bottom decoration overrides bg, leaving fg (glyph) legible", () => {
    const bottom = rect(0, 0, 0, "bottom", { bg: 0xbb0000 });
    expect(composeCellColors(base, bottom, null, null)).toEqual({ fg: 0xaaaaaa, bg: 0xbb0000 });
  });

  it("a decoration that sets only fg recolours the glyph, leaving bg", () => {
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    expect(composeCellColors(base, bottom, null, null)).toEqual({ fg: 0x00ff00, bg: 0x111111 });
  });

  // The selection/match highlight sits ABOVE a bottom decoration: an active
  // selection over a decorated line blends its colour over the decoration's bg
  // (so the decoration shows through, #115), while the decoration's fg override
  // still applies underneath. Expected bg = blendOver(0xbb0000, 0x445566, 0x80).
  it("blends the highlight over a bottom decoration's bg, keeping its fg", () => {
    const bottom = rect(0, 0, 0, "bottom", { bg: 0xbb0000, fg: 0x00ff00 });
    expect(composeCellColors(base, bottom, 0x445566, null)).toEqual({ fg: 0x00ff00, bg: 0x7f2b33 });
  });

  // A top decoration is foreground-most — it wins over the selection/match
  // highlight (AC: top paints over the cell).
  it("top decoration wins over the highlight bg", () => {
    const top = rect(0, 0, 0, "top", { bg: 0x0000ee });
    expect(composeCellColors(base, null, 0x445566, top)).toEqual({ fg: 0xaaaaaa, bg: 0x0000ee });
  });

  // Full stack: bottom bg, then highlight over it, then top over that. Top's bg
  // wins; fg falls back to the bottom decoration's fg when top sets none.
  it("layers bottom → highlight → top in order", () => {
    const bottom = rect(0, 0, 0, "bottom", { bg: 0xbb0000, fg: 0x00ff00 });
    const top = rect(0, 0, 0, "top", { bg: 0x0000ee });
    expect(composeCellColors(base, bottom, 0x445566, top)).toEqual({ fg: 0x00ff00, bg: 0x0000ee });
  });
});
