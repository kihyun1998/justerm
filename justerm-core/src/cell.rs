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
        /// A row that soft-wrapped (auto-wrap) into the next — distinguishing it from a hard
        /// CR/LF line-end so reflow (#7) can merge and re-split logical lines.
        ///
        /// **Wire-only.** The live grid holds this on the `Row` (`Grid::is_row_wrapped`); it used
        /// to live here, where every whole-cell write and clear destroyed it and ordinary typing
        /// in the last column silently split the logical line (#538). The wire has no per-row
        /// slot, so it is derived back onto a span's last cell at encode time — which is why the
        /// storage could move without a format change. On a cell read from the live grid this bit
        /// is never set.
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
// 2-bit `wcwidth` value (justerm's model is flag-based — the spacer markers are load-bearing for
// overwrite/selection/reflow; WRAPLINE is wire-only, the live flag is on the `Row`, #538).
//
//   content u32: codepoint(21) | COMBINED(1) | WIDE | SPACER | WRAP | reserved
//   fg/bg   u32: colour value(24) | colour mode(2) | flags(6)
//
// COMBINED_PRESENT (content) and LINK_PRESENT (bg, xterm's HAS_EXTENDED slot) are
// both live: combining clusters (#45) and OSC 8 hyperlink indices (#46) live in
// per-row, column-keyed maps, and these bits gate every read of them. The cell is
// now pure packed words — three u32, no `Option` field (the epic's 12 B target).

const CODEPOINT_MASK: u32 = 0x001F_FFFF; // bits 0..21
const C_COMBINED: u32 = 1 << 21; // a combining cluster lives in the row's map at this column (#45)
const C_WIDE: u32 = 1 << 22;
const C_SPACER: u32 = 1 << 23;
const C_WRAP: u32 = 1 << 24;
// The vacated column left when a width-2 glyph wraps off the right edge (#113):
// a blank that holds no character but isn't a hard line-end. Unlike C_SPACER it
// has *no wide lead to its left*, so the overwrite/erase repair paths (which key
// off C_SPACER) must not treat it as one — it's a separate marker the text
// extractors skip. Engine-internal: it stays in the content word and never
// reaches `flags()` / the wire (a frame-mode consumer gets the already-correct
// text, and the cell renders as the blank it is).
const C_LEADING_SPACER: u32 = 1 << 25;
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
const FG_FLAG_MASK: u32 = FG_INVERSE | FG_BOLD | FG_UNDERLINE | FG_BLINK | FG_HIDDEN | FG_STRIKE;

// bg flags, bits 26..28 — xterm BgFlags order.
const BG_ITALIC: u32 = 1 << 26;
const BG_DIM: u32 = 1 << 27;
// LINK_PRESENT: an OSC 8 hyperlink index lives in the row's link map at this
// column (#46). Reuses xterm's `BgFlags.HAS_EXTENDED = 0x10000000` (bit 28)
// exactly — in xterm this is a *shared* "extended attrs present" gate (link +
// underline colour). justerm keeps the two concerns in *separate* per-row maps
// (as combining and links are separate, #520/ADR-none), so it gates each with
// its own bit rather than xterm's one shared object.
const BG_LINK: u32 = 1 << 28;
// UCOLOR_PRESENT (#520): a non-default underline colour (SGR 58) lives in the
// row's ucolor map at this column. Its own presence bit, gating a separate map —
// the 12-byte cell (three packed words) has no room for a fourth colour, so the
// colour rides a side map exactly as the hyperlink does (bits 30,31 stay free
// for a later underline *style* / a second extended attr).
const BG_UCOLOR: u32 = 1 << 29;
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

/// One character position: a base glyph, fg/bg colour references, and flags.
/// Combining marks (#45) and an OSC 8 hyperlink (#46) attach via per-row maps,
/// signalled by the `COMBINED_PRESENT` / `LINK_PRESENT` bits — the cell itself is
/// three packed words, no `Option` field. All access is through the accessor seam
/// (#44); construct with [`Cell::from_parts`] or [`Cell::default`].
///
/// `Eq` is a derived bitwise compare, which is exact because the packing is
/// canonical — every logical cell maps to one bit pattern (unused bits stay 0).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    content: u32,
    fg: u32,
    bg: u32,
}

impl Default for Cell {
    fn default() -> Self {
        // The packed form of a blank cell: ' ' (U+0020) in the codepoint field,
        // every other word zero (Default colours, no flags, no combining/link
        // bits). Built directly rather than through `from_parts` so scroll/erase
        // blanking — which constructs defaults by the rowful — stays a cheap copy.
        Cell {
            content: ' ' as u32,
            fg: 0,
            bg: 0,
        }
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
            .field("linked", &self.is_linked())
            .finish()
    }
}

