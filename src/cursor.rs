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
            extra: None,
        }
    }
}

/// The input position, its pending-wrap state, and the current pen.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Deferred last-column wrap (xterm's "wrapnext"). Set when a print fills the
    /// last column: the cursor stays put and the actual line wrap happens on the
    /// *next* print. Eager wrapping here is the classic off-by-one that shifts
    /// lines (see `docs/architecture.md` "Hidden VT state").
    pub pending_wrap: bool,
    pub pen: Pen,
    /// Whether the cursor is shown (DEC ?25). The engine only reports this;
    /// blink is a renderer-local animation, not an engine concern.
    pub visible: bool,
}

impl Cursor {
    /// The cursor's `(row, col)` position.
    pub(crate) fn point(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Set the position, clamped to a `rows` x `cols` screen.
    pub(crate) fn set_point(&mut self, point: (usize, usize), rows: usize, cols: usize) {
        self.row = point.0.min(rows - 1);
        self.col = point.1.min(cols - 1);
    }
}

impl Default for Cursor {
    fn default() -> Self {
        // The cursor starts visible; a manual impl is needed because `bool`'s
        // derived default is `false`.
        Cursor {
            row: 0,
            col: 0,
            pending_wrap: false,
            pen: Pen::default(),
            visible: true,
        }
    }
}
