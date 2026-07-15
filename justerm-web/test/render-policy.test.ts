import { describe, expect, it } from "vitest";
import { resolveCell } from "../src/render-policy";
import type { FlagBits } from "../src/render-core";
import type { Palette } from "justerm-wasm-decode/colors.js";

// Same minimal palette shape the render-core tests use.
function palette(): Palette {
  const colors = new Uint32Array(256);
  colors[1] = 0xff0000; // ANSI red at index 1
  return { colors, defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
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

// justerm colour refs (tag = high byte): 0 Default, 1 Indexed, 2 Rgb.
const DEFAULT = 0x00000000;
const INDEXED_1 = 0x01000001; // Indexed(1) → palette.colors[1] = 0xff0000
const RGB_AABBCC = 0x02aabbcc; // Rgb → 0xaabbcc passthrough
const RGB_112233 = 0x02112233; // Rgb → 0x112233 passthrough

describe("resolveCell — stage-1 ref resolution", () => {
  // Inverse renders the cell with fg/bg exchanged. xterm resolves a Default fg
  // under inverse to the theme *background* and a Default bg to the theme
  // *foreground* — which for justerm equals resolving each ref in its own role
  // (Default→defaultFg/defaultBg) then swapping the two resolved colours.
  it("swaps resolved fg/bg for a Default/Default cell under inverse", () => {
    const { fg, bg } = resolveCell(DEFAULT, DEFAULT, F.inverse, palette(), F);

    // Non-inverse would be fg=defaultFg(0xc0c0c0), bg=defaultBg(0x101010).
    expect({ fg, bg }).toEqual({ fg: 0x101010, bg: 0xc0c0c0 });
  });

  // Equivalence matrix: the swap is a single post-resolve exchange, so it is
  // uniform across colour-mode kinds. These rows prove "inverse ≡ resolve-then-
  // swap" holds for Indexed and Rgb too (hand-derived from the xterm rule), and
  // guard the invariant when #223 moves bold→bright into this ref-space stage.
  it("swaps an Indexed fg against a Default bg under inverse", () => {
    // Non-inverse: fg=colors[1]=0xff0000, bg=defaultBg=0x101010.
    const { fg, bg } = resolveCell(INDEXED_1, DEFAULT, F.inverse, palette(), F);

    expect({ fg, bg }).toEqual({ fg: 0x101010, bg: 0xff0000 });
  });

  it("swaps two Rgb refs (passthrough) under inverse", () => {
    // Non-inverse: fg=0xaabbcc, bg=0x112233.
    const { fg, bg } = resolveCell(RGB_AABBCC, RGB_112233, F.inverse, palette(), F);

    expect({ fg, bg }).toEqual({ fg: 0x112233, bg: 0xaabbcc });
  });

  it("does not swap when the inverse flag is clear", () => {
    const { fg, bg } = resolveCell(INDEXED_1, DEFAULT, 0, palette(), F);

    expect({ fg, bg }).toEqual({ fg: 0xff0000, bg: 0x101010 });
  });

  // #223 bold→bright: a bold cell whose fg is an ANSI 0-7 palette index draws with
  // the bright 8-15 variant (xterm drawBoldTextInBrightColors). Only when the
  // caller enables it (last arg) — off, the dim index stays.
  function brightPalette(): Palette {
    const colors = new Uint32Array(256);
    colors[3] = 0x808000; // ANSI 3 (dim yellow)
    colors[11] = 0xffff00; // ANSI 11 (bright yellow)
    return { colors, defaultFg: 0xc0c0c0, defaultBg: 0x101010 };
  }
  const INDEXED_3 = 0x01000003;

  it("brightens a bold ANSI 0-7 indexed fg to its 8-15 variant when enabled", () => {
    const { fg } = resolveCell(INDEXED_3, DEFAULT, F.bold, brightPalette(), F, true);

    expect(fg).toBe(0xffff00); // colors[11], not colors[3]
  });

  it("does not brighten when boldToBright is disabled (the default)", () => {
    const { fg } = resolveCell(INDEXED_3, DEFAULT, F.bold, brightPalette(), F, false);

    expect(fg).toBe(0x808000); // colors[3] unchanged
  });

  // The coupling #115 flagged: inverse swaps the slots FIRST, so bold+inverse
  // brightens whatever ref becomes the drawn fg — the original BG index. fg=Indexed
  // (2), bg=Indexed(5), bold+inverse → drawn fg = Indexed(5) → bright Indexed(13).
  it("brightens the post-inverse fg (the original bg index) under bold+inverse", () => {
    const pal = brightPalette();
    pal.colors[5] = 0x008000; // ANSI 5 (dim)
    pal.colors[13] = 0x00ff00; // ANSI 13 (bright)
    const INDEXED_2 = 0x01000002;
    const INDEXED_5 = 0x01000005;

    const { fg } = resolveCell(INDEXED_2, INDEXED_5, F.bold | F.inverse, pal, F, true);

    expect(fg).toBe(0x00ff00); // colors[13], the bright of the swapped-in bg index 5
  });

  // Only ANSI 0-7 brighten; an already-bright (8-15) or higher index is untouched.
  it("leaves an index >= 8 unchanged under bold", () => {
    const pal = brightPalette();
    pal.colors[11] = 0xffff00;
    const INDEXED_11 = 0x0100000b;

    const { fg } = resolveCell(INDEXED_11, DEFAULT, F.bold, pal, F, true);

    expect(fg).toBe(0xffff00); // colors[11], no +8 into 19
  });
});
