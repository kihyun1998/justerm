/**
 * Compositing the per-frame draw ops with overlay tints, including the #140
 * partial-frame delta. beamterm has no overlay layer, so a selection/search
 * highlight and a marker decoration are per-cell colour overrides (like the cursor's
 * cell-invert). The frame's damage (`ops`) covers cells that changed *content*, but
 * an overlay's *membership* can flip on a cell whose content didn't change — newly
 * selected, or just de-selected — and that cell is then absent from `ops`. On a
 * partial (damage-only) frame its tint would never be added or cleared.
 *
 * So, exactly like the cursor's old+new cell damage (#107, alacritty `last_cursor`),
 * the renderer tracks the set of overlay-tinted cells across frames and repaints the
 * delta from the cell mirror: re-tint cells still/newly covered, restore (un-tint)
 * cells that left. Pure — no beamterm, no DOM; the adapter only feeds the returned
 * {@link DrawOp}s to a batch and carries `overlay` to the next frame.
 */

import type { DecorationRect } from "./decorations";
import { composeCellColors, decorationAt } from "./decoration-render";
import { highlightAt, type HighlightRect } from "./overlay";
import type { DrawOp } from "./render-core";

/** The renderer's resolved blend colours per highlight kind (#115). */
export interface OverlayColors {
  readonly selectionBg: number;
  readonly matchBg: number;
  /** Applied against the EFFECTIVE (post-overlay) bg so a highlight can't leave the
   * fg illegible (#225). 1 = off (the default). */
  readonly minimumContrastRatio: number;
}

/** The set of viewport cell keys (`y * cols + x`) tinted by any overlay this frame —
 * a highlight span or a decoration rect. Columns are clamped to `[0, cols)` (a
 * decoration may extend past an edge or start left of 0); rows are trusted to be
 * on-viewport (core clips them). Tracking this set frame-to-frame is what lets the
 * renderer find the #140 delta. */
export function overlayCellKeys(
  highlights: readonly HighlightRect[],
  decorations: readonly DecorationRect[],
  cols: number,
): Set<number> {
  const keys = new Set<number>();
  const add = (row: number, left: number, right: number): void => {
    if (row < 0) return; // off-viewport-above — defensive; core keeps rows >= 0, and a
    // negative row would otherwise encode a negative key (`row*cols + x`) that breaks the
    // `key % cols` decode. Symmetric with the column clamp below.
    for (let x = Math.max(0, left); x <= right && x < cols; x++) {
      keys.add(row * cols + x);
    }
  };
  for (const h of highlights) add(h.row, h.left, h.right);
  for (const d of decorations) add(d.row, d.left, d.right);
  return keys;
}

/** The cell keys to repaint from the mirror because their overlay membership changed
 * yet the frame's damage doesn't cover them: `(prev ∪ current) \ damaged`, deduped.
 * Repainting each with the CURRENT tint re-tints cells still/newly covered and
 * restores cells that left (their current tint is `null` → plain). */
export function overlayRepaintKeys(
  prev: ReadonlySet<number>,
  current: ReadonlySet<number>,
  damaged: ReadonlySet<number>,
): number[] {
  const out: number[] = [];
  for (const k of prev) if (!damaged.has(k)) out.push(k);
  for (const k of current) if (!damaged.has(k) && !prev.has(k)) out.push(k);
  return out;
}

/** One cell's {@link DrawOp} after compositing the overlay tint at `(x, y)`: the
 * selection/match highlight and the bottom/top decorations, layered back-to-front
 * ({@link composeCellColors}). An untinted cell returns `base` unchanged. Shared by
 * {@link composeOverlayDraws} (per-frame) and {@link cursorCellDraw} (blink-off), so
 * the cursor cell's off-phase tint uses the exact same layering, not a reimpl. */
