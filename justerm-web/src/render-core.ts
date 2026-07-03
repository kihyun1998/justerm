import type { Palette } from "justerm-wasm-decode/colors.js";
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
  /** Whether a selection/match highlight should ALPHA-BLEND over this cell's bg
   * (true) or paint the highlight colour SOLID (false). xterm blends only when the
   * cell has a non-default bg or is inverse — so a coloured cell shows through the
   * selection, but plain text on the default bg gets a crisp solid highlight. */
  blendHighlight: boolean;
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
 * selection/match blend is the overlay layer, `composeCellColors`.) S2 ships
 * {@link identityPolicy}.
 */
export type RenderPolicy = (fg: number, bg: number, flags: number) => { fg: number; bg: number };

/** No-op policy — colours pass through unchanged. The S2 default. */
export const identityPolicy: RenderPolicy = (fg, bg) => ({ fg, bg });

/** `[line, left, right, cell_offset, count]`. */
const SPAN_STRIDE = 5;

/**
 * Walk a {@link DecodedFrame}'s span directory into draw ops — one per painted
 * cell. Pure: no beamterm, no wasm. The adapter feeds the ops to a batch and
 * decides clear-on-full; this just maps cells.
 */
export function frameToDrawOps(
  frame: DecodedFrame,
  palette: Palette,
  F: FlagBits,
  policy: RenderPolicy = identityPolicy,
  boldToBright = false,
): DrawOp[] {
  const ops: DrawOp[] = [];
  const { spans } = frame;
  for (let s = 0; s < spans.length; s += SPAN_STRIDE) {
    const line = spans[s]!;
    const left = spans[s + 1]!;
    const cellOffset = spans[s + 3]!;
    const count = spans[s + 4]!;
    for (let i = 0; i < count; i++) {
      const idx = cellOffset + i;
      const flags = frame.flags[idx]!;
      // Trailing half of a wide glyph — the body cell already covers this column.
      if ((flags & F.wide_char_spacer) !== 0) continue;
      // A nonzero `extra` is a 1-based index into the frame's grapheme clusters;
      // it holds the full combining sequence the single base codepoint can't.
      const extra = frame.extra[idx]!;
      const code = frame.codepoints[idx]!;
      const symbol =
        extra !== 0 ? frame.sideTable[extra - 1]! : code === 0 ? " " : String.fromCodePoint(code);
      ops.push(cellToDrawOp(left + i, line, symbol, frame.fg[idx]!, frame.bg[idx]!, flags, palette, F, policy, boldToBright));
    }
  }
  return ops;
}

/**
 * Map a single resolved cell to a {@link DrawOp}: resolve its colour refs, run
 * the {@link RenderPolicy}, and unpack the style flags. Shared by
 * {@link frameToDrawOps} (frame spans) and the cell mirror (scroll-shifted
 * cells). The caller skips `wide_char_spacer` cells before calling.
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
  const { fg, bg } = policy(resolved.fg, resolved.bg, flags);
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
    // Blend the highlight only for a non-default (Indexed/Rgb) bg or an inverse
    // cell — matches xterm's CellColorResolver branch (else = solid selection).
    blendHighlight: (flags & F.inverse) !== 0 || bgRef >>> 24 !== 0,
  };
}
