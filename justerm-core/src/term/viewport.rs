//! The viewport surface: which window of `[scrollback ++ screen]` is on screen (`display_offset`),
//! the conversion from a viewport row to an absolute buffer line, and the per-cell queries a
//! consumer asks of what it is looking at (a viewport row's cells, the link and underline colour
//! at a cell).

use crate::cell::Cell;
use crate::color::Color;
use crate::selection::BufferPoint;

use super::{Hyperlink, Term};

impl Term {
    /// The cells of visible row `i` (0..rows) at the current scroll position.
    /// The viewport windows into `[history.. ; screen..]`: rows above
    /// `scrollback.len()` come from history, the rest from the live screen.
    pub fn viewport_line(&self, i: usize) -> &[Cell] {
        let top = self.scrollback.len() - self.display_offset;
        let idx = top + i;
        if idx < self.scrollback.len() {
            &self.scrollback[idx]
        } else {
            self.grid.row(idx - self.scrollback.len())
        }
    }

    /// Scroll the viewport up by `n` lines into history (clamped to the oldest).
    pub fn scroll_up(&mut self, n: usize) {
        let target = (self.display_offset + n).min(self.scrollback.len());
        self.set_display_offset(target);
    }

    /// Scroll the viewport down by `n` lines toward the live screen.
    pub fn scroll_down(&mut self, n: usize) {
        let target = self.display_offset.saturating_sub(n);
        self.set_display_offset(target);
    }

    /// Jump the viewport back to the live screen (follow the bottom).
    pub fn scroll_to_bottom(&mut self) {
        self.set_display_offset(0);
    }

    /// Move the viewport. A user scroll changes which lines are visible, so the
    /// whole viewport is repainted (full damage) when the offset actually moves.
    pub(super) fn set_display_offset(&mut self, offset: usize) {
        // The alt screen has no scrollback to view; scroll intents are no-ops.
        if self.on_alt {
            return;
        }
        if offset != self.display_offset {
            self.display_offset = offset;
            self.mark_fully_damaged();
        }
    }

    /// Map a viewport cell `(row, col)` to an absolute buffer point. The top visible row is
    /// `scrollback.len() - display_offset`, so viewport row `i` is that plus `i`.
    ///
    /// Both axes are clamped, `row` to the last visible row and `col` to the last grid column,
    /// because a pointer past the grid is ordinary input (#660, #671, ADR-0026 D1). Every stored
    /// selection anchor arrives through here, which is what keeps `resolve`'s `+ 1`s in range:
    /// `docs/map/territory/selection.md`.
    pub(super) fn viewport_to_abs(&self, row: usize, col: usize) -> BufferPoint {
        let top = self.scrollback.len() - self.display_offset;
        let last = self.grid.rows().saturating_sub(1);
        BufferPoint {
            line: top + row.min(last),
            col: col.min(self.grid.cols().saturating_sub(1)),
        }
    }

    /// The hyperlink URI at **screen** `(row, col)` (the live grid), or `None`, read from the
    /// row's link map. Mirrors `grid().cell(row, col)`.
    pub(crate) fn screen_link_at(&self, row: usize, col: usize) -> Option<Hyperlink> {
        self.grid
            .row_ref(row)
            .link_at(col)
            .cloned()
            .map(Hyperlink::new)
    }

    /// The underline colour (SGR 58, #520) at screen `(row, col)`, as a theme-agnostic
    /// reference. `Color::Default` means "follow the fg" — the common case, and what an
    /// unset cell returns. Mirror of [`Term::screen_link_at`].
    pub(crate) fn screen_underline_color_at(&self, row: usize, col: usize) -> Color {
        self.grid.row_ref(row).ucolor_at(col).unwrap_or_default()
    }

    /// The hyperlink URI at **viewport** `(row, col)` (visible window, history
    /// included at the current scroll), or `None`. Mirrors `viewport_line(row)`.
    pub(crate) fn viewport_link_at(&self, row: usize, col: usize) -> Option<Hyperlink> {
        let idx = self.scrollback.len() - self.display_offset + row;
        self.abs_row(idx).link_at(col).cloned().map(Hyperlink::new)
    }
}
