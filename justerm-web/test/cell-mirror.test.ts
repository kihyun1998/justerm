import { describe, expect, it } from "vitest";
import { CellMirror } from "../src/cell-mirror";
import { makeRenderPolicy } from "../src/render-policy";
import type { FlagBits } from "../src/render-core";
import type { DecodedFrame } from "../src/types";
import type { Palette } from "justerm-wasm-decode/colors.js";

function palette(): Palette {
  return { colors: new Uint32Array(256), defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
}

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

describe("CellMirror.applyFrame", () => {
  it("emits draw ops for a full frame's cells", () => {
    const mirror = new CellMirror(80, 24, palette(), F);

    const ops = mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "hi" }]));

    expect(ops.map((o) => ({ x: o.x, y: o.y, symbol: o.symbol }))).toEqual([
      { x: 0, y: 0, symbol: "h" },
      { x: 1, y: 0, symbol: "i" },
    ]);
  });

  // #115 stage-1: the mirror path (scroll-shifted repaint) shares cellToDrawOp, so
  // an inverse cell must swap fg/bg here too — a per-path regression guard (the
  // family has had reader-path-specific bugs, #113/#207).
  it("applies inverse through the mirror path", () => {
    const mirror = new CellMirror(1, 1, palette(), F);
    const invFrame = {
      cols: 1,
      rows: 1,
      kind: 0,
      codepoints: [cp("a")],
      fg: [0],
      bg: [0],
      flags: [F.inverse],
      extra: [0],
      spans: [0, 0, 0, 0, 1],
      sideTable: [],
    } as DecodedFrame;

    const [op] = mirror.applyFrame(invFrame);

    // Default fg=0xc0c0c0 / bg=0x101010 resolved, then swapped by inverse.
    expect({ fg: op!.fg, bg: op!.bg }).toEqual({ fg: 0x101010, bg: 0xc0c0c0 });
  });

  // #115 stage-2: the dim policy the renderer injects must flow through the live
  // mirror path (cellToDrawOp). A dim white-on-black cell renders its fg halved
  // toward the bg (0x808080); the bg is untouched.
  it("applies the stage-2 dim policy through the mirror path", () => {
    const mirror = new CellMirror(1, 1, palette(), F, makeRenderPolicy(F));
    const dimFrame = {
      cols: 1,
      rows: 1,
      kind: 0,
      codepoints: [cp("a")],
      fg: [0x02ffffff], // Rgb white
      bg: [0x02000000], // Rgb black
      flags: [F.dim],
      extra: [0],
      spans: [0, 0, 0, 0, 1],
      sideTable: [],
    } as DecodedFrame;

    const [op] = mirror.applyFrame(dimFrame);

    expect({ fg: op!.fg, bg: op!.bg }).toEqual({ fg: 0x808080, bg: 0x000000 });
  });

  // #223 bold→bright: the mirror path passes boldToBright to cellToDrawOp, so a
  // bold ANSI 0-7 indexed fg draws with its bright variant when the theme enables it.
  it("applies bold→bright through the mirror path when enabled", () => {
    const pal: Palette = { colors: new Uint32Array(256), defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
    pal.colors[3] = 0x808000; // ANSI 3 (dim)
    pal.colors[11] = 0xffff00; // ANSI 11 (bright)
    const mirror = new CellMirror(1, 1, pal, F, makeRenderPolicy(F), true);
    const boldFrame = {
      cols: 1,
      rows: 1,
      kind: 0,
      codepoints: [cp("a")],
      fg: [0x01000003], // Indexed(3)
      bg: [0],
      flags: [F.bold],
      extra: [0],
      spans: [0, 0, 0, 0, 1],
      sideTable: [],
    } as DecodedFrame;

    const [op] = mirror.applyFrame(boldFrame);

    expect(op!.fg).toBe(0xffff00); // bright colors[11], not dim colors[3]
  });

  // #115 theme switch: cells are stored as colour refs, so a new palette re-resolves
  // them without re-decoding a frame. recolor() swaps the palette/policy and returns
  // a full repaint of every stored cell.
  it("re-resolves stored cells under a new palette (theme switch)", () => {
    const paletteA: Palette = { colors: new Uint32Array(256), defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
    paletteA.colors[1] = 0xff0000; // ANSI 1 = red under theme A
    const mirror = new CellMirror(1, 1, paletteA, F);
    mirror.applyFrame({
      cols: 1,
      rows: 1,
      kind: 0,
      codepoints: [cp("a")],
      fg: [0x01000001], // Indexed(1)
      bg: [0],
      flags: [0],
      extra: [0],
      spans: [0, 0, 0, 0, 1],
      sideTable: [],
    } as DecodedFrame);

    const paletteB: Palette = { colors: new Uint32Array(256), defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
    paletteB.colors[1] = 0x00ff00; // ANSI 1 = green under theme B
    const ops = mirror.recolor(paletteB, makeRenderPolicy(F));

    expect(ops[0]!.fg).toBe(0x00ff00);
  });

  // The core of ADR-0011: a scroll-op frame shifts the stored region so the moved
  // cells repaint at their new rows (beamterm can't shift them). 2×3 mirror holds
  // AA/BB/CC; scroll up 1 in [0,2] + a new bottom span "DD" → BB/CC/DD. Damage is
  // the whole region, emitted row-major from the mirror's stored content.
  it("shifts the stored region on a scroll op and repaints it", () => {
    const mirror = new CellMirror(2, 3, palette(), F);
    mirror.applyFrame(
      frame(0, [
        { line: 0, left: 0, text: "AA" },
        { line: 1, left: 0, text: "BB" },
        { line: 2, left: 0, text: "CC" },
      ]),
    );

    const ops = mirror.applyFrame(
      frame(1, [{ line: 2, left: 0, text: "DD" }], { top: 0, bottom: 2, count: 1 }),
    );

    expect(ops.map((o) => `${o.symbol}@${o.x},${o.y}`)).toEqual([
      "B@0,0",
      "B@1,0",
      "C@0,1",
      "C@1,1",
      "D@0,2",
      "D@1,2",
    ]);
  });

  // Negative count = scroll down (justerm-core damage.rs: "positive = up, negative
  // = down"): content moves down, the top row is exposed. AA/BB/CC scrolled down 1
  // + a new top span "DD" → DD/AA/BB.
  it("scrolls down on a negative count, exposing the top row", () => {
    const mirror = new CellMirror(2, 3, palette(), F);
    mirror.applyFrame(
      frame(0, [
        { line: 0, left: 0, text: "AA" },
        { line: 1, left: 0, text: "BB" },
        { line: 2, left: 0, text: "CC" },
      ]),
    );

    const ops = mirror.applyFrame(
      frame(1, [{ line: 0, left: 0, text: "DD" }], { top: 0, bottom: 2, count: -1 }),
    );

    expect(ops.map((o) => `${o.symbol}@${o.x},${o.y}`)).toEqual([
      "D@0,0",
      "D@1,0",
      "A@0,1",
      "A@1,1",
      "B@0,2",
      "B@1,2",
    ]);
  });

  // A Full frame (kind 0) is the whole viewport — it must reset the mirror, or
  // stale cells survive and a later scroll shifts ghosts up. After a full frame
  // with only row0col0="X", rows 1-2 are blank; scrolling up + a new row2 "Z"
  // must yield blanks, not the prior AA/BB/CC.
  it("resets the mirror on a full frame so a later scroll finds no ghosts", () => {
    const mirror = new CellMirror(2, 3, palette(), F);
    mirror.applyFrame(
      frame(0, [
        { line: 0, left: 0, text: "AA" },
        { line: 1, left: 0, text: "BB" },
        { line: 2, left: 0, text: "CC" },
      ]),
    );
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "X" }])); // full → only X, rest blank

    const ops = mirror.applyFrame(
      frame(1, [{ line: 2, left: 0, text: "Z" }], { top: 0, bottom: 2, count: 1 }),
    );

    expect(ops.map((o) => o.symbol)).toEqual([" ", " ", " ", " ", "Z", " "]);
  });

  // cellAt exposes a stored cell's draw op so the cursor overlay can read (and
  // restore) the cursor cell independently of the current frame's damage.
  it("returns the stored cell's draw op via cellAt", () => {
    const mirror = new CellMirror(80, 24, palette(), F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "hi" }]));

    expect(mirror.cellAt(1, 0)).toMatchObject({ x: 1, y: 0, symbol: "i" });
  });
});

