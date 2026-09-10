//! Viewport logical lines: the soft-wrap-joined text of everything touching the
//! viewport, plus a per-char map back to the cell it came from.
//!
//! The returned shape and *why* this is core rather than consumer are stated in
//! [`crate::logical`]. Read it there; this module is the `Term` half — the cell-aware
//! assembly, which needs the whole buffer and so cannot live in a frame-mode consumer
//! (ADR-0017).
//!
//! Two things are local to this site. It is the **first of the three alt-screen floor
//! misses** (#113): the up-walk into scrollback stops at `abs_floor()`, because on the alt
//! screen that scrollback belongs to the *primary* buffer — see
//! `docs/map/invariant/alt-screen-buffer-floor.md`, where this is site one of the discovery
//! history. And the walk deliberately reaches **past the viewport in both directions**, so a
//! line wrapping in from above the top or out past the bottom still joins whole; the
//! off-screen rows surface as an out-of-range `row` in `LogicalLine::cells` for the consumer
//! to clip.
//!
//! Nothing here is `pub(super)`. The one entry point is public API, and every helper it
//! walks with was already in `walk.rs` before this module existed — which is what made this
//! the cheapest of #584's five slices rather than a measure of its importance.

use crate::logical::LogicalLine;

use super::Term;

impl Term {
    /// The viewport's logical lines (#113/ADR-0017): each line's text plus a
    /// per-char map to its viewport `(row, col)`. Wide-char spacers are skipped
    /// and trailing blanks trimmed (so the text is 1:1 with `cells`). Empty rows
    /// are dropped. The cell-aware assembly the consumer can't do in frame mode.
    ///
    /// **The run walk is unbounded on purpose** (`docs/architecture.md`), and this is the
    /// one of the three walks where that costs anything: normally `O(viewport)`, but on a
    /// buffer whose whole scrollback is one soft-wrapped run it is `O(scrollback)` —
    /// measured at 7.3 ms against 17 µs for the same bytes as short lines. If a bound is
    /// ever wanted, two things are already decided and neither is obvious from here:
    ///
    /// - **It is a sibling, not a parameter.** Adding `max_run: Option<usize>` to this
    ///   signature is a breaking change; the crate's idiom for exactly this is
    ///   [`Term::search`] / [`Term::search_with`], and #844 pinned the growth rule on the
    ///   options struct (*"a new option lands through `..Default::default()`"*). So a
    ///   `viewport_logical_lines_with` is additive — meaning 1.0.0 does not gate it.
    /// - **The hard part is the trim, not the counter** — see the trim below.
    ///
    /// Closed as #206 with the reach measured at zero: nothing outside this crate's tests
    /// and benches calls this today. `benches/wrap_run.rs` re-measures on demand.
    pub fn viewport_logical_lines(&self) -> Vec<LogicalLine> {
        let rows = self.grid.rows();
        let total = self.scrollback.len() + rows;
        let top = self.scrollback.len() - self.display_offset; // abs line of viewport row 0
        let bottom = top + rows; // abs lines [top, bottom) are on screen

        // If viewport row 0 is a wrap-continuation, walk up into scrollback to
        // the logical line's true start so an edge-spanning URL still matches —
        // floored, because on alt that scrollback is the *primary* buffer's.
        let floor = self.abs_floor();
        let mut start = top;
        while start > floor && self.abs_row(start - 1).is_wrapped() {
            start -= 1;
        }

        let mut out = Vec::new();
        let mut line = start;
        while line < bottom {
            // Accumulate one logical line forward while each row soft-wraps; the
            // tail may run past `bottom` (off-screen below) — included too.
            let mut text = String::new();
            let mut map: Vec<(i32, usize)> = Vec::new();
            let mut cur = line;
            loop {
                let cells = self.abs_line(cur);
                for (col, cell) in cells.iter().enumerate() {
                    if cell.is_spacer() {
                        continue;
                    }
                    // Signed viewport row: < 0 above the top, >= rows below.
                    let vrow = cur as i32 - top as i32;
                    text.push(cell.c());
                    map.push((vrow, col));
                    // Combining marks (#45) ride the same cell — append each and
                    // map it to that cell so `text` stays 1:1 with `cells`.
                    if let Some(marks) = self.combining_at(cur, col) {
                        for &m in marks {
                            text.push(m);
                            map.push((vrow, col));
                        }
                    }
                }
                let soft = self.abs_row(cur).is_wrapped();
                if soft && cur + 1 < total {
                    cur += 1;
                } else {
                    break;
                }
            }
            // Trim trailing blanks (only the last row can have them), keeping
            // `text` and `cells` in lockstep.
            //
            // "Only the last row can have them" is a premise about where the loop above
            // stopped, and it is what a run-length bound (#206) would break: a window that
            // cuts mid-run ends the text at a row that is *not* the logical end, where the
            // padding this trims is not padding. The rule it would then be applying to
            // written content is `only-U+0020-can-be-padding` (#685), so a bound has to
            // re-answer that at the cut point rather than reuse this line.
            let trimmed = text.trim_end_matches(' ');
            map.truncate(trimmed.chars().count());
            text.truncate(trimmed.len());
            if !text.is_empty() {
                out.push(LogicalLine { text, cells: map });
            }
            line = cur + 1;
        }
        out
    }
}
