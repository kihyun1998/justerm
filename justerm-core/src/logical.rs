//! Viewport logical lines (#113, ADR-0017): soft-wrap-joined text plus a
//! per-char map back to viewport cells. This is the buffer-wide *mechanism* a
//! frame-mode consumer needs for URL detection — the regex and `new URL()`
//! validation stay consumer-side (policy). The cell-aware assembly lives in
//! `term/logical.rs`, the `Term` half of this model; this module is just the returned shape.

/// One soft-wrap-joined logical line touching the viewport.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): nothing outside this crate has a reason to build one.** No
/// public function accepts it — the engine hands it out — and there are zero out-of-crate literal
/// sites, so the attribute would bind nothing it does not already bind.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogicalLine {
    /// The line text: wrap-joined across soft-wrapped rows, wide-char spacers skipped, and
    /// trailing `' '` (U+0020 only — the codepoint a blank cell packs) trimmed, so a printed
    /// trailing U+00A0 / U+3000 survives and a printed trailing ASCII space does not. Not
    /// xterm.js's `BufferLine.translateToString(true)`, which spans one row and keeps a
    /// printed trailing space. See [`docs/agents/reference-facts.md`](https://github.com/kihyun1998/justerm/blob/master/docs/agents/reference-facts.md) § "Trimming a line's end" and
    /// [`docs/map/invariant/only-u0020-can-be-padding.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/invariant/only-u0020-can-be-padding.md).
    pub text: String,
    /// Per `text` char, the viewport cell `(row, col)` it came from. A `row`
    /// outside `0..rows` is off-screen wrapped context (a line that wraps in from
    /// above the top / out past the bottom) — present so a URL spanning the edge
    /// still matches; the consumer highlights only the in-range cells.
    pub cells: Vec<(i32, usize)>,
}
