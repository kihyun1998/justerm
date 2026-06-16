//! The grid — the 2D array of cells representing the current screen.
//!
//! Rows are stored as separate `Vec`s (not one flat buffer) so the scrollback
//! ring (a later slice) can move whole rows in/out cheaply.

use crate::cell::Cell;

/// One row of cells.
pub type Row = Vec<Cell>;

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
}
