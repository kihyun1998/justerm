//! Glyph classes that tile seamlessly with the adjacent background (#226/#272) — pure, host-testable.
//!
//! Powerline separators and box-drawing / block elements butt cleanly against the neighbouring cell.
//! A `minimumContrastRatio` nudge on one would shift its colour and open a visible seam, so xterm
//! excludes them from the contrast demand (`excludeFromContrastRatioDemands`), and re-tints them
//! toward the selection colour rather than standing them out as a hard stroke (#239). Ported faithfully
//! from justerm-web `glyph-class.ts` / xterm's `RendererUtils`.

/// Powerline symbols, `U+E0A4..=U+E0D6` — the full range xterm treats as a background colour (not the
/// narrower restricted set it uses for glyph rescaling).
fn is_powerline_glyph(codepoint: u32) -> bool {
    (0xE0A4..=0xE0D6).contains(&codepoint)
}

/// Box-drawing (`U+2500..=U+257F`) + block elements (`U+2580..=U+259F`).
fn is_box_or_block_glyph(codepoint: u32) -> bool {
    (0x2500..=0x259F).contains(&codepoint)
}

/// Whether a glyph is meant to tile with the background — xterm's `treatGlyphAsBackgroundColor`.
/// Such a cell's fg is excluded from the `minimumContrastRatio` correction (so the glyph keeps butting
/// cleanly against its neighbour) and re-tinted under a selection (#239). Classify a cell's **base**
/// codepoint (the first scalar of the resolved symbol, matching the web's `symbol.codePointAt(0)`) —
/// tile glyphs are all single-codepoint BMP, never grapheme clusters, so the base is authoritative.
pub fn treat_glyph_as_background_color(codepoint: u32) -> bool {
    is_powerline_glyph(codepoint) || is_box_or_block_glyph(codepoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_drawing_and_block_elements_tile() {
        assert!(treat_glyph_as_background_color(0x2500)); // ─ box-drawing start
        assert!(treat_glyph_as_background_color(0x257F)); // box-drawing end
        assert!(treat_glyph_as_background_color(0x2580)); // ▀ block start
        assert!(treat_glyph_as_background_color(0x2588)); // █ full block
        assert!(treat_glyph_as_background_color(0x259F)); // block end
    }

    #[test]
    fn powerline_symbols_tile() {
        assert!(treat_glyph_as_background_color(0xE0A4)); // powerline start
        assert!(treat_glyph_as_background_color(0xE0B0)); //  a common separator
        assert!(treat_glyph_as_background_color(0xE0D6)); // powerline end
    }

    #[test]
    fn ordinary_text_and_the_range_edges_do_not_tile() {
        assert!(!treat_glyph_as_background_color(0x41)); // 'A'
        assert!(!treat_glyph_as_background_color(0x24FF)); // just below box-drawing
        assert!(!treat_glyph_as_background_color(0x25A0)); // just above block elements (■)
        assert!(!treat_glyph_as_background_color(0xE0A3)); // just below powerline
        assert!(!treat_glyph_as_background_color(0xE0D7)); // just above powerline
        assert!(!treat_glyph_as_background_color(0x1F680)); // 🚀 emoji
    }
}
