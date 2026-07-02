import { describe, expect, it } from "vitest";
import { CellMirror } from "../src/cell-mirror";
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
