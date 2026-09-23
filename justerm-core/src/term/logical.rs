//! Viewport logical lines: the soft-wrap-joined text of everything touching the
//! viewport, plus a per-char map back to the cell it came from.
//!
//! The returned shape and *why* this is core rather than consumer are stated in
//! [`crate::logical`]. Read it there; this module is the `Term` half — the cell-aware
//! assembly, which needs the whole buffer and so cannot live in a frame-mode consumer
//! ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)).
//!
//! The up-walk into scrollback stops at `abs_floor()` (on the alt screen that scrollback is
//! the primary buffer's — `docs/map/invariant/alt-screen-buffer-floor.md`), and the walk
//! deliberately reaches **past the viewport in both directions**, so an edge-spanning line
//! joins whole; the off-screen rows surface as an out-of-range `row` in `LogicalLine::cells`.

use crate::logical::LogicalLine;

use super::Term;

impl Term {
    /// The viewport's logical lines ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)): each line's text plus a
    /// per-char map to its viewport `(row, col)`. Wide-char spacers are skipped
    /// and trailing blanks trimmed (so the text is 1:1 with `cells`). Empty rows
    /// are dropped. The cell-aware assembly the consumer can't do in frame mode.
    ///
    /// **The run walk is unbounded on purpose** ([`docs/architecture.md`](https://github.com/kihyun1998/justerm/blob/master/docs/architecture.md)): normally
    /// `O(viewport)`, but `O(scrollback)` on a buffer whose whole scrollback is one
    /// soft-wrapped run (7.3 ms against 17 µs for the same bytes as short lines).
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
            // `text` and `cells` in lockstep. That premise is what a run-length bound (#206)
            // would break — `docs/map/territory/logical-lines.md`.
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
