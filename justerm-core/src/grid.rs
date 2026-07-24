//! The grid — the 2D array of cells representing the current screen.
//!
//! Rows are stored as separate `Vec`s (not one flat buffer) so the scrollback
//! ring (a later slice) can move whole rows in/out cheaply.

use crate::cell::Cell;
use crate::color::Color;
use core::num::NonZeroU32;
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

/// A row's combining clusters: column → the combining marks attached to that
/// column's base glyph. Sparse (most rows have none) and **flag-gated** — an
/// entry is only ever read when the cell at that column has its
/// `COMBINED_PRESENT` bit set (xterm's `_combined` invariant, #45). Stale entries
/// left by an overwrite/erase are therefore harmless; only live entries must be
/// carried when cells move column (ICH/DCH/reflow).
type Combining = BTreeMap<usize, Vec<char>>;

/// A row's hyperlinks: column → the global `hyperlink_pool` index (OSC 8). Same
/// per-row, flag-gated sparse-map design as [`Combining`], gated by the cell's
/// `LINK_PRESENT` bit instead (xterm's `_extendedAttrs` / `HAS_EXTENDED`, #46).
type Links = BTreeMap<usize, NonZeroU32>;

/// A row's non-default underline colours (SGR 58, #520): column → the underline
/// `Color` reference. Same per-row, flag-gated sparse-map design as [`Links`],
/// gated by the cell's `UCOLOR_PRESENT` bit. Only non-`Default` colours get an
/// entry — a `Default` underline follows the fg and needs no storage.
type UColors = BTreeMap<usize, Color>;

/// Every **extended attribute** live at one column — the family that rides the
/// row's flag-gated side maps rather than the 12-byte cell: the OSC 8 hyperlink
/// (#46) and the SGR 58 underline colour (#520). Combining marks are deliberately
/// *not* here: they are content, re-attached mark-by-mark through
/// [`Row::push_combining`], not carried as an opaque value.
///
/// It exists so a path that *moves* or *grows* a cell carries the whole family in
/// one step ([`Row::ext_attrs_at`] → [`Row::set_ext_attrs`]) instead of naming each
/// rider — the same shape as xterm.js's `_copyCellMapsFrom`, which re-keys
/// `_combined` and `_extendedAttrs` together for every cell `copyCellsFrom` moves.
/// Adding a rider (an underline *style*, say) is a field here plus the two arms
/// below; every carry site is covered by construction (#521).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExtAttrs {
    link: Option<NonZeroU32>,
    ucolor: Option<Color>,
}

impl ExtAttrs {
    /// The family as the *pen* currently holds it — the other source besides a cell
    /// (`Row::ext_attrs_at`). Every print-path site that stamps a freshly built cell
    /// goes through here, so the gating rules live in one place and a later rider is
    /// added once (#521/#528).
    pub(crate) fn from_pen(link: Option<NonZeroU32>, ucolor: Option<Color>) -> ExtAttrs {
        ExtAttrs { link, ucolor }
    }
}

/// Re-key a sparse column map to follow a `copy_within(src, dst)` cell shift: the
/// live entry for a moved cell travels to the cell's new column. Vacated source
/// keys whose cell loses its gate bit are left stale — harmless under the
/// flag-gate — so only the live carry is done. Generic over the value type so the
/// combining and link maps share one implementation.
fn move_map<V>(map: &mut BTreeMap<usize, V>, src: std::ops::Range<usize>, dst: usize) {
    if map.is_empty() {
        return;
    }
    let start = src.start;
    let moved: Vec<(usize, V)> = src
        .filter_map(|s| map.remove(&s).map(|v| (dst + (s - start), v)))
        .collect();
    for (col, v) in moved {
        map.insert(col, v);
    }
}

