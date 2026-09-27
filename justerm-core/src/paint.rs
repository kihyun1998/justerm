//! Settled-cell paint ([#967](https://github.com/kihyun1998/justerm/issues/967)): the shapes of the
//! changed-logical-lines query and of the colour write that answers it. A consumer that colours
//! output by a rule of its own — a regular expression over each line — asks which lines changed,
//! matches them itself, and hands the matched spans back; the engine writes the colour into the
//! cells. The rule is policy and stays with the consumer
//! ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md));
//! the `Term` half is `term/paint.rs`.

use crate::color::Color;

/// Where a [`ChangedLine`] was in the buffer when it was reported: the absolute line of its first
/// row in `[scrollback ++ screen]`, the eviction count it is relative to, and which screen's
/// buffer it belongs to. Hand it back to
/// [`Engine::paint_logical_line`](crate::Engine::paint_logical_line) unchanged.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)).** A
/// consumer receives one from `changed_logical_lines` and hands it back — round-trip, not
/// construction, like [`Match`](crate::Match).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineRef {
    /// The absolute buffer line of the logical line's first row.
    pub line: usize,
    /// Lines evicted off the front of the buffer when this was reported — the same count as
    /// [`MarkerIndex::evicted_total`](crate::MarkerIndex::evicted_total).
    pub evicted_total: u64,
    /// Whether the line is on the alternate screen's buffer.
    pub alt: bool,
}

/// One soft-wrap-joined line whose content changed since the last
/// [`Engine::changed_logical_lines`](crate::Engine::changed_logical_lines).
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): nothing
/// outside this crate has a reason to build one.** No public function accepts it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChangedLine {
    /// Where the line is; pass it back to paint it.
    pub at: LineRef,
    /// The line text, built exactly as [`LogicalLine::text`](crate::LogicalLine::text) is:
    /// wrap-joined, wide-char spacers skipped, trailing `' '` trimmed.
    pub text: String,
}

/// A colour to write into the cells behind `text[start..end]` of a [`ChangedLine`], counted in
/// `char`s (Unicode scalar values) of its text. `None` leaves that channel as it is.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)).** A
/// consumer builds one, and the all-`None` `Default` is meaningful — a span that changes nothing —
/// so a new channel lands through `..Default::default()`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PaintSpan {
    /// First `char` of the span.
    pub start: usize,
    /// One past the last `char` of the span; clipped to the text's length.
    pub end: usize,
    /// The foreground colour reference to write, if any.
    pub fg: Option<Color>,
    /// The background colour reference to write, if any.
    pub bg: Option<Color>,
}
