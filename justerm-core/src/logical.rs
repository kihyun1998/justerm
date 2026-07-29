//! Viewport logical lines (#113, ADR-0017): soft-wrap-joined text plus a
//! per-char map back to viewport cells. This is the buffer-wide *mechanism* a
//! frame-mode consumer needs for URL detection — the regex and `new URL()`
//! validation stay consumer-side (policy). It also serves the a11y screen-reader
//! mirror (#119). The cell-aware assembly lives in `term/logical.rs` — the `Term` half
//! of this model, moved out of `term.rs` in #601; this module is just the returned shape.

/// One soft-wrap-joined logical line touching the viewport.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogicalLine {
    /// The line text: wrap-joined across soft-wrapped rows, wide-char spacers
    /// skipped, trailing blanks trimmed. This is **not** the equivalence to xterm.js's
    /// `BufferLine.translateToString(true)` an earlier version of this comment claimed. The
    /// spacer skip matches; the wrap join does not (that method spans one `BufferLine` —
    /// xterm pairs it with `Buffer.getWrappedRangeForLine`); and the trim differs on a line
    /// ending in *printed* spaces, which xterm keeps and `trim_end` drops. Pinned in
    /// `docs/agents/reference-facts.md` § "Logical-line assembly".
    pub text: String,
    /// Per `text` char, the viewport cell `(row, col)` it came from. A `row`
    /// outside `0..rows` is off-screen wrapped context (a line that wraps in from
    /// above the top / out past the bottom) — present so a URL spanning the edge
    /// still matches; the consumer highlights only the in-range cells.
    pub cells: Vec<(i32, usize)>,
}
