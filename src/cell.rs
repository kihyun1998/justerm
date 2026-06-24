//! The cell — one character position in the grid (see CONTEXT.md "Cell").

use crate::color::Color;

bitflags::bitflags! {
    /// Per-cell flags: the standard SGR attributes plus layout markers.
    ///
    /// The high bits are intentionally left free so underline-style + underline
    /// colour and an OSC 8 hyperlink id can be added later without a format
    /// change (see `docs/architecture.md` "Cell").
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub struct CellFlags: u16 {
        // --- standard SGR attributes ---
        const BOLD          = 1 << 0;
        const DIM           = 1 << 1;
        const ITALIC        = 1 << 2;
        const UNDERLINE     = 1 << 3;
        const BLINK         = 1 << 4;
        const INVERSE       = 1 << 5;
        const HIDDEN        = 1 << 6;
        const STRIKETHROUGH = 1 << 7;

        // --- layout markers (not SGR): a width-2 glyph occupies two cells ---
        /// The first cell of a width-2 glyph; holds the actual character.
        const WIDE_CHAR        = 1 << 8;
        /// The trailing cell of a width-2 glyph. A distinct marker, *not* a
        /// plain blank — overwrite, erase, selection, and cursor positioning all
        /// depend on knowing this column belongs to the wide char to its left.
        const WIDE_CHAR_SPACER = 1 << 9;
        /// Set on the last cell of a row that soft-wrapped (auto-wrap) into the
        /// next — distinguishes a soft wrap from a hard CR/LF line-end so reflow
        /// (#7) can merge and re-split logical lines.
        const WRAPLINE = 1 << 10;
        // bits 11..=15 reserved (underline style/colour, hyperlink id).
    }
}

// --- packed bit layout (#44) ----------------------------------------------
//
// Three 32-bit words mirroring xterm.js's `BufferLine` cell (verified against
// `xtermjs/xterm.js@master` `src/common/buffer/Constants.ts`). The fg/bg colour
// words are byte-identical to xterm's `Attributes` + `FgFlags`/`BgFlags`; the
// content word keeps justerm's explicit layout-marker flags where xterm stores a
// 2-bit `wcwidth` value (justerm's model is flag-based — the spacer and per-cell
// WRAPLINE are load-bearing for overwrite/selection/reflow).
//
//   content u32: codepoint(21) | COMBINED(1) | WIDE | SPACER | WRAP | reserved
//   fg/bg   u32: colour value(24) | colour mode(2) | flags(6)
//
// COMBINED_PRESENT (content) is live as of slice B: combining clusters live in
// the row's column-keyed map, and this bit gates every read of it. LINK_PRESENT
// (bg, xterm's HAS_EXTENDED slot) stays dormant until slice C, where `link` moves
// out of the cell into a per-row map the same way.

const CODEPOINT_MASK: u32 = 0x001F_FFFF; // bits 0..21
const C_COMBINED: u32 = 1 << 21; // a combining cluster lives in the row's map at this column (#45)
const C_WIDE: u32 = 1 << 22;
const C_SPACER: u32 = 1 << 23;
const C_WRAP: u32 = 1 << 24;
const CONTENT_MARKER_MASK: u32 = C_WIDE | C_SPACER | C_WRAP;

const COLOR_VALUE_MASK: u32 = 0x00FF_FFFF; // bits 0..24
const COLOR_MODE_SHIFT: u32 = 24; // bits 24..26
const CM_DEFAULT: u32 = 0;
const CM_INDEXED: u32 = 1;
const CM_RGB: u32 = 2;

// fg flags, bits 26..32 — xterm FgFlags order (HIDDEN == xterm INVISIBLE).
const FG_INVERSE: u32 = 1 << 26;
const FG_BOLD: u32 = 1 << 27;
const FG_UNDERLINE: u32 = 1 << 28;
const FG_BLINK: u32 = 1 << 29;
const FG_HIDDEN: u32 = 1 << 30;
const FG_STRIKE: u32 = 1 << 31;
const FG_FLAG_MASK: u32 =
    FG_INVERSE | FG_BOLD | FG_UNDERLINE | FG_BLINK | FG_HIDDEN | FG_STRIKE;

