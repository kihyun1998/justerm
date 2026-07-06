import { describe, expect, it } from "vitest";
import type { DecorationRect } from "../src/decorations";
import { composeCellColors, decorationAt } from "../src/decoration-render";
import { ensureContrastRatio } from "../src/contrast";
import { HIGHLIGHT_BLEND_ALPHA, blendOver, dimForeground } from "../src/render-policy";

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

  // #230: a decoration fg override KEEPS the cell's DIM on a non-selected cell —
  // xterm leaves BgFlags.DIM set (CellColorResolver else-branch), so the atlas
  // `multiplyOpacity(DIM_OPACITY)`s the override fg. justerm bakes the same:
  // dimForeground(overrideFg, effectiveBg). A bottom decoration's 0x00ff00 over a
  // black cell dims to 0x008000, not the full-opacity 0x00ff00.
  it("dims a bottom decoration fg override on a non-selected dim cell (#230)", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg } = composeCellColors(dimBase, bottom, null, null, false, false, dimBase.fg, 1, true);
    expect(fg).toBe(dimForeground(0x00ff00, 0x000000)); // 0x008000, not 0x00ff00
  });

  // A TOP decoration fg override is dimmed the same way, against the final bg.
  it("dims a top decoration fg override on a non-selected dim cell", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const top = rect(0, 0, 0, "top", { fg: 0x00ff00 });
    const { fg } = composeCellColors(dimBase, null, null, top, false, false, dimBase.fg, 1, true);
    expect(fg).toBe(dimForeground(0x00ff00, 0x000000));
  });

  // The dim is baked against the EFFECTIVE bg (after a highlight changed it), like
  // xterm's multiplyOpacity composites the override fg over the drawn bg.
  it("dims a decoration fg against the effective (highlight) bg", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg, bg } = composeCellColors(dimBase, bottom, 0xffffff, null, false, false, dimBase.fg, 1, true);
    expect(bg).toBe(0xffffff); // solid match highlight
    expect(fg).toBe(dimForeground(0x00ff00, 0xffffff)); // dimmed toward white
  });

  // Selection un-dims (xterm clears DIM), so a decoration fg under a selection stays
  // FULL opacity — the `dim && !isSelection` gate must exclude selection.
  it("does not dim a decoration fg override under a selection (DIM cleared)", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg } = composeCellColors(dimBase, bottom, null, null, false, true, 0xffffff, 1, true);
    expect(fg).toBe(0x00ff00); // selection → full-opacity decoration fg
  });

  // Control: a NON-dim cell's decoration fg is full opacity (nothing to dim).
  it("leaves a decoration fg at full opacity on a non-dim cell", () => {
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg } = composeCellColors(base, bottom, null, null, false, false, base.fg, 1, false);
    expect(fg).toBe(0x00ff00);
  });

  // Guard against double-dim: the base fg is ALREADY dimmed by stage-2, so with no
  // decoration override it must pass through unchanged (only an override is re-dimmed).
  it("does not re-dim the already-dimmed base fg when no decoration overrides it", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 }; // stage-2 dimmed
    const { fg } = composeCellColors(dimBase, null, null, null, false, false, dimBase.fg, 1, true);
    expect(fg).toBe(0x808080); // unchanged
  });

  // #230 × #232 order lock (adversarial coverage gap): a dim decoration fg override
  // with mcr>1 is DIMMED first, then the halved-ratio contrast pass runs on the
  // dimmed colour (justerm's double-pass, mirroring how a base dim fg is treated).
  // Pinning the composition catches a regression that skips the dim, uses the full
  // ratio, or reverses the order.
  it("dims a decoration fg override THEN halves contrast on it (mcr>1)", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg } = composeCellColors(dimBase, bottom, null, null, false, false, dimBase.fg, 21, true);
    const dimmed = dimForeground(0x00ff00, 0x000000);
    const expected = ensureContrastRatio(0x000000, dimmed, 21 / 2) ?? dimmed;
    expect(fg).toBe(expected);
  });

  // #230 × #115 (adversarial coverage gap): the effective bg a decoration fg is
  // dimmed against can itself be a BLENDED highlight bg, not just a solid one.
  it("dims a decoration fg against a blended highlight bg", () => {
    const dimBase = { fg: 0x808080, bg: 0x000000 };
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    // blendHighlight=true → bg = blendOver(0x000000, 0xffffff, 0x80) = 0x808080.
    const { fg, bg } = composeCellColors(dimBase, bottom, 0xffffff, null, true, false, dimBase.fg, 1, true);
    expect(bg).toBe(0x808080);
    expect(fg).toBe(dimForeground(0x00ff00, 0x808080)); // dimmed toward the blended bg
  });

  // #226: a Powerline/box glyph is excluded from the OVERLAY contrast pass too. The
  // "re-applies contrast against the effective bg" case above darkens a white fg to
  // black over a white highlight; excluded (last arg true) it is LEFT white so the
  // glyph keeps tiling with the neighbour instead of seaming.
  it("skips the overlay contrast correction for an excluded glyph", () => {
    const onBlack = { fg: 0xffffff, bg: 0x000000 };
    const { fg } = composeCellColors(onBlack, null, 0xffffff, null, false, false, 0xffffff, 21, false, true);
    expect(fg).toBe(0xffffff); // excluded → not darkened for the white highlight
  });

  // #227: an explicit selectionForeground (last arg) overrides the fg on a SELECTED
  // cell — xterm forces the text colour under a selection. Focus-independent, and only
  // for a selection.
  it("overrides the fg with selectionForeground on a selected cell", () => {
    const { fg } = composeCellColors(base, null, 0x445566, null, false, true, base.fg, 1, false, false, 0x00ff00);
    expect(fg).toBe(0x00ff00);
  });

  // A search MATCH is not a selection, so selectionForeground does not apply (xterm
  // sets it only in the $isSelected branch).
  it("does not apply selectionForeground to a search match (only a selection)", () => {
    const { fg } = composeCellColors(base, null, 0x445566, null, false, false, base.fg, 1, false, false, 0x00ff00);
    expect(fg).toBe(base.fg); // match → cell's own fg
  });

  // Option off (undefined, the default): the selected cell keeps its own (undimmed) fg.
  it("keeps the cell fg when selectionForeground is unset", () => {
    const { fg } = composeCellColors(base, null, 0x445566, null, false, true, base.fg);
    expect(fg).toBe(base.fg);
  });

  // A TOP decoration fg wins over selectionForeground (xterm applies the top layer
  // after the selection block).
  it("lets a top decoration fg win over selectionForeground", () => {
    const top = rect(0, 0, 0, "top", { fg: 0x0000ee });
    const { fg } = composeCellColors(base, null, 0x445566, top, false, true, base.fg, 1, false, false, 0x00ff00);
    expect(fg).toBe(0x0000ee); // top decoration wins
  });

  // selectionForeground supersedes a BOTTOM decoration fg and the #224 un-dim (the
  // selection block runs after the bottom layer in xterm).
  it("overrides a bottom decoration fg with selectionForeground", () => {
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg } = composeCellColors(base, bottom, 0x445566, null, false, true, base.fg, 1, false, false, 0xff0000);
    expect(fg).toBe(0xff0000); // selectionForeground wins over the bottom decoration
  });

  // selectionForeground is still subject to the contrast pass (xterm resolves it then
  // runs minimumContrastRatio): a white selectionForeground over a white selection bg
  // at ratio 21 is darkened to black.
  it("still contrast-corrects a selectionForeground that is illegible on the selection bg", () => {
    const { fg } = composeCellColors(base, null, 0xffffff, null, false, true, base.fg, 21, false, false, 0xffffff);
    expect(fg).toBe(0x000000);
  });

  // Black (0x000000) is a valid selectionForeground — the `!== undefined` guard must
  // admit it, not treat the falsy 0 as "unset".
  it("applies a black (0x000000) selectionForeground", () => {
    const { fg } = composeCellColors(base, null, 0x445566, null, false, true, base.fg, 1, false, false, 0x000000);
    expect(fg).toBe(0x000000);
  });

  // #239: a Powerline/box glyph under a SELECTION tiles with the highlight — its fg is
  // blended 50% toward the selection bg (from the cell's undimmed fg). Reuses the #226
  // excludeFromContrast signal (10th arg). White fg over a black selection → mid-grey.
  it("blends a powerline/box glyph's fg toward the selection bg", () => {
    const white = { fg: 0xffffff, bg: 0x111111 };
    const { fg } = composeCellColors(white, null, 0x000000, null, false, true, 0xffffff, 1, false, true);
    expect(fg).toBe(blendOver(0xffffff, 0x000000, HIGHLIGHT_BLEND_ALPHA)); // 0x7f7f7f
  });

  // The recolor overrides selectionForeground (xterm re-resolves the cell fg here,
  // discarding the explicit override for a background-tile glyph).
  it("overrides selectionForeground for a powerline glyph under selection", () => {
    const white = { fg: 0xffffff, bg: 0x111111 };
    const { fg } = composeCellColors(white, null, 0x000000, null, false, true, 0xffffff, 1, false, true, 0x00ff00);
    expect(fg).toBe(blendOver(0xffffff, 0x000000, HIGHLIGHT_BLEND_ALPHA)); // not 0x00ff00
  });

  // Not a selection (a search match) → no recolor, even for a powerline glyph.
  it("does not recolor a powerline glyph on a search match (selection only)", () => {
    const { fg } = composeCellColors(base, null, 0x000000, null, false, false, base.fg, 1, false, true);
    expect(fg).toBe(base.fg);
  });

  // A non-tile glyph under selection is untouched by the recolor (excludeFromContrast
  // false → keeps its own fg / the #227 override path).
  it("does not recolor an ordinary glyph under selection", () => {
    const white = { fg: 0xffffff, bg: 0x111111 };
    const { fg } = composeCellColors(white, null, 0x000000, null, false, true, 0xffffff, 1, false, false);
    expect(fg).toBe(0xffffff); // no blend
  });

  // A top decoration fg still wins over the recolor (xterm applies the top layer last).
  it("lets a top decoration fg win over the powerline recolor", () => {
    const white = { fg: 0xffffff, bg: 0x111111 };
    const top = rect(0, 0, 0, "top", { fg: 0x0000ee });
    const { fg } = composeCellColors(white, null, 0x000000, top, false, true, 0xffffff, 1, false, true);
    expect(fg).toBe(0x0000ee);
  });

  // The recolor blends toward the RAW selection colour (highlightBg), NOT the effective
  // post-blend bg — so a blendHighlight cell (its own bg shows through) still fuses
  // toward the one shared selection colour (xterm blends fg → raw selectionBg, unlike
  // the contrast pass which uses the effective bg). Here the effective bg (0x404040)
  // differs from the raw highlightBg (0x000000); the fg blends toward the raw colour.
  it("blends a powerline glyph toward the raw selection colour on a blendHighlight cell", () => {
    const white = { fg: 0xffffff, bg: 0x808080 };
    const { fg, bg } = composeCellColors(white, null, 0x000000, null, true, true, 0xffffff, 1, false, true);
    expect(bg).toBe(blendOver(0x808080, 0x000000, HIGHLIGHT_BLEND_ALPHA)); // effective bg = 0x404040
    expect(fg).toBe(blendOver(0xffffff, 0x000000, HIGHLIGHT_BLEND_ALPHA)); // fg → raw 0x000000, not 0x404040
  });

  // The recolor re-resolves the cell's OWN fg (fgUndimmed), so a bottom decoration's fg
  // override is discarded for a tile glyph under selection — matching xterm, which
  // re-reads the model fg here (not the decoration's $fg).
  it("ignores a bottom decoration fg when recoloring a powerline glyph under selection", () => {
    const white = { fg: 0xffffff, bg: 0x111111 };
    const bottom = rect(0, 0, 0, "bottom", { fg: 0x00ff00 });
    const { fg } = composeCellColors(white, bottom, 0x000000, null, false, true, 0xffffff, 1, false, true);
    expect(fg).toBe(blendOver(0xffffff, 0x000000, HIGHLIGHT_BLEND_ALPHA)); // from fgUndimmed, not 0x00ff00
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
