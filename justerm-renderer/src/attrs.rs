//! SGR cell attributes — pure, host-testable. Bit positions mirror `justerm_core::CellFlags`
//! (the `flags` column the wasm decoder emits). #267 handles bold/italic (font style),
//! underline/strikethrough (glyph-field bits, shader-drawn) and inverse (fg/bg swap); dim
//! (#272), blink, hidden come later.

use crate::glyph_cache::FontStyle;

// justerm_core::CellFlags bit values (frozen by the wire format).
pub const BOLD: u16 = 1 << 0;
pub const ITALIC: u16 = 1 << 2;
pub const UNDERLINE: u16 = 1 << 3;
pub const INVERSE: u16 = 1 << 5;
pub const STRIKETHROUGH: u16 = 1 << 7;
/// A double-width glyph's lead cell (it renders the glyph's left half).
pub const WIDE_CHAR: u16 = 1 << 8;
/// The trailing cell a wide glyph occupies (it renders the glyph's right half).
pub const WIDE_CHAR_SPACER: u16 = 1 << 9;

/// Whether the cell is a wide glyph's lead (occupies this + the next column).
pub fn is_wide_lead(flags: u16) -> bool {
    flags & WIDE_CHAR != 0
}

/// Whether the cell is a wide glyph's trailing spacer.
pub fn is_wide_spacer(flags: u16) -> bool {
    flags & WIDE_CHAR_SPACER != 0
}

/// Underline/strikethrough are packed into the glyph field's high bits (mirrors beamterm
/// `cell.frag`: bit 13 = underline, bit 14 = strikethrough; the slot address is bits 0..12).
pub const GLYPH_UNDERLINE: u16 = 1 << 13;
pub const GLYPH_STRIKETHROUGH: u16 = 1 << 14;

/// Font style from a cell's flags — bold + italic select the atlas variant.
pub fn font_style(flags: u16) -> FontStyle {
    match (flags & BOLD != 0, flags & ITALIC != 0) {
        (false, false) => FontStyle::Normal,
        (true, false) => FontStyle::Bold,
        (false, true) => FontStyle::Italic,
        (true, true) => FontStyle::BoldItalic,
    }
}

/// Fold underline/strikethrough into the glyph field alongside the slot index.
pub fn glyph_field(slot: u16, flags: u16) -> u16 {
    let mut field = slot;
    if flags & UNDERLINE != 0 {
        field |= GLYPH_UNDERLINE;
    }
    if flags & STRIKETHROUGH != 0 {
        field |= GLYPH_STRIKETHROUGH;
    }
    field
}

/// Whether the cell swaps foreground and background.
pub fn is_inverse(flags: u16) -> bool {
    flags & INVERSE != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_style_selects_the_bold_italic_variant() {
        assert_eq!(font_style(0), FontStyle::Normal);
        assert_eq!(font_style(BOLD), FontStyle::Bold);
        assert_eq!(font_style(ITALIC), FontStyle::Italic);
        assert_eq!(font_style(BOLD | ITALIC), FontStyle::BoldItalic);
        // Unrelated flags (e.g. underline) don't change the style.
        assert_eq!(font_style(BOLD | UNDERLINE), FontStyle::Bold);
    }

    #[test]
    fn glyph_field_sets_the_decoration_bits() {
        assert_eq!(glyph_field(95, 0), 95);
        assert_eq!(glyph_field(95, UNDERLINE), 95 | GLYPH_UNDERLINE);
        assert_eq!(glyph_field(95, STRIKETHROUGH), 95 | GLYPH_STRIKETHROUGH);
        assert_eq!(
            glyph_field(95, UNDERLINE | STRIKETHROUGH),
            95 | GLYPH_UNDERLINE | GLYPH_STRIKETHROUGH
        );
        // The slot address bits (0..12) survive; decoration is above them.
        assert_eq!(glyph_field(95, UNDERLINE) & 0x1FFF, 95);
    }

    #[test]
    fn is_inverse_reads_the_inverse_bit() {
        assert!(!is_inverse(0));
        assert!(is_inverse(INVERSE));
        assert!(is_inverse(INVERSE | BOLD));
    }

    #[test]
    fn wide_lead_and_spacer_read_their_bits() {
        assert!(is_wide_lead(WIDE_CHAR));
        assert!(!is_wide_lead(WIDE_CHAR_SPACER));
        assert!(is_wide_spacer(WIDE_CHAR_SPACER));
        assert!(!is_wide_spacer(WIDE_CHAR));
        assert!(!is_wide_lead(0) && !is_wide_spacer(0));
    }
}
