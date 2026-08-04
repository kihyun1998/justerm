//! SGR cell attributes — pure, host-testable. Bit positions mirror `justerm_core::CellFlags`
//! (the `flags` column the wasm decoder emits). #267 handles bold/italic (font style),
//! underline/strikethrough (glyph-field bits, shader-drawn) and inverse (fg/bg swap); #272 adds
//! dim (the fg fades toward the bg — see [`render_policy`](crate::render_policy)). #282 folds hidden
//! and blink into one *conceal* mechanism: a concealed cell renders background only (glyph coverage
//! and decorations suppressed) by pointing at the blank slot.

use crate::glyph_cache::FontStyle;

// justerm_core::CellFlags bit values (frozen by the wire format).
pub const BOLD: u16 = 1 << 0;
/// Faint/dim intensity (`ESC[2m`) — the fg is faded toward the bg at render time (#272,
/// `justerm_core::CellFlags::DIM`). A selection clears it so the text stays legible (#224).
pub const DIM: u16 = 1 << 1;
pub const ITALIC: u16 = 1 << 2;
pub const UNDERLINE: u16 = 1 << 3;
/// The cell blinks — its glyph is concealed on the render loop's "off" phase (#282). Timing
/// is the consumer's policy (mirrors xterm.js `TextBlinkStateManager` living in the render
/// loop, not the buffer); the renderer only takes the phase bool.
pub const BLINK: u16 = 1 << 4;
pub const INVERSE: u16 = 1 << 5;
/// The cell's glyph is concealed (`ESC[8m`) — renders background only (#282). Prior-art:
/// alacritty models HIDDEN as `fg == bg` (content.rs guards selection-reveal on it); we
/// instead suppress glyph coverage, which also conceals a colour emoji (#284 samples the
/// texture, not `fg`, so `fg == bg` would leak it).
pub const HIDDEN: u16 = 1 << 6;
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

/// The glyph's ink CLASS (ADR-0019 rule 4 R1): set iff this cell's glyph is `BACKGROUND` class —
/// a Powerline / box-drawing / block tile, whatever [`builtin::owns`](crate::builtin::owns) draws.
///
/// It rides bit 16, **above** the `u16` the rest of the field fits in, because the shader reads the
/// attribute as `uint(a_glyph)` and an `f32` carries every integer below `2²⁴` exactly — so the bit
/// is free where a thirteenth instance float would not have been. Bit 15 is the colour-emoji flag
/// and bits 0..12 the slot address, which is why the u16 looked full: it was, and the *transport*
/// was not.
///
/// The shader needs it because occlusion between ink sources follows their class (#712): R1 puts a
/// background-class glyph's ink on the **background** channel while `I_underline` is `TEXT` class
/// always, so the band draws **over** a tile and **under** a letter's descender. Nothing else in the
/// field is about the glyph's class — the other bits are its address and its decorations.
pub const GLYPH_BG_CLASS: u32 = 1 << 16;

/// Font style from a cell's flags — bold + italic select the atlas variant.
pub fn font_style(flags: u16) -> FontStyle {
    match (flags & BOLD != 0, flags & ITALIC != 0) {
        (false, false) => FontStyle::Normal,
        (true, false) => FontStyle::Bold,
        (false, true) => FontStyle::Italic,
        (true, true) => FontStyle::BoldItalic,
    }
}

/// Fold underline/strikethrough and the glyph's ink class into the field alongside the slot index.
///
/// `bg_class` is [`GLYPH_BG_CLASS`] — ADR-0019 R1's answer for this cell's glyph, which the caller
/// has already computed for the #226/#239 treatments. It is a *separate argument* rather than
/// another `flags` bit because it is not an SGR attribute: it is derived from the **codepoint**,
/// which this function never sees.
///
/// #338 briefly carried wide-lead / wide-spacer bits here so the shader could keep a split wide
/// glyph's halves touching. #359 made the atlas slot the padded CELL, so the halves are cut from a
/// bitmap that was baked centred over its two-cell advance — they touch by construction, and the
/// bits are gone.
pub fn glyph_field(slot: u16, flags: u16, bg_class: bool) -> u32 {
    let mut field = u32::from(slot);
    if flags & UNDERLINE != 0 {
        field |= u32::from(GLYPH_UNDERLINE);
    }
    if flags & STRIKETHROUGH != 0 {
        field |= u32::from(GLYPH_STRIKETHROUGH);
    }
    if bg_class {
        field |= GLYPH_BG_CLASS;
    }
    field
}

/// Whether the cell swaps foreground and background.
pub fn is_inverse(flags: u16) -> bool {
    flags & INVERSE != 0
}

/// Whether the cell is faint/dim (`ESC[2m`) — its foreground is faded toward the background (#272).
pub fn is_dim(flags: u16) -> bool {
    flags & DIM != 0
}

