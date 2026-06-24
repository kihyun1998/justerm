//! The grid — the 2D array of cells representing the current screen.
//!
//! Rows are stored as separate `Vec`s (not one flat buffer) so the scrollback
//! ring (a later slice) can move whole rows in/out cheaply.

use crate::cell::{Cell, CellFlags};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

/// A row's combining clusters: column → the combining marks attached to that
/// column's base glyph. Sparse (most rows have none) and **flag-gated** — an
/// entry is only ever read when the cell at that column has its
/// `COMBINED_PRESENT` bit set (xterm's `_combined` invariant, #45). Stale entries
/// left by an overwrite/erase are therefore harmless; only live entries must be
/// carried when cells move column (ICH/DCH/reflow).
type Combining = BTreeMap<usize, Vec<char>>;

/// One row of cells **plus** its per-row, column-keyed combining map.
///
/// The map rides with the row through scroll / scrollback / reflow for free (the
/// row is the unit that moves), which is why combining lives here rather than in a
/// global pool — no leak, cleared on row reuse (#45). `Row` derefs to `[Cell]`, so
/// index/iterate/slice sites are unchanged; combining is reached through the
/// dedicated methods so the flag-gate is never bypassed.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Row {
    cells: Vec<Cell>,
    combining: Combining,
}

impl Row {
    /// A row of `cols` blank cells.
    pub(crate) fn blank(cols: usize) -> Row {
        Row { cells: vec![Cell::default(); cols], combining: Combining::new() }
    }

    /// Wrap a cell vector as a row with no combining marks.
    pub(crate) fn from_cells(cells: Vec<Cell>) -> Row {
        Row { cells, combining: Combining::new() }
    }

    /// Build a row from cells and a combining map (the reflow re-split path).
    pub(crate) fn new(cells: Vec<Cell>, combining: Combining) -> Row {
        Row { cells, combining }
    }

    /// Consume the row into its cells and combining map (the reflow join path).
    pub(crate) fn into_parts(self) -> (Vec<Cell>, Combining) {
        (self.cells, self.combining)
    }

    /// Resize to `cols`, padding with blanks or truncating; combining entries for
    /// dropped columns are pruned (xterm's shrink-prune).
    pub(crate) fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::default());
        if let Some(&max) = self.combining.keys().next_back()
            && max >= cols
        {
            self.combining.retain(|&col, _| col < cols);
        }
    }

    /// Empty the row, keeping the cell allocation — for recycling a row buffer
    /// (`scroll_up_recycle`). Clears both cells and combining so a reused row
    /// never surfaces a previous occupant's marks.
    pub(crate) fn clear(&mut self) {
        self.cells.clear();
        self.combining.clear();
    }

    /// The combining marks at `col`, or `None`. Flag-gated: returns `Some` only
    /// when the cell carries the `COMBINED_PRESENT` bit, so a stale map entry is
    /// never surfaced.
    pub(crate) fn combining_at(&self, col: usize) -> Option<&[char]> {
        if self.cells[col].is_combined() {
            self.combining.get(&col).map(Vec::as_slice)
        } else {
            None
        }
    }

    /// Attach a combining mark to `col`'s glyph. The first mark on a cell starts a
    /// fresh cluster — dropping any stale entry an overwrite left behind (the bit
    /// was clear) — and sets the presence bit; subsequent marks append. Mirrors
    /// xterm's `addCodepointToCell`.
    pub(crate) fn push_combining(&mut self, col: usize, mark: char) {
        if self.cells[col].is_combined() {
            self.combining.entry(col).or_default().push(mark);
        } else {
            self.cells[col].set_combined(true);
            self.combining.insert(col, vec![mark]);
        }
    }

    /// Re-key combining entries to follow a `copy_within(src, dst)` cell shift
    /// (ICH/DCH): the live entry for a moved cell travels to the cell's new
    /// column. Vacated source keys whose cell loses the bit are left stale —
    /// harmless under the flag-gate — so only the live carry is done here.
    pub(crate) fn move_combining(&mut self, src: std::ops::Range<usize>, dst: usize) {
        if self.combining.is_empty() {
            return;
        }
        let start = src.start;
        let moved: Vec<(usize, Vec<char>)> = src
            .filter_map(|s| self.combining.remove(&s).map(|v| (dst + (s - start), v)))
            .collect();
        for (col, marks) in moved {
            self.combining.insert(col, marks);
        }
    }
}

impl Deref for Row {
    type Target = [Cell];
    fn deref(&self) -> &[Cell] {
        &self.cells
    }
}

