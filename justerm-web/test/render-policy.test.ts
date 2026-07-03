import { describe, expect, it } from "vitest";
import { makeRenderPolicy, resolveCell } from "../src/render-policy";
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
});

describe("makeRenderPolicy — stage-2 RGB policy", () => {
  // DIM (xterm BgFlags.DIM) halves the foreground toward the background — xterm
  // draws the glyph at DIM_OPACITY 0.5 over the cell bg, so on beamterm (no
  // per-glyph alpha) the dim is baked in as the midpoint of fg and bg. Independent
  // check: white halfway to black is mid-grey; bg is untouched.
  it("dims the foreground to the fg/bg midpoint when the dim flag is set", () => {
    const policy = makeRenderPolicy(F);

    expect(policy(0xffffff, 0x000000, F.dim)).toEqual({ fg: 0x808080, bg: 0x000000 });
  });

  it("leaves colours unchanged when the dim flag is clear", () => {
    const policy = makeRenderPolicy(F);

    expect(policy(0xffffff, 0x000000, 0)).toEqual({ fg: 0xffffff, bg: 0x000000 });
  });

  // minimumContrastRatio (default 1 = off): when configured, an unreadable fg is
  // adjusted to meet the ratio. Black-on-black at ratio 21 forces the fg to white.
  it("raises fg to meet the configured minimumContrastRatio", () => {
    const policy = makeRenderPolicy(F, 21);

    expect(policy(0x000000, 0x000000, 0)).toEqual({ fg: 0xffffff, bg: 0x000000 });
  });

  it("does not adjust contrast when minimumContrastRatio is 1 (default)", () => {
    const policy = makeRenderPolicy(F, 1);

    expect(policy(0x000000, 0x000000, 0)).toEqual({ fg: 0x000000, bg: 0x000000 });
  });

  // Contrast wins over dim and skips it (xterm early-returns the contrast colour):
  // a dim black-on-black cell would stay black under dim alone, but contrast
  // lightens the fg instead. (dim also halves the required ratio, ratio/2.)
  it("applies contrast instead of dim when both would fire", () => {
    const policy = makeRenderPolicy(F, 21);

    const { fg } = policy(0x000000, 0x000000, F.dim);
    expect(fg).toBeGreaterThan(0); // lightened by contrast, not left black by dim
  });
});
