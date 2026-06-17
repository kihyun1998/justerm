//! Engine-owned selection state (see `docs/architecture.md` "Selection").
//!
//! Anchors are stored in **absolute buffer coordinates** — a line index into the
//! concatenated `[scrollback ++ screen]` stream, counted from the oldest line.
//! This coordinate is stable under a normal top-anchored scroll (the evicted
//! line entering scrollback grows `scrollback.len()` by exactly the screen
//! shift, so existing content keeps its absolute index); the only places it
//! moves are cap eviction, in-screen region/RI scrolls, and reflow — each
//! handled explicitly by `Term`. The cell-aware logic (text extraction, range
//! clipping) lives in `term.rs`, where the cells are.

/// What a selection covers.
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
