//! The cursor and its drawing pen.

use crate::cell::{Cell, CellFlags};
use crate::color::Color;

/// The current SGR state — the appearance copied into each printed cell.
///
/// Modelling it as a "template cell" mirrors Alacritty: a later slice can make
/// erase (ED/EL) fill cleared cells with `bg` instead of `Default` and that
/// *is* Background Color Erase (BCE), no structural change. See `term.rs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
}

impl Pen {
    /// Reset to default appearance (SGR 0).
    pub fn reset(&mut self) {
        *self = Pen::default();
    }

    /// Build a cell carrying this pen's appearance and the given glyph.
    pub fn cell(&self, c: char) -> Cell {
        Cell {
            c,
            fg: self.fg,
            bg: self.bg,
            flags: self.flags,
        }
    }
}

/// The input position, its pending-wrap state, and the current pen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Deferred last-column wrap (xterm's "wrapnext"). Set when a print fills the
    /// last column: the cursor stays put and the actual line wrap happens on the
    /// *next* print. Eager wrapping here is the classic off-by-one that shifts
    /// lines (see `docs/architecture.md` "Hidden VT state").
    pub pending_wrap: bool,
    pub pen: Pen,
}