/// The glyph slot for a concealed cell: slot `0`, the pre-baked transparent ASCII space
/// (the fast path maps `' '` = 0x20 to slot `codepoint - 0x20` = 0). Pointing a cell here
/// gives zero coverage — background only — and, being a bare slot, carries no underline/
/// strikethrough/emoji bit. See [`glyph_cache::ascii_fast_path`](crate::glyph_cache).
pub const BLANK_SLOT: u16 = 0;

/// Whether the cell is hidden/concealed (`ESC[8m`).
pub fn is_hidden(flags: u16) -> bool {
    flags & HIDDEN != 0
}

/// Whether the cell has the blink attribute set.
pub fn is_blink(flags: u16) -> bool {
    flags & BLINK != 0
}

/// Whether the cell's glyph is currently concealed — hidden cells always, blink cells only
/// on the render loop's "off" phase (`blink_on == false`). A concealed cell renders
/// background only; the consumer flips `blink_on` on its own cadence (timing is policy).
pub fn is_concealed(flags: u16, blink_on: bool) -> bool {
    is_hidden(flags) || (is_blink(flags) && !blink_on)
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
        const U: u32 = GLYPH_UNDERLINE as u32;
        const S: u32 = GLYPH_STRIKETHROUGH as u32;
        assert_eq!(glyph_field(95, 0, false), 95);
        assert_eq!(glyph_field(95, UNDERLINE, false), 95 | U);
        assert_eq!(glyph_field(95, STRIKETHROUGH, false), 95 | S);
        assert_eq!(
            glyph_field(95, UNDERLINE | STRIKETHROUGH, false),
            95 | U | S
        );
        // The slot address bits (0..12) survive; decoration is above them.
        assert_eq!(glyph_field(95, UNDERLINE, false) & 0x1FFF, 95);
    }

    #[test]
    fn glyph_field_carries_the_ink_class_without_disturbing_the_rest() {
        // #712: the class is orthogonal to every other bit — it neither moves the slot address nor
        // implies a decoration, and a decorated tile carries both.
        const U: u32 = GLYPH_UNDERLINE as u32;
        assert_eq!(glyph_field(95, 0, true), 95 | GLYPH_BG_CLASS);
        assert_eq!(glyph_field(95, UNDERLINE, true), 95 | U | GLYPH_BG_CLASS);
        assert_eq!(glyph_field(95, UNDERLINE, true) & 0x1FFF, 95);
        assert_eq!(glyph_field(95, UNDERLINE, false) & GLYPH_BG_CLASS, 0);
        // It stays inside the f32 exact-integer range the attribute relies on (< 2²⁴), so the
        // shader's `uint(a_glyph)` reads back what the packer wrote.
        assert!(glyph_field(0x1FFF, UNDERLINE | STRIKETHROUGH, true) < (1 << 24));
    }

    #[test]
    fn is_inverse_reads_the_inverse_bit() {
        assert!(!is_inverse(0));
        assert!(is_inverse(INVERSE));
        assert!(is_inverse(INVERSE | BOLD));
    }

    #[test]
    fn is_dim_reads_the_dim_bit_at_the_core_frozen_position() {
        assert!(!is_dim(0));
        assert!(is_dim(DIM));
        assert!(is_dim(DIM | BOLD));
        assert!(!is_dim(BOLD), "bold is not dim");
        // Frozen wire position — justerm_core::CellFlags::DIM = 1 << 1 (cell.rs), the value the
        // wasm decoder's flags().dim reports. A drift here silently mis-reads every dim cell.
        assert_eq!(DIM, 1 << 1);
    }

    #[test]
    fn hidden_and_blink_read_their_bits() {
        assert!(is_hidden(HIDDEN));
        assert!(!is_hidden(BLINK));
        assert!(is_blink(BLINK));
        assert!(!is_blink(HIDDEN));
        assert!(!is_hidden(0) && !is_blink(0));
        // Bit positions match justerm_core::CellFlags (frozen wire format).
        assert_eq!(HIDDEN, 1 << 6);
        assert_eq!(BLINK, 1 << 4);
    }

    #[test]
    fn concealed_covers_hidden_always_and_blink_on_the_off_phase() {
        // Hidden is concealed regardless of the blink phase.
        assert!(is_concealed(HIDDEN, true));
        assert!(is_concealed(HIDDEN, false));
        // Blink is concealed only when the phase is off.
        assert!(!is_concealed(BLINK, true));
        assert!(is_concealed(BLINK, false));
        // A plain cell is never concealed.
        assert!(!is_concealed(0, true));
        assert!(!is_concealed(0, false));
        // Hidden wins even mid-blink-on (hidden ⇒ concealed).
        assert!(is_concealed(HIDDEN | BLINK, true));
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
