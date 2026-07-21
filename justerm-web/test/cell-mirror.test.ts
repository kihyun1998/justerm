import { describe, expect, it } from "vitest";
import { CellMirror } from "../src/cell-mirror";
import type { DecodedFrame, FlagBits } from "../src/types";

const F: FlagBits = {
  bold: 0x01,
  italic: 0x02,
  underline: 0x04,
  strikethrough: 0x08,
  wide_char_spacer: 0x100,
  inverse: 0x200,
  dim: 0x400,
  hidden: 0x800,
};

const cp = (s: string): number => s.codePointAt(0)!;

// A one-span frame. kind 0 = Full, 1 = Partial. Optional scroll op.
function frame(
  kind: number,
  spans: { line: number; left: number; text: string }[],
  scroll?: { top: number; bottom: number; count: number },
): DecodedFrame {
  const codepoints: number[] = [];
  const spanDir: number[] = [];
  let offset = 0;
  for (const s of spans) {
    const chars = [...s.text];
    spanDir.push(s.line, s.left, s.left + chars.length - 1, offset, chars.length);
    for (const c of chars) codepoints.push(cp(c));
    offset += chars.length;
  }
  const n = codepoints.length;
  return {
    cols: 80,
    rows: 24,
    kind,
    codepoints,
    fg: new Array(n).fill(0),
    bg: new Array(n).fill(0),
    flags: new Array(n).fill(0),
    extra: new Array(n).fill(0),
    spans: spanDir,
    sideTable: [],
    ...(scroll
      ? { hasScroll: true, scrollTop: scroll.top, scrollBottom: scroll.bottom, scrollCount: scroll.count }
      : {}),
  } as DecodedFrame;
}

describe("CellMirror.applyFrame (scroll-op mirroring)", () => {
  it("shifts the stored region on a scroll op and repaints it", () => {
    const mirror = new CellMirror(2, 3, F);
    mirror.applyFrame(
      frame(0, [
        { line: 0, left: 0, text: "AA" },
        { line: 1, left: 0, text: "BB" },
        { line: 2, left: 0, text: "CC" },
      ]),
    );

    mirror.applyFrame(
      frame(1, [{ line: 2, left: 0, text: "DD" }], { top: 0, bottom: 2, count: 1 }),
    );

    expect([0, 1, 2].map((y) => mirror.rowText(y))).toEqual(["BB", "CC", "DD"]);
  });

  // Negative count = scroll down (justerm-core damage.rs: "positive = up, negative
  // = down"): content moves down, the top row is exposed. AA/BB/CC scrolled down 1
  // + a new top span "DD" → DD/AA/BB.
  it("scrolls down on a negative count, exposing the top row", () => {
    const mirror = new CellMirror(2, 3, F);
    mirror.applyFrame(
      frame(0, [
        { line: 0, left: 0, text: "AA" },
        { line: 1, left: 0, text: "BB" },
        { line: 2, left: 0, text: "CC" },
      ]),
    );

    mirror.applyFrame(
      frame(1, [{ line: 0, left: 0, text: "DD" }], { top: 0, bottom: 2, count: -1 }),
    );

    expect([0, 1, 2].map((y) => mirror.rowText(y))).toEqual(["DD", "AA", "BB"]);
  });

  // A Full frame (kind 0) is the whole viewport — it must reset the mirror, or
  // stale cells survive and a later scroll shifts ghosts up. After a full frame
  // with only row0col0="X", rows 1-2 are blank; scrolling up + a new row2 "Z"
  // must yield blanks, not the prior AA/BB/CC.
  it("resets the mirror on a full frame so a later scroll finds no ghosts", () => {
    const mirror = new CellMirror(2, 3, F);
    mirror.applyFrame(
      frame(0, [
        { line: 0, left: 0, text: "AA" },
        { line: 1, left: 0, text: "BB" },
        { line: 2, left: 0, text: "CC" },
      ]),
    );
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "X" }])); // full → only X, rest blank

    mirror.applyFrame(
      frame(1, [{ line: 2, left: 0, text: "Z" }], { top: 0, bottom: 2, count: 1 }),
    );

    expect([0, 1, 2].map((y) => mirror.rowText(y))).toEqual(["", "", "Z"]);
  });

});

