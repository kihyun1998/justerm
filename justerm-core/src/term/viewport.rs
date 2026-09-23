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

    /// Map a viewport cell `(row, col)` to an absolute buffer point. The top
    /// visible row is `scrollback.len() - display_offset`, so viewport row `i`
    /// is that plus `i`.
    ///
    /// **`row` is clamped to the last visible row, and that is the anchor's whole defence
    /// against a caller that hands it one past the end (#660).** The row arrives from a
    /// pointer position, so it is off the end whenever a drag leaves the grid — including
    /// the ordinary case where the container is a sub-cell remainder taller than
    /// `rows × cell_height` and a click lands in that strip. Stored unclamped it does not
    /// fail here: it detonates later, in whichever read walks the selection
    /// (`selection_range`, `selection_text`, the word extents), so the stack trace accuses
    /// the reader rather than the caller — the delay `damage_span`'s doc describes and
    /// #536 was filed about.
    ///
    /// **Clamped, and deliberately *not* `debug_assert`ed** — which is where this differs
    /// from `damage_span`, whose split it otherwise mirrors. That function is engine-
    /// internal, so an out-of-range span there is a justerm bug and the assert names its
    /// producer. This one is reached from `Engine::selection_begin` / `selection_extend`,
    /// whose documented input is *"what a mouse event carries"* — and a pointer leaves the
    /// grid whenever a drag does, so a row past the end is **ordinary input, not a defect**.
    /// Asserting on it would panic a consumer's debug build for a legal gesture.
    ///
    /// Clamping is also what every producer already wants: a drag past the bottom edge
    /// selects to the edge. alacritty clamps at the same boundary (`Point::grid_clamp`);
    /// ghostty's pins cannot express an out-of-range row at all.
    ///
    /// **This is a backstop, and since #667 nothing in the family relies on it.** The
    /// sentence here used to read that justerm-web's selection converter did *not* clamp,
    /// which was what made an unclamped row reachable in the shipped stack rather than
    /// only in theory; that converter now bounds both axes at its own seam, as all three
    /// references do at theirs. The claim is retracted rather than deleted because it was
    /// the record of why this clamp was worth adding.
    ///
    /// **`col` is bounded the same way, and against the grid rather than the line
    /// (#671).** #660 reasoned about the row alone and this function passed the column
    /// through, which was not a smaller version of the same gap — it was a *different*
    /// one, because the two axes are consumed differently downstream. A column reaches
    /// `resolve`, where the `Side` decides whether it gets a `+ 1`, and the two readers
    /// then bound only one end each: `selection_range`'s Linear arm clips `right_excl`
    /// and not `left`, `selection_text`'s Block arm clips `hi` and not `from`. So
    /// `Side::Right` was already safe by accident (its `+ 1` lands past the end and the
    /// clip catches it) while **`Side::Left` had no `+ 1` to clip**, and the raw column
    /// survived into `left` — silently deleting the anchor's own row from both the
    /// projection and the copy. `usize::MAX` was the one value that panicked instead,
    /// on the `+ 1`.
    ///
    /// Bounding here rather than in `resolve` keeps one site answering "what does a
    /// viewport coordinate mean", and makes both axes total for the same reason; the
    /// alternative — clamping at each `+ 1` — is five sites for one rule. alacritty
    /// bounds both endpoints' columns in `Selection::to_range` *before* its own side
    /// arithmetic and pairs the `+ 1` with an explicit *"column == columns → wrap to the
    /// next line"*; justerm reaches that same outcome through the reader's
    /// `right_excl > left`, which is why an **in-range** `Side::Right` on the last column
    /// still starts the selection on the following row and is pinned as unchanged.
    ///
    /// The grid, not `abs_line(..).len()`, is the bound: `SelectionType::Line` already
    /// resolves `to` as `grid.cols()`, so the whole type works in grid coordinates and a
    /// short line must not shrink a selection that reaches past it.
    ///
    /// **`resolve`'s five `+ 1`s stay unguarded, and that is sound only while every stored
    /// anchor arrives through here.** The completeness pass enumerated the writers: the
    /// three coordinate fixups move `.line` or write columns that are in range by
    /// construction, `resize`'s primary branch re-clamps the reflowed points (#562) and its
    /// alt branch drops the selection outright (#660), and `Term::resize` is the only writer
    /// of `grid.cols()`. So no path strands a column that was clamped here. The condition is
    /// **a fourth writer of `self.selection`** — one that builds an `Anchor` without this
    /// function would put `resolve` back in reach of its own arithmetic.
    pub(super) fn viewport_to_abs(&self, row: usize, col: usize) -> BufferPoint {
        let top = self.scrollback.len() - self.display_offset;
        let last = self.grid.rows().saturating_sub(1);
        BufferPoint {
            line: top + row.min(last),
            col: col.min(self.grid.cols().saturating_sub(1)),
        }
    }

    /// The hyperlink **URI** at **screen** `(row, col)` (the live grid), or `None` —
    /// flag-gated through the row's link map. Since #628 the map holds the URI itself,
    /// so there is no index and no second call to resolve one.
    /// Mirrors `grid().cell(row, col)`.
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
