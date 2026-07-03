import { describe, expect, it } from "vitest";
import type { DecorationRect } from "../src/decorations";
import type { HighlightRect } from "../src/overlay";
import {
  composeOverlayDraws,
  cursorCellDraw,
  overlayCellKeys,
  overlayRepaintKeys,
} from "../src/overlay-compose";
import type { DrawOp } from "../src/render-core";

const SEL = 0x111111;
const MATCH = 0x222222;
const colors = { selectionBg: SEL, matchBg: MATCH };
// #115: a highlight is SOLID on a default-bg cell (blendHighlight=false) but an
// alpha-0x80 blend on a non-default/inverse cell. SEL_ON_BLACK is the blended
// value for a blendHighlight cell over a black bg: blendOver(0x000000, SEL, 0x80).
const SEL_ON_BLACK = 0x090909;

function op(x: number, y: number, bg = 0, fg = 0xffffff, blendHighlight = false, fgUndimmed = fg): DrawOp {
  return { x, y, symbol: "a", fg, bg, bold: false, italic: false, underline: false, strikethrough: false, blendHighlight, fgUndimmed };
}
const sel = (row: number, left: number, right: number): HighlightRect => ({ row, left, right, kind: "selection" });
const match = (row: number, left: number, right: number): HighlightRect => ({ row, left, right, kind: "match" });

describe("overlayCellKeys", () => {
  // Highlight cols are on-viewport; a decoration may start left of 0 or run past the
  // right edge, so cells are clamped to [0, cols).
  it("collects highlight + decoration cells, clamped to [0, cols)", () => {
    const highlights: HighlightRect[] = [sel(0, 1, 2)];
    const decos: DecorationRect[] = [{ row: 1, left: -1, right: 5, layer: "bottom", bg: 9 }];
    // cols=4: highlight (0,1)(0,2) → keys 1,2; deco row1 left -1→0, right 5→3 → keys 4,5,6,7.
    expect([...overlayCellKeys(highlights, decos, 4)].sort((a, b) => a - b)).toEqual([1, 2, 4, 5, 6, 7]);
  });

  // Defensive: a negative row (off-viewport-above; core never emits one) is dropped
  // rather than encoding a negative key that would break the `key % cols` decode.
  it("drops a negative-row rect (symmetric with the column clamp)", () => {
    expect(overlayCellKeys([{ row: -1, left: 0, right: 3, kind: "selection" }], [], 4).size).toBe(0);
  });
});

describe("overlayRepaintKeys", () => {
  it("returns (prev ∪ current) minus damaged, deduped", () => {
    const prev = new Set([1, 2, 3]);
    const current = new Set([3, 4, 5]);
    const damaged = new Set([2, 4]);
    // prev\damaged: 1,3 ; current\damaged\prev: 5 → [1,3,5]
    expect(overlayRepaintKeys(prev, current, damaged).sort((a, b) => a - b)).toEqual([1, 3, 5]);
  });
});

