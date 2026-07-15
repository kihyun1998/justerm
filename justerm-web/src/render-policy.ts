import { FG, resolveRgb } from "justerm-wasm-decode/colors.js";
import type { Palette } from "justerm-wasm-decode/colors.js";
import type { FlagBits } from "./render-core";

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
