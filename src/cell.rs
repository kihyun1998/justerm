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

/// One character position: a base glyph, fg/bg colour references, flags, and an
/// optional reference to a grapheme cluster's combining marks.
///
/// `c` is the base code point. Combining marks (and ZWJ emoji sequences) attach
/// via `extra` — a 1-based index into the engine's grapheme side-table — so the
/// common single-code-point cell stays small and `Copy` (the index travels with
/// the cell through scrolls/shifts/reflow). `None` for the overwhelming majority
/// of cells. The side-table (not the cell) holds the actual code points; see
/// `term.rs` and the serialization slice (#6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    /// 1-based index into the grapheme side-table for this cell's combining
    /// marks, or `None` when the cell is a single code point.
    pub extra: Option<core::num::NonZeroU32>,
    /// 1-based index into the hyperlink side-table (OSC 8) — set on every cell
    /// printed while a hyperlink is open, `None` otherwise. Like `extra`, the
    /// index travels with the cell through scroll/shift/reflow (#26).
    pub link: Option<core::num::NonZeroU32>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            fg: Color::Default,
            bg: Color::Default,
            flags: CellFlags::empty(),
            extra: None,
            link: None,
        }
    }
}

impl Cell {
    /// Reset to a blank default cell.
    pub fn reset(&mut self) {
        *self = Cell::default();
    }

    /// Is this the trailing spacer cell of a width-2 glyph?
    pub fn is_wide_spacer(&self) -> bool {
        self.flags.contains(CellFlags::WIDE_CHAR_SPACER)
    }
}

#[cfg(test)]
mod tests {
    use super::Cell;

    /// Baseline pin: `Cell` is 24 bytes today (c: char + fg/bg: Color×2 + flags:
    /// u16 + extra/link: Option<NonZeroU32>×2). Flood throughput is
    /// memory-bandwidth-bound, so this size is touched on every print/scroll-blank
    /// — #43 (the deferred pack) drives it toward ~12. This test documents the
    /// starting point and guards against accidental `Cell` bloat. [#42]
    #[test]
    fn cell_is_24_bytes() {
        assert_eq!(std::mem::size_of::<Cell>(), 24);
    }
}