describe("CellMirror.rowText", () => {
  // The a11y row tree (#119) reads each viewport row as text. Trailing blanks
  // are trimmed — a screen reader shouldn't read the padding to end-of-line.
  it("joins a row's symbols and trims trailing blanks", () => {
    const mirror = new CellMirror(80, 24, F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "echo hi" }]));

    expect(mirror.rowText(0)).toBe("echo hi");
    expect(mirror.rowText(1)).toBe(""); // a blank row is empty, not 80 spaces
  });

  // #153 G8: a trailing NBSP (U+00A0) is real content, not padding — only regular
  // trailing spaces are trimmed. `\s` would eat the NBSP, contradicting the copy
  // invariant (justerm never emits NBSP as padding). Regular spaces after it still go.
  it("preserves a trailing NBSP but still trims regular padding spaces", () => {
    const mirror = new CellMirror(80, 24, F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "a " }])); // "a" + NBSP, rest is padding

    expect(mirror.rowText(0)).toBe("a "); // NBSP kept, blank-cell padding trimmed
  });

  // A wide glyph occupies two cells: the symbol then a spacer half. The text
  // must read the glyph once, not glyph-plus-blank (else the SR hears a gap).
  it("reads a wide glyph once, skipping its spacer half", () => {
    const mirror = new CellMirror(4, 1, F);
    // "가b": wide "가" at col 0 with a spacer at col 1, then "b" at col 2.
    const wide: DecodedFrame = {
      cols: 4,
      rows: 1,
      kind: 0,
      codepoints: [cp("가"), cp(" "), cp("b")],
      fg: [0, 0, 0],
      bg: [0, 0, 0],
      flags: [0, F.wide_char_spacer, 0],
      extra: [0, 0, 0],
      spans: [0, 0, 2, 0, 3],
      sideTable: [],
    } as DecodedFrame;
    mirror.applyFrame(wide);

    expect(mirror.rowText(0)).toBe("가b");
  });
});

