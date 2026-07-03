/**
 * Rendering decorations (#120 S2): the per-cell query and colour composition that
 * turn {@link DecorationRect}s (from {@link DecorationRegistry}) into a cell's
 * final `(fg, bg)`. beamterm has no overlay layer, so — like the selection blend
 * and the cursor's cell-invert — a decoration is a per-cell colour override, and
 * the two layers (`bottom`/`top`) are expressed as *precedence* around the
 * selection/match highlight rather than true z-stacked draws.
 *
 * Pure logic (no beamterm, no DOM): the renderer resolves the highlight kind to a
 * theme colour and calls {@link composeCellColors}; the layering lives here so it
 * is unit-tested (the GL adapter is not).
 */

import { ensureContrastRatio } from "./contrast";
import type { DecorationLayer, DecorationRect } from "./decorations";
import { HIGHLIGHT_BLEND_ALPHA, blendOver } from "./render-policy";

/**
 * The decoration covering viewport cell `(col, row)` on `layer`, or `null`. Columns
 * are inclusive (`left..=right`), mirroring `overlay.ts` `highlightAt`. When several
 * decorations at the same layer overlap the cell, the LAST wins (later registration
 * paints on top — xterm's draw order).
 */
export function decorationAt(
  rects: DecorationRect[],
  col: number,
  row: number,
  layer: DecorationLayer,
): DecorationRect | null {
  let hit: DecorationRect | null = null;
  for (const r of rects) {
    if (r.layer === layer && r.row === row && col >= r.left && col <= r.right) hit = r;
  }
  return hit;
}

/**
 * A cell's final `(fg, bg)` after layering, back-to-front: base < bottom
 * decoration < selection/match highlight < top decoration. Each decoration
 * overrides `bg` and/or `fg` only where it sets one — so a `bottom` decoration
 * tints the background under a legible glyph, while `highlightBg` (the resolved
 * selection/match colour, or `null`) is alpha-blended over it (#115, so the cell
 * colour shows through), and a `top` decoration is foreground-most and wins over
 * the highlight.
 *
 * This order follows xterm's DOCUMENTED contract (`xterm.d.ts`: "'bottom' will
 * render under the selection, 'top' will render above the selection"). Note it
 * diverges from xterm's *actual* `DomRendererRowFactory` implementation, which
 * SKIPS the selection entirely on a cell that has a top decoration — silently
 * suppressing the selection. justerm keeps the selection always visible (spec-
 * faithful, and arguably better UX); a consumer porting code that relied on a top
 * decoration hiding the selection would see the selection here instead.
 */
export function composeCellColors(
  base: { fg: number; bg: number },
  bottom: DecorationRect | null,
  highlightBg: number | null,
  top: DecorationRect | null,
  blendHighlight = false,
  isSelection = false,
  fgUndimmed: number = base.fg,
  minimumContrastRatio = 1,
): { fg: number; bg: number } {
  // #224: a selected cell is un-dimmed (xterm force-clears DIM under selection), so
  // it starts from the undimmed fg. Only selection un-dims (not a search match); a
  // bottom/top decoration fg override below still wins.
  let fg = isSelection ? fgUndimmed : base.fg;
  let { bg } = base;
  if (bottom) {
    if (bottom.bg !== undefined) bg = bottom.bg;
    if (bottom.fg !== undefined) fg = bottom.fg;
  }
  // #115: apply the highlight over the running bg (base or a bottom decoration).
  // `blendHighlight` cells (non-default/inverse bg) alpha-blend so the cell colour
  // shows through; the rest paint the highlight SOLID (xterm's default-bg branch).
  // A top decoration still overrides it below either way.
  if (highlightBg !== null) {
    bg = blendHighlight ? blendOver(bg, highlightBg, HIGHLIGHT_BLEND_ALPHA) : highlightBg;
  }
  if (top) {
    if (top.bg !== undefined) bg = top.bg;
    if (top.fg !== undefined) fg = top.fg;
  }
  // #225: minimumContrastRatio is applied against the EFFECTIVE bg (the one the
  // glyph is actually drawn over, after the highlight/decoration changed it) — not
  // the cell's own bg the stage-2 policy saw. xterm bakes the selection bg in
  // before computing contrast, so a fg made legible on the cell bg is re-corrected
  // for the highlight bg here.
  if (minimumContrastRatio > 1) {
    const adjusted = ensureContrastRatio(bg, fg, minimumContrastRatio);
    if (adjusted !== undefined) fg = adjusted;
  }
  return { fg, bg };
}