// bg flags, bits 26..28 — xterm BgFlags order.
const BG_ITALIC: u32 = 1 << 26;
const BG_DIM: u32 = 1 << 27;
#[allow(dead_code)] // lit by slice C (xterm BgFlags::HAS_EXTENDED)
const BG_LINK: u32 = 1 << 28;
const BG_FLAG_MASK: u32 = BG_ITALIC | BG_DIM;

/// Pack a colour reference into the low 26 bits of a colour word (mode + value);
/// the high 6 bits are left for the SGR flags.
fn pack_color(c: Color) -> u32 {
    match c {
        Color::Default => CM_DEFAULT << COLOR_MODE_SHIFT,
        Color::Indexed(i) => (CM_INDEXED << COLOR_MODE_SHIFT) | i as u32,
        Color::Rgb(r, g, b) => {
            (CM_RGB << COLOR_MODE_SHIFT) | (r as u32) << 16 | (g as u32) << 8 | b as u32
        }
    }
}

/// Inverse of [`pack_color`] — reads only the mode + value bits, ignoring the
/// flag bits that share the word.
fn unpack_color(w: u32) -> Color {
    match (w >> COLOR_MODE_SHIFT) & 0b11 {
        CM_INDEXED => Color::Indexed((w & 0xFF) as u8),
        CM_RGB => Color::Rgb((w >> 16) as u8, (w >> 8) as u8, w as u8),
        _ => Color::Default, // CM_DEFAULT (and the unused mode 3) resolve to Default
    }
}

/// Scatter a `CellFlags` bit set (as a `u32`) into the three words' flag-bit
/// positions: `(content_markers, fg_flags, bg_flags)`. Branchless — each group is
/// masked and shifted in one step. The `CellFlags` bit values are frozen by the
/// wire format (`serialize` encodes `flags().bits()`), so the source positions are
/// fixed; see the shift comments. One place for store / insert / remove to share.
#[inline]
fn flag_words(f: u32) -> (u32, u32, u32) {
    let content = (f & 0x0700) << 14; // WIDE/SPACER/WRAP bits 8,9,10 -> 22,23,24
    let fg = ((f & 0x0001) << 27)     // BOLD     bit 0  -> 27
        | ((f & 0x0020) << 21)        // INVERSE  bit 5  -> 26
        | ((f & 0x0018) << 25)        // UNDERLINE/BLINK bits 3,4 -> 28,29
        | ((f & 0x00C0) << 24); // HIDDEN/STRIKE   bits 6,7 -> 30,31
    let bg = ((f & 0x0004) << 24)     // ITALIC bit 2 -> 26
        | ((f & 0x0002) << 26); // DIM    bit 1 -> 27
    (content, fg, bg)
}

/// One character position: a base glyph, fg/bg colour references, flags, and
/// optional references to a grapheme cluster's combining marks and an OSC 8
/// hyperlink. Stored as the packed words above; all access is through the
/// accessor seam (#44), construction through [`Cell::from_parts`] or
/// [`Cell::default`].
///
/// `Eq` is a derived bitwise compare, which is exact because the packing is
/// canonical — every logical cell maps to one bit pattern (unused bits stay 0).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    content: u32,
    fg: u32,
    bg: u32,
    link: Option<core::num::NonZeroU32>,
}

impl Default for Cell {
    fn default() -> Self {
        // The packed form of a blank cell: ' ' (U+0020) in the codepoint field,
        // every other field zero (Default colours, no flags, no combining bit, no
        // link). Built directly rather than through `from_parts` so scroll/erase
        // blanking — which constructs defaults by the rowful — stays a cheap copy.
        Cell { content: ' ' as u32, fg: 0, bg: 0, link: None }
    }
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cell")
            .field("c", &self.c())
            .field("fg", &self.fg())
            .field("bg", &self.bg())
            .field("flags", &self.flags())
            .field("combined", &self.is_combined())
            .field("link", &self.link())
            .finish()
    }
}

impl Cell {
    /// Assemble a cell from its logical parts. The single construction seam —
    /// `Pen::cell` and the wire decoder funnel through here, so the bit-packing
    /// lives in exactly one place (#44).
    pub fn from_parts(
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        link: Option<core::num::NonZeroU32>,
    ) -> Self {
        let mut cell = Cell {
            content: c as u32, // a `char` is <= U+10FFFF, so it fits the 21-bit field
            fg: pack_color(fg),
            bg: pack_color(bg),
            link,
        };
        cell.store_flags(flags);
        cell
    }

