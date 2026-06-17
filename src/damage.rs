//! Damage tracking — what changed since the last reset, as line + column spans.
//! See ADR-0003 for the model (incremental bounds, ack-gated reset).

/// The damaged column span of a single line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineDamage {
    pub line: usize,
    pub left: usize,
    pub right: usize,
}

/// A first-class scroll: rows `[top..=bottom]` shifted by `count` lines
/// (positive = up, negative = down). The renderer moves the rows instead of
/// redrawing them. Recorded by the engine — which executes the scroll — rather
/// than diff-detected (ADR-0003).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScrollOp {
    pub top: usize,
    pub bottom: usize,
    pub count: isize,
}

/// What changed since the last `reset_damage()`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TermDamage {
    /// The whole screen must be redrawn (flood / resize / alt-screen clear).
    Full,
    /// Only these lines changed, each carrying its damaged column span.
    Partial(Vec<LineDamage>),
}

/// Per-line damage bounds. "Undamaged" is encoded as `left > right`, so an
/// untouched line never reports as damaged and the first `expand` sets a real
/// span. (Mirrors Alacritty's `LineDamageBounds`.)
#[derive(Clone, Copy)]
pub(crate) struct LineBounds {
    left: usize,
    right: usize,
    cols: usize,
}

impl LineBounds {
    pub(crate) fn undamaged(cols: usize) -> Self {
        LineBounds {
            left: cols,
            right: 0,
            cols,
        }
    }

    /// Widen the span to include columns `[left, right]`.
    pub(crate) fn expand(&mut self, left: usize, right: usize) {
        self.left = self.left.min(left);
        self.right = self.right.max(right);
    }

    pub(crate) fn is_damaged(&self) -> bool {
        self.left <= self.right
    }

    pub(crate) fn reset(&mut self) {
        self.left = self.cols;
        self.right = 0;
    }

    pub(crate) fn span(&self) -> (usize, usize) {
        (self.left, self.right)
    }
}
