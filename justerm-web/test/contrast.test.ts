import { describe, expect, it } from "vitest";
import { contrastRatio, ensureContrastRatio, relativeLuminance } from "../src/contrast";

describe("WCAG colour maths", () => {
  // WCAG relative luminance: pure white = 1, pure black = 0 (the channel weights
  // 0.2126 + 0.7152 + 0.0722 sum to 1).
  it("relativeLuminance is 1 for white and 0 for black", () => {
    expect(relativeLuminance(0xffffff)).toBe(1);
    expect(relativeLuminance(0x000000)).toBe(0);
  });

  // WCAG contrast ratio of the luminance extremes (1 vs 0) is 21:1 — the maximum.
  it("contrastRatio of white-on-black luminances is 21", () => {
    expect(contrastRatio(1, 0)).toBe(21);
    expect(contrastRatio(0, 1)).toBe(21); // order-independent
  });
});

describe("ensureContrastRatio", () => {
  // Already-sufficient contrast returns undefined (caller keeps the original fg).
  // White on black is 21:1, which trivially meets any ratio <= 21.
  it("returns undefined when the pair already meets the ratio", () => {
    expect(ensureContrastRatio(0x000000, 0xffffff, 4.5)).toBeUndefined();
  });

  // Black fg on black bg has contrast 1:1. To reach 21:1 the only colour that
  // works against black is pure white — so the adjusted fg must be white.
  it("lightens an invisible fg to meet a high ratio (black on black -> white)", () => {
    expect(ensureContrastRatio(0x000000, 0x000000, 21)).toBe(0xffffff);
  });
});
