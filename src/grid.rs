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
            .is_some_and(|c| c.flags().contains(CellFlags::WRAPLINE));
        if soft {
            current.extend(row.into_iter().map(|mut c| {
                c.remove_flags(CellFlags::WRAPLINE);
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
                if i + take < line.len() && line[i + take - 1].flags().contains(CellFlags::WIDE_CHAR)
                {
                    take -= 1;
                }
                let take = take.max(1); // guard the 1-col degenerate case
                let mut row: Row = line[i..i + take].to_vec();
                row.resize(new_cols, Cell::default());
                i += take;
                if i < line.len() {
                    row[new_cols - 1].insert_flags(CellFlags::WRAPLINE);
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
#[derive(Clone, Debug)]
pub struct Grid {
    cols: usize,
    rows: usize,
    lines: Vec<Row>,
}

impl Grid {
    /// A blank grid of the given size.
    pub fn new(cols: usize, rows: usize) -> Self {
        let lines = vec![vec![Cell::default(); cols]; rows];
        Grid { cols, rows, lines }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Read a cell. Panics on out-of-bounds (callers clamp to the grid).
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.lines[row][col]
    }

    /// Mutable access to a cell.
    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        &mut self.lines[row][col]
    }

    /// Read a whole row.
    pub fn row(&self, row: usize) -> &[Cell] {
        &self.lines[row]
    }

    /// Mutable access to a whole row — for in-row cell shifts (ICH/DCH).
    pub(crate) fn row_mut(&mut self, row: usize) -> &mut [Cell] {
        &mut self.lines[row]
    }

    /// Scroll the rows `[top..=bottom]` up by one line: the top line of the
    /// region is dropped and a blank line appears at `bottom`. Rows outside the
    /// region are untouched.
    ///
    /// `rotate_left` moves whole-row `Vec` *handles* (24 bytes each), not cell
    /// data — cheap even at the screen's bounded row count, so the per-newline
    /// scrollback cost lives in the *eviction*, not here (see `scroll_up_recycle`
    /// and ADR-0009).
    pub fn scroll_up_region(&mut self, top: usize, bottom: usize) {
        // Rotate the region's top line to its bottom, then blank it: every line
        // in the region shifts up one and the region's bottom becomes empty.
        self.lines[top..=bottom].rotate_left(1);
        for cell in &mut self.lines[bottom] {
            cell.reset();
        }
    }

    /// Full-screen scroll up that **moves** the evicted top row out instead of
    /// copying it (`Term::linefeed`'s hot path): `rotate_left` puts logical row 0
    /// in the bottom slot, then a recycled `blank` is swapped into that slot and
    /// the evicted row returned by value (the caller pushes it into scrollback).
    /// The grid clears + fits `blank` to `cols`, so the caller may hand it a
    /// dirty recycled row — reusing its allocation, so a steady-state flood does
    /// no per-line alloc/copy (ADR-0009). No ring: the win is recycling the row
    /// buffer, not making the cheap handle-rotate O(1).
    pub(crate) fn scroll_up_recycle(&mut self, mut blank: Row) -> Row {
        blank.clear(); // drop any recycled content (keeps the allocation)
        blank.resize(self.cols, Cell::default());
        self.lines.rotate_left(1); // logical row 0 -> the bottom slot
        let last = self.rows - 1;
        std::mem::replace(&mut self.lines[last], blank)
    }

    /// Extract all rows, leaving the grid empty. Used by `Term::resize` to
    /// reflow the screen together with scrollback as one stream.
    pub(crate) fn take_lines(&mut self) -> Vec<Row> {
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
    }

    /// Reset every cell to a blank default. Used when switching to the alt
    /// screen (which always starts cleared).
    pub fn clear(&mut self) {
        for row in &mut self.lines {
            for cell in row {
                cell.reset();
            }
        }
    }

    /// Scroll the rows `[top..=bottom]` down by one line: a blank line appears at
    /// `top` and the bottom region line is dropped. Rows outside are untouched.
    /// Used by RI (reverse index) at the top margin.
    pub fn scroll_down_region(&mut self, top: usize, bottom: usize) {
        // Rotate the region's bottom line to its top, then blank it: every line
        // in the region shifts down one and the region's top becomes empty.
        self.lines[top..=bottom].rotate_right(1);
        for cell in &mut self.lines[top] {
            cell.reset();
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
            g.cell_mut(r, 0).set_c(char::from(b'a' + r as u8));
        }
        g
    }

    /// Column-0 chars read top-to-bottom in *logical* row order.
    fn col0(g: &Grid) -> String {
        (0..g.rows()).map(|r| g.cell(r, 0).c()).collect()
    }

    #[test]
    fn full_screen_scroll_up_shifts_content_and_blanks_bottom() {
        let mut g = stamped(2, 3); // logical col0 = "abc"
        g.scroll_up_region(0, 2);
        assert_eq!(col0(&g), "bc "); // shifted up, bottom blanked
    }

    #[test]
    fn full_screen_scroll_down_shifts_content_and_blanks_top() {
        // RI at the top margin: blank appears at the top, the bottom line is lost.
        let mut g = stamped(2, 3); // "abc"
        g.scroll_down_region(0, 2);
        assert_eq!(col0(&g), " ab");
    }

    #[test]
    fn sub_region_scroll_leaves_rows_outside_the_region_untouched() {
        let mut g = stamped(2, 4); // "abcd"
        g.scroll_up_region(0, 1); // sub-region [0..=1] only
        // rows 0..=1 ("ab") scroll up → "b" then blank; rows 2,3 ("c","d") stay.
        assert_eq!(col0(&g), "b cd");
    }

    #[test]
    fn scroll_up_recycle_moves_out_row0_and_blanks_a_dirty_recycled_row() {
        let mut g = stamped(2, 3); // "abc"
        // Hand it a *dirty* recycled row (full width, stale content) — the new
        // bottom must come out blank, not carrying the recycled row's text.
        let mut x = Cell::default();
        x.set_c('X');
        let dirty = vec![x; 2];
        let evicted = g.scroll_up_recycle(dirty);
        assert_eq!(evicted[0].c(), 'a'); // logical row 0 moved out, not copied
        assert_eq!(col0(&g), "bc "); // shifted up; bottom blank, NOT "bcX"
    }

    #[test]
    fn take_lines_returns_rows_in_logical_order_after_a_scroll() {
        // `reflow` assumes logical row order; `take_lines` must deliver it.
        let mut g = stamped(1, 3); // "abc"
        g.scroll_up_region(0, 2); // "bc "
        let lines = g.take_lines();
        let got: String = lines.iter().map(|r| r[0].c()).collect();
        assert_eq!(got, "bc ");
    }
}