export function overlayTint(
  base: DrawOp,
  x: number,
  y: number,
  highlights: readonly HighlightRect[],
  decorations: readonly DecorationRect[],
  colors: OverlayColors,
): DrawOp {
  const kind = highlights.length ? highlightAt(highlights as HighlightRect[], x, y) : null;
  const highlightBg = kind ? (kind === "selection" ? colors.selectionBg : colors.matchBg) : null;
  const bottom = decorations.length ? decorationAt(decorations as DecorationRect[], x, y, "bottom") : null;
  const top = decorations.length ? decorationAt(decorations as DecorationRect[], x, y, "top") : null;
  if (highlightBg === null && bottom === null && top === null) return base;
  const { fg, bg } = composeCellColors(
    { fg: base.fg, bg: base.bg },
    bottom,
    highlightBg,
    top,
    base.blendHighlight,
    kind === "selection", // #224: only a selection un-dims (not a search match)
    base.fgUndimmed,
    colors.minimumContrastRatio, // #225: contrast against the effective bg
    base.dim, // #232: halve the ratio for a dim non-selection cell
  );
  return { ...base, fg, bg };
}

/** The {@link DrawOp} for the cursor's OWN cell at a blink phase (#210). Blink **on**:
 * the injected `styled` op (the cursor cell-invert) — visual precedence over any
 * overlay, like the selection block hiding it. Blink **off**: the cursor is not shown,
 * so the cell renders its composited overlay tint ({@link overlayTint}), NOT the bare
 * `base` — else a selected/decorated cursor cell would flash un-tinted each blink gap.
 * Pure so the on/off decision is unit-tested (the blink loop's GL draw is not). */
export function cursorCellDraw(
  base: DrawOp,
  on: boolean,
  styled: DrawOp,
  highlights: readonly HighlightRect[],
  decorations: readonly DecorationRect[],
  colors: OverlayColors,
): DrawOp {
  return on ? styled : overlayTint(base, base.x, base.y, highlights, decorations, colors);
}

/** Every {@link DrawOp} to paint this frame with overlays composited, plus the new
 * overlay-cell set to carry forward. The damaged cells (`ops`) are drawn with their
 * tint; then the #140 delta — cells whose overlay membership changed since
 * `prevOverlay` but that damage doesn't cover — is repainted from `cellAt`. Draws are
 * disjoint (delta = `(prev ∪ current) \ damaged`). The GL adapter feeds `draws` to a
 * batch (after its clear-on-full) and stores `overlay` as the next `prevOverlay`. */
export function composeOverlayDraws(args: {
  ops: readonly DrawOp[];
  highlights: readonly HighlightRect[];
  decorations: readonly DecorationRect[];
  prevOverlay: ReadonlySet<number>;
  cols: number;
  rows: number;
  colors: OverlayColors;
  cellAt: (x: number, y: number) => DrawOp | undefined;
}): { draws: DrawOp[]; overlay: Set<number> } {
  const { ops, highlights, decorations, prevOverlay, cols, rows, colors, cellAt } = args;
  const draws: DrawOp[] = [];
  const damaged = new Set<number>();

  // Damaged cells: draw each, composited with any overlay tint (the existing path).
  for (const op of ops) {
    damaged.add(op.y * cols + op.x);
    draws.push(overlayTint(op, op.x, op.y, highlights, decorations, colors));
  }

  // #140 delta: repaint cells whose overlay membership flipped but that damage
  // doesn't cover — from the mirror, with the CURRENT tint (restore or re-tint).
  const overlay = overlayCellKeys(highlights, decorations, cols);
  for (const key of overlayRepaintKeys(prevOverlay, overlay, damaged)) {
    const x = key % cols;
    const y = (key - x) / cols;
    if (x >= cols || y >= rows) continue; // stale key guard (mirror is rebuilt on resize)
    // `cellAt` skips a wide-char spacer half (undefined) — the lead glyph covers it,
    // so a blank repaint there would clip the wide glyph (matches the ops-loop skip).
    const base = cellAt(x, y);
    if (base) draws.push(overlayTint(base, x, y, highlights, decorations, colors));
  }

  return { draws, overlay };
}
