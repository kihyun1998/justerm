//! The grid — the 2D array of cells representing the current screen.
//!
//! Rows are stored as separate `Vec`s (not one flat buffer) so the scrollback
//! ring (a later slice) can move whole rows in/out cheaply.

use crate::cell::{Cell, CellFlags};

/// One row of cells.
pub type Row = Vec<Cell>;

/// Re-wrap physical `rows` to `new_cols`. Soft-wrapped rows (WRAPLINE on the
/// last cell) are joined into logical lines, then each logical line is re-split
/// at `new_cols` with WRAPLINE set on every segment but the last. Trailing blank
/// rows are absorbed (re-created by the caller's row-count fit). See #7.
///
/// `points` are `(row, col)` coordinates to track through the reflow (the cursor
/// and any selection anchors); the returned `Vec` maps each to its new position,
/// index-aligned with the input.
///
/// Common-90%: trailing blanks on a hard-ended row are trimmed, and a wide-char
/// split across the new boundary is not yet special-cased.
pub(crate) fn reflow(
    rows: Vec<Row>,
    new_cols: usize,
    points: &[(usize, usize)],
) -> (Vec<Row>, Vec<(usize, usize)>) {
    // 1. Join soft-wrapped rows into logical lines, recording each tracked
    //    point's logical coordinate (line index + offset within the line).
    let mut logical: Vec<Vec<Cell>> = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    // Per point: (logical line, offset, found-yet).
    let mut tracked: Vec<(usize, usize, bool)> = vec![(0, 0, false); points.len()];
    for (i, row) in rows.into_iter().enumerate() {
        for (pi, &(pr, pc)) in points.iter().enumerate() {
            if i == pr && !tracked[pi].2 {
                tracked[pi] = (logical.len(), current.len() + pc, true);
            }
        }
        let soft = row
            .last()
            .is_some_and(|c| c.flags.contains(CellFlags::WRAPLINE));
        if soft {
            current.extend(row.into_iter().map(|mut c| {
                c.flags.remove(CellFlags::WRAPLINE);
                c
            }));
        } else {
            let mut cells = row;
            while cells.last() == Some(&Cell::default()) {
                cells.pop();
            }
            current.extend(cells);
            logical.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        logical.push(current);
    }
    // Trailing blank lines are absorbed, not preserved as rows.
    while logical.last().is_some_and(|l| l.is_empty()) {
        logical.pop();
    }

    // 2. Re-split each logical line into `new_cols`-wide rows, mapping each
    //    tracked point to its new (row, col).
    let mut out: Vec<Row> = Vec::new();
    let mut new_points = vec![(0usize, 0usize); points.len()];
    for (li, line) in logical.iter().enumerate() {
        let start = out.len();
        if line.is_empty() {
            out.push(vec![Cell::default(); new_cols]);
        } else {
            let mut i = 0;
            while i < line.len() {
                let mut take = (line.len() - i).min(new_cols);
                // Don't split a wide char from its spacer: if the row would end
                // on a WIDE_CHAR lead, drop it to the next row (xterm's newCols-1).
                if i + take < line.len() && line[i + take - 1].flags.contains(CellFlags::WIDE_CHAR)
                {
                    take -= 1;
                }
                let take = take.max(1); // guard the 1-col degenerate case
                let mut row: Row = line[i..i + take].to_vec();
                row.resize(new_cols, Cell::default());
                i += take;
                if i < line.len() {
                    row[new_cols - 1].flags.insert(CellFlags::WRAPLINE);
                }
                out.push(row);
            }
        }
        for (pi, &(pl, poff, _)) in tracked.iter().enumerate() {
            if pl == li {
                let off = poff.min(line.len());
                new_points[pi] = (start + off / new_cols, off % new_cols);
            }
        }
    }
    // A point whose logical line was trimmed (trailing blank) clamps to the end.
    for (pi, &(pl, _, _)) in tracked.iter().enumerate() {
        if pl >= logical.len() {
            new_points[pi] = (out.len().saturating_sub(1), 0);
        }
    }

    (out, new_points)
}

/// The current screen: `rows` × `cols` cells.
///
/// `lines` is a **row ring**: logical row `r` lives at physical slot `phys(r)`,
/// offset by `zero` (the physical slot of logical row 0). A full-screen scroll
/// advances `zero` in O(1) instead of moving every row (ADR-0009). All accessors
/// map through `phys`; the offset is invisible outside this module.
#[derive(Clone, Debug)]
pub struct Grid {
    cols: usize,
    rows: usize,
    lines: Vec<Row>,
    /// Physical slot of logical row 0. `0..rows`. Advanced by a full-screen
    /// scroll; every accessor maps a logical row through `phys`.
    zero: usize,
}

impl Grid {
    /// A blank grid of the given size.
    pub fn new(cols: usize, rows: usize) -> Self {
        let lines = vec![vec![Cell::default(); cols]; rows];
        Grid {
            cols,
            rows,
            lines,
            zero: 0,
        }
    }

    /// Map a logical row to its physical slot in the ring. `zero + row < 2·rows`
    /// (callers clamp `row < rows`, and `zero < rows`), so one conditional
    /// subtraction replaces a modulo (alacritty `Storage::compute_index`).
    fn phys(&self, row: usize) -> usize {
        let slot = self.zero + row;
        if slot >= self.rows {
            slot - self.rows
        } else {
            slot
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Read a cell. Panics on out-of-bounds (callers clamp to the grid).
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.lines[self.phys(row)][col]
    }

    /// Mutable access to a cell.
    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        let p = self.phys(row);
        &mut self.lines[p][col]
    }

    /// Read a whole row.
    pub fn row(&self, row: usize) -> &[Cell] {
        &self.lines[self.phys(row)]
    }

    /// Mutable access to a whole row — for in-row cell shifts (ICH/DCH).
    pub(crate) fn row_mut(&mut self, row: usize) -> &mut [Cell] {
        let p = self.phys(row);
        &mut self.lines[p]
    }

    /// Scroll the rows `[top..=bottom]` up by one line: the top line of the
    /// region is dropped and a blank line appears at `bottom`. Rows outside the
    /// region are untouched.
    ///
    /// Scrollback retention (when the region is the full screen) is a later
    /// slice (#3) — for now the scrolled-off line is discarded rather than
    /// pushed into a history ring.
    pub fn scroll_up_region(&mut self, top: usize, bottom: usize) {
        if top == 0 && bottom == self.rows - 1 {
            // Full-screen scroll: recycle logical row 0's slot as the new blank
            // bottom and advance `zero` — O(1), no row moves (ADR-0009).
            for cell in &mut self.lines[self.zero] {
                cell.reset();
            }
            self.zero = self.phys(1);
        } else {
            // Sub-region scroll stays O(region). The logical region may straddle
            // the ring's wrap, so shift by *logical*-indexed swaps (alacritty's
            // `Storage::swap`), not a contiguous `rotate_left`.
            for r in top..bottom {
                let (a, b) = (self.phys(r), self.phys(r + 1));
                self.lines.swap(a, b);
            }
            for cell in self.row_mut(bottom) {
                cell.reset();
            }
        }
    }

    /// Full-screen scroll up that **moves** the evicted top row out instead of
    /// copying it: install `blank` as the new bottom line and return logical
    /// row 0 by value (the caller pushes it into scrollback). The grid clears +
    /// fits `blank` to `cols`, so the caller may hand it a dirty recycled row
    /// (reusing its allocation — zero-alloc steady state, ADR-0009). O(1).
    pub(crate) fn scroll_up_recycle(&mut self, mut blank: Row) -> Row {
        blank.clear(); // drop any recycled content (keeps the allocation)
        blank.resize(self.cols, Cell::default());
        let evicted = std::mem::replace(&mut self.lines[self.zero], blank);
        self.zero = self.phys(1);
        evicted
    }

    /// Extract all rows in **logical** order, leaving the grid empty. Used by
    /// `Term::resize` to reflow the screen together with scrollback as one
    /// stream — `reflow` assumes logical order, so the ring is linearized first
    /// (rotate the physical buffer by `zero`, then reset the offset).
    pub(crate) fn take_lines(&mut self) -> Vec<Row> {
        self.lines.rotate_left(self.zero);
        self.zero = 0;
        std::mem::take(&mut self.lines)
    }

    /// Replace the screen with `lines` at `cols` x `rows`: each row is fit to
    /// `cols` and the screen is padded with blank rows / truncated to `rows`.
    pub(crate) fn set_screen(&mut self, mut lines: Vec<Row>, cols: usize, rows: usize) {
        for row in &mut lines {
            row.resize(cols, Cell::default());
        }
        while lines.len() < rows {
            lines.push(vec![Cell::default(); cols]);
        }
        lines.truncate(rows);
        self.lines = lines;
        self.cols = cols;
        self.rows = rows;
        self.zero = 0; // `lines` is freshly built in logical order
    }

    /// Reset every cell to a blank default. Used when switching to the alt
    /// screen (which always starts cleared).
    pub fn clear(&mut self) {
        for row in &mut self.lines {
            for cell in row {
                cell.reset();
            }
        }
        self.zero = 0; // an all-blank grid normalizes the offset
    }

    /// Scroll the rows `[top..=bottom]` down by one line: a blank line appears at
    /// `top` and the bottom region line is dropped. Rows outside are untouched.
    /// Used by RI (reverse index) at the top margin.
    pub fn scroll_down_region(&mut self, top: usize, bottom: usize) {
        if top == 0 && bottom == self.rows - 1 {
            // Full-screen reverse scroll: retreat `zero` and blank the slot that
            // becomes the new logical top — O(1), the mirror of `scroll_up`.
            self.zero = if self.zero == 0 {
                self.rows - 1
            } else {
                self.zero - 1
            };
            for cell in &mut self.lines[self.zero] {
                cell.reset();
            }
        } else {
            // Sub-region: logical-indexed swaps from the bottom up (rotate_right),
            // then blank the logical top. See `scroll_up_region`.
            for r in (top..bottom).rev() {
                let (a, b) = (self.phys(r), self.phys(r + 1));
                self.lines.swap(a, b);
            }
            for cell in self.row_mut(top) {
                cell.reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid whose row `r` carries the char `'a' + r` in column 0 — a distinct
    /// marker per logical row so a scroll's row mapping is observable.
    fn stamped(cols: usize, rows: usize) -> Grid {
        let mut g = Grid::new(cols, rows);
        for r in 0..rows {
            g.cell_mut(r, 0).c = char::from(b'a' + r as u8);
        }
        g
    }

    /// Column-0 chars read top-to-bottom in *logical* row order.
    fn col0(g: &Grid) -> String {
        (0..g.rows()).map(|r| g.cell(r, 0).c).collect()
    }

    #[test]
    fn full_screen_scroll_up_shifts_content_and_blanks_bottom() {
        let mut g = stamped(2, 3); // logical col0 = "abc"
        g.scroll_up_region(0, 2);
        assert_eq!(col0(&g), "bc "); // shifted up, bottom blanked
    }

    #[test]
    fn full_screen_scroll_up_reads_correctly_past_a_full_wrap() {
        // Scroll the whole screen more than `rows` times: a ring's `zero` wraps,
        // so this exercises the logical→physical mapping across the wrap point.
        let mut g = stamped(2, 3); // "abc"
        for _ in 0..3 {
            g.scroll_up_region(0, 2);
        }
        assert_eq!(col0(&g), "   "); // everything scrolled off → all blank
        // And the grid is still usable from a wrapped offset: write + read back.
        g.cell_mut(1, 0).c = 'z';
        assert_eq!(col0(&g), " z ");
    }

    #[test]
    fn full_screen_scroll_down_shifts_content_and_blanks_top() {
        // RI at the top margin: blank appears at the top, the bottom line is lost.
        let mut g = stamped(2, 3); // "abc"
        g.scroll_down_region(0, 2);
        assert_eq!(col0(&g), " ab");
    }

    #[test]
    fn sub_region_scroll_under_a_wrapped_offset_isolates_outside_rows() {
        // The hard ring path: a full-screen scroll advances `zero`, *then* a
        // sub-region scroll must shift the in-region rows (whose physical slots
        // now straddle the wrap) while leaving rows outside the region untouched.
        let mut g = stamped(2, 4); // "abcd"
        g.scroll_up_region(0, 3); // full screen → "bcd ", zero now 1
        assert_eq!(col0(&g), "bcd ");
        g.scroll_up_region(0, 1); // sub-region [0..=1] only
        // rows 0..=1 ("bc") scroll up → "c" then blank; rows 2,3 ("d", blank) stay.
        assert_eq!(col0(&g), "c d ");
    }

    #[test]
    fn scroll_up_recycle_moves_out_row0_and_blanks_a_dirty_recycled_row() {
        let mut g = stamped(2, 3); // "abc"
        // Hand it a *dirty* recycled row (full width, stale content) — the new
        // bottom must come out blank, not carrying the recycled row's text.
        let dirty = vec![
            Cell {
                c: 'X',
                ..Cell::default()
            };
            2
        ];
        let evicted = g.scroll_up_recycle(dirty);
        assert_eq!(evicted[0].c, 'a'); // logical row 0 moved out, not copied
        assert_eq!(col0(&g), "bc "); // shifted up; bottom blank, NOT "bcX"
    }

    #[test]
    fn take_lines_returns_rows_in_logical_order_after_a_scroll() {
        // `reflow` assumes logical order, so `take_lines` must linearize the ring.
        let mut g = stamped(1, 3); // "abc"
        g.scroll_up_region(0, 2); // "bc ", zero now 1 (physically rotated)
        let lines = g.take_lines();
        let got: String = lines.iter().map(|r| r[0].c).collect();
        assert_eq!(got, "bc "); // logical order, not the physical [_, b, c]
    }
}