impl Cell {
    /// Assemble a cell from its logical parts. The single construction seam —
    /// `Pen::cell` and the wire decoder funnel through here, so the bit-packing
    /// lives in exactly one place (#44).
    pub fn from_parts(c: char, fg: Color, bg: Color, flags: CellFlags) -> Self {
        let mut cell = Cell {
            content: c as u32, // a `char` is <= U+10FFFF, so it fits the 21-bit field
            fg: pack_color(fg),
            bg: pack_color(bg),
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

    /// Does this cell hold no **content** — no glyph and no layout marker?
    ///
    /// A blank the app never wrote and one it erased to a coloured background are both blank: the
    /// background is not content. But a wide-char spacer, a leading-spacer wrap artefact, or a
    /// combining-cluster carrier all *mean* something at their column even though their base
    /// code point is a space — they are not blank. Used by reflow to find where a hard-ended
    /// line ends (mirrors xterm.js `getTrimmedLength` / alacritty `line_length`, which likewise
    /// test content, not the background); it says nothing about a cell's colour.
    pub fn is_blank(&self) -> bool {
        // Space codepoint, and none of the content-marker bits set. `content` holds the codepoint
        // plus the COMBINED / WIDE / SPACER / WRAP / LEADING_SPACER markers, so a single check on
        // the whole word covers every "means something here" case at once.
        self.content & (CODEPOINT_MASK | CONTENT_MARKER_MASK | C_COMBINED | C_LEADING_SPACER)
            == ' ' as u32
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
    /// three words — the branchless inverse of `Cell::store_flags`.
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

    /// Does this column carry an OSC 8 hyperlink? When true, the hyperlink-pool
    /// index lives in the row's link map at this column (#46) — flag-gated like
    /// combining: never read the link map without first checking this bit.
    pub fn is_linked(&self) -> bool {
        self.bg & BG_LINK != 0
    }

    /// Does this column carry a non-default underline colour (SGR 58, #520)? When
    /// true, the `Color` reference lives in the row's ucolor map at this column —
    /// flag-gated exactly like the hyperlink: never read the ucolor map without
    /// first checking this bit.
    pub fn is_ucolored(&self) -> bool {
        self.bg & BG_UCOLOR != 0
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

    /// Mark (or unmark) this column as carrying an OSC 8 hyperlink in the row map.
    pub fn set_linked(&mut self, on: bool) {
        if on {
            self.bg |= BG_LINK;
        } else {
            self.bg &= !BG_LINK;
        }
    }

    /// Mark (or unmark) this column as carrying a non-default underline colour in
    /// the row's ucolor map (#520). Mirror of [`Cell::set_linked`].
    pub fn set_ucolored(&mut self, on: bool) {
        if on {
            self.bg |= BG_UCOLOR;
        } else {
            self.bg &= !BG_UCOLOR;
        }
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

    /// Reset to a blank **default** cell — default background included.
    ///
    /// That is rarely what a terminal operation wants on its own: a blank the engine creates
    /// carries the current background (BCE for an erase, and the same for a structural repair,
    /// #530). Callers pair this with `set_bg`; `Term::free_cell` and the erase paths are the
    /// places that do. Using it bare leaves an uncoloured notch in a coloured run.
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

    /// Is this the blank column vacated when a wide glyph wrapped off the right
    /// edge (#113)? It holds no character; unlike a trailing spacer it has no
    /// wide lead to its left, so only the *text* extractors skip it.
    pub fn is_leading_spacer(&self) -> bool {
        self.content & C_LEADING_SPACER != 0
    }

    /// Does this column hold no text — either half of a wide glyph's trailing
    /// spacer or a wide-wrap leading spacer? Used by the text extractors (search,
    /// selection text, logical lines) to skip non-character columns.
    pub fn is_spacer(&self) -> bool {
        self.content & (C_SPACER | C_LEADING_SPACER) != 0
    }

    /// Mark this column as the leading spacer of a wrapped wide glyph.
    ///
    /// **Records** that the column is blank; it does not make it so. The caller must have
    /// written the blank first — this only ORs a marker onto whatever cell is there. Setting
    /// it over a live glyph leaves a cell the text extractors skip while a renderer still
    /// draws it, which is exactly the defect #528 fixed (`Term::vacate_for_wrap` is the one
    /// place that establishes the precondition).
    /// Drop the leading-spacer marker, leaving the cell otherwise untouched.
    ///
    /// The marker is only meaningful on a row that soft-wraps — it records that a width-2 glyph
    /// could not fit and moved on. When the wrap ends, or when an erase blanks the column the
    /// marker described, it has to go, or the text extractors keep skipping a column that is now
    /// a real blank (#538).
    pub fn clear_leading_spacer(&mut self) {
        self.content &= !C_LEADING_SPACER;
    }

    pub fn set_leading_spacer(&mut self) {
        self.content |= C_LEADING_SPACER;
    }

    /// Does this **wire** cell end a soft-wrapped row? See `CellFlags::WRAPLINE` — on the live
    /// grid this is always false and `Grid::is_row_wrapped` is the question to ask (#538).
    pub fn is_wrapline(&self) -> bool {
        self.content & C_WRAP != 0
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, CellFlags};
    use crate::color::Color;

    /// Size pin: slice C moves `link` out of the cell into the row's link map (a
    /// cell now signals a hyperlink with only the `LINK_PRESENT` bg bit), so `Cell`
    /// is **12 bytes** — three packed `u32` words, matching xterm.js's `BufferLine`
    /// cell. This is the epic's target (#43): combining and link both ride per-row
    /// maps, the cell is pure packed words. Flood throughput is
    /// memory-bandwidth-bound, so this size is touched on every print/scroll-blank.
    /// [#42, #46]
    #[test]
    fn cell_is_12_bytes() {
        assert_eq!(std::mem::size_of::<Cell>(), 12);
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
                let cell = Cell::from_parts('x', fg, bg, CellFlags::empty());
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
            let cell = Cell::from_parts('x', Color::Default, Color::Default, bit);
            assert_eq!(cell.flags(), bit, "single {bit:?}");
        }
        let cell = Cell::from_parts('x', Color::Default, Color::Default, all);
        assert_eq!(cell.flags(), all, "all flags at once");
    }

    /// The codepoint occupies 21 bits — the full Unicode range, up to the
    /// maximum scalar value, survives alongside flags set in the same word.
    #[test]
    fn codepoint_round_trips_to_the_unicode_max() {
        for c in ['a', ' ', '한', '🦀', '\u{10FFFF}'] {
            let cell = Cell::from_parts(c, Color::Default, Color::Default, CellFlags::WIDE_CHAR);
            assert_eq!(cell.c(), c, "codepoint {c:?}");
            assert!(cell.flags().contains(CellFlags::WIDE_CHAR));
        }
    }

    /// The combining-presence bit (content word) and link-presence bit (bg word)
    /// are independent of each other, of the codepoint/markers, of the colours, and
    /// of the SGR flags — toggling one must disturb none of the others.
    #[test]
    fn combined_and_linked_bits_are_independent() {
        let mut cell = Cell::from_parts(
            'e',
            Color::Indexed(3),
            Color::Rgb(1, 2, 3),
            CellFlags::WIDE_CHAR | CellFlags::DIM,
        );
        assert!(!cell.is_combined());
        assert!(!cell.is_linked());

        cell.set_combined(true);
        cell.set_linked(true);
        assert!(cell.is_combined() && cell.is_linked());
        // Everything else survives both bits being set.
        assert_eq!(cell.c(), 'e');
        assert_eq!(cell.fg(), Color::Indexed(3));
        assert_eq!(
            cell.bg(),
            Color::Rgb(1, 2, 3),
            "link bit shares the bg word"
        );
        assert!(cell.flags().contains(CellFlags::WIDE_CHAR | CellFlags::DIM));

        cell.set_linked(false);
        assert!(cell.is_combined() && !cell.is_linked());
        cell.set_combined(false);
        assert!(!cell.is_combined() && !cell.is_linked());
        assert_eq!(
            cell.bg(),
            Color::Rgb(1, 2, 3),
            "bg colour intact after clearing"
        );

        let spacer = Cell::from_parts(
            ' ',
            Color::Default,
            Color::Default,
            CellFlags::WIDE_CHAR_SPACER,
        );
        assert!(spacer.is_wide_spacer());
        assert!(!Cell::default().is_wide_spacer());
    }

    /// The underline-colour presence bit (#520) is its own bg-word bit, independent
    /// of the link bit that shares the word and of the bg colour value — toggling
    /// it disturbs neither, and it does NOT grow the cell (the 12-byte pin above
    /// still holds because the colour rides a side map, not the cell).
    #[test]
    fn the_ucolor_presence_bit_is_independent_of_the_link_bit_and_bg_colour() {
        let mut cell = Cell::from_parts('u', Color::Default, Color::Rgb(1, 2, 3), CellFlags::DIM);
        assert!(!cell.is_ucolored());
        assert!(!cell.is_linked());

        cell.set_ucolored(true);
        cell.set_linked(true);
        assert!(cell.is_ucolored() && cell.is_linked());
        // The bg colour and the DIM flag (both in the bg word) survive both bits.
        assert_eq!(cell.bg(), Color::Rgb(1, 2, 3));
        assert!(cell.flags().contains(CellFlags::DIM));

        // Clearing one leaves the other and the colour intact.
        cell.set_linked(false);
        assert!(cell.is_ucolored() && !cell.is_linked());
        cell.set_ucolored(false);
        assert!(!cell.is_ucolored());
        assert_eq!(
            cell.bg(),
            Color::Rgb(1, 2, 3),
            "bg colour intact after clearing"
        );
    }
}