describe("CellMirror.rowCells (#152 column map)", () => {
  // Each UTF-16 unit of the row text maps to its source terminal column, so an AT
  // selection offset in the row tree reverses to a grid column. A wide glyph's char
  // maps to its lead column; the spacer half is skipped (so "b" is column 2, not 1).
  it("maps each character to its terminal column, skipping a wide glyph's spacer", () => {
    const mirror = new CellMirror(4, 1, F);
    const wide: DecodedFrame = {
      cols: 4,
      rows: 1,
      kind: 0,
      codepoints: [cp("가"), cp(" "), cp("b")],
      fg: [0, 0, 0],
      bg: [0, 0, 0],
      flags: [0, F.wide_char_spacer, 0],
      extra: [0, 0, 0],
      spans: [0, 0, 2, 0, 3],
      sideTable: [],
    } as DecodedFrame;
    mirror.applyFrame(wide);

    expect(mirror.rowCells(0)).toEqual({ text: "가b", columns: [0, 2] });
  });

  // Trailing padding is trimmed from BOTH text and columns in lockstep; a real
  // trailing NBSP (U+00A0) survives with its column (mirrors #153 G8).
  it("trims trailing padding columns in lockstep, preserving a real NBSP", () => {
    const mirror = new CellMirror(80, 24, F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "a " }])); // "a" + NBSP + padding

    expect(mirror.rowCells(0)).toEqual({ text: "a ", columns: [0, 1] });
  });

  // `columns.length === text.length` so a DOM offset indexes straight in — locked so
  // a future change can't desync them (they must trim together).
  it("keeps columns and text the same length", () => {
    const mirror = new CellMirror(80, 24, F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "hello" }]));

    const { text, columns } = mirror.rowCells(0);
    expect(columns).toEqual([0, 1, 2, 3, 4]); // one column per char, in order
    expect(columns.length).toBe(text.length);
  });

  // A surrogate-pair emoji is TWO UTF-16 units in one cell; DOM selection offsets are
  // unit-based, so BOTH units map to the cell's column (`columns.length === text.length`
  // holds through the pair). The trim can't split it — a low surrogate isn't U+0020.
  it("maps both UTF-16 units of a surrogate-pair emoji to the lead column", () => {
    const mirror = new CellMirror(4, 1, F);
    const f: DecodedFrame = {
      cols: 4,
      rows: 1,
      kind: 0,
      codepoints: [0x1f600, cp("b")], // 😀 (one code point, two UTF-16 units), then b
      fg: [0, 0],
      bg: [0, 0],
      flags: [0, 0],
      extra: [0, 0],
      spans: [0, 0, 1, 0, 2],
      sideTable: [],
    } as DecodedFrame;
    mirror.applyFrame(f);

    expect(mirror.rowCells(0)).toEqual({ text: "\u{1F600}b", columns: [0, 0, 1] });
  });

  // A combining/grapheme cluster's marks arrive via the side table (`extra` -> `sideTable`), but
  // justerm-core stores ONLY the trailing width-0 marks there — the BASE glyph stays in the
  // codepoint column (verified: `Engine::feed("e\u{0301}")` -> side_table=[['\u{0301}']],
  // cell.c()='e'). So the mirror must PREPEND the base codepoint to the marks (#294). Each UTF-16
  // unit of the assembled cluster maps to the one cell's column, so a mid-cluster AT offset still
  // reverses to that column and text/columns stay aligned.
  it("prepends the base codepoint to side-table combining marks (core stores marks only)", () => {
    const mirror = new CellMirror(4, 1, F);
    const f: DecodedFrame = {
      cols: 4,
      rows: 1,
      kind: 0,
      codepoints: [cp("e"), cp("x")], // cell 0's base is 'e' — in the codepoint column
      fg: [0, 0],
      bg: [0, 0],
      flags: [0, 0],
      extra: [1, 0], // cell 0 -> sideTable[0]
      spans: [0, 0, 1, 0, 2],
      sideTable: ["\u{0301}"], // core stores ONLY the combining acute (not the base 'e')
    } as DecodedFrame;
    mirror.applyFrame(f);

    // "é" = 'e' (base) + U+0301 (mark) = two UTF-16 units, both at column 0.
    expect(mirror.rowCells(0)).toEqual({ text: "e\u{0301}x", columns: [0, 0, 1] });
  });

  it("keeps a wide emoji whose cell carries a trailing ZWJ/VS16 mark (not a blank cell)", () => {
    // A wide emoji + a trailing width-0 mark (ZWJ/VS16) lands as base=emoji, side-table=[mark].
    // Dropping the base (the old bug) rendered a blank wide cell; the base must survive.
    const mirror = new CellMirror(4, 1, F);
    const f: DecodedFrame = {
      cols: 4,
      rows: 1,
      kind: 0,
      codepoints: [0x1f680, 0], // 🚀 lead, then its wide spacer
      fg: [0, 0],
      bg: [0, 0],
      flags: [0, F.wide_char_spacer],
      extra: [1, 0], // cell 0 carries the trailing mark
      spans: [0, 0, 1, 0, 2],
      sideTable: ["\u{200D}"], // a lone trailing ZWJ (width-0)
    } as DecodedFrame;
    mirror.applyFrame(f);

    // The rocket survives with its ZWJ appended — not replaced by a bare ZWJ (blank).
    expect(mirror.rowCells(0).text).toBe("\u{1F680}\u{200D}");
  });
});
