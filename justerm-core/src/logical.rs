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
    /// xterm pairs it with `Buffer.getWrappedRangeForLine`); and the trim still differs,
    /// but on a **narrower** case than this comment used to name. Until #685 the trim was
    /// `str::trim_end()`, the Unicode `White_Space` property, so it dropped a printed
    /// U+00A0 / U+3000 / U+2003 as well. It now removes only `' '` — the codepoint a blank
    /// cell packs, and therefore the only one that can be padding. What remains is a
    /// printed trailing **ASCII space**, which xterm keeps (it bounds by written extent)
    /// and this cannot, because `Cell` has no bit distinguishing a written `' '` from a
    /// blank. Pinned in `docs/agents/reference-facts.md` § "Trimming a line's end" and
    /// `docs/map/invariant/only-u0020-can-be-padding.md`.
    pub text: String,
    /// Per `text` char, the viewport cell `(row, col)` it came from. A `row`
    /// outside `0..rows` is off-screen wrapped context (a line that wraps in from
    /// above the top / out past the bottom) — present so a URL spanning the edge
    /// still matches; the consumer highlights only the in-range cells.
    pub cells: Vec<(i32, usize)>,
}
