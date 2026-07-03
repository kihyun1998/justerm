import { BG, FG, resolveRgb } from "justerm-wasm-decode/colors.js";
import type { Palette } from "justerm-wasm-decode/colors.js";
import type { FlagBits } from "./render-core";

/**
 * Stage-1 of the #115 render policy: resolve a cell's colour refs to packed
 * `0xRRGGBB`, applying the ref-space transforms (inverse; bold→bright is #223).
 * The RGB-space policy (dim, minimumContrastRatio, selection blend) runs after,
 * in stage-2. Refs (not resolved RGB) come in so #223 can bright-map an index.
 */
export function resolveCell(
  fgRef: number,
  bgRef: number,
  flags: number,
  palette: Palette,
  F: FlagBits,
): { fg: number; bg: number } {
  const fg = resolveRgb(fgRef, palette, FG);
  const bg = resolveRgb(bgRef, palette, BG);
  // Inverse: exchange the two role-resolved colours. For justerm this is exactly
  // xterm's colour-mode-aware swap — a Default fg resolves in the FG role and a
  // Default bg in the BG role, so swapping the results matches xterm's rule that
  // an inverse Default fg draws as theme bg and vice versa.
  if ((flags & F.inverse) !== 0) return { fg: bg, bg: fg };
  return { fg, bg };
}

/**
 * Alpha of the selection/search highlight blend, matching xterm's
 * CellColorResolver (it forces the selection colour to `0x80` before blending).
 */
export const HIGHLIGHT_BLEND_ALPHA = 0x80;

/**
 * Composite `over` (packed `0xRRGGBB`) onto `base` at `alpha` (0..255), per
 * channel: `out = base + round((over - base) * alpha/255)`. This is xterm's
 * `rgba.blend` channel math (common/Color.ts) on 24-bit colours — the alpha lives
 * in the call, not the colour. Used for the selection/match highlight tint so a
 * coloured cell background shows through, rather than a solid fill.
 */
export function blendOver(base: number, over: number, alpha: number): number {
  const a = alpha / 0xff;
  const br = (base >> 16) & 0xff;
  const bg = (base >> 8) & 0xff;
  const bb = base & 0xff;
  const or = (over >> 16) & 0xff;
  const og = (over >> 8) & 0xff;
  const ob = over & 0xff;
  const r = br + Math.round((or - br) * a);
  const g = bg + Math.round((og - bg) * a);
  const b = bb + Math.round((ob - bb) * a);
  return (r << 16) | (g << 8) | b;
}