    /// Replace the flag bits across the three words from `flags`, preserving the
    /// codepoint, colours, and the dormant presence bits. The inverse is
    /// [`Cell::flags`].
    fn store_flags(&mut self, flags: CellFlags) {
        let (content, fg, bg) = flag_words(flags.bits() as u32);
        self.content = (self.content & !CONTENT_MARKER_MASK) | content;
        self.fg = (self.fg & !FG_FLAG_MASK) | fg;
        self.bg = (self.bg & !BG_FLAG_MASK) | bg;
    }

    /// The base code point.
    pub fn c(&self) -> char {
        char::from_u32(self.content & CODEPOINT_MASK)
            .expect("codepoint bits always hold a valid char")
    }

    /// The foreground colour reference.
    pub fn fg(&self) -> Color {
        unpack_color(self.fg)
    }

    /// The background colour reference.
    pub fn bg(&self) -> Color {
        unpack_color(self.bg)
    }

    /// The cell's flags (SGR attributes + layout markers), reassembled from the
    /// three words — the branchless inverse of [`Cell::store_flags`].
    pub fn flags(&self) -> CellFlags {
        let bits = ((self.content & CONTENT_MARKER_MASK) >> 14)         // 22,23,24 -> 8,9,10
            | ((self.fg & FG_BOLD) >> 27)                              // 27 -> 0
            | ((self.fg & FG_INVERSE) >> 21)                           // 26 -> 5
            | ((self.fg & (FG_UNDERLINE | FG_BLINK)) >> 25)            // 28,29 -> 3,4
            | ((self.fg & (FG_HIDDEN | FG_STRIKE)) >> 24)              // 30,31 -> 6,7
            | ((self.bg & BG_ITALIC) >> 24)                           // 26 -> 2
            | ((self.bg & BG_DIM) >> 26); // 27 -> 1
        CellFlags::from_bits_retain(bits as u16)
    }

    /// Does this column carry combining marks? When true, the cluster lives in
    /// the row's combining map at this column (#45) — a flag-gated cache: never
    /// read the map without first checking this bit.
    pub fn is_combined(&self) -> bool {
        self.content & C_COMBINED != 0
    }

    /// 1-based index into the hyperlink side-table (OSC 8), or `None`. Set on
    /// every cell printed while a hyperlink is open; the index travels with the
    /// cell through scroll/shift/reflow (#26).
    pub fn link(&self) -> Option<core::num::NonZeroU32> {
        self.link
    }

    /// Overwrite the base code point, preserving the layout markers.
    pub fn set_c(&mut self, c: char) {
        self.content = (self.content & !CODEPOINT_MASK) | c as u32;
    }

    /// Overwrite the background colour (the BCE erase fill, #16), preserving the
    /// bg-word flag bits.
    pub fn set_bg(&mut self, bg: Color) {
        self.bg = pack_color(bg) | (self.bg & !(COLOR_VALUE_MASK | (0b11 << COLOR_MODE_SHIFT)));
    }

    /// Mark (or unmark) this column as carrying combining marks in the row map.
    pub fn set_combined(&mut self, on: bool) {
        if on {
            self.content |= C_COMBINED;
        } else {
            self.content &= !C_COMBINED;
        }
    }

    /// Set (or clear) the hyperlink side-table index.
    pub fn set_link(&mut self, link: Option<core::num::NonZeroU32>) {
        self.link = link;
    }

    /// Add the given flags (leaving the others set). Sets the word bits directly —
    /// no round-trip through `flags()`/`store_flags`.
    pub fn insert_flags(&mut self, flags: CellFlags) {
        let (content, fg, bg) = flag_words(flags.bits() as u32);
        self.content |= content;
        self.fg |= fg;
        self.bg |= bg;
    }

    /// Clear the given flags (leaving the others as they are).
    pub fn remove_flags(&mut self, flags: CellFlags) {
        let (content, fg, bg) = flag_words(flags.bits() as u32);
        self.content &= !content;
        self.fg &= !fg;
        self.bg &= !bg;
    }

    /// Reset to a blank default cell.
    pub fn reset(&mut self) {
        *self = Cell::default();
    }

