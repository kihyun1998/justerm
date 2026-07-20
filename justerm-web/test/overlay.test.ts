import { describe, expect, it } from "vitest";
import {
  activeMatchHighlights,
  highlightAt,
  highlightRects,
  matchHighlights,
  selectionHighlights,
} from "../src/overlay";
import type { DecodedFrame } from "../src/types";

// A frame stripped to just the fields the overlay walk reads. The cell arrays
// are empty — `selectionHighlights` projects positions only (colour is #115's
// job), so it never touches codepoints/fg/bg.
function overlayFrame(over: Partial<DecodedFrame>): DecodedFrame {
  return {
    cols: 80,
    rows: 24,
    kind: 0,
    codepoints: [],
    fg: [],
    bg: [],
    flags: [],
    extra: [],
    spans: [],
    sideTable: [],
    ...over,
  };
}

describe("selectionHighlights — overlay span walk", () => {
  // The wire carries the live selection as stride-3 `(row, left, right)` viewport
  // triples (#108). The walk turns one triple into one inclusive highlight rect.
  it("reads selectionSpans (stride 3) into one {row,left,right} highlight", () => {
    const frame = overlayFrame({ selectionSpans: [0, 2, 7] });

    expect(selectionHighlights(frame)).toEqual([{ row: 0, left: 2, right: 7 }]);
  });

  // The same wire group carries search-match highlights (#108) under a separate
  // directory — `matchHighlights` reads it the same stride-3 way, so S9 paints
  // matches without re-deriving the walk. Two triples → two rects.
  it("reads matchSpans (stride 3) into one highlight per triple", () => {
    const frame = overlayFrame({ matchSpans: [1, 0, 3, 4, 9, 9] });

    expect(matchHighlights(frame)).toEqual([
      { row: 1, left: 0, right: 3 },
      { row: 4, left: 9, right: 9 },
    ]);
  });

  // Most frames carry no overlay at all — frame mode omits the directories when
  // nothing is selected / searched. An absent directory must read as empty, not
  // throw, so the renderer can call these every frame unconditionally.
  it("yields no highlights when the overlay directories are absent", () => {
    const frame = overlayFrame({});

    expect(selectionHighlights(frame)).toEqual([]);
    expect(matchHighlights(frame)).toEqual([]);
  });
});

describe("highlightRects — kinded overlay rects for the renderer", () => {
  // The renderer paints one highlight layer; it needs both groups tagged by kind
  // so #115 can pick a colour (selection vs search-match blend) per cell.
  it("merges selection and match spans into kind-tagged rects", () => {
    const frame = overlayFrame({ selectionSpans: [0, 2, 7], matchSpans: [3, 1, 4] });

    expect(highlightRects(frame)).toEqual([
      { row: 0, left: 2, right: 7, kind: "selection" },
      { row: 3, left: 1, right: 4, kind: "match" },
    ]);
  });

  // The renderer asks per cell whether it's highlighted (to blend its bg, like
  // the cursor's cell-invert). Coverage is inclusive of both column edges,
  // matching core's `left..=right` span.
  it("reports the highlight kind covering a cell, both edges inclusive", () => {
    const rects = highlightRects(overlayFrame({ selectionSpans: [1, 2, 4] }));

    expect(highlightAt(rects, 2, 1)).toBe("selection"); // left edge
    expect(highlightAt(rects, 4, 1)).toBe("selection"); // right edge
    expect(highlightAt(rects, 3, 1)).toBe("selection"); // interior
    expect(highlightAt(rects, 5, 1)).toBeNull(); // past the right edge
    expect(highlightAt(rects, 1, 1)).toBeNull(); // before the left edge
    expect(highlightAt(rects, 3, 2)).toBeNull(); // different row
  });

  // When the selection covers a search match, the selection blend wins — even if
  // the match rect is listed first. The renderer shows one colour per cell.
  it("prefers selection over a match covering the same cell", () => {
    const rects: ReturnType<typeof highlightRects> = [
      { row: 0, left: 0, right: 5, kind: "match" },
      { row: 0, left: 2, right: 3, kind: "selection" },
    ];

    expect(highlightAt(rects, 2, 0)).toBe("selection"); // both cover col 2 → selection
    expect(highlightAt(rects, 0, 0)).toBe("match"); // only the match covers col 0
  });
});

// #429: the ACTIVE (current) search match is a third overlay group in the public
// projection, mirroring the wire (#428) and the renderer's ranking (#427) so a
// consumer building its own renderer off these utilities gets the same model.
describe("active-match channel in the public projection (#429)", () => {
  it("reads activeMatchSpans (stride 3) into highlights; absent → empty", () => {
    expect(activeMatchHighlights(overlayFrame({ activeMatchSpans: [2, 5, 8] }))).toEqual([
      { row: 2, left: 5, right: 8 },
    ]);
    expect(activeMatchHighlights(overlayFrame({}))).toEqual([]);
  });

  it("tags active rects in highlightRects after selection and match", () => {
    const frame = overlayFrame({
      selectionSpans: [0, 2, 7],
      matchSpans: [3, 1, 4],
      activeMatchSpans: [3, 1, 4],
    });

    expect(highlightRects(frame)).toEqual([
      { row: 0, left: 2, right: 7, kind: "selection" },
      { row: 3, left: 1, right: 4, kind: "match" },
      { row: 3, left: 1, right: 4, kind: "active" },
    ]);
  });

  // The renderer's ranking: active > selection > match (#427). The active match
  // is ALSO present in the match group (the wire keeps it there; ranking, not
  // exclusion, resolves the overlap) — so a cell under all three reads "active",
  // regardless of listing order.
  it("ranks active above selection above match on the same cell", () => {
    const rects: ReturnType<typeof highlightRects> = [
      { row: 0, left: 0, right: 9, kind: "match" },
      { row: 0, left: 2, right: 6, kind: "active" },
      { row: 0, left: 4, right: 9, kind: "selection" },
    ];

    expect(highlightAt(rects, 5, 0)).toBe("active"); // all three cover col 5
    expect(highlightAt(rects, 3, 0)).toBe("active"); // active + match
    expect(highlightAt(rects, 8, 0)).toBe("selection"); // selection + match
    expect(highlightAt(rects, 0, 0)).toBe("match"); // match alone
  });
});