describe("CellMirror.rowText", () => {
  // The a11y row tree (#119) reads each viewport row as text. Trailing blanks
  // are trimmed — a screen reader shouldn't read the padding to end-of-line.
  it("joins a row's symbols and trims trailing blanks", () => {
    const mirror = new CellMirror(80, 24, palette(), F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "echo hi" }]));

    expect(mirror.rowText(0)).toBe("echo hi");
    expect(mirror.rowText(1)).toBe(""); // a blank row is empty, not 80 spaces
  });

  // #153 G8: a trailing NBSP (U+00A0) is real content, not padding — only regular
  // trailing spaces are trimmed. `\s` would eat the NBSP, contradicting the copy
  // invariant (justerm never emits NBSP as padding). Regular spaces after it still go.
  it("preserves a trailing NBSP but still trims regular padding spaces", () => {
    const mirror = new CellMirror(80, 24, palette(), F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "a " }])); // "a" + NBSP, rest is padding

    expect(mirror.rowText(0)).toBe("a "); // NBSP kept, blank-cell padding trimmed
  });

  // A wide glyph occupies two cells: the symbol then a spacer half. The text
  // must read the glyph once, not glyph-plus-blank (else the SR hears a gap).
  it("reads a wide glyph once, skipping its spacer half", () => {
    const mirror = new CellMirror(4, 1, palette(), F);
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
    const mirror = new CellMirror(4, 1, palette(), F);
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
    const mirror = new CellMirror(80, 24, palette(), F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "a " }])); // "a" + NBSP + padding

    expect(mirror.rowCells(0)).toEqual({ text: "a ", columns: [0, 1] });
  });

  // `columns.length === text.length` so a DOM offset indexes straight in — locked so
  // a future change can't desync them (they must trim together).
  it("keeps columns and text the same length", () => {
    const mirror = new CellMirror(80, 24, palette(), F);
    mirror.applyFrame(frame(0, [{ line: 0, left: 0, text: "hello" }]));

    const { text, columns } = mirror.rowCells(0);
    expect(columns).toEqual([0, 1, 2, 3, 4]); // one column per char, in order
    expect(columns.length).toBe(text.length);
  });

  // A surrogate-pair emoji is TWO UTF-16 units in one cell; DOM selection offsets are
  // unit-based, so BOTH units map to the cell's column (`columns.length === text.length`
  // holds through the pair). The trim can't split it — a low surrogate isn't U+0020.
  it("maps both UTF-16 units of a surrogate-pair emoji to the lead column", () => {
    const mirror = new CellMirror(4, 1, palette(), F);
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

  // A combining/grapheme cluster arrives as a multi-unit `symbol` via the side table
  // (`extra` → `sideTable`). Each of its UTF-16 units maps to the one cell's column, so
  // a mid-cluster AT offset still reverses to that column and text/columns stay aligned.
  it("maps every UTF-16 unit of a side-table grapheme cluster to its cell column", () => {
    const mirror = new CellMirror(4, 1, palette(), F);
    const f: DecodedFrame = {
      cols: 4,
      rows: 1,
      kind: 0,
      codepoints: [cp("e"), cp("x")], // cell 0's base cp is overridden by `extra`
      fg: [0, 0],
      bg: [0, 0],
      flags: [0, 0],
      extra: [1, 0], // cell 0 → sideTable[0]
      spans: [0, 0, 1, 0, 2],
      sideTable: ["é"], // "é" as e + combining acute — two UTF-16 units
    } as DecodedFrame;
    mirror.applyFrame(f);

    expect(mirror.rowCells(0)).toEqual({ text: "éx", columns: [0, 0, 1] });
  });
});
