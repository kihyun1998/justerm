import { cellToDrawOp, identityPolicy } from "./render-core";
import type { DrawOp, FlagBits, RenderPolicy } from "./render-core";
import type { DecodedFrame } from "./types";
import type { Palette } from "justerm-wasm-decode/colors.js";

/** One stored cell: the resolved glyph + raw colour refs + flag bits. Colours
 * stay as refs so a palette/policy change can re-resolve without re-decoding. */
interface MirrorCell {
  symbol: string;
  fg: number;
  bg: number;
  flags: number;
}

const blank = (): MirrorCell => ({ symbol: " ", fg: 0, bg: 0, flags: 0 });

const SPAN_STRIDE = 5;

/**
 * A viewport-sized local copy of the rendered cells (ADR-0011). Frame mode keeps
 * it so scroll-op damage can be applied — beamterm can neither shift retained
 * cells nor return their styling, so the shifted region is repainted from here.
 *
 * `applyFrame` updates the mirror from a {@link DecodedFrame} and returns the
 * draw ops for the cells that changed; the adapter feeds them to beamterm.
 */
export class CellMirror {
  private readonly cells: MirrorCell[];

  constructor(
    private readonly cols: number,
    private readonly rows: number,
    private palette: Palette,
    private readonly F: FlagBits,
    private policy: RenderPolicy = identityPolicy,
  ) {
    this.cells = Array.from({ length: cols * rows }, blank);
  }

  /**
   * Swap the palette/policy (a theme or minimumContrastRatio change) and return a
   * full repaint of every stored cell under the new colours. Cells are kept as
   * refs (see {@link MirrorCell}), so this re-resolves without a new frame — the
   * adapter clears beamterm and feeds the ops, exactly like a Full frame.
   */
  recolor(palette: Palette, policy: RenderPolicy): DrawOp[] {
    this.palette = palette;
    this.policy = policy;
    return this.repaintAll();
  }

  /** A full repaint of every stored (non-spacer) cell under the CURRENT palette/
   * policy — the base draws for a themeless full redraw (e.g. a focus change that
   * only re-tints the selection overlay). {@link recolor} is this after a swap. */
  repaintAll(): DrawOp[] {
    const ops: DrawOp[] = [];
    for (let i = 0; i < this.cells.length; i++) {
      const cell = this.cells[i]!;
      if ((cell.flags & this.F.wide_char_spacer) !== 0) continue;
      const x = i % this.cols;
      const y = (i - x) / this.cols;
      ops.push(cellToDrawOp(x, y, cell.symbol, cell.fg, cell.bg, cell.flags, this.palette, this.F, this.policy));
    }
    return ops;
  }

  applyFrame(frame: DecodedFrame): DrawOp[] {
    const damaged = new Set<number>();

    // 0. A Full frame is the whole viewport — wipe stale cells before the spans
    // fill it, or content outside the new spans (and any later scroll of it)
    // would resurrect ghosts. The adapter clears beamterm to match.
    if (frame.kind === 0) {
      for (let i = 0; i < this.cells.length; i++) this.cells[i] = blank();
    }

    // 1. Scroll op first (it precedes spans): shift the stored region; the whole
    // region is damaged since every row moved (or was newly exposed).
    if (frame.hasScroll) {
      const top = frame.scrollTop!;
      const bottom = frame.scrollBottom!;
      this.shiftRegion(top, bottom, frame.scrollCount!);
      for (let y = top; y <= bottom; y++) {
        for (let x = 0; x < this.cols; x++) damaged.add(y * this.cols + x);
      }
    }

    // 2. Spans: store each cell and mark it damaged.
    const { spans } = frame;
    for (let s = 0; s < spans.length; s += SPAN_STRIDE) {
      const line = spans[s]!;
      const left = spans[s + 1]!;
      const cellOffset = spans[s + 3]!;
      const count = spans[s + 4]!;
      for (let i = 0; i < count; i++) {
        const idx = cellOffset + i;
        const x = left + i;
        const flags = frame.flags[idx]!;
        const extra = frame.extra[idx]!;
        const code = frame.codepoints[idx]!;
        const symbol =
          extra !== 0 ? frame.sideTable[extra - 1]! : code === 0 ? " " : String.fromCodePoint(code);
        this.cells[line * this.cols + x] = { symbol, fg: frame.fg[idx]!, bg: frame.bg[idx]!, flags };
        damaged.add(line * this.cols + x);
      }
    }

    // 3. Emit damaged cells from the mirror, row-major (index = y·cols + x).
    const ops: DrawOp[] = [];
    for (const i of [...damaged].sort((a, b) => a - b)) {
      const cell = this.cells[i]!;
      if ((cell.flags & this.F.wide_char_spacer) !== 0) continue;
      const x = i % this.cols;
      const y = (i - x) / this.cols;
      ops.push(cellToDrawOp(x, y, cell.symbol, cell.fg, cell.bg, cell.flags, this.palette, this.F, this.policy));
    }
    return ops;
  }