impl DerefMut for Row {
    fn deref_mut(&mut self) -> &mut [Cell] {
        &mut self.cells
    }
}

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
    //    point's logical coordinate (line index + offset within the line). The
    //    combining map is carried alongside: a row's entries are re-keyed by the
    //    join offset so a cluster stays attached to its glyph across the wrap.
    let mut logical: Vec<Vec<Cell>> = Vec::new();
    let mut logical_comb: Vec<Combining> = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    let mut current_comb: Combining = Combining::new();
    // Per point: (logical line, offset, found-yet).
    let mut tracked: Vec<(usize, usize, bool)> = vec![(0, 0, false); points.len()];
    for (i, row) in rows.into_iter().enumerate() {
        for (pi, &(pr, pc)) in points.iter().enumerate() {
            if i == pr && !tracked[pi].2 {
                tracked[pi] = (logical.len(), current.len() + pc, true);
            }
        }
        let soft = row.last().is_some_and(|c| c.is_wrapline());
        let base = current.len();
        let (cells, comb) = row.into_parts();
        // Carry live combining entries, re-keyed to the logical-line offset
        // (flag-gated: a stale entry whose cell lost the bit is dropped).
        for (col, marks) in comb {
            if cells[col].is_combined() {
                current_comb.insert(base + col, marks);
            }
        }
        if soft {
            current.extend(cells.into_iter().map(|mut c| {
                c.remove_flags(CellFlags::WRAPLINE);
                c
            }));
        } else {
            let mut cells = cells;
            while cells.last() == Some(&Cell::default()) {
                cells.pop();
            }
            current.extend(cells);
            logical.push(std::mem::take(&mut current));
            logical_comb.push(std::mem::take(&mut current_comb));
        }
    }
    if !current.is_empty() {
        logical.push(current);
        logical_comb.push(current_comb);
    }
    // Trailing blank lines are absorbed, not preserved as rows (combining map
    // trimmed in lockstep so the two stay index-aligned).
    while logical.last().is_some_and(|l| l.is_empty()) {
        logical.pop();
        logical_comb.pop();
    }

    // 2. Re-split each logical line into `new_cols`-wide rows, mapping each
    //    tracked point to its new (row, col).
    let mut out: Vec<Row> = Vec::new();
    let mut new_points = vec![(0usize, 0usize); points.len()];
    for (li, line) in logical.iter().enumerate() {
        let comb = &logical_comb[li];
        let start = out.len();
        if line.is_empty() {
            out.push(Row::blank(new_cols));
        } else {
            let mut i = 0;
            while i < line.len() {
                let mut take = (line.len() - i).min(new_cols);
                // Don't split a wide char from its spacer: if the row would end
                // on a WIDE_CHAR lead, drop it to the next row (xterm's newCols-1).
                if i + take < line.len() && line[i + take - 1].is_wide() {
                    take -= 1;
                }
                let take = take.max(1); // guard the 1-col degenerate case
                // Segment combining: entries in [i, i+take) re-keyed to col - i.
                let seg_comb: Combining = comb
                    .range(i..i + take)
                    .map(|(&col, marks)| (col - i, marks.clone()))
                    .collect();
                let mut row = Row::new(line[i..i + take].to_vec(), seg_comb);
                row.resize(new_cols);
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
        let lines = vec![Row::blank(cols); rows];
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

    /// Read a whole row including its combining map — for combining-aware reads
    /// (text extraction, serialization).
    pub(crate) fn row_ref(&self, row: usize) -> &Row {
        &self.lines[row]
    }

    /// Mutable access to a whole row (cells + combining map) — for in-row cell
    /// shifts (ICH/DCH), which must re-key combining alongside the cell move.
    pub(crate) fn row_mut(&mut self, row: usize) -> &mut Row {
        &mut self.lines[row]
    }

    /// A clone of a whole row (cells + combining map) — for the sub-region scroll
    /// eviction, which copies row 0 out to scrollback (the full-screen path moves
    /// the row instead, via `scroll_up_recycle`).
    pub(crate) fn row_owned(&self, row: usize) -> Row {
        self.lines[row].clone()
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
        for cell in self.lines[bottom].iter_mut() {
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
        blank.resize(self.cols);
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
            row.resize(cols);
        }
        while lines.len() < rows {
            lines.push(Row::blank(cols));
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
            for cell in row.iter_mut() {
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
        for cell in self.lines[top].iter_mut() {
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
        let dirty = Row::from_cells(vec![x; 2]);
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