describe("composeOverlayDraws (#140 partial-frame overlay damage)", () => {
  const cellAt = (x: number, y: number): DrawOp => op(x, y, 0, 0xabcdef); // plain mirror cell

  // Baseline: damaged cells are tinted where the overlay covers them; with no prior
  // overlay there is no delta to repaint.
  it("tints damaged cells and, with no prior overlay, adds no delta", () => {
    const ops = [op(0, 0), op(1, 0), op(2, 0)];
    const { draws, overlay } = composeOverlayDraws({
      ops,
      highlights: [sel(0, 1, 1)], // only (1,0) selected
      decorations: [],
      prevOverlay: new Set(),
      cols: 3,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws).toHaveLength(3); // the 3 damaged cells only, no delta
    expect(draws.find((d) => d.x === 1)!.bg).toBe(SEL); // (1,0) tinted
    expect(draws.find((d) => d.x === 0)!.bg).toBe(0); // (0,0) plain
    expect([...overlay]).toEqual([1]); // key = 0*3 + 1
  });

  // #115: on a default-bg cell (blendHighlight=false) the search-match highlight
  // is painted SOLID — the same solid rule as the selection (xterm's default-bg
  // branch, CellColorResolver else-clause).
  it("paints a search-match highlight solid on a default-bg cell", () => {
    const { draws } = composeOverlayDraws({
      ops: [op(0, 0)],
      highlights: [match(0, 0, 0)],
      decorations: [],
      prevOverlay: new Set(),
      cols: 1,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws[0]!.bg).toBe(MATCH);
  });

  // #115: a non-default / inverse cell (blendHighlight=true) alpha-blends the
  // highlight over its own bg, so the cell colour shows through. Over black,
  // blendOver(0x000000, SEL, 0x80) = SEL_ON_BLACK.
  it("blends the highlight over a non-default-bg cell (blendHighlight)", () => {
    const { draws } = composeOverlayDraws({
      ops: [op(0, 0, 0x000000, 0xffffff, true)],
      highlights: [sel(0, 0, 0)],
      decorations: [],
      prevOverlay: new Set(),
      cols: 1,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws[0]!.bg).toBe(SEL_ON_BLACK);
  });

  // #224: a selected dim cell renders its UNDIMMED fg (xterm clears DIM under
  // selection) — the op carries fgUndimmed and overlayTint swaps it in.
  it("un-dims a selected cell's fg", () => {
    const dimOp = op(0, 0, 0x000000, 0x808080, false, 0xffffff); // fg dimmed, undimmed bright
    const { draws } = composeOverlayDraws({
      ops: [dimOp],
      highlights: [sel(0, 0, 0)],
      decorations: [],
      prevOverlay: new Set(),
      cols: 1,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws[0]!.fg).toBe(0xffffff);
  });

  it("keeps a matched (not selected) dim cell dimmed", () => {
    const dimOp = op(0, 0, 0x000000, 0x808080, false, 0xffffff);
    const { draws } = composeOverlayDraws({
      ops: [dimOp],
      highlights: [match(0, 0, 0)],
      decorations: [],
      prevOverlay: new Set(),
      cols: 1,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws[0]!.fg).toBe(0x808080); // match doesn't un-dim
  });

  // The core #140 fix (restore): a cell that WAS selected last frame is now
  // de-selected. On a partial frame that doesn't damage it, it must be repainted
  // PLAIN from the mirror — otherwise its stale tint lingers.
  it("restores a cell that left the selection on a partial frame (not in ops)", () => {
    const { draws } = composeOverlayDraws({
      ops: [],
      highlights: [],
      decorations: [],
      prevOverlay: new Set([1]), // (1,0) was selected
      cols: 3,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws).toHaveLength(1);
    expect(draws[0]).toMatchObject({ x: 1, y: 0, bg: 0 }); // restored plain (mirror bg 0)
  });

  // The core #140 fix (re-tint): a newly selected cell that isn't otherwise damaged
  // must still get its tint on a partial frame.
  it("tints a newly selected cell on a partial frame (not in ops)", () => {
    const { draws, overlay } = composeOverlayDraws({
      ops: [],
      highlights: [sel(0, 2, 2)], // (2,0) newly selected
      decorations: [],
      prevOverlay: new Set(),
      cols: 3,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws).toHaveLength(1);
    expect(draws[0]).toMatchObject({ x: 2, y: 0, bg: SEL });
    expect([...overlay]).toEqual([2]);
  });

  // A cell both damaged and in the overlay is drawn once (by the damage pass); the
  // delta must not re-add it.
  it("does not double-draw a cell that is both damaged and in the overlay", () => {
    const { draws } = composeOverlayDraws({
      ops: [op(1, 0)],
      highlights: [sel(0, 1, 1)],
      decorations: [],
      prevOverlay: new Set([1]),
      cols: 3,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws).toHaveLength(1); // damaged cell only
    expect(draws[0]!.bg).toBe(SEL);
  });

  // A wide-char spacer half in the delta is skipped: `cellAt` returns undefined for
  // it (the lead glyph covers that column), so a blank repaint can't clip the glyph —
  // the same skip the damage path (`applyFrame`) already does.
  it("skips a delta cell whose mirror cell is a wide-char spacer (cellAt undefined)", () => {
    const cellAtSpacer = (x: number, y: number): DrawOp | undefined =>
      x === 1 ? undefined : op(x, y, 0, 0xabcdef); // (1,0) is a spacer half
    const { draws } = composeOverlayDraws({
      ops: [],
      highlights: [sel(0, 0, 1)], // covers the lead (0,0) and the spacer (1,0)
      decorations: [],
      prevOverlay: new Set(),
      cols: 3,
      rows: 1,
      colors,
      cellAt: cellAtSpacer,
    });
    expect(draws).toHaveLength(1); // only the lead repainted; spacer skipped
    expect(draws[0]).toMatchObject({ x: 0, y: 0, bg: SEL });
  });

  // Decorations share the delta path: a bottom decoration that left a cell restores
  // it on a partial frame (mirrors the selection case, since #140 covers both).
  it("restores a cell that left a decoration on a partial frame", () => {
    const { draws } = composeOverlayDraws({
      ops: [],
      highlights: [],
      decorations: [], // decoration gone this frame
      prevOverlay: new Set([2]), // (2,0) was decorated
      cols: 3,
      rows: 1,
      colors,
      cellAt,
    });
    expect(draws).toHaveLength(1);
    expect(draws[0]).toMatchObject({ x: 2, y: 0, bg: 0 });
  });
});

describe("cursorCellDraw (#210 blink-off keeps the overlay tint)", () => {
  const base = op(1, 0, 0, 0xffffff); // cursor cell at (1,0), plain bg 0
  const styled = { ...base, fg: 0, bg: 0xffff00 }; // stand-in cursor invert

  // Blink ON: the cursor cell-invert wins over any overlay (the block hides it),
  // exactly the pre-#210 on-phase behaviour.
  it("blink ON draws the styled cursor op even over a selection", () => {
    expect(cursorCellDraw(base, true, styled, [sel(0, 1, 1)], [], colors)).toBe(styled);
  });

  // Blink OFF: the cursor isn't shown, so the cell keeps its selection tint instead
  // of flashing to the bare cell (the #210 fix).
  it("blink OFF composites the selection tint, not the bare cell", () => {
    const out = cursorCellDraw(base, false, styled, [sel(0, 1, 1)], [], colors);
    expect(out.bg).toBe(SEL); // NOT base bg 0, NOT the cursor colour
  });

  // Blink OFF composites a decoration too (the cursor cell need not be selected).
  it("blink OFF composites a decoration on the cursor cell", () => {
    const deco: DecorationRect = { row: 0, left: 1, right: 1, layer: "bottom", bg: 0x00ff00 };
    const out = cursorCellDraw(base, false, styled, [], [deco], colors);
    expect(out.bg).toBe(0x00ff00);
  });

  // Blink OFF with no overlay on the cursor cell returns the bare cell (unchanged).
  it("blink OFF returns the bare cell when no overlay covers it", () => {
    expect(cursorCellDraw(base, false, styled, [], [], colors)).toBe(base);
  });
});
