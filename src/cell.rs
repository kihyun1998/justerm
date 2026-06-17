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

/// One character position: a glyph, fg/bg colour references, and flags.
///
/// `content` is a single `char` for now. Full grapheme clusters (a base plus
/// combining marks, kept in a side-table to stay fixed-width) are a later slice;
/// the field is the seam where that grows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            fg: Color::Default,
            bg: Color::Default,
            flags: CellFlags::empty(),
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