/// One row of cells **plus** its per-row, column-keyed combining, link, and
/// underline-colour maps.
///
/// The maps ride with the row through scroll / scrollback / reflow for free (the
/// row is the unit that moves), which is why combining (#45), hyperlinks (#46),
/// and underline colours (#520) live here rather than in global per-cell indices —
/// no leak, cleared on row reuse. `Row` derefs to `[Cell]`, so index/iterate/slice
/// sites are unchanged; the maps are reached through the dedicated methods so the
/// flag-gate (read iff the cell's `COMBINED_PRESENT` / `LINK_PRESENT` /
/// `UCOLOR_PRESENT` bit is set) is never bypassed.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Row {
    cells: Vec<Cell>,
    combining: Combining,
    links: Links,
    ucolors: UColors,
    /// Did this row soft-wrap (auto-wrap) into the next one?
    ///
    /// A property of the **row**, and stored on the row for a reason: it used to ride
    /// `CellFlags::WRAPLINE` in the last cell, where every whole-cell write and clear destroyed it
    /// — ordinary typing in the last column silently split the logical line (#538). Here no cell
    /// operation can reach it. Both references keep it off the cell too, though the field is not
    /// the same: ghostty's `Row.wrap` is this exact flag (the row wraps *into* the next), while
    /// xterm.js's `BufferLine.isWrapped` is the opposite-polarity link (the row *continues* the
    /// previous one) — ghostty's `wrap_continuation`, not its `wrap`. The distinction matters when
    /// borrowing xterm.js's `clearWrap` values, which describe the *previous* row's link.
    ///
    /// It still crosses the wire as the last cell's `WRAPLINE` bit, derived at encode time, so the
    /// format is unchanged.
    wrapped: bool,
}

impl Row {
    /// A row of `cols` blank cells.
    pub(crate) fn blank(cols: usize) -> Row {
        Row {
            cells: vec![Cell::default(); cols],
            combining: Combining::new(),
            links: Links::new(),
            ucolors: UColors::new(),
            wrapped: false,
        }
    }

    /// Wrap a cell vector as a row with no combining marks, links, or ucolors.
    pub(crate) fn from_cells(cells: Vec<Cell>) -> Row {
        Row {
            cells,
            combining: Combining::new(),
            links: Links::new(),
            ucolors: UColors::new(),
            wrapped: false,
        }
    }

    /// Build a row from cells and its maps (the reflow re-split path).
    pub(crate) fn new(
        cells: Vec<Cell>,
        combining: Combining,
        links: Links,
        ucolors: UColors,
    ) -> Row {
        Row {
            cells,
            combining,
            links,
            ucolors,
            wrapped: false,
        }
    }

    /// Consume the row into its cells, combining map, link map, and ucolor map
    /// (the reflow join path).
    pub(crate) fn into_parts(self) -> (Vec<Cell>, Combining, Links, UColors) {
        (self.cells, self.combining, self.links, self.ucolors)
    }

