// Glyph classes that xterm draws to tile seamlessly with the adjacent background
// (#226) — Powerline separators and box-drawing / block elements. Ported faithfully
// from xterm's `RendererUtils` (`isPowerlineGlyph` / `isBoxOrBlockGlyph` /
// `treatGlyphAsBackgroundColor`). A `minimumContrastRatio` correction on one of
// these would nudge its colour and open a visible seam against its neighbour, so
// xterm excludes them from the contrast demand (`excludeFromContrastRatioDemands`).

/** Powerline symbols, U+E0A4..U+E0D6 (the full range xterm treats as bg, not the
 * narrower restricted set used for rescaling). */
function isPowerlineGlyph(codepoint: number): boolean {
  return codepoint >= 0xe0a4 && codepoint <= 0xe0d6;
}

/** Box-drawing (U+2500..U+257F) + block elements (U+2580..U+259F). */
function isBoxOrBlockGlyph(codepoint: number): boolean {
  return codepoint >= 0x2500 && codepoint <= 0x259f;
}

/**
 * Whether a glyph is meant to tile with the background — xterm's
 * `treatGlyphAsBackgroundColor`. The renderer excludes such a cell's fg from the
 * `minimumContrastRatio` correction so the glyph keeps butting cleanly against the
 * neighbouring cell instead of seaming. Both ranges are BMP, so a UTF-16 code unit
 * (`charCodeAt`) and a full code point agree here.
 *
 * Callers classify a cell's **base** scalar (`symbol.codePointAt(0)`, see
 * `render-core.ts`), so a tile glyph carrying a combining mark (`█` + U+0301) still
 * tiles. That is a declared divergence from xterm's `CellColorResolver`, which
 * classifies `cell.getCode()` — the *last* UTF-16 unit of a combined cell — while
 * xterm's own `TextureAtlas` classifies the first; the rule and the reasoning are
 * recorded once, in justerm-renderer `glyph_class.rs` (#495). Keep the two sides in
 * step: this function and the renderer's must classify the same scalar.
 */
export function treatGlyphAsBackgroundColor(codepoint: number): boolean {
  return isPowerlineGlyph(codepoint) || isBoxOrBlockGlyph(codepoint);
}
