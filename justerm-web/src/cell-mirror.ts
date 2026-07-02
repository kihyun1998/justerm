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
    private readonly palette: Palette,
    private readonly F: FlagBits,
    private readonly policy: RenderPolicy = identityPolicy,
  ) {
    this.cells = Array.from({ length: cols * rows }, blank);
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
    let text = "";
    for (let x = 0; x < this.cols; x++) {
      const cell = this.cells[y * this.cols + x]!;
      if ((cell.flags & this.F.wide_char_spacer) !== 0) continue; // trailing half
      text += cell.symbol;
    }
    return text.replace(/\s+$/u, "");
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
