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

  // #115: for a blendHighlight cell (non-default/inverse bg) the highlight is an
  // alpha-0x80 blend over the cell's own bg, so the cell colour shows through.
  // Independent check: white at ~50% over black is mid-grey.
  it("blends the highlight over the cell bg for a blendHighlight cell", () => {
    const onBlack = { fg: 0xaaaaaa, bg: 0x000000 };
    expect(composeCellColors(onBlack, null, 0xffffff, null, true).bg).toBe(0x808080);
  });

  // #115: for a default-bg cell (blendHighlight=false, the default) the highlight
  // is painted SOLID — xterm's default-bg branch paints selectionBackgroundOpaque
  // with no blend, giving a crisp highlight on plain text.
  it("paints the highlight solid for a default-bg cell", () => {
    const onBlack = { fg: 0xaaaaaa, bg: 0x000000 };
    expect(composeCellColors(onBlack, null, 0xffffff, null).bg).toBe(0xffffff);
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
    expect(composeCellColors(base, bottom, 0x445566, null, true)).toEqual({ fg: 0x00ff00, bg: 0x7f2b33 });
  });

  // #224: a SELECTED cell is un-dimmed — xterm force-clears DIM under selection so
  // the text stays legible over the highlight. composeCellColors swaps in the
  // caller's undimmed fg for a selection (not a search match).
  it("un-dims the fg under a selection (uses fgUndimmed)", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 }; // dimmed fg
    const { fg } = composeCellColors(dimBase, null, 0x445566, null, false, true, 0xffffff);
    expect(fg).toBe(0xffffff); // restored to the undimmed fg
  });

  it("keeps the dimmed fg under a search match (only selection un-dims)", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const { fg } = composeCellColors(dimBase, null, 0x445566, null, false, false, 0xffffff);
    expect(fg).toBe(0x808080); // match is not a selection → no un-dim
  });

  // #225: minimumContrastRatio is applied against the EFFECTIVE bg (after the
  // highlight/decoration changes it), not the cell's own bg — matching xterm,
  // which bakes the selection bg in before computing contrast. A white fg over a
  // white (solid) selection is illegible; at ratio 21 it must darken to black.
  it("re-applies contrast against the effective (highlight) bg", () => {
    const onBlack = { fg: 0xffffff, bg: 0x000000 }; // white fg, fine on black
    const { fg, bg } = composeCellColors(onBlack, null, 0xffffff, null, false, false, 0xffffff, 21);
    expect({ fg, bg }).toEqual({ fg: 0x000000, bg: 0xffffff }); // fg darkened for the white bg
  });

  it("does not adjust contrast when minimumContrastRatio is 1 (default)", () => {
    const onBlack = { fg: 0xffffff, bg: 0x000000 };
    const { fg } = composeCellColors(onBlack, null, 0xffffff, null, false, false, 0xffffff, 1);
    expect(fg).toBe(0xffffff); // no contrast pass → white stays
  });

  // #232: a non-selection dim cell (e.g. a search match) KEEPS its DIM flag —
  // xterm clears DIM only under a selection (CellColorResolver `& ~BgFlags.DIM`).
  // So the overlay contrast pass must halve the ratio for it (TextureAtlas
  // `ensureContrastRatio(bg, fg, mcr / (dim ? 2 : 1))`), else a dim glyph is
  // over-corrected to full contrast and loses its dim look. A dimmed grey
  // (0x808080) over a white match bg has ratio ≈3.95: it clears the halved
  // target 3.5 (mcr=7) so it must stay put, where the full 7 would darken it.
  it("halves the contrast ratio for a non-selection dim cell (xterm ratio/2)", () => {
    const dimGrey = { fg: 0x808080, bg: 0x000000 };
    const { fg } = composeCellColors(dimGrey, null, 0xffffff, null, false, false, 0x808080, 7, true);
    expect(fg).toBe(0x808080); // meets mcr/2 → unchanged, stays dim
  });

  // Control (proves the halving is what spared it): the SAME cell without DIM is
  // corrected at the full ratio 7 (3.95 < 7 → darkens).
  it("uses the full ratio for a non-dim cell in the same spot", () => {
    const grey = { fg: 0x808080, bg: 0x000000 };
    const { fg } = composeCellColors(grey, null, 0xffffff, null, false, false, 0x808080, 7, false);
    expect(fg).not.toBe(0x808080); // full ratio corrects (darkens)
  });

  // Never illegible: a dim cell that fails EVEN the halved ratio is still
  // corrected — halving softens the pull, it does not disable it. Near-white
  // (0xeeeeee) on white is ratio ≈1.16, below mcr/2=3.5 → must darken.
  it("still corrects a dim cell that is illegible even at the halved ratio", () => {
    const nearWhiteDim = { fg: 0xeeeeee, bg: 0x000000 };
    const { fg } = composeCellColors(nearWhiteDim, null, 0xffffff, null, false, false, 0xeeeeee, 7, true);
    expect(fg).not.toBe(0xeeeeee);
  });

  // Under a SELECTION the DIM flag is cleared (xterm), so even a dim cell uses the
  // FULL ratio — the `dim && !isSelection` gate must exclude selection. Here the
  // undimmed grey at full 7 is corrected (3.95 < 7).
  it("uses the full ratio under a selection even for a dim cell (DIM cleared)", () => {
    const dimGrey = { fg: 0x808080, bg: 0x000000 };
    const { fg } = composeCellColors(dimGrey, null, 0xffffff, null, false, true, 0x808080, 7, true);
    expect(fg).not.toBe(0x808080); // selection clears DIM → full ratio corrects
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
