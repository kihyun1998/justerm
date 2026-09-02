//! Engine-owned selection state (see `docs/architecture.md` "Selection").
//!
//! Anchors are stored in **absolute buffer coordinates** — a line index into the
//! concatenated `[scrollback ++ screen]` stream, counted from the oldest line.
//! This coordinate is stable under a normal top-anchored scroll (the evicted
//! line entering scrollback grows `scrollback.len()` by exactly the screen
//! shift, so existing content keeps its absolute index); the only places it
//! moves are cap eviction, in-screen region/RI scrolls, and reflow — each
//! handled explicitly by `Term`. The cell-aware logic (text extraction, range
//! clipping) lives in `term/selection.rs` — the `Term` half of this model, moved out
//! of `term.rs` in #587.

/// What a selection covers.
///
/// **Deliberately exhaustive (#843), on convergence rather than on traffic.**
///
/// An earlier draft of that sweep argued *"a consumer cannot fall back on a
/// neighbour for one it does not know"* — which describes matching traffic this
/// API does not have. Measured: this type appears in exactly one public
/// signature, as a **parameter** to [`crate::Engine::selection_begin`], and
/// nothing hands one outward. So the attribute would cost a consumer nothing,
/// and that argument cannot be what keeps it off.
///
/// What keeps it off is that the set really is closed: **alacritty arrives at the
/// same four modes independently**, under different names —
/// `Simple` / `Semantic` / `Lines` / `Block`
/// (`alacritty_terminal/src/selection.rs:93`), where its own doc glosses `simple`
/// as tracking cells "without any expansion" ([`Char`](Self::Char)) and
/// `semantic` as expanding "to the nearest semantic escape char"
/// ([`Word`](Self::Word)). Two implementations landing on one partition of the
/// space is the non-arbitrariness signal, and it is a stronger ground than the
/// one it replaces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionType {
    /// Contiguous run, wrapping line to line.
    Char,
    /// Expanded to word boundaries.
    Word,
    /// Whole lines.
    Line,
    /// Rectangular column range on every line.
    Block,
}

/// Which half of a cell an anchor sits on — the left or right edge. Lets a drag
/// include or exclude the cell under the pointer (mouse precision).
///
/// **Deliberately exhaustive (#843).** Closed by geometry — there is no third side.
/// Left exhaustive on purpose, not by omission.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

/// One highlighted run on a single **viewport** row: columns `left..=right`
/// (both inclusive). `selection_range` returns one per visible row the selection
/// touches — the renderer paints these. Off-screen rows are not emitted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectionSpan {
    pub row: usize,
    pub left: usize,
    pub right: usize,
}

/// A point in absolute buffer coordinates: `line` indexes `[scrollback ++ screen]`
/// from the oldest line, `col` is the column.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct BufferPoint {
    pub line: usize,
    pub col: usize,
}

/// A selection endpoint: a buffer point plus which side of the cell it touches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Anchor {
    pub point: BufferPoint,
    pub side: Side,
}

/// The live selection: where the drag began (`anchor`) and where it currently
/// reaches (`focus`). Either may be the earlier point — `ordered` sorts them.
pub(crate) struct Selection {
    pub ty: SelectionType,
    pub anchor: Anchor,
    pub focus: Anchor,
}

impl Selection {
    /// The two anchors sorted so the first is the earlier buffer point.
    pub fn ordered(&self) -> (Anchor, Anchor) {
        if self.anchor.point <= self.focus.point {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}
