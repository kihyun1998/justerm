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
    /// The underline colour (SGR 58, #520): what an underline / strikethrough draws
    /// in, independent of `fg`. `Default` means "follow the fg". It is *not* packed
    /// into the printed `Cell` (the 12-byte cell is full); the print path stamps a
    /// non-default value into the row's ucolor map. See `term.rs::write_glyph`.
    pub underline_color: Color,
}

impl Pen {
    /// Reset to default appearance (SGR 0).
    pub fn reset(&mut self) {
        *self = Pen::default();
    }

    /// Build a cell carrying this pen's appearance and the given glyph.
    pub fn cell(&self, c: char) -> Cell {
        Cell::from_parts(c, self.fg, self.bg, self.flags)
    }
}

/// The cursor's drawn shape (DECSCUSR / the renderer's caret glyph). The engine
/// reports it on the frame; the renderer draws it. Default `Block` (#81).
///
/// **Deliberately exhaustive (#843) — and the reason is the wire, not the spec.**
///
/// An earlier draft of that sweep said "DECSCUSR's shape space, closed". **That is
/// false**, and the counter-example is in this repository: `justerm-renderer` has
/// carried a fourth shape, `HollowBlock`, since before the sweep
/// (`justerm-renderer/src/cursor.rs:60`, wire id `3`), and no core frame can ask
/// for it. The space is not closed by the spec; it has already been grown once,
/// one crate over.
///
/// What actually decides it is that **this enum is mapped onto wire values by a
/// `match` outside this crate** — `justerm-wasm-decode/src/lib.rs:198` turns each
/// member into an int for the frame header. Marking it non-exhaustive would force
/// a `_` arm there, converting a future compile error into a silently wrong wire
/// value. That is the same construct that reddened `cargo test --workspace` for
/// [`crate::MarkerKind`], where the rule is stated in full.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
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
    /// Whether the cursor is shown (DEC ?25). The engine only reports it.
    pub visible: bool,
    /// The caret shape (DECSCUSR, #89) — reported on the frame, drawn by the
    /// renderer.
    pub shape: CursorShape,
    /// Whether the caret blinks (att610 ?12, #81). The engine reports the *mode*;
    /// the actual animation is the renderer's.
    pub blink: bool,
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
            shape: CursorShape::Block,
            blink: false,
        }
    }
}
