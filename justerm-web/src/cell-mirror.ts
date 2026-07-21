import type { DecodedFrame, FlagBits } from "./types";

/** One stored cell: the resolved glyph + its flag bits. No colour — the renderer
 * resolves and composites colour in wasm (#273), and the only reader here is the
 * a11y text mirror (#504). */
interface MirrorCell {
  symbol: string;
  flags: number;
}

const blank = (): MirrorCell => ({ symbol: " ", flags: 0 });

const SPAN_STRIDE = 5;

/**
 * A viewport-sized local copy of the rendered cells (ADR-0011). Frame mode keeps
 * it so scroll-op damage can be applied — a GPU renderer can neither shift retained
 * cells nor return their styling, so the shifted region is repainted from here.
 *
 * Text-only since #504. It fed the beamterm adapter's TypeScript compositing until
 * #273 moved compositing into the renderer's wasm; the colour half survived that as
 * per-frame work whose result the sole caller discarded. The scroll-op mirroring
 * below is the part that is genuinely load-bearing.
 */
export class CellMirror {
  private readonly cells: MirrorCell[];

  constructor(
    private readonly cols: number,
    private readonly rows: number,
    private readonly F: FlagBits,
  ) {
    this.cells = Array.from({ length: cols * rows }, blank);
  }

  applyFrame(frame: DecodedFrame): void {
    // 0. A Full frame is the whole viewport — wipe stale cells before the spans
    // fill it, or content outside the new spans (and any later scroll of it)
    // would resurrect ghosts. The mirror wipes to match.
    if (frame.kind === 0) {
      for (let i = 0; i < this.cells.length; i++) this.cells[i] = blank();
    }

    // 1. Scroll op first (it precedes spans): shift the stored region.
    if (frame.hasScroll) {
      this.shiftRegion(frame.scrollTop!, frame.scrollBottom!, frame.scrollCount!);
    }

    // 2. Spans: store each cell.
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
        // The side table holds ONLY the trailing width-0 combining marks (justerm-core convention,
        // #294); the base glyph stays in `code`. Prepend the base to the marks so "é" (e + U+0301)
        // and "🚀‍" (emoji + trailing ZWJ) keep their base instead of rendering a bare mark.
        const marks = extra !== 0 ? frame.sideTable[extra - 1]! : "";
        const symbol = code === 0 ? " " : String.fromCodePoint(code) + marks;
        this.cells[line * this.cols + x] = { symbol, flags };
      }
    }
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
