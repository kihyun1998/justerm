import { FG, resolveRgb } from "justerm-wasm-decode/colors.js";
import type { Palette } from "justerm-wasm-decode/colors.js";
import { ensureContrastRatio } from "./contrast";
import type { FlagBits, RenderPolicy } from "./render-core";

/**
 * Resolve one drawn slot's ref to packed `0xRRGGBB`. A `Default` ref's fg/bg
 * meaning flips under inverse (xterm: an inverse Default fg draws as theme bg and
 * vice versa), so it resolves against defaultFg iff the slot is a foreground XOR
 * inverse. Indexed/Rgb are role- and inverse-independent (via {@link resolveRgb}).
 */
function resolveSlot(ref: number, palette: Palette, isForeground: boolean, inverse: boolean): number {
  if (ref >>> 24 === 0) return isForeground !== inverse ? palette.defaultFg : palette.defaultBg;
  return resolveRgb(ref, palette, FG); // FG role is ignored for Indexed/Rgb
}

/**
 * Stage-1 of the #115 render policy: resolve a cell's colour refs to packed
 * `0xRRGGBB`, applying the ref-space transforms — inverse and bold→bright (#223).
 * The RGB-space policy (dim, minimumContrastRatio, selection blend) runs after,
 * in stage-2. Refs (not resolved RGB) come in so bright can remap an index.
 */
export function resolveCell(
  fgRef: number,
  bgRef: number,
  flags: number,
  palette: Palette,
  F: FlagBits,
  boldToBright = false,
): { fg: number; bg: number } {
  const inverse = (flags & F.inverse) !== 0;
  // Inverse swaps the slots in REF space (so bright below sees the drawn fg's ref);
  // resolveSlot then flips Default's meaning by the inverse flag.
  let drawnFg = inverse ? bgRef : fgRef;
  const drawnBg = inverse ? fgRef : bgRef;
  // #223 bold→bright: a bold ANSI 0-7 Indexed foreground becomes its 8-15 bright
  // variant. Applied to the POST-swap fg ref, so under bold+inverse it brightens
  // the original bg index — xterm couples the (mode,index) swap with the +8.
  if (boldToBright && (flags & F.bold) !== 0 && drawnFg >>> 24 === 1 && (drawnFg & 0xff) < 8) {
    drawnFg += 8;
  }
  return {
    fg: resolveSlot(drawnFg, palette, true, inverse),
    bg: resolveSlot(drawnBg, palette, false, inverse),
  };
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

/**
 * Alpha of the DIM blend. xterm's `multiplyOpacity(fg, DIM_OPACITY=0.5)` sets the
 * fg's alpha byte to `round(0.5 * 255) = 128`, then source-over-composites it over
 * the bg — so the effective blend fraction is `128/255`, NOT an exact 0.5.
 */
export const DIM_BLEND_ALPHA = 0x80;

/**
 * DIM (xterm `BgFlags.DIM`): fade the foreground toward the background. beamterm
 * has no per-glyph alpha, so the dim is baked into the fg RGB — the same
 * {@link blendOver} composite xterm performs (fg at alpha 128 over the cell bg).
 */
export function dimForeground(fg: number, bg: number): number {
  return blendOver(bg, fg, DIM_BLEND_ALPHA);
}

/**
 * Build the stage-2 (RGB-space) {@link RenderPolicy} from the decoder's flag bits
 * and the consumer's `minimumContrastRatio` (1 = off, the default). Runs after
 * {@link resolveCell}. The `F` bit positions are captured so the returned policy
 * can decode `flags` without re-passing them.
 *
 * Order follows xterm's `_getForegroundColor`: minimumContrastRatio is checked
 * FIRST and, if it fires, its adjusted fg is returned and dim is skipped (the two
 * are mutually exclusive on the fg). A dim cell needs only half the ratio.
 */
export function makeRenderPolicy(F: FlagBits, minimumContrastRatio = 1): RenderPolicy {
  return (fg, bg, flags) => {
    const dim = (flags & F.dim) !== 0;
    if (minimumContrastRatio > 1) {
      const adjusted = ensureContrastRatio(bg, fg, dim ? minimumContrastRatio / 2 : minimumContrastRatio);
      if (adjusted !== undefined) return { fg: adjusted, bg };
    }
    if (dim) fg = dimForeground(fg, bg);
    return { fg, bg };
  };
}
