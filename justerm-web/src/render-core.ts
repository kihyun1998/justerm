import type { Palette } from "justerm-wasm-decode/colors.js";
import { treatGlyphAsBackgroundColor } from "./glyph-class";
import { resolveCell } from "./render-policy";
import type { DecodedFrame } from "./types";

/** One cell to paint, in viewport coords. Colours are packed `0xRRGGBB`. */
export interface DrawOp {
  x: number;
  y: number;
  symbol: string;
  fg: number;
  bg: number;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikethrough: boolean;
}

/** Flag bit positions, from the decoder's `flags()`. Structural for testability. */
export interface FlagBits {
  bold: number;
  italic: number;
  underline: number;
  strikethrough: number;
  wide_char_spacer: number;
  inverse: number;
  dim: number;
  hidden: number;
}

/**
 * Stage-2 (RGB-space) colour policy: maps a cell's already-resolved `(fg, bg)`
 * plus its raw `flags` to final colours. Runs after {@link resolveCell} (stage-1:
 * inverse). This is the seam #115 fills with the RGB-space parts of xterm's
 * colour pipeline — `dim` and `minimumContrastRatio`. (Inverse is stage-1; the
 * selection/match blend is now the renderer's, composited in wasm.) The accessible
 * view ships {@link identityPolicy} — it needs the text, not the colours.
 */
export type RenderPolicy = (
  fg: number,
  bg: number,
  flags: number,
  excludeFromContrast?: boolean,
) => { fg: number; bg: number };

/** No-op policy — colours pass through unchanged. The S2 default. */
export const identityPolicy: RenderPolicy = (fg, bg) => ({ fg, bg });

/**
 * Map a single resolved cell to a {@link DrawOp}: resolve its colour refs, run
 * the {@link RenderPolicy}, and unpack the style flags. Used by the cell mirror
 * (ADR-0011) to decode scroll-shifted cells for the accessible view. The caller
 * skips `wide_char_spacer` cells before calling.
 */
export function cellToDrawOp(
  x: number,
  y: number,
  symbol: string,
  fgRef: number,
  bgRef: number,
  flags: number,
  palette: Palette,
  F: FlagBits,
  policy: RenderPolicy = identityPolicy,
  boldToBright = false,
): DrawOp {
  // Stage-1: resolve refs to RGB, applying inverse + bold→bright (#223). Stage-2:
  // the RGB-space RenderPolicy (dim, minimumContrastRatio).
  const resolved = resolveCell(fgRef, bgRef, flags, palette, F, boldToBright);
  // #226: a Powerline/box glyph tiles with the bg, so its fg is excluded from the
  // minimumContrastRatio correction (computed from the real glyph, not the HIDDEN
  // substitution below). Both ranges are BMP → the first code point suffices.
  // The tile-glyph classification (#226) feeds the POLICY seam, not a field: a
  // stage-2 policy may exclude a tiling glyph from its contrast correction.
  const excludeFromContrast = symbol.length > 0 && treatGlyphAsBackgroundColor(symbol.codePointAt(0)!);
  const { fg, bg } = policy(resolved.fg, resolved.bg, flags, excludeFromContrast);
  return {
    x,
    y,
    // HIDDEN (SGR 8 conceal): draw no glyph (xterm's NULL glyph). A blank has no
    // ink, so only the bg shows — and it stays invisible when an overlay later
    // recolours the bg (unlike collapsing fg→bg, which an overlay would undo).
    symbol: (flags & F.hidden) !== 0 ? " " : symbol,
    fg,
    bg,
    bold: (flags & F.bold) !== 0,
    italic: (flags & F.italic) !== 0,
    underline: (flags & F.underline) !== 0,
    strikethrough: (flags & F.strikethrough) !== 0,
  };
}