    /// Is this the lead cell of a width-2 glyph? Direct content-bit query — the
    /// hot overwrite/erase/reflow paths use this instead of reconstructing the
    /// full `flags()` to test one marker.
    pub fn is_wide(&self) -> bool {
        self.content & C_WIDE != 0
    }

    /// Is this the trailing spacer cell of a width-2 glyph?
    pub fn is_wide_spacer(&self) -> bool {
        self.content & C_SPACER != 0
    }

    /// Did this row soft-wrap into the next (WRAPLINE on its last cell)?
    pub fn is_wrapline(&self) -> bool {
        self.content & C_WRAP != 0
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, CellFlags};
    use crate::color::Color;
    use core::num::NonZeroU32;

    /// Size pin: slice B moves `extra` out of the cell into the row's combining
    /// map (a cell now signals combining with only the `COMBINED_PRESENT` content
    /// bit), so `Cell` is 16 bytes — the three packed words plus the `link` index.
    /// Flood throughput is memory-bandwidth-bound, so this size is touched on every
    /// print/scroll-blank; slice C moves `link` out too to reach 12. [#42, #45]
    #[test]
    fn cell_is_16_bytes() {
        assert_eq!(std::mem::size_of::<Cell>(), 16);
    }

    /// The packing must be lossless: every colour reference read back equal in
    /// both the fg and bg word, including the tag-distinguished trio that must not
    /// collapse (`Default` / `Indexed(0)` / `Rgb(0,0,0)`).
    #[test]
    fn every_colour_round_trips_in_both_words() {
        let colours = [
            Color::Default,
            Color::Indexed(0),
            Color::Indexed(255),
            Color::Rgb(0, 0, 0),
            Color::Rgb(255, 128, 1),
        ];
        for &fg in &colours {
            for &bg in &colours {
                let cell = Cell::from_parts('x', fg, bg, CellFlags::empty(), None);
                assert_eq!(cell.fg(), fg, "fg {fg:?} / bg {bg:?}");
                assert_eq!(cell.bg(), bg, "fg {fg:?} / bg {bg:?}");
            }
        }
    }

    /// Every flag bit — SGR attributes (split across the fg/bg words) and the
    /// layout markers (in the content word) — round-trips, alone and combined.
    #[test]
    fn every_flag_round_trips() {
        let all = CellFlags::all();
        for bit in all.iter() {
            let cell = Cell::from_parts('x', Color::Default, Color::Default, bit, None);
            assert_eq!(cell.flags(), bit, "single {bit:?}");
        }
        let cell = Cell::from_parts('x', Color::Default, Color::Default, all, None);
        assert_eq!(cell.flags(), all, "all flags at once");
    }

    /// The codepoint occupies 21 bits — the full Unicode range, up to the
    /// maximum scalar value, survives alongside flags set in the same word.
    #[test]
    fn codepoint_round_trips_to_the_unicode_max() {
        for c in ['a', ' ', '한', '🦀', '\u{10FFFF}'] {
            let cell = Cell::from_parts(c, Color::Default, Color::Default, CellFlags::WIDE_CHAR, None);
            assert_eq!(cell.c(), c, "codepoint {c:?}");
            assert!(cell.flags().contains(CellFlags::WIDE_CHAR));
        }
    }

    /// The `link` index, the combining-presence bit, and the spacer query are
    /// independent and survive construction. The combining bit shares the content
    /// word with the codepoint and layout markers, so toggling it must not disturb
    /// them.
    #[test]
    fn link_combined_bit_and_spacer_query_round_trip() {
        let link = NonZeroU32::new(42);
        let mut cell = Cell::from_parts('e', Color::Default, Color::Default, CellFlags::WIDE_CHAR, link);
        assert_eq!(cell.link(), link);
        assert!(!cell.is_combined());

        cell.set_combined(true);
        assert!(cell.is_combined());
        assert_eq!(cell.c(), 'e', "codepoint intact through the combined bit");
        assert!(cell.flags().contains(CellFlags::WIDE_CHAR), "marker intact");
        assert_eq!(cell.link(), link, "link intact");

        cell.set_combined(false);
        assert!(!cell.is_combined());
        assert_eq!(cell.c(), 'e');

        let spacer = Cell::from_parts(' ', Color::Default, Color::Default, CellFlags::WIDE_CHAR_SPACER, None);
        assert!(spacer.is_wide_spacer());
        assert!(!Cell::default().is_wide_spacer());
    }
}
