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

function run(sel: TreeSelection) {
  const port = new StubSelectionPort();
  a11ySelectionToPort(sel, columnsFor, port);
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
});
