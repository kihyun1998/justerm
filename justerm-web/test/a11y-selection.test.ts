import { describe, expect, it } from "vitest";
import { a11ySelectionToPort, type TreeSelection } from "../src/a11y-selection";
import { StubSelectionPort } from "../src/selection";

/** Column maps by row for the tests: row 0 = 5 single-width cols; row 1 = "가b"
 * (wide glyph at col 0, "b" at col 2, spacer col 1 skipped). */
const maps: Record<number, number[]> = {
  0: [0, 1, 2, 3, 4],
  1: [0, 2],
};
const columnsFor = (row: number): number[] => maps[row] ?? [];

function run(sel: TreeSelection, rowCount = 10) {
  const port = new StubSelectionPort();
  a11ySelectionToPort(sel, columnsFor, rowCount, port);
  return port.calls;
}

describe("a11ySelectionToPort (#152 AT selection → core selection)", () => {
  // A caret (collapsed) inside the tree is not a selection — clear the engine's.
  it("clears the engine selection on a collapsed selection in the tree", () => {
    expect(run({ anchor: { row: 0, offset: 2 }, focus: { row: 0, offset: 2 }, collapsed: true })).toEqual([
      { kind: "clear" },
    ]);
  });

  // A selection whose anchor is outside the row tree isn't ours — do nothing (leave
  // the engine selection untouched, xterm's `_rowContainer.contains` guard).
  it("ignores a selection whose anchor is outside the tree", () => {
    expect(run({ anchor: null, focus: { row: 0, offset: 3 }, collapsed: false })).toEqual([]);
  });

  // A forward single-row selection: begin at the left of the first char, extend to the
  // left of the char past the last selected one (offsets [1,4) = chars 1,2,3).
  it("maps a forward single-row selection to begin/extend boundaries", () => {
    expect(run({ anchor: { row: 0, offset: 1 }, focus: { row: 0, offset: 4 }, collapsed: false })).toEqual([
      { kind: "begin", row: 0, col: 1, side: "left", ty: "char" },
      { kind: "extend", row: 0, col: 4, side: "left" },
    ]);
  });

  // Selecting to end-of-line (focus offset === text length) lands on the RIGHT of the
  // last char, so it's included.
  it("maps an end-of-line focus to the right side of the last char", () => {
    expect(run({ anchor: { row: 0, offset: 0 }, focus: { row: 0, offset: 5 }, collapsed: false })).toEqual([
      { kind: "begin", row: 0, col: 0, side: "left", ty: "char" },
      { kind: "extend", row: 0, col: 4, side: "right" }, // columns[4] = 4, right
    ]);
  });

  // Wide glyph: the "b" is at column 2 (spacer col 1 skipped), so selecting "가b"
  // (offsets 0..2) begins at col 0 and extends to the right of col 2.
  it("maps offsets through a wide glyph's column map", () => {
    expect(run({ anchor: { row: 1, offset: 0 }, focus: { row: 1, offset: 2 }, collapsed: false })).toEqual([
      { kind: "begin", row: 1, col: 0, side: "left", ty: "char" },
      { kind: "extend", row: 1, col: 2, side: "right" }, // offset 2 == length → columns[1]=2, right
    ]);
  });

  // A multi-row selection: anchor mid-row-0, focus mid-row-1 — each endpoint uses its
  // own row's column map.
  it("maps a selection spanning two rows", () => {
    expect(run({ anchor: { row: 0, offset: 2 }, focus: { row: 1, offset: 1 }, collapsed: false })).toEqual([
      { kind: "begin", row: 0, col: 2, side: "left", ty: "char" },
      { kind: "extend", row: 1, col: 2, side: "left" }, // row1 columns[1] = 2 (the "b")
    ]);
  });

  // A backwards selection (focus before anchor) passes through unchanged — the engine
  // tracks anchor→focus, so begin=anchor / extend=focus is correct either direction.
  it("passes a backwards selection through (begin=anchor, extend=focus)", () => {
    expect(run({ anchor: { row: 0, offset: 4 }, focus: { row: 0, offset: 1 }, collapsed: false })).toEqual([
      { kind: "begin", row: 0, col: 4, side: "left", ty: "char" },
      { kind: "extend", row: 0, col: 1, side: "left" },
    ]);
  });

  // A selection on a blank row (empty column map) collapses to column 0 rather than
  // reading past the end of an empty map.
  it("maps a blank-row endpoint to column 0", () => {
    expect(run({ anchor: { row: 9, offset: 0 }, focus: { row: 9, offset: 1 }, collapsed: false })).toEqual([
      { kind: "begin", row: 9, col: 0, side: "left", ty: "char" },
      { kind: "extend", row: 9, col: 0, side: "left" }, // empty map → column 0 either way
    ]);
  });

  // #217: an out-of-tree endpoint that still overlaps the tree is CLAMPED, not dropped
  // (xterm's `_handleSelectionChange` begin/end clamp). A `before` anchor (a drag that
  // began above row 0, or the native Select-All anchor) clamps to the start of row 0.
  it("clamps a `before` endpoint to the start of row 0", () => {
    expect(run({ anchor: "before", focus: { row: 0, offset: 3 }, collapsed: false })).toEqual([
      { kind: "begin", row: 0, col: 0, side: "left", ty: "char" },
      { kind: "extend", row: 0, col: 3, side: "left" },
    ]);
  });

  // #217: an `after` endpoint (a drag overshooting past the last row, or into the sibling
  // live region) clamps to the end of the last row (`rowCount-1`), end-of-text → RIGHT of
  // its last char. Here rowCount=2, so the last row is row 1 ("가b", cols [0,2]).
  it("clamps an `after` endpoint to the end of the last row", () => {
    expect(run({ anchor: { row: 0, offset: 1 }, focus: "after", collapsed: false }, 2)).toEqual([
      { kind: "begin", row: 0, col: 1, side: "left", ty: "char" },
      { kind: "extend", row: 1, col: 2, side: "right" }, // row 1 end-of-text → columns[1]=2, right
    ]);
  });

  // #217 casualty #1 — native Select-All: anchor above the tree, focus below it. Both
  // endpoints clamp, selecting the whole viewport (row 0 start → last row end).
  it("selects the whole tree for a spanning Select-All (before → after)", () => {
    expect(run({ anchor: "before", focus: "after", collapsed: false }, 2)).toEqual([
      { kind: "begin", row: 0, col: 0, side: "left", ty: "char" },
      { kind: "extend", row: 1, col: 2, side: "right" },
    ]);
  });

  // A selection wholly on one side of the tree doesn't overlap it — no-op (xterm bails
  // when `begin` is below the last row, or `end` above the first).
  it("ignores a selection wholly above the tree (both before)", () => {
    expect(run({ anchor: "before", focus: "before", collapsed: false })).toEqual([]);
  });
  it("ignores a selection wholly below the tree (both after)", () => {
    expect(run({ anchor: "after", focus: "after", collapsed: false })).toEqual([]);
  });

  // A collapsed caret OUTSIDE the tree must not clear the engine selection (only a caret
  // inside the tree clears — xterm's `_rowContainer.contains(anchorNode)` guard).
  it("does not clear on a collapsed caret outside the tree", () => {
    expect(run({ anchor: "before", focus: "before", collapsed: true })).toEqual([]);
  });
});
