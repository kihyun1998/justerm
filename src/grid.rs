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

    /// Scroll the rows `[top..=bottom]` up by one line: the top line of the
    /// region is dropped and a blank line appears at `bottom`. Rows outside the
    /// region are untouched.
    ///
    /// Scrollback retention (when the region is the full screen) is a later
    /// slice (#3) — for now the scrolled-off line is discarded rather than
    /// pushed into a history ring.
    pub fn scroll_up_region(&mut self, top: usize, bottom: usize) {
        // Rotate the region's top line to its bottom, then blank it: every line
        // in the region shifts up one and the region's bottom becomes empty.
        self.lines[top..=bottom].rotate_left(1);
        for cell in &mut self.lines[bottom] {
            cell.reset();
        }
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