    /// Resize to `cols`, padding with blanks or truncating; map entries for
    /// dropped columns are pruned (xterm's shrink-prune).
    pub(crate) fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::default());
        if self
            .combining
            .keys()
            .next_back()
            .is_some_and(|&m| m >= cols)
        {
            self.combining.retain(|&col, _| col < cols);
        }
        if self.links.keys().next_back().is_some_and(|&m| m >= cols) {
            self.links.retain(|&col, _| col < cols);
        }
        if self.ucolors.keys().next_back().is_some_and(|&m| m >= cols) {
            self.ucolors.retain(|&col, _| col < cols);
        }
    }

    /// Empty the row, keeping the cell allocation — for recycling a row buffer
    /// (`scroll_up_recycle`). Clears cells and both maps so a reused row never
    /// surfaces a previous occupant's marks or links.
    pub(crate) fn clear(&mut self) {
        self.cells.clear();
        self.combining.clear();
        self.links.clear();
        self.ucolors.clear();
        self.wrapped = false;
    }

    /// Blank this row **in place** — every cell reset, and every row-scoped property with them.
    ///
    /// The distinction from a cell loop is the whole point. Soft-wrap is a property of the row
    /// (#538), so `for cell in row { cell.reset() }` leaves a blanked row still claiming to
    /// continue into the next one — and because the row *struct* is what scroll rotates and what
    /// the alt grid keeps, that stale claim outlives the content it described. Blanking is one
    /// operation so a caller cannot blank half of a row's state; a future row-scoped field is
    /// covered by construction, the same way `Row::clear` covers the side maps for a recycled
    /// buffer.
    ///
    /// Keeps the cell allocation and the row's width — unlike [`Row::clear`], which empties the
    /// `Vec` for a buffer about to be re-fitted.
    pub(crate) fn blank_in_place(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.reset();
        }
        self.wrapped = false;
    }

    /// Did this row soft-wrap into the next one? See [`Row::wrapped`] for why this is a row
    /// property and not a cell flag (#538).
    pub(crate) fn is_wrapped(&self) -> bool {
        self.wrapped
    }

    /// Mark (or unmark) this row as soft-wrapped into the next.
    ///
    /// Unmarking is per-verb, not derivable from what was erased — see `Term::end_wrap`, which is
    /// the only place that unmarks and carries the rule with its references. An *overwrite* of the
    /// last column must leave it set (that was the whole point of #538: a cell write cannot decide
    /// a row property), and so must a leftward erase.
    pub(crate) fn set_wrapped(&mut self, wrapped: bool) {
        self.wrapped = wrapped;
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

    /// The hyperlink-pool index at `col`, or `None`. Flag-gated by the cell's
    /// `LINK_PRESENT` bit (mirror of [`Row::combining_at`]).
    pub(crate) fn link_at(&self, col: usize) -> Option<NonZeroU32> {
        if self.cells[col].is_linked() {
            self.links.get(&col).copied()
        } else {
            None
        }
    }

    /// The non-default underline colour at `col`, or `None` (which the caller reads
    /// as `Default` — follow the fg). Flag-gated by the cell's `UCOLOR_PRESENT` bit,
    /// so a stale map entry an overwrite left behind is never surfaced (#520).
    pub(crate) fn ucolor_at(&self, col: usize) -> Option<Color> {
        if self.cells[col].is_ucolored() {
            self.ucolors.get(&col).copied()
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

    /// Stamp `col`'s glyph with a hyperlink-pool index, setting the presence bit
    /// (the print path calls this on every cell written while a link is open).
    pub(crate) fn set_link(&mut self, col: usize, link: NonZeroU32) {
        self.cells[col].set_linked(true);
        self.links.insert(col, link);
    }

    /// Stamp `col`'s glyph with a non-default underline colour, setting the presence
    /// bit (the print path calls this on every cell written while the pen's underline
    /// colour is non-default, #520). Mirror of [`Row::set_link`].
    pub(crate) fn set_ucolor(&mut self, col: usize, color: Color) {
        self.cells[col].set_ucolored(true);
        self.ucolors.insert(col, color);
    }

    /// Every extended attribute live at `col`, as one value (#521). Flag-gated per
    /// rider, so a stale entry an overwrite left behind is never picked up.
    pub(crate) fn ext_attrs_at(&self, col: usize) -> ExtAttrs {
        ExtAttrs {
            link: self.link_at(col),
            ucolor: self.ucolor_at(col),
        }
    }

    /// Make `col` carry **exactly** `attrs` — each rider's presence bit and map
    /// entry set together, or *both cleared*. Clearing matters as much as setting:
    /// the promotion paths write over a column that may still hold a live entry, and
    /// they build the new cell by copying one that may still carry a presence bit,
    /// so "set what is there" alone would leave either half of the gate dangling
    /// (#521).
    pub(crate) fn set_ext_attrs(&mut self, col: usize, attrs: ExtAttrs) {
        match attrs.link {
            Some(link) => self.set_link(col, link),
            None => {
                self.cells[col].set_linked(false);
                self.links.remove(&col);
            }
        }
        match attrs.ucolor {
            Some(color) => self.set_ucolor(col, color),
            None => {
                self.cells[col].set_ucolored(false);
                self.ucolors.remove(&col);
            }
        }
    }

    /// Re-key every map to follow a `copy_within(src, dst)` cell shift (ICH/DCH),
    /// so a cluster, link, or underline colour stays attached to its glyph at the
    /// new column.
    pub(crate) fn move_maps(&mut self, src: std::ops::Range<usize>, dst: usize) {
        move_map(&mut self.combining, src.clone(), dst);
        move_map(&mut self.links, src.clone(), dst);
        move_map(&mut self.ucolors, src, dst);
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
    let mut logical_links: Vec<Links> = Vec::new();
    let mut logical_ucolors: Vec<UColors> = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    let mut current_comb: Combining = Combining::new();
    let mut current_links: Links = Links::new();
    let mut current_ucolors: UColors = UColors::new();
    // Per point: (logical line, offset, found-yet).
    let mut tracked: Vec<(usize, usize, bool)> = vec![(0, 0, false); points.len()];
    for (i, row) in rows.into_iter().enumerate() {
        for (pi, &(pr, pc)) in points.iter().enumerate() {
            if i == pr && !tracked[pi].2 {
                tracked[pi] = (logical.len(), current.len() + pc, true);
            }
        }
        let soft = row.is_wrapped();
        let base = current.len();
        let (cells, comb, links, ucolors) = row.into_parts();
        // Carry live map entries, re-keyed to the logical-line offset (flag-gated:
        // a stale entry whose cell lost its bit is dropped).
        for (col, marks) in comb {
            if cells[col].is_combined() {
                current_comb.insert(base + col, marks);
            }
        }
        for (col, link) in links {
            if cells[col].is_linked() {
                current_links.insert(base + col, link);
            }
        }
        for (col, color) in ucolors {
            if cells[col].is_ucolored() {
                current_ucolors.insert(base + col, color);
            }
        }
        if soft {
            let mut cells = cells;
            // A wide char that wrapped at the boundary (write_glyph / relocate_cluster_wide) left a
            // leading-spacer placeholder in the vacated last column. It is a wrap artefact, not
            // content — drop it on the join so the logical line (and re-split) never carries a
            // phantom blank into accessible_text / search / copy (#303). The `soft` flag was already
            // read from this cell above, so removing it now is safe.
            if cells.last().is_some_and(Cell::is_leading_spacer) {
                cells.pop();
            }
            current.extend(cells);
        } else {
            let mut cells = cells;
            while cells.last() == Some(&Cell::default()) {
                cells.pop();
            }
            current.extend(cells);
            logical.push(std::mem::take(&mut current));
            logical_comb.push(std::mem::take(&mut current_comb));
            logical_links.push(std::mem::take(&mut current_links));
            logical_ucolors.push(std::mem::take(&mut current_ucolors));
        }
    }
    if !current.is_empty() {
        logical.push(current);
        logical_comb.push(current_comb);
        logical_links.push(current_links);
        logical_ucolors.push(current_ucolors);
    }
    // Trailing blank lines are absorbed, not preserved as rows (the maps are
    // trimmed in lockstep so all four stay index-aligned).
    while logical.last().is_some_and(|l| l.is_empty()) {
        logical.pop();
        logical_comb.pop();
        logical_links.pop();
        logical_ucolors.pop();
    }

    // 2. Re-split each logical line into `new_cols`-wide rows, mapping each
    //    tracked point to its new (row, col).
    let mut out: Vec<Row> = Vec::new();
    let mut new_points = vec![(0usize, 0usize); points.len()];
    for (li, line) in logical.iter().enumerate() {
        let comb = &logical_comb[li];
        let links = &logical_links[li];
        let ucolors = &logical_ucolors[li];
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
                // Segment maps: entries in [i, i+take) re-keyed to col - i.
                let seg_comb: Combining = comb
                    .range(i..i + take)
                    .map(|(&col, marks)| (col - i, marks.clone()))
                    .collect();
                let seg_links: Links = links
                    .range(i..i + take)
                    .map(|(&col, &link)| (col - i, link))
                    .collect();
                let seg_ucolors: UColors = ucolors
                    .range(i..i + take)
                    .map(|(&col, &color)| (col - i, color))
                    .collect();
                let mut row =
                    Row::new(line[i..i + take].to_vec(), seg_comb, seg_links, seg_ucolors);
                row.resize(new_cols);
                i += take;
                if i < line.len() {
                    row.set_wrapped(true);
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

    /// Did `row` soft-wrap (auto-wrap) into the next one — i.e. are the two rows one logical
    /// line?
    ///
    /// Ask this, not the last cell's `WRAPLINE` flag: soft-wrap is a property of the row and is
    /// stored there, so a cell never carries it on a live grid (#538). The flag still appears on
    /// the *wire*, derived onto a span's last cell at encode time, which is a different layer —
    /// see `docs/architecture.md` §Cell on the two things called "cell" here.
    pub fn is_row_wrapped(&self, row: usize) -> bool {
        self.lines[row].is_wrapped()
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
        self.lines[bottom].blank_in_place();
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
            row.blank_in_place();
        }
    }

    /// Scroll the rows `[top..=bottom]` down by one line: a blank line appears at
    /// `top` and the bottom region line is dropped. Rows outside are untouched.
    /// Used by RI (reverse index) at the top margin.
    pub fn scroll_down_region(&mut self, top: usize, bottom: usize) {
        // Rotate the region's bottom line to its top, then blank it: every line
        // in the region shifts down one and the region's top becomes empty.
        self.lines[top..=bottom].rotate_right(1);
        self.lines[top].blank_in_place();
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

    /// `set_ext_attrs` is "make this column carry **exactly** these attrs". The
    /// clearing half is invisible through the public API — the flag-gate hides a
    /// stale entry either way — so it is pinned here, at the primitive that owns
    /// the guarantee: a caller handing it `None` must leave neither a set presence
    /// bit nor a readable map entry behind (#521).
    #[test]
    fn set_ext_attrs_clears_both_halves_of_the_gate() {
        let mut row = Row::blank(2);
        let link = NonZeroU32::new(7).unwrap();
        row.set_link(0, link);
        row.set_ucolor(0, Color::Indexed(3));
        assert_eq!(row.ext_attrs_at(0).link, Some(link));
        assert_eq!(row.ext_attrs_at(0).ucolor, Some(Color::Indexed(3)));

        row.set_ext_attrs(0, ExtAttrs::default());
        assert!(!row.cells[0].is_linked(), "presence bit cleared");
        assert!(!row.cells[0].is_ucolored(), "presence bit cleared");
        assert!(row.links.is_empty(), "and the map entry with it");
        assert!(row.ucolors.is_empty());
        // Re-arming the bit by hand must not resurrect anything.
        row.cells[0].set_linked(true);
        row.cells[0].set_ucolored(true);
        assert_eq!(row.ext_attrs_at(0), ExtAttrs::default());
    }

    /// The carry itself: reading a column's family and stamping it onto another
    /// column reproduces both riders together — the one step the promotion paths
    /// rely on so a future rider needs no new call site (#521).
    #[test]
    fn ext_attrs_round_trip_from_one_column_to_another() {
        let mut row = Row::blank(2);
        let link = NonZeroU32::new(4).unwrap();
        row.set_link(0, link);
        row.set_ucolor(0, Color::Rgb(1, 2, 3));
        let carried = row.ext_attrs_at(0);
        row.set_ext_attrs(1, carried);
        assert_eq!(row.link_at(1), Some(link));
        assert_eq!(row.ucolor_at(1), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(row.ext_attrs_at(1), carried);
    }
}
