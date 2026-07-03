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
// #115: a highlight is an alpha-0x80 blend over the cell bg, not a solid fill.
// These plumbing tests paint over a base bg of 0, so the tint they observe is
// blendOver(0x000000, SEL, 0x80) — the selection colour at ~50% over black.
const SEL_ON_BLACK = 0x090909;

function op(x: number, y: number, bg = 0, fg = 0xffffff): DrawOp {
  return { x, y, symbol: "a", fg, bg, bold: false, italic: false, underline: false, strikethrough: false };
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
    expect(draws.find((d) => d.x === 1)!.bg).toBe(SEL_ON_BLACK); // (1,0) tinted
    expect(draws.find((d) => d.x === 0)!.bg).toBe(0); // (0,0) plain
    expect([...overlay]).toEqual([1]); // key = 0*3 + 1
  });

  // #115: a search-match highlight blends over the cell bg too (not only the
  // selection) — the policy is uniform per kind. MATCH over black at 0x80 is
  // blendOver(0x000000, 0x222222, 0x80) = 0x111111.
  it("blends a search-match highlight over the cell bg, not just selection", () => {
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
    expect(draws[0]!.bg).toBe(0x111111);
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
    expect(draws[0]).toMatchObject({ x: 2, y: 0, bg: SEL_ON_BLACK });
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
    expect(draws[0]!.bg).toBe(SEL_ON_BLACK);
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
    expect(draws[0]).toMatchObject({ x: 0, y: 0, bg: SEL_ON_BLACK });
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
    expect(out.bg).toBe(SEL_ON_BLACK); // NOT base bg 0, NOT the cursor colour
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
