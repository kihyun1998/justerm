import { BG, FG, resolveRgb } from "justerm-wasm-decode/colors.js";
import type { Palette } from "justerm-wasm-decode/colors.js";
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
}

/**
 * Post-resolve colour policy: maps a cell's resolved `(fg, bg)` plus its raw
 * `flags` to final colours. This is the seam #115 fills with xterm's
 * CellColorResolver policy (inverse colour-mode swap, selection blend, dim,
 * minimumContrastRatio). S2 ships {@link identityPolicy}.
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
      const { fg, bg } = policy(
        resolveRgb(frame.fg[idx]!, palette, FG),
        resolveRgb(frame.bg[idx]!, palette, BG),
        flags,
      );
      ops.push({
        x: left + i,
        y: line,
        symbol,
        fg,
        bg,
        bold: (flags & F.bold) !== 0,
        italic: (flags & F.italic) !== 0,
        underline: (flags & F.underline) !== 0,
        strikethrough: (flags & F.strikethrough) !== 0,
      });
    }
  }
  return ops;
}