  /** Row `y`'s text for the a11y mirror (#119): the stored symbols left to
   * right, skipping wide-char spacer halves, with trailing blanks trimmed (a
   * screen reader shouldn't read end-of-line padding). */
  rowText(y: number): string {
    return this.rowCells(y).text;
  }

  /** Row `y`'s SR text plus, per UTF-16 unit, its source terminal column (#152) —
   * the bridge from an AT text selection in the row tree back to grid coordinates
   * (xterm's `translateToString` `outColumns`). A wide glyph's char maps to its lead
   * column (the spacer half is skipped); a combining cluster's units all map to their
   * cell's column; trailing *regular*-space padding is trimmed (columns in lockstep;
   * a real trailing NBSP survives — #153 G8). `columns.length === text.length`, so a
   * DOM selection offset indexes straight into `columns`. */
  rowCells(y: number): { text: string; columns: number[] } {
    let text = "";
    const columns: number[] = [];
    for (let x = 0; x < this.cols; x++) {
      const cell = this.cells[y * this.cols + x]!;
      if ((cell.flags & this.F.wide_char_spacer) !== 0) continue; // trailing half
      text += cell.symbol;
      // One column per UTF-16 unit (DOM selection offsets are unit-based), all = x.
      for (let k = 0; k < cell.symbol.length; k++) columns.push(x);
    }
    // Trim trailing regular-space padding, keeping columns in lockstep (a `/ +$/`
    // equivalent that also slices `columns`; NBSP is not U+0020 so it stays).
    let end = text.length;
    while (end > 0 && text[end - 1] === " ") end--;
    return { text: text.slice(0, end), columns: columns.slice(0, end) };
  }

  /** The stored cell at `(x, y)` as a draw op (for the cursor overlay and the #140
   * overlay-delta to read and repaint a cell independent of the frame's damage), or
   * `undefined` for a wide-char spacer half — the lead glyph already covers that
   * column, so drawing a blank there would clip it (same skip as {@link applyFrame}). */
  cellAt(x: number, y: number): DrawOp | undefined {
    const cell = this.cells[y * this.cols + x]!;
    if ((cell.flags & this.F.wide_char_spacer) !== 0) return undefined;
    return cellToDrawOp(x, y, cell.symbol, cell.fg, cell.bg, cell.flags, this.palette, this.F, this.policy);
  }

  /** Shift rows `[top, bottom]` by `count` (>0 = up, exposing blanks at the
   * bottom; <0 = down, exposing at the top). Reassigns slots, never mutates a
   * stored cell, so the iteration order can't clobber a not-yet-copied source. */
  private shiftRegion(top: number, bottom: number, count: number): void {
    if (count > 0) {
      for (let y = top; y <= bottom; y++) {
        const src = y + count;
        for (let x = 0; x < this.cols; x++) {
          this.cells[y * this.cols + x] = src <= bottom ? this.cells[src * this.cols + x]! : blank();
        }
      }
    } else if (count < 0) {
      for (let y = bottom; y >= top; y--) {
        const src = y + count;
        for (let x = 0; x < this.cols; x++) {
          this.cells[y * this.cols + x] = src >= top ? this.cells[src * this.cols + x]! : blank();
        }
      }
    }
  }
}
