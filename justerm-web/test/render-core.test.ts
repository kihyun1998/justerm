import { describe, expect, it } from "vitest";
import { frameToDrawOps } from "../src/render-core";
import type { FlagBits } from "../src/render-core";
import type { DecodedFrame } from "../src/types";
import type { Palette } from "justerm-wasm-decode/colors.js";

// A minimal palette: defaults plus one indexed colour we can assert on later.
function palette(): Palette {
  const colors = new Uint32Array(256);
  colors[1] = 0xff0000; // ANSI red, for the indexed-colour cycle
  return { colors, defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
}

// Test-local flag bits. The structural FlagBits interface lets a test pick any
// consistent set — the function never hard-codes the decoder's actual values, so
// these tests don't couple to the Rust bitflags layout.
const F: FlagBits = {
  bold: 0x01,
  italic: 0x02,
  underline: 0x04,
  strikethrough: 0x08,
  wide_char_spacer: 0x100,
};

const cp = (s: string): number => s.codePointAt(0)!;

// Build a one-span Full frame. `refs` default to Default (tag 0) colours.
function spanFrame(
  line: number,
  left: number,
  cells: { cp: number; fg?: number; bg?: number; flags?: number; extra?: number }[],
  sideTable: string[] = [],
): DecodedFrame {
  return {
    cols: 80,
    rows: 24,
    kind: 0, // Full
    codepoints: cells.map((c) => c.cp),
    fg: cells.map((c) => c.fg ?? 0),
    bg: cells.map((c) => c.bg ?? 0),
    flags: cells.map((c) => c.flags ?? 0),
    extra: cells.map((c) => c.extra ?? 0),
    // span directory stride 5: [line, left, right, cell_offset, count]
    spans: [line, left, left + cells.length - 1, 0, cells.length],
    sideTable,
  };
}

describe("frameToDrawOps — span walk", () => {
  it("emits one op per cell at (left+i, line) with the cell's glyph", () => {
    const frame = spanFrame(2, 5, [{ cp: cp("H") }, { cp: cp("i") }]);

    const ops = frameToDrawOps(frame, palette(), F);

    expect(ops.map((o) => ({ x: o.x, y: o.y, symbol: o.symbol }))).toEqual([
      { x: 5, y: 2, symbol: "H" },
      { x: 6, y: 2, symbol: "i" },
    ]);
  });

  // Colour-ref kinds (tag = high byte): 0 Default, 1 Indexed, 2 Rgb. Resolution
  // matches xterm v6 CellColorResolver: CM_DEFAULT→theme default, CM_P16/P256→
  // palette[idx], CM_RGB→passthrough.
  it("resolves Default, Indexed, and Rgb colour refs to packed 0xRRGGBB", () => {
    const frame = spanFrame(0, 0, [
      { cp: cp("a"), fg: 0x00000000, bg: 0x00000000 }, // Default → theme defaults
      { cp: cp("b"), fg: 0x01000001, bg: 0x00000000 }, // Indexed 1 → palette red
      { cp: cp("c"), fg: 0x02aabbcc, bg: 0x02112233 }, // Rgb → passthrough
    ]);

    const ops = frameToDrawOps(frame, palette(), F);

    expect(ops.map((o) => ({ fg: o.fg, bg: o.bg }))).toEqual([
      { fg: 0xc0c0c0, bg: 0x101010 }, // defaultFg / defaultBg
      { fg: 0xff0000, bg: 0x101010 }, // colors[1] / defaultBg
      { fg: 0xaabbcc, bg: 0x112233 }, // RGB passthrough
    ]);
  });

  // Each style flag maps to its DrawOp boolean independently — same four
  // attributes xterm v6 AttributeData exposes (isBold/isItalic/isUnderline/
  // isStrikethrough), here all in one u16.
  it("maps style flags to per-op booleans", () => {
    const frame = spanFrame(0, 0, [
      { cp: cp("a"), flags: F.bold | F.underline },
      { cp: cp("b"), flags: F.italic | F.strikethrough },
      { cp: cp("c"), flags: 0 },
    ]);

    const ops = frameToDrawOps(frame, palette(), F);

    expect(
      ops.map((o) => ({
        bold: o.bold,
        italic: o.italic,
        underline: o.underline,
        strikethrough: o.strikethrough,
      })),
    ).toEqual([
      { bold: true, italic: false, underline: true, strikethrough: false },
      { bold: false, italic: true, underline: false, strikethrough: true },
      { bold: false, italic: false, underline: false, strikethrough: false },
    ]);
  });

  // A wide glyph occupies two columns: the body cell holds the glyph, the
  // trailing WIDE_CHAR_SPACER cell must NOT be painted (else the next column
  // would show a ghost). The body renders once; beamterm's atlas draws its width.
  it("skips wide_char_spacer cells and paints the wide body once", () => {
    const frame = spanFrame(0, 0, [
      { cp: cp("한") }, // wide body at col 0
      { cp: 0, flags: F.wide_char_spacer }, // spacer at col 1 — skip
      { cp: cp("x") }, // next glyph at col 2
    ]);

    const ops = frameToDrawOps(frame, palette(), F);

    expect(ops.map((o) => ({ x: o.x, symbol: o.symbol }))).toEqual([
      { x: 0, symbol: "한" },
      { x: 2, symbol: "x" },
    ]);
  });

  // Combining clusters can't fit one codepoint, so the decoder stores the full
  // grapheme in sideTable and the cell carries a 1-based `extra` index. When set,
  // the glyph is the cluster string, not the base codepoint.
  it("uses the sideTable cluster for cells with a nonzero extra index", () => {
    const frame = spanFrame(
      0,
      0,
      [
        { cp: cp("e"), extra: 1 }, // base 'e', cluster index 1
        { cp: cp("z") }, // plain cell, extra 0
      ],
      ["é"], // sideTable[0] = "é" (e + combining acute)
    );

    const ops = frameToDrawOps(frame, palette(), F);

    expect(ops.map((o) => o.symbol)).toEqual(["é", "z"]);
  });

  // A Partial frame carries only the damaged spans. The walk must read each
  // span's cells at its own cell_offset (column-level damage — finer than
  // xterm's webgl row-level redraw) and emit ops for exactly those columns.
  it("walks multiple spans by cell_offset, emitting only their cells", () => {
    const frame: DecodedFrame = {
      cols: 80,
      rows: 24,
      kind: 1, // Partial
      // two spans flattened: "ab" then "XYZ"
      codepoints: ["a", "b", "X", "Y", "Z"].map(cp),
      fg: [0, 0, 0, 0, 0],
      bg: [0, 0, 0, 0, 0],
      flags: [0, 0, 0, 0, 0],
      extra: [0, 0, 0, 0, 0],
      // [line,left,right,cell_offset,count] × 2
      spans: [1, 3, 4, 0, 2, 5, 0, 2, 2, 3],
      sideTable: [],
    };

    const ops = frameToDrawOps(frame, palette(), F);

    expect(ops.map((o) => ({ x: o.x, y: o.y, symbol: o.symbol }))).toEqual([
      { x: 3, y: 1, symbol: "a" },
      { x: 4, y: 1, symbol: "b" },
      { x: 0, y: 5, symbol: "X" },
      { x: 1, y: 5, symbol: "Y" },
      { x: 2, y: 5, symbol: "Z" },
    ]);
  });

  // The render-policy seam runs after resolveRgb, mapping (fg, bg, flags) → final
  // colours. S2 ships identity; #115 plugs in inverse/selection/dim/contrast here.
  // A swap policy proves the resolved colours flow through it.
  it("applies the render policy to resolved colours", () => {
    const frame = spanFrame(0, 0, [{ cp: cp("a") }]); // Default fg/bg
    const swap = (fg: number, bg: number) => ({ fg: bg, bg: fg });

    const [op] = frameToDrawOps(frame, palette(), F, swap);

    // resolved fg=defaultFg(0xc0c0c0), bg=defaultBg(0x101010) → swapped
    expect({ fg: op!.fg, bg: op!.bg }).toEqual({ fg: 0x101010, bg: 0xc0c0c0 });
  });
});
