//! The terminal state model: a `vte::Perform` that maps parsed VT actions onto
//! the grid, cursor, and pen. This is where the "hidden VT state" lives —
//! pending-wrap, the wide-char spacer, and the pen (BCE seam).

use std::collections::VecDeque;

use unicode_width::UnicodeWidthChar;
use vte::{Params, Perform};

use crate::cell::{Cell, CellFlags};
use crate::color::Color;
use crate::cursor::{Cursor, Pen};
use crate::damage::{LineBounds, LineDamage, ScrollOp, TermDamage};
use crate::event::TermEvent;
use crate::grid::{Grid, Row};
use crate::input::{
    KeyEvent, MouseEncoding, MouseEvent, MouseProtocol, encode_focus, encode_key, encode_mouse,
    encode_paste,
};
use crate::search::Match;
use crate::selection::{Anchor, BufferPoint, Selection, SelectionSpan, SelectionType, Side};
use crate::serialize::{Frame, FrameKind, Span};

/// Owns the authoritative screen state and applies VT actions to it.
pub struct Term {
    grid: Grid,
    /// The inactive screen. Swapped with `grid` on alt-screen enter/leave; holds
    /// whichever of primary/alternate is not currently shown. The alt screen has
    /// no scrollback (#3 only rings the primary).
    alt_grid: Grid,
    cursor: Cursor,
    /// Cursor saved on alt-screen enter (DEC 1049), restored on leave.
    saved_cursor: Cursor,
    /// Whether the alternate screen is currently active. Guards enter/leave so a
    /// double-enter or double-leave is a no-op.
    on_alt: bool,
    /// One flag per column: is there a tab stop here? Explicit per-column state
    /// (HTS sets, TBC clears), not a fixed modulo. Default = every 8th column.
    tabs: Vec<bool>,
    /// Origin mode (DECOM ?6): when set, cursor addressing is relative to the
    /// scroll region's top margin (and clamped to it).
    origin_mode: bool,
    /// Bracketed-paste mode (DEC ?2004). The engine owns the flag; the input
    /// encoder (#11) reads it to decide whether to wrap pasted text in markers.
    bracketed_paste: bool,
    /// Application cursor keys (DECCKM ?1): when set, cursor keys / Home / End
    /// encode as SS3 rather than CSI (see `input.rs`).
    app_cursor_keys: bool,
    /// Mouse tracking mode — what events the app asked to be reported
    /// (?1000/?1002/?1003). `Off` by default.
    mouse_protocol: MouseProtocol,
    /// Mouse coordinate encoding (default X10 vs ?1006 SGR).
    mouse_encoding: MouseEncoding,
    /// Focus in/out reporting (?1004): emit `CSI I`/`CSI O` on focus change.
    focus_events: bool,
    /// Consumer events (title / bell / cwd) accumulated since the last
    /// `drain_events` (#12). Pull, not push — see `event.rs`.
    events: Vec<TermEvent>,
    /// Scroll region top/bottom margins (DECSTBM), 0-based inclusive. A
    /// line-feed at `scroll_bottom` scrolls only rows `[scroll_top..=scroll_bottom]`.
    /// Default = the full screen.
    scroll_top: usize,
    scroll_bottom: usize,
    /// Lines that have scrolled off the top of the primary screen, oldest at the
    /// front. Accrues only on a top-anchored, primary-screen scroll.
    scrollback: VecDeque<Row>,
    /// How many lines the viewport is scrolled up from the bottom. 0 = following
    /// the live screen; clamped to `[0, scrollback.len()]`.
    display_offset: usize,
    /// Maximum scrollback lines retained; the oldest are evicted past this.
    scrollback_limit: usize,
    /// Per-line damage bounds since the last `reset_damage` (ack), one per row.
    line_damage: Vec<LineBounds>,
    /// A first-class scroll recorded since the last `reset_damage`.
    scroll: Option<ScrollOp>,
    /// The whole screen changed (alt switch / clear / later resize+flood) — the
    /// renderer must redraw everything.
    full_damage: bool,
    /// The live selection, in absolute buffer coordinates. `None` when nothing
    /// is selected. See `selection.rs`.
    selection: Option<Selection>,
    /// Cursor state saved by DECSC (ESC 7), restored by DECRC (ESC 8). A slot
    /// separate from `saved_cursor` (which is the alt-screen save). Defaults to
    /// home/default so a DECRC with no prior DECSC restores a sane state.
    decsc: SavedCursor,
    /// Grapheme side-table: each entry is one cell's combining marks, referenced
    /// by `Cell.extra` (1-based). Append-only; a cell overwritten or reset drops
    /// its reference and leaves a dead entry (compacted on resize — common-90%).
    grapheme_pool: Vec<Vec<char>>,
}

/// Default scrollback retention when not specified.
const DEFAULT_SCROLLBACK: usize = 10_000;

/// The state DECSC (ESC 7) saves and DECRC (ESC 8) restores: position, pen/SGR,
/// pending-wrap, and origin mode (per ADR-0004 — DECRC restores origin mode,
/// which Alacritty omits). Cursor *visibility* is deliberately not part of this
/// (DECTCEM is separate from DECSC).
#[derive(Clone, Copy, Default)]
struct SavedCursor {
    row: usize,
    col: usize,
    pen: Pen,
    pending_wrap: bool,
    origin_mode: bool,
}

/// A selection resolved to absolute-coordinate bounds, ready for text extraction
/// or viewport-span projection. Columns are half-open (`from..to`).
enum Resolved {
    /// Char/Word/Line: a run that joins soft-wrapped rows. Columns apply to the
    /// first/last line; middle lines are whole.
    Linear {
        start_line: usize,
        from: usize,
        end_line: usize,
        to: usize,
    },
    /// Block: a rectangle — the same `from..to` columns on every row.
    Block {
        line0: usize,
        line1: usize,
        from: usize,
        to: usize,
    },
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        Term {
            grid: Grid::new(cols, rows),
            alt_grid: Grid::new(cols, rows),
            cursor: Cursor::default(),
            saved_cursor: Cursor::default(),
            on_alt: false,
            origin_mode: false,
            bracketed_paste: false,
            app_cursor_keys: false,
            mouse_protocol: MouseProtocol::Off,
            mouse_encoding: MouseEncoding::Default,
            focus_events: false,
            events: Vec::new(),
            tabs: default_tabs(cols),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            scrollback: VecDeque::new(),
            display_offset: 0,
            scrollback_limit,
            line_damage: vec![LineBounds::undamaged(cols); rows],
            scroll: None,
            full_damage: false,
            selection: None,
            decsc: SavedCursor::default(),
            grapheme_pool: Vec::new(),
        }
    }

    /// What changed since the last `reset_damage()` — line ranges, each with a
    /// changed column span. See ADR-0003.
    pub fn damage(&self) -> TermDamage {
        if self.full_damage {
            return TermDamage::Full;
        }
        // Scrolled up under follow-bottom "stay": the viewport is frozen, so
        // screen changes below it are not visible — report nothing. (A user
        // scroll that moves the viewport sets full_damage above.)
        if self.display_offset > 0 {
            return TermDamage::Partial(Vec::new());
        }
        let lines: Vec<LineDamage> = self
            .line_damage
            .iter()
            .enumerate()
            .filter(|(_, b)| b.is_damaged())
            .map(|(line, b)| {
                let (left, right) = b.span();
                LineDamage { line, left, right }
            })
            .collect();
        TermDamage::Partial(lines)
    }

    /// Clear accumulated damage. The consumer calls this after applying a frame
    /// (the ack); the next `damage()` reflects only changes since.
    pub fn reset_damage(&mut self) {
        for b in &mut self.line_damage {
            b.reset();
        }
        self.scroll = None;
        self.full_damage = false;
    }

    /// Mark the whole screen damaged (alt switch / clear / flood).
    fn mark_fully_damaged(&mut self) {
        self.full_damage = true;
    }

    /// Record that columns `[left, right]` of `row` changed.
    fn damage_span(&mut self, row: usize, left: usize, right: usize) {
        self.line_damage[row].expand(left, right);
    }

    /// The first-class scroll recorded since the last `reset_damage`, if any.
    /// Suppressed while scrolled up — a content scroll must not shift the frozen
    /// viewport.
    pub fn scroll_delta(&self) -> Option<ScrollOp> {
        if self.display_offset > 0 {
            return None;
        }
        self.scroll
    }

    /// Build a serializable [`Frame`] from the current damage + grid + grapheme
    /// pool (#6). `Full` ships every row; `Partial` ships the damaged spans. The
    /// global side-table is remapped to **frame-local** indices — the engine pool
    /// is append-only and leaky, so a frame carries only the clusters its cells
    /// reference, renumbered, with each cell's `extra` rewritten to the local id.
    pub fn frame(&self) -> Frame {
        let cols = self.grid.cols();
        let rows = self.grid.rows();
        let (kind, line_spans): (FrameKind, Vec<(usize, usize, usize)>) = match self.damage() {
            TermDamage::Full => (
                FrameKind::Full,
                (0..rows).map(|l| (l, 0, cols - 1)).collect(),
            ),
            TermDamage::Partial(lines) => (
                FrameKind::Partial,
                lines
                    .into_iter()
                    .map(|d| (d.line, d.left, d.right))
                    .collect(),
            ),
        };

        let mut side_table: Vec<Vec<char>> = Vec::new();
        // global pool index (1-based) -> frame-local index (1-based, 0 = unset).
        let mut remap = vec![0u16; self.grapheme_pool.len() + 1];
        let mut spans = Vec::with_capacity(line_spans.len());
        for (line, left, right) in line_spans {
            let mut cells = Vec::with_capacity(right - left + 1);
            for col in left..=right {
                let mut cell = *self.grid.cell(line, col);
                if let Some(gidx) = cell.extra {
                    let g = gidx.get() as usize;
                    if remap[g] == 0 {
                        side_table.push(self.grapheme_pool[g - 1].clone());
                        remap[g] = side_table.len() as u16;
                    }
                    cell.extra = core::num::NonZeroU32::new(remap[g] as u32);
                }
                cells.push(cell);
            }
            spans.push(Span {
                line: line as u16,
                left: left as u16,
                right: right as u16,
                cells,
            });
        }

        Frame {
            cols: cols as u16,
            rows: rows as u16,
            kind,
            scroll: self.scroll_delta(),
            spans,
            side_table,
        }
    }

    /// Record a scroll of rows `[top, bottom]` by `count` (positive = up).
    ///
    /// Damage is indexed by row position, so it must follow the content the
    /// scroll just moved: rotate the bounds the same way and mark the newly
    /// exposed line fully damaged (it is new blank content for the consumer).
    fn record_scroll(&mut self, top: usize, bottom: usize, count: isize) {
        let cols = self.grid.cols();
        match count {
            1 => {
                self.line_damage[top..=bottom].rotate_left(1);
                self.line_damage[bottom] = LineBounds::fully_damaged(cols);
            }
            -1 => {
                self.line_damage[top..=bottom].rotate_right(1);
                self.line_damage[top] = LineBounds::fully_damaged(cols);
            }
            _ => {}
        }
        // Accumulate repeated scrolls of the same region into one op (flow
        // control). A *different* region cannot be expressed as one op, so
        // degrade to full rather than silently dropping the earlier scroll.
        match self.scroll {
            Some(op) if op.top == top && op.bottom == bottom => {
                self.scroll = Some(ScrollOp {
                    top,
                    bottom,
                    count: op.count + count,
                });
            }
            None => self.scroll = Some(ScrollOp { top, bottom, count }),
            Some(_) => {
                self.scroll = None;
                self.mark_fully_damaged();
            }
        }
    }

    /// Number of lines currently held in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

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
    fn set_display_offset(&mut self, offset: usize) {
        // The alt screen has no scrollback to view; scroll intents are no-ops.
        if self.on_alt {
            return;
        }
        if offset != self.display_offset {
            self.display_offset = offset;
            self.mark_fully_damaged();
        }
    }

    // ---- selection -----------------------------------------------------------

    /// Map a viewport cell `(row, col)` to an absolute buffer point. The top
    /// visible row is `scrollback.len() - display_offset`, so viewport row `i`
    /// is that plus `i`.
    fn viewport_to_abs(&self, row: usize, col: usize) -> BufferPoint {
        let top = self.scrollback.len() - self.display_offset;
        BufferPoint {
            line: top + row,
            col,
        }
    }

    /// The cells of absolute buffer line `line` (`[scrollback ++ screen]`).
    fn abs_line(&self, line: usize) -> &[Cell] {
        if line < self.scrollback.len() {
            &self.scrollback[line]
        } else {
            self.grid.row(line - self.scrollback.len())
        }
    }

    /// Literal search over the whole buffer (`[scrollback ++ screen]`), returning
    /// every non-overlapping match top-to-bottom in absolute coordinates. Matches
    /// cross soft-wrapped rows (one logical line) and skip wide-char spacers.
    /// Smart-case: a query with no uppercase matches case-insensitively.
    pub fn search(&self, query: &str) -> Vec<Match> {
        let q: Vec<char> = query.chars().collect();
        if q.is_empty() {
            return Vec::new();
        }
        let ci = !q.iter().any(|c| c.is_uppercase());
        // Fold to a single representative char so the haystack stays 1:1 with its
        // positions (rare multi-char case expansions take their first char).
        let fold = |c: char| {
            if ci {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c
            }
        };
        let needle: Vec<char> = q.iter().map(|&c| fold(c)).collect();
        let total = self.scrollback.len() + self.grid.rows();

        let mut matches = Vec::new();
        let mut r = 0;
        while r < total {
            // Build the logical line at `r`: join soft-wrapped rows, recording
            // each char's source position and skipping wide-char spacers.
            let mut hay: Vec<char> = Vec::new();
            let mut pos: Vec<(usize, usize)> = Vec::new();
            let mut line = r;
            loop {
                let cells = self.abs_line(line);
                for (col, cell) in cells.iter().enumerate() {
                    if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    hay.push(fold(cell.c));
                    pos.push((line, col));
                }
                let soft = cells
                    .last()
                    .is_some_and(|c| c.flags.contains(CellFlags::WRAPLINE));
                if soft && line + 1 < total {
                    line += 1;
                } else {
                    break;
                }
            }

            // Slide the needle over the logical line (non-overlapping).
            let mut i = 0;
            while needle.len() <= hay.len() && i + needle.len() <= hay.len() {
                if hay[i..i + needle.len()] == needle[..] {
                    let (start_line, start_col) = pos[i];
                    let (end_line, end_col) = pos[i + needle.len() - 1];
                    matches.push(Match {
                        start_line,
                        start_col,
                        end_line,
                        end_col,
                    });
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
            r = line + 1;
        }
        matches
    }

    /// Scroll the viewport so a match's start line is visible (placed at the top
    /// when it sits in history; the live view when it is already on screen).
    pub fn search_scroll_to(&mut self, m: &Match) {
        let target = self.scrollback.len().saturating_sub(m.start_line);
        self.set_display_offset(target);
    }

    /// Project a match onto the current viewport as inclusive-column spans, one
    /// per visible row (off-screen parts dropped) — for the renderer to
    /// highlight, like `selection_range`.
    pub fn match_spans(&self, m: &Match) -> Vec<SelectionSpan> {
        let rows = self.grid.rows();
        let top = self.scrollback.len() - self.display_offset;
        let mut spans = Vec::new();
        for line in m.start_line..=m.end_line {
            if line < top {
                continue;
            }
            let row = line - top;
            if row >= rows {
                break;
            }
            let last = self.abs_line(line).len().saturating_sub(1);
            let left = if line == m.start_line { m.start_col } else { 0 };
            let right = if line == m.end_line {
                m.end_col.min(last)
            } else {
                last
            };
            if right >= left {
                spans.push(SelectionSpan { row, left, right });
            }
        }
        spans
    }

    /// Begin a selection of `ty` at viewport `(row, col)`, `side`.
    pub fn selection_begin(&mut self, row: usize, col: usize, side: Side, ty: SelectionType) {
        let anchor = Anchor {
            point: self.viewport_to_abs(row, col),
            side,
        };
        self.selection = Some(Selection {
            ty,
            anchor,
            focus: anchor,
        });
    }

    /// Extend the live selection's focus to viewport `(row, col)`, `side`.
    pub fn selection_extend(&mut self, row: usize, col: usize, side: Side) {
        let focus = Anchor {
            point: self.viewport_to_abs(row, col),
            side,
        };
        if let Some(sel) = &mut self.selection {
            sel.focus = focus;
        }
    }

    /// Clear the selection.
    pub fn selection_clear(&mut self) {
        self.selection = None;
    }

    /// Shift the selection up by one absolute line after the oldest history line
    /// is evicted by the scrollback cap. An endpoint clamps to the new top; if
    /// the whole selection was on the evicted line, it is cleared.
    fn selection_evict_oldest(&mut self) {
        let Some((a, f)) = self
            .selection
            .as_ref()
            .map(|s| (s.anchor.point.line, s.focus.point.line))
        else {
            return;
        };
        if a == 0 && f == 0 {
            self.selection = None;
            return;
        }
        if let Some(sel) = &mut self.selection {
            sel.anchor.point.line = a.saturating_sub(1);
            sel.focus.point.line = f.saturating_sub(1);
        }
    }

    /// Rotate the selection within an in-screen scroll of absolute lines
    /// `[top, bottom]`. `up` = content scrolled up (a line dropped at `top`);
    /// otherwise down (dropped at `bottom`). Lines outside the region are
    /// untouched; an endpoint on the dropped line scrolls out, so the whole
    /// selection is cleared rather than copy stale content.
    fn selection_rotate_region(&mut self, top: usize, bottom: usize, up: bool) {
        let rotate = |line: usize| -> Option<usize> {
            if line < top || line > bottom {
                return Some(line); // outside the region — unchanged
            }
            if up {
                (line != top).then(|| line - 1)
            } else {
                (line != bottom).then_some(line + 1)
            }
        };
        let Some((a, f)) = self
            .selection
            .as_ref()
            .map(|s| (s.anchor.point.line, s.focus.point.line))
        else {
            return;
        };
        match (rotate(a), rotate(f)) {
            (Some(al), Some(fl)) => {
                if let Some(sel) = &mut self.selection {
                    sel.anchor.point.line = al;
                    sel.focus.point.line = fl;
                }
            }
            _ => self.selection = None,
        }
    }

    /// The selection projected onto the current viewport: one inclusive-column
    /// span per visible row. Rows scrolled off-screen (above or below) are
    /// dropped. Empty when nothing is selected. See `SelectionSpan`.
    pub fn selection_range(&self) -> Vec<SelectionSpan> {
        let Some(resolved) = self.resolve() else {
            return Vec::new();
        };
        let rows = self.grid.rows();
        // Absolute index of viewport row 0.
        let top = self.scrollback.len() - self.display_offset;
        let mut spans = Vec::new();

        // Add a span for absolute `line` with inclusive cols `left..=right`, if
        // the line is currently visible.
        let mut push = |line: usize, left: usize, right: usize| {
            if line >= top {
                let row = line - top;
                if row < rows {
                    spans.push(SelectionSpan { row, left, right });
                }
            }
        };

        match resolved {
            Resolved::Linear {
                start_line,
                from,
                end_line,
                to,
            } => {
                for line in start_line..=end_line {
                    let len = self.abs_line(line).len();
                    let left = if line == start_line { from } else { 0 };
                    let right_excl = if line == end_line { to.min(len) } else { len };
                    if right_excl > left {
                        push(line, left, right_excl - 1);
                    }
                }
            }
            Resolved::Block {
                line0,
                line1,
                from,
                to,
            } => {
                if to > from {
                    for line in line0..=line1 {
                        push(line, from, to - 1);
                    }
                }
            }
        }
        spans
    }

    /// Resolve the live selection into absolute-coordinate bounds per type:
    /// a `Linear` run (char/word/line, which join soft wraps) or a `Block`
    /// rectangle. `None` when nothing is selected. Columns are half-open
    /// (`from..to`). Shared by `selection_text` and `selection_range`.
    fn resolve(&self) -> Option<Resolved> {
        let sel = self.selection.as_ref()?;
        let (start, end) = sel.ordered();
        Some(match sel.ty {
            SelectionType::Char => {
                // Half-open columns: each side decides if its own cell is in.
                let from = match start.side {
                    Side::Left => start.point.col,
                    Side::Right => start.point.col + 1,
                };
                let to = match end.side {
                    Side::Left => end.point.col,
                    Side::Right => end.point.col + 1,
                };
                Resolved::Linear {
                    start_line: start.point.line,
                    from,
                    end_line: end.point.line,
                    to,
                }
            }
            SelectionType::Word => {
                // Snap both ends to word boundaries (side is ignored).
                let ws = self.word_start(start.point);
                let we = self.word_end(end.point);
                Resolved::Linear {
                    start_line: ws.line,
                    from: ws.col,
                    end_line: we.line,
                    to: we.col + 1,
                }
            }
            SelectionType::Line => Resolved::Linear {
                start_line: start.point.line,
                from: 0,
                end_line: end.point.line,
                to: self.grid.cols(),
            },
            SelectionType::Block => {
                // Rectangular: the same column range on every row. Columns come
                // from the two anchors (min/max, with each edge's side).
                let cols = self.grid.cols();
                let (a, b) = (sel.anchor, sel.focus);
                let (lcol, lside, rcol, rside) = if a.point.col <= b.point.col {
                    (a.point.col, a.side, b.point.col, b.side)
                } else {
                    (b.point.col, b.side, a.point.col, a.side)
                };
                let from = match lside {
                    Side::Left => lcol,
                    Side::Right => lcol + 1,
                };
                let to = match rside {
                    Side::Left => rcol,
                    Side::Right => rcol + 1,
                };
                Resolved::Block {
                    line0: a.point.line.min(b.point.line),
                    line1: a.point.line.max(b.point.line),
                    from,
                    to: to.min(cols).max(from),
                }
            }
        })
    }

    /// The selected text (for copy), or `None` when nothing is selected.
    pub fn selection_text(&self) -> Option<String> {
        match self.resolve()? {
            Resolved::Linear {
                start_line,
                from,
                end_line,
                to,
            } => Some(self.extract_lines(start_line, from, end_line, to)),
            Resolved::Block {
                line0,
                line1,
                from,
                to,
            } => {
                // Each row independently — no soft-wrap joining.
                let mut out = String::new();
                for line in line0..=line1 {
                    let cells = self.abs_line(line);
                    let mut seg = String::new();
                    for cell in &cells[from..to.min(cells.len())] {
                        self.append_cell(&mut seg, cell);
                    }
                    out.push_str(seg.trim_end());
                    if line != line1 {
                        out.push('\n');
                    }
                }
                Some(out)
            }
        }
    }

    /// Append a cell's text — its base glyph plus any combining marks from the
    /// grapheme side-table — to `out`. Wide-char spacers contribute nothing.
    fn append_cell(&self, out: &mut String, cell: &Cell) {
        if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            return;
        }
        out.push(cell.c);
        if let Some(idx) = cell.extra {
            out.extend(self.grapheme_pool[idx.get() as usize - 1].iter());
        }
    }

    /// Concatenate the selected cells from `(start_line, from)` to
    /// `(end_line, to_end)` (half-open columns on the first/last line, whole
    /// lines between). Soft-wrapped rows (WRAPLINE) accumulate into one *logical*
    /// line so trailing-blank trimming happens only at the logical end — spaces
    /// at a wrap boundary are real content. A hard line-end flushes with `\n`.
    fn extract_lines(
        &self,
        start_line: usize,
        from: usize,
        end_line: usize,
        to_end: usize,
    ) -> String {
        let mut out = String::new();
        let mut current = String::new();
        for line in start_line..=end_line {
            let cells = self.abs_line(line);
            let left = if line == start_line { from } else { 0 };
            let right = if line == end_line {
                to_end.min(cells.len())
            } else {
                cells.len()
            };
            // A degenerate range (sides inverting one cell) gives left > right;
            // clamp to empty rather than panic on the slice.
            let right = right.max(left);
            for cell in &cells[left..right] {
                self.append_cell(&mut current, cell);
            }

            let is_last = line == end_line;
            let soft = cells
                .last()
                .is_some_and(|c| c.flags.contains(CellFlags::WRAPLINE));
            if is_last || !soft {
                out.push_str(current.trim_end());
                current.clear();
                if !is_last {
                    out.push('\n');
                }
            }
        }
        out
    }

    /// The cell position before `(line, col)` in the *logical* line — the column
    /// to the left, or the end of the previous row if it soft-wrapped into this
    /// one. `None` at the buffer start or across a hard line-end.
    fn prev_pos(&self, line: usize, col: usize) -> Option<(usize, usize)> {
        if col > 0 {
            return Some((line, col - 1));
        }
        if line > 0 {
            let prev = self.abs_line(line - 1);
            if prev
                .last()
                .is_some_and(|c| c.flags.contains(CellFlags::WRAPLINE))
            {
                return Some((line - 1, prev.len() - 1));
            }
        }
        None
    }

    /// The cell position after `(line, col)` in the *logical* line — the column
    /// to the right, or the start of the next row if this row soft-wrapped.
    /// `None` at the buffer end or across a hard line-end.
    fn next_pos(&self, line: usize, col: usize) -> Option<(usize, usize)> {
        let cells = self.abs_line(line);
        if col + 1 < cells.len() {
            return Some((line, col + 1));
        }
        let total = self.scrollback.len() + self.grid.rows();
        if line + 1 < total
            && cells
                .last()
                .is_some_and(|c| c.flags.contains(CellFlags::WRAPLINE))
        {
            return Some((line + 1, 0));
        }
        None
    }

    /// Walk left to the first cell of `p`'s word (a maximal run of non-boundary
    /// chars), following a soft wrap into the previous row.
    fn word_start(&self, p: BufferPoint) -> BufferPoint {
        let cells = self.abs_line(p.line);
        let (mut line, mut col) = (p.line, p.col.min(cells.len().saturating_sub(1)));
        while let Some((pl, pc)) = self.prev_pos(line, col) {
            if is_word_boundary(self.abs_line(pl)[pc].c) {
                break;
            }
            line = pl;
            col = pc;
        }
        BufferPoint { line, col }
    }

    /// Walk right to the last cell of `p`'s word, following a soft wrap into the
    /// next row.
    fn word_end(&self, p: BufferPoint) -> BufferPoint {
        let cells = self.abs_line(p.line);
        let (mut line, mut col) = (p.line, p.col.min(cells.len().saturating_sub(1)));
        while let Some((nl, nc)) = self.next_pos(line, col) {
            if is_word_boundary(self.abs_line(nl)[nc].c) {
                break;
            }
            line = nl;
            col = nc;
        }
        BufferPoint { line, col }
    }

    /// Resize the screen to `cols` x `rows`. Rows dropped off the top (on shrink)
    /// enter scrollback. Column reflow of soft-wrapped lines is layered on top
    /// separately (#7). The whole screen is damaged.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        // A terminal is never 0-wide/0-tall; clamp so the math below (rows - 1,
        // chunking by cols) can't underflow or divide by zero.
        let cols = cols.max(1);
        let rows = rows.max(1);
        let old_cols = self.grid.cols();
        let limit = self.scrollback_limit;

        // Both screens are resized. Scrollback pairs with the PRIMARY screen
        // (whichever is active) — the alt screen has no history of its own.
        let dims = ReflowDims {
            old_cols,
            cols,
            rows,
            limit,
        };
        let scrollback = std::mem::take(&mut self.scrollback);
        if self.on_alt {
            // Active = alt (cursor, no scrollback); inactive = primary. Selection
            // is primary-only and cleared on alt enter, so no anchors to track.
            let alt = self.grid.take_lines();
            let r = reflow_pane(alt, VecDeque::new(), self.cursor.point(), &[], dims);
            self.grid.set_screen(r.screen, cols, rows);
            self.cursor.set_point(r.cursor, rows, cols);

            let primary = self.alt_grid.take_lines();
            let r = reflow_pane(primary, scrollback, self.saved_cursor.point(), &[], dims);
            self.alt_grid.set_screen(r.screen, cols, rows);
            self.scrollback = r.scrollback;
            self.saved_cursor.set_point(r.cursor, rows, cols);
        } else {
            // Active = primary (cursor, scrollback); inactive = alt. The selection
            // anchors (absolute) reflow alongside the cursor so they keep their
            // content across a column change.
            let sel_pts: Vec<(usize, usize)> = self
                .selection
                .as_ref()
                .map(|s| {
                    vec![
                        (s.anchor.point.line, s.anchor.point.col),
                        (s.focus.point.line, s.focus.point.col),
                    ]
                })
                .unwrap_or_default();

            let primary = self.grid.take_lines();
            let r = reflow_pane(primary, scrollback, self.cursor.point(), &sel_pts, dims);
            self.grid.set_screen(r.screen, cols, rows);
            self.scrollback = r.scrollback;
            self.cursor.set_point(r.cursor, rows, cols);
            if let Some(sel) = &mut self.selection {
                sel.anchor.point = BufferPoint {
                    line: r.extras[0].0,
                    col: r.extras[0].1,
                };
                sel.focus.point = BufferPoint {
                    line: r.extras[1].0,
                    col: r.extras[1].1,
                };
            }

            let alt = self.alt_grid.take_lines();
            let r = reflow_pane(alt, VecDeque::new(), (0, 0), &[], dims);
            self.alt_grid.set_screen(r.screen, cols, rows);
        }

        // Margins reset to the full screen; tab stops reset to the default grid.
        self.cursor.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.tabs = default_tabs(cols);
        self.display_offset = self.display_offset.min(self.scrollback.len());

        // Damage tracking is sized to the screen; a resize repaints everything,
        // so drop any pending scroll op (it points at the old rows).
        self.line_damage = vec![LineBounds::undamaged(cols); rows];
        self.scroll = None;
        self.mark_fully_damaged();
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Whether bracketed-paste mode (DEC ?2004) is enabled. The input encoder
    /// (#11) reads this to decide whether to wrap pasted text in markers.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    // ---- input encoding (#11) ------------------------------------------------

    /// Encode a key event to bytes using the active cursor-key mode (DECCKM).
    pub fn encode_key(&self, ev: KeyEvent) -> Option<Vec<u8>> {
        encode_key(&ev, self.app_cursor_keys)
    }

    /// Encode a mouse event using the active tracking mode + encoding. `None`
    /// when reporting is off or the event is filtered by the mode.
    pub fn encode_mouse(&self, ev: MouseEvent) -> Option<Vec<u8>> {
        encode_mouse(&ev, self.mouse_protocol, self.mouse_encoding)
    }

    /// Encode pasted text, wrapping it in bracketed-paste markers when ?2004 is
    /// on.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        encode_paste(text, self.bracketed_paste)
    }

    /// Encode a focus change (`CSI I`/`CSI O`), or `None` when focus reporting
    /// (?1004) is off.
    pub fn encode_focus(&self, focused: bool) -> Option<Vec<u8>> {
        encode_focus(focused, self.focus_events)
    }

    /// Take the consumer events queued since the last drain, emptying the queue.
    pub fn drain_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    // ---- cursor / scroll primitives ------------------------------------------

    /// Move down one line. At the bottom margin, scroll the region instead;
    /// below the region, just descend (no scroll). Column is unchanged (raw LF;
    /// CR is what returns to column 0).
    fn linefeed(&mut self) {
        if self.cursor.row == self.scroll_bottom {
            // A top-anchored primary-screen scroll pushes the evicted top line
            // into scrollback history.
            if self.scroll_top == 0 && !self.on_alt {
                let evicted = self.grid.row(0).to_vec();
                self.scrollback.push_back(evicted);
                // Follow-bottom = stay: if the user is scrolled up, bump the
                // offset so the same lines stay in view instead of being yanked
                // to the bottom.
                if self.display_offset > 0 {
                    self.display_offset = (self.display_offset + 1).min(self.scrollback.len());
                }
                // Cap: evict the oldest line past the limit. The view is anchored
                // to history, so dropping the front shifts the offset down too
                // (xterm.js trims ybase and ydisp together) — also keeps the
                // offset within `[0, len]`.
                if self.scrollback.len() > self.scrollback_limit {
                    self.scrollback.pop_front();
                    // Every absolute index just shifted down by one; move the
                    // selection with it so its anchors keep their content.
                    self.selection_evict_oldest();
                    if self.display_offset > 0 {
                        // Scrolled up: evicting the oldest line advanced the
                        // viewport, so it must be repainted (the "frozen while
                        // scrolled" rule does not apply when the view itself moved).
                        self.display_offset -= 1;
                        self.mark_fully_damaged();
                    }
                }
            } else {
                // Region (top margin > 0) or alt-screen scroll: the evicted line
                // does NOT enter scrollback, so content moves *within* the screen
                // and absolute indices in the region shift. Rotate the selection
                // up so it follows; an endpoint on the dropped line clears it.
                let base = self.scrollback.len();
                self.selection_rotate_region(
                    base + self.scroll_top,
                    base + self.scroll_bottom,
                    true,
                );
            }
            self.grid
                .scroll_up_region(self.scroll_top, self.scroll_bottom);
            self.record_scroll(self.scroll_top, self.scroll_bottom, 1);
        } else if self.cursor.row + 1 < self.grid.rows() {
            self.cursor.row += 1;
        }
    }

    /// DECSTBM (CSI r): set the top/bottom scroll margins (1-based inclusive).
    /// An invalid region (top ≥ bottom) is ignored.
    fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.grid.rows());
        if top >= bottom {
            return;
        }
        self.scroll_top = top - 1;
        self.scroll_bottom = bottom - 1;
        self.goto(0, 0); // DECSTBM homes the cursor (absolute)
    }

    // ---- alt screen (DEC 1049) -----------------------------------------------

    /// Enter the alternate screen: save the cursor, swap in the other grid, and
    /// clear it.
    fn enter_alt_screen(&mut self) {
        if self.on_alt {
            return;
        }
        self.saved_cursor = self.cursor;
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.grid.clear();
        self.on_alt = true;
        self.display_offset = 0; // the alt screen has no scrollback to view
        self.selection = None; // a selection cannot survive a screen swap
        self.mark_fully_damaged();
    }

    /// Leave the alternate screen: swap the primary grid back in and restore the
    /// saved cursor.
    fn leave_alt_screen(&mut self) {
        if !self.on_alt {
            return;
        }
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.cursor = self.saved_cursor;
        self.on_alt = false;
        self.display_offset = 0; // return to the primary at its bottom
        self.selection = None; // a selection cannot survive a screen swap
        self.mark_fully_damaged();
    }

    /// RI (ESC M): move up one line. At the top margin, scroll the region down
    /// instead.
    fn reverse_index(&mut self) {
        if self.cursor.row == self.scroll_top {
            // RI never enters scrollback; the region scrolls down within the
            // screen, so absolute indices in it shift down. Rotate the selection.
            let base = self.scrollback.len();
            self.selection_rotate_region(base + self.scroll_top, base + self.scroll_bottom, false);
            self.grid
                .scroll_down_region(self.scroll_top, self.scroll_bottom);
            self.record_scroll(self.scroll_top, self.scroll_bottom, -1);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
    }

    // ---- cursor save/restore (DECSC / DECRC) ---------------------------------

    /// DECSC (ESC 7): save the cursor position, pen, pending-wrap, and origin
    /// mode. Visibility is not saved (DECTCEM is separate).
    fn save_cursor(&mut self) {
        self.decsc = SavedCursor {
            row: self.cursor.row,
            col: self.cursor.col,
            pen: self.cursor.pen,
            pending_wrap: self.cursor.pending_wrap,
            origin_mode: self.origin_mode,
        };
    }

    /// DECRC (ESC 8): restore what DECSC saved. Origin mode is restored (per
    /// ADR-0004); visibility is left as-is. The position is clamped to the
    /// current screen in case it shrank since the save.
    fn restore_cursor(&mut self) {
        let s = self.decsc;
        self.cursor.row = s.row.min(self.grid.rows() - 1);
        self.cursor.col = s.col.min(self.grid.cols() - 1);
        self.cursor.pen = s.pen;
        self.cursor.pending_wrap = s.pending_wrap;
        self.origin_mode = s.origin_mode;
    }

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    /// Auto-wrap at end of line: line-feed then return to column 0.
    fn wrapline(&mut self) {
        self.linefeed();
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    // ---- tab stops (HT / HTS / TBC) ------------------------------------------

    /// HT: advance to the next set tab stop, or the last column if none remain
    /// (no wrap).
    fn put_tab(&mut self) {
        let cols = self.grid.cols();
        let mut col = self.cursor.col;
        while col + 1 < cols {
            col += 1;
            if self.tabs[col] {
                break;
            }
        }
        self.cursor.col = col;
        self.cursor.pending_wrap = false;
    }

    /// HTS (ESC H): set a tab stop at the cursor column.
    fn set_tab_stop(&mut self) {
        let col = self.cursor.col;
        self.tabs[col] = true;
    }

    /// TBC (CSI g): clear the tab stop at the cursor (mode 0) or all stops
    /// (mode 3).
    fn clear_tab_stop(&mut self, mode: u16) {
        match mode {
            0 => {
                let col = self.cursor.col;
                self.tabs[col] = false;
            }
            3 => self.tabs.iter_mut().for_each(|t| *t = false),
            _ => {}
        }
    }

    // ---- printing ------------------------------------------------------------

    /// Write one glyph at the cursor, handling deferred wrap and the wide-char
    /// spacer, then advance the cursor (deferring the wrap if it hits the edge).
    fn write_glyph(&mut self, c: char, width: usize) {
        let cols = self.grid.cols();

        // Resolve a deferred last-column wrap before placing the next glyph.
        // The row being left soft-wrapped: mark its last cell so reflow (#7) can
        // tell it from a hard CR/LF line-end.
        if self.cursor.pending_wrap {
            let row = self.cursor.row;
            self.grid
                .cell_mut(row, cols - 1)
                .flags
                .insert(CellFlags::WRAPLINE);
            self.wrapline();
        }

        // A width-2 glyph that cannot fit in the last column wraps first.
        // TODO: xterm leaves a LEADING_WIDE_CHAR_SPACER in the vacated column;
        // common-90% just wraps and leaves it blank.
        if width == 2 && self.cursor.col + 1 >= cols {
            self.wrapline();
        }

        let (row, col) = (self.cursor.row, self.cursor.col);

        // Overwriting one half of an existing wide glyph orphans the other —
        // clear it so no stray lead/spacer is left behind.
        let last = col + width - 1;
        if col > 0
            && self
                .grid
                .cell(row, col)
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            self.grid.cell_mut(row, col - 1).reset();
        }
        if last + 1 < cols
            && self
                .grid
                .cell(row, last)
                .flags
                .contains(CellFlags::WIDE_CHAR)
        {
            self.grid.cell_mut(row, last + 1).reset();
        }

        let mut cell = self.cursor.pen.cell(c);
        if width == 2 {
            cell.flags.insert(CellFlags::WIDE_CHAR);
        }
        *self.grid.cell_mut(row, col) = cell;

        // The trailing column of a wide glyph carries a distinct spacer marker.
        if width == 2 && col + 1 < cols {
            let mut spacer = self.cursor.pen.cell(' ');
            spacer.flags.insert(CellFlags::WIDE_CHAR_SPACER);
            *self.grid.cell_mut(row, col + 1) = spacer;
        }

        // Record damage for the cell(s) just written.
        self.damage_span(row, col, col + width - 1);

        // Advance. Reaching/passing the last column sets pending-wrap instead of
        // wrapping eagerly — the cursor parks on the last column.
        let new_col = col + width;
        if new_col >= cols {
            self.cursor.col = cols - 1;
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = new_col;
        }
    }

    /// Attach a combining mark (width-0 code point) to the grapheme it modifies —
    /// the cell the cursor just left. With pending-wrap the cursor still sits on
    /// the just-written last-column glyph, so attach in place (no back-up, no
    /// deferred wrap); otherwise step back one column, and once more over a
    /// wide-char spacer to reach its lead. Stored in the grapheme side-table.
    fn push_combining(&mut self, c: char) {
        let row = self.cursor.row;
        let mut col = if self.cursor.pending_wrap {
            self.cursor.col
        } else {
            self.cursor.col.saturating_sub(1)
        };
        if self
            .grid
            .cell(row, col)
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            col = col.saturating_sub(1);
        }
        match self.grid.cell(row, col).extra {
            Some(idx) => self.grapheme_pool[idx.get() as usize - 1].push(c),
            None => {
                self.grapheme_pool.push(vec![c]);
                let idx = core::num::NonZeroU32::new(self.grapheme_pool.len() as u32)
                    .expect("pool len is >= 1 after push");
                self.grid.cell_mut(row, col).extra = Some(idx);
            }
        }
        self.damage_span(row, col, col);
    }

    // ---- cursor movement (CSI A/B/C/D/G/d/H/f) -------------------------------

    fn move_up(&mut self, n: usize) {
        self.cursor.row = self.cursor.row.saturating_sub(n);
        self.cursor.pending_wrap = false;
    }

    fn move_down(&mut self, n: usize) {
        self.cursor.row = (self.cursor.row + n).min(self.grid.rows() - 1);
        self.cursor.pending_wrap = false;
    }

    fn move_forward(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn move_back(&mut self, n: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(n);
        self.cursor.pending_wrap = false;
    }

    fn set_col(&mut self, col: usize) {
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn set_row(&mut self, row: usize) {
        self.cursor.row = row.min(self.grid.rows() - 1);
        self.cursor.pending_wrap = false;
    }

    fn goto(&mut self, row: usize, col: usize) {
        // Origin mode addresses rows relative to the scroll region's top margin
        // and clamps to its bottom; otherwise rows are absolute to the screen.
        let (offset, max_row) = if self.origin_mode {
            (self.scroll_top, self.scroll_bottom)
        } else {
            (0, self.grid.rows() - 1)
        };
        self.cursor.row = (row + offset).min(max_row);
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    // ---- erase (CSI J / K) ---------------------------------------------------

    /// Clear cells `from..to` on `row`.
    ///
    /// Background Color Erase (BCE): erased cells carry the current SGR
    /// background only — fg and text attributes reset to default (matches
    /// xterm/alacritty, where the fill is `cursor.template.bg.into()`).
    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let cols = self.grid.cols();
        // Don't orphan a wide char straddling the erase boundary.
        if from > 0
            && self
                .grid
                .cell(row, from)
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            self.grid.cell_mut(row, from - 1).reset();
        }
        if to > from
            && to < cols
            && self
                .grid
                .cell(row, to - 1)
                .flags
                .contains(CellFlags::WIDE_CHAR)
        {
            self.grid.cell_mut(row, to).reset();
        }

        let bg = self.cursor.pen.bg;
        for col in from..to {
            let cell = self.grid.cell_mut(row, col);
            cell.reset();
            cell.bg = bg;
        }
        if to > from {
            self.damage_span(row, from, to - 1);
        }
    }

    /// Erase in display (ED): 0 = cursor→end, 1 = start→cursor, 2 = all.
    fn erase_display(&mut self, mode: u16) {
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            0 => {
                self.clear_cells(cr, cc, cols);
                for row in (cr + 1)..rows {
                    self.clear_cells(row, 0, cols);
                }
            }
            1 => {
                for row in 0..cr {
                    self.clear_cells(row, 0, cols);
                }
                self.clear_cells(cr, 0, cc + 1);
            }
            2 => {
                for row in 0..rows {
                    self.clear_cells(row, 0, cols);
                }
            }
            _ => {}
        }
    }

    /// Erase in line (EL): 0 = cursor→end, 1 = start→cursor, 2 = whole line.
    fn erase_line(&mut self, mode: u16) {
        let cols = self.grid.cols();
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            0 => self.clear_cells(cr, cc, cols),
            1 => self.clear_cells(cr, 0, cc + 1),
            2 => self.clear_cells(cr, 0, cols),
            _ => {}
        }
    }

    // ---- intra-line editing (ICH / DCH / ECH) --------------------------------

    /// ECH (CSI Pn X): erase `n` cells in place from the cursor — no shift.
    /// BCE-filled (via `clear_cells`); pending-wrap is left untouched.
    fn erase_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (row, col) = (self.cursor.row, self.cursor.col);
        let to = (col + n).min(cols);
        self.clear_cells(row, col, to);
    }

    /// ICH (CSI Pn @): insert `n` blanks at the cursor, shifting the rest of the
    /// line right; cells pushed past the right edge are lost. The opened gap is
    /// BCE-filled; pending-wrap is left untouched.
    fn insert_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (r, col) = (self.cursor.row, self.cursor.col);
        let n = n.min(cols - col);
        if n == 0 {
            return;
        }
        let bg = self.cursor.pen.bg;
        let row = self.grid.row_mut(r);
        // Shift [col .. cols-n) right by n; the tail falls off the edge.
        row.copy_within(col..cols - n, col + n);
        for cell in &mut row[col..col + n] {
            cell.reset();
            cell.bg = bg;
        }
        // Repair wide-char halves split at the seams (no-orphan invariant):
        // a lead just before the gap lost its spacer; the first shifted cell may
        // be a spacer whose lead did not move.
        if col > 0
            && self
                .grid
                .cell(r, col - 1)
                .flags
                .contains(CellFlags::WIDE_CHAR)
        {
            self.grid.cell_mut(r, col - 1).reset();
        }
        if col + n < cols
            && self
                .grid
                .cell(r, col + n)
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            self.grid.cell_mut(r, col + n).reset();
        }
        // A lead shifted to the last column lost its spacer off the edge.
        if self
            .grid
            .cell(r, cols - 1)
            .flags
            .contains(CellFlags::WIDE_CHAR)
        {
            self.grid.cell_mut(r, cols - 1).reset();
        }
        self.damage_span(r, col, cols - 1);
    }

    /// DCH (CSI Pn P): delete `n` cells at the cursor, shifting the tail left; the
    /// vacated cells at the right are BCE-blanked. Pending-wrap is left untouched.
    fn delete_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (r, col) = (self.cursor.row, self.cursor.col);
        let n = n.min(cols - col);
        if n == 0 {
            return;
        }
        let bg = self.cursor.pen.bg;
        let row = self.grid.row_mut(r);
        // Shift [col+n .. cols) left to [col ..); BCE-fill the vacated tail.
        row.copy_within(col + n..cols, col);
        for cell in &mut row[cols - n..cols] {
            cell.reset();
            cell.bg = bg;
        }
        // Repair wide-char halves split by the deletion (no-orphan invariant):
        // a lead just before the cut lost its spacer; the cell now at the cursor
        // may be a spacer whose lead was deleted.
        if col > 0
            && self
                .grid
                .cell(r, col - 1)
                .flags
                .contains(CellFlags::WIDE_CHAR)
        {
            self.grid.cell_mut(r, col - 1).reset();
        }
        if self
            .grid
            .cell(r, col)
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            self.grid.cell_mut(r, col).reset();
        }
        self.damage_span(r, col, cols - 1);
    }

    // ---- line/region editing (IL / DL / SU / SD) -----------------------------

    /// Scroll rows `[top..=bottom]` by `n` lines, BCE-filling the exposed lines.
    /// `down` inserts blanks at the top (content moves down); otherwise content
    /// moves up and blanks appear at the bottom. Reuses the one-line region scroll
    /// primitives (so damage + scroll-op accumulation come for free), then fills
    /// the exposed lines with the current SGR background.
    fn scroll_region_lines(&mut self, top: usize, bottom: usize, n: usize, down: bool) {
        let height = bottom - top + 1;
        let n = n.min(height);
        if n == 0 {
            return;
        }
        for _ in 0..n {
            if down {
                self.grid.scroll_down_region(top, bottom);
                self.record_scroll(top, bottom, -1);
            } else {
                self.grid.scroll_up_region(top, bottom);
                self.record_scroll(top, bottom, 1);
            }
        }
        // BCE-fill the n exposed lines (the primitives blank to default).
        let bg = self.cursor.pen.bg;
        let (fill_top, fill_end) = if down {
            (top, top + n)
        } else {
            (bottom + 1 - n, bottom + 1)
        };
        let cols = self.grid.cols();
        for r in fill_top..fill_end {
            for c in 0..cols {
                let cell = self.grid.cell_mut(r, c);
                cell.reset();
                cell.bg = bg;
            }
        }
    }

    /// SU (CSI Pn S): scroll the scroll region up by `n`.
    fn scroll_up_lines(&mut self, n: usize) {
        self.scroll_region_lines(self.scroll_top, self.scroll_bottom, n, false);
    }

    /// SD (CSI Pn T): scroll the scroll region down by `n`.
    fn scroll_down_lines(&mut self, n: usize) {
        self.scroll_region_lines(self.scroll_top, self.scroll_bottom, n, true);
    }

    /// IL (CSI Pn L): insert `n` blank lines at the cursor, scrolling
    /// `[cursor..=scroll_bottom]` down. A no-op when the cursor is outside the
    /// scroll region.
    fn insert_lines(&mut self, n: usize) {
        let cur = self.cursor.row;
        if cur < self.scroll_top || cur > self.scroll_bottom {
            return;
        }
        self.scroll_region_lines(cur, self.scroll_bottom, n, true);
    }

    /// DL (CSI Pn M): delete `n` lines at the cursor, scrolling
    /// `[cursor..=scroll_bottom]` up. A no-op when the cursor is outside the
    /// scroll region.
    fn delete_lines(&mut self, n: usize) {
        let cur = self.cursor.row;
        if cur < self.scroll_top || cur > self.scroll_bottom {
            return;
        }
        self.scroll_region_lines(cur, self.scroll_bottom, n, false);
    }

    // ---- SGR (CSI m) ---------------------------------------------------------

    fn sgr(&mut self, params: &Params) {
        let pen = &mut self.cursor.pen;
        let mut iter = params.iter();
        while let Some(param) = iter.next() {
            let code = param.first().copied().unwrap_or(0);
            match code {
                0 => pen.reset(),
                1 => pen.flags.insert(CellFlags::BOLD),
                2 => pen.flags.insert(CellFlags::DIM),
                3 => pen.flags.insert(CellFlags::ITALIC),
                4 => pen.flags.insert(CellFlags::UNDERLINE),
                5 => pen.flags.insert(CellFlags::BLINK),
                7 => pen.flags.insert(CellFlags::INVERSE),
                8 => pen.flags.insert(CellFlags::HIDDEN),
                9 => pen.flags.insert(CellFlags::STRIKETHROUGH),
                22 => pen.flags.remove(CellFlags::BOLD | CellFlags::DIM),
                23 => pen.flags.remove(CellFlags::ITALIC),
                24 => pen.flags.remove(CellFlags::UNDERLINE),
                25 => pen.flags.remove(CellFlags::BLINK),
                27 => pen.flags.remove(CellFlags::INVERSE),
                28 => pen.flags.remove(CellFlags::HIDDEN),
                29 => pen.flags.remove(CellFlags::STRIKETHROUGH),
                30..=37 => pen.fg = Color::Indexed((code - 30) as u8),
                38 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.fg = c;
                    }
                }
                39 => pen.fg = Color::Default,
                40..=47 => pen.bg = Color::Indexed((code - 40) as u8),
                48 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.bg = c;
                    }
                }
                49 => pen.bg = Color::Default,
                // bright foreground/background (aixterm) → palette 8..=15.
                90..=97 => pen.fg = Color::Indexed((code - 90 + 8) as u8),
                100..=107 => pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => {}
            }
        }
    }
}

/// Parse `38`/`48` extended colour, in either form:
/// - sub-parameter (colon) form inline in `param`: `38:5:n`, `38:2:r:g:b`
///   (optionally `38:2:cs:r:g:b` with a colorspace id), or
/// - legacy (semicolon) form: pull the following top-level params from `iter`.
fn parse_extended_color<'a, I>(param: &[u16], iter: &mut I) -> Option<Color>
where
    I: Iterator<Item = &'a [u16]>,
{
    if param.len() > 1 {
        // Colon sub-parameter form: kind is param[1].
        match param[1] {
            2 => {
                // 38:2:r:g:b (len 5) or 38:2:cs:r:g:b (len 6, colorspace skipped).
                let off = if param.len() >= 6 { 3 } else { 2 };
                let r = *param.get(off)? as u8;
                let g = *param.get(off + 1)? as u8;
                let b = *param.get(off + 2)? as u8;
                Some(Color::Rgb(r, g, b))
            }
            5 => Some(Color::Indexed(*param.get(2)? as u8)),
            _ => None,
        }
    } else {
        // Legacy semicolon form: kind, then its operands, are separate params.
        match iter.next()?.first().copied()? {
            2 => {
                let r = iter.next()?.first().copied()? as u8;
                let g = iter.next()?.first().copied()? as u8;
                let b = iter.next()?.first().copied()? as u8;
                Some(Color::Rgb(r, g, b))
            }
            5 => Some(Color::Indexed(iter.next()?.first().copied()? as u8)),
            _ => None,
        }
    }
}

/// Reflow one screen (joined with its `scrollback`) to `cols` x `rows`, tracking
/// `point` (a cursor in screen coordinates). Returns the new screen rows, the new
/// scrollback (capped to `limit`), and the new point. The alt screen passes an
/// empty scrollback and discards the returned one.
/// The fixed dimensions a resize reflows toward.
#[derive(Clone, Copy)]
struct ReflowDims {
    old_cols: usize,
    cols: usize,
    rows: usize,
    limit: usize,
}

/// The result of reflowing one pane.
struct PaneReflow {
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    /// The cursor's new screen-relative position.
    cursor: (usize, usize),
    /// Each tracked extra point's new **absolute** position, index-aligned with
    /// the `extra_abs` argument.
    extras: Vec<(usize, usize)>,
}

/// Reflow one pane (its `scrollback` joined with `screen`) to `dims`, tracking
/// the screen-relative cursor `point` plus any `extra_abs` points given in
/// **absolute** `[scrollback ++ screen]` coordinates (selection anchors).
fn reflow_pane(
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    point: (usize, usize),
    extra_abs: &[(usize, usize)],
    dims: ReflowDims,
) -> PaneReflow {
    let scroll_len = scrollback.len();
    let mut all: Vec<Row> = scrollback.into();
    all.extend(screen);

    // The cursor is screen-relative; lift it to absolute, then track it together
    // with the already-absolute extras.
    let mut pts: Vec<(usize, usize)> = Vec::with_capacity(1 + extra_abs.len());
    pts.push((scroll_len + point.0, point.1));
    pts.extend_from_slice(extra_abs);

    let pts = if dims.cols != dims.old_cols {
        let (reflowed, np) = crate::grid::reflow(all, dims.cols, &pts);
        all = reflowed;
        np
    } else {
        pts
    };

    let split = all.len().saturating_sub(dims.rows);
    let history: Vec<Row> = all.drain(0..split).collect();
    let mut sb: VecDeque<Row> = history.into();
    let mut dropped = 0usize;
    while sb.len() > dims.limit {
        sb.pop_front();
        dropped += 1;
    }

    // The cursor returns to screen-relative (its absolute index minus the
    // history split). The extras stay absolute, shifted down by any lines the
    // cap dropped from the front of history.
    PaneReflow {
        cursor: (pts[0].0.saturating_sub(split), pts[0].1),
        extras: pts[1..]
            .iter()
            .map(|&(l, c)| (l.saturating_sub(dropped), c))
            .collect(),
        screen: all,
        scrollback: sb,
    }
}

/// Whether `c` ends a word for Word (semantic) selection. Whitespace plus a
/// punctuation set mirroring Alacritty's default `semantic_escape_chars`, so
/// path/URL-ish runs (`.`, `/`, `-`) stay one word.
fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || ",│`|:\"'()[]{}<>".contains(c)
}

/// Default tab stops: one every 8 columns (incl. column 0), matching xterm.
fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|i| i % 8 == 0).collect()
}

/// First sub-parameter of CSI param `idx`, or `default` when absent or zero
/// (a zero/omitted numeric param means "1" for cursor movement and "0" for
/// erase — callers pass the right default).
fn param_or(params: &Params, idx: usize, default: u16) -> u16 {
    match params.iter().nth(idx).and_then(|p| p.first().copied()) {
        Some(v) if v != 0 => v,
        _ => default,
    }
}

impl Perform for Term {
    fn print(&mut self, c: char) {
        match c.width() {
            // Zero-width (combining marks): the grapheme-cluster side-table is a
            // later slice; drop for now rather than mis-place it as its own cell.
            // A zero-width code point is a combining mark — attach it to the
            // previous base glyph rather than dropping it.
            Some(0) => self.push_combining(c),
            None => {}
            Some(width) => self.write_glyph(c, width),
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            // LF, VT, FF all line-feed.
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            b'\r' => self.carriage_return(),
            0x08 => {
                // Backspace.
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.cursor.pending_wrap = false;
            }
            b'\t' => self.put_tab(),
            0x07 => self.events.push(TermEvent::Bell), // BEL (#12)
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // DEC private modes arrive with a '?' intermediate.
        if intermediates.first() == Some(&b'?') {
            let mode = param_or(params, 0, 0);
            match (action, mode) {
                ('h', 1049) => self.enter_alt_screen(),
                ('l', 1049) => self.leave_alt_screen(),
                ('h', 6) => {
                    // DECOM: set homes the cursor to the region top.
                    self.origin_mode = true;
                    self.goto(0, 0);
                }
                ('l', 6) => self.origin_mode = false, // unset leaves the cursor put
                ('h', 25) => self.cursor.visible = true, // DECTCEM show
                ('l', 25) => self.cursor.visible = false, // DECTCEM hide
                ('h', 2004) => self.bracketed_paste = true,
                ('l', 2004) => self.bracketed_paste = false,

                // Input-encoding modes (#11): DECCKM, mouse tracking + encoding,
                // focus reporting. Each set assigns the level; each reset clears
                // it (apps enable/disable the same mode, not a stack).
                ('h', 1) => self.app_cursor_keys = true, // DECCKM
                ('l', 1) => self.app_cursor_keys = false,
                ('h', 1000) => self.mouse_protocol = MouseProtocol::Normal,
                ('h', 1002) => self.mouse_protocol = MouseProtocol::ButtonEvent,
                ('h', 1003) => self.mouse_protocol = MouseProtocol::AnyEvent,
                ('l', 1000) | ('l', 1002) | ('l', 1003) => self.mouse_protocol = MouseProtocol::Off,
                ('h', 1006) => self.mouse_encoding = MouseEncoding::Sgr,
                ('l', 1006) => self.mouse_encoding = MouseEncoding::Default,
                ('h', 1004) => self.focus_events = true,
                ('l', 1004) => self.focus_events = false,

                _ => {} // other DEC modes are later slices
            }
            return;
        }
        // Other private/intermediate sequences are later slices; ignore them
        // rather than misinterpret.
        if !intermediates.is_empty() {
            return;
        }
        match action {
            'A' => self.move_up(param_or(params, 0, 1) as usize),
            'B' | 'e' => self.move_down(param_or(params, 0, 1) as usize),
            'C' | 'a' => self.move_forward(param_or(params, 0, 1) as usize),
            'D' => self.move_back(param_or(params, 0, 1) as usize),
            'G' | '`' => self.set_col(param_or(params, 0, 1) as usize - 1),
            'd' => self.set_row(param_or(params, 0, 1) as usize - 1),
            'H' | 'f' => {
                let row = param_or(params, 0, 1) as usize - 1;
                let col = param_or(params, 1, 1) as usize - 1;
                self.goto(row, col);
            }
            'J' => self.erase_display(param_or(params, 0, 0)),
            'K' => self.erase_line(param_or(params, 0, 0)),
            'X' => self.erase_chars(param_or(params, 0, 1) as usize),
            '@' => self.insert_chars(param_or(params, 0, 1) as usize),
            'P' => self.delete_chars(param_or(params, 0, 1) as usize),
            'S' => self.scroll_up_lines(param_or(params, 0, 1) as usize),
            'T' => self.scroll_down_lines(param_or(params, 0, 1) as usize),
            'L' => self.insert_lines(param_or(params, 0, 1) as usize),
            'M' => self.delete_lines(param_or(params, 0, 1) as usize),
            'g' => self.clear_tab_stop(param_or(params, 0, 0)),
            'r' => {
                let rows = self.grid.rows() as u16;
                let top = param_or(params, 0, 1) as usize;
                let bottom = param_or(params, 1, rows) as usize;
                self.set_scroll_region(top, bottom);
            }
            'm' => self.sgr(params),
            's' => self.save_cursor(),    // SCOSC (CSI s) — alias of DECSC
            'u' => self.restore_cursor(), // SCORC (CSI u) — alias of DECRC
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            b'D' => self.linefeed(), // IND (line-feed without CR)
            b'E' => {
                // NEL (next line): carriage return + line-feed.
                self.carriage_return();
                self.linefeed();
            }
            b'H' => self.set_tab_stop(),   // HTS
            b'M' => self.reverse_index(),  // RI
            b'7' => self.save_cursor(),    // DECSC
            b'8' => self.restore_cursor(), // DECRC
            _ => {}
        }
    }

    /// OSC dispatch (#12 event surface): title (0/2), cwd (7). OSC 8 hyperlink
    /// is per-cell state, handled in its own slice (#26), not here.
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // params[0] is the OSC number; params[1..] the payload fields.
        let Some(&number) = params.first() else {
            return;
        };
        match number {
            // OSC 0 = icon + window title, OSC 2 = window title. Both set title.
            b"0" | b"2" => {
                if let Some(&title) = params.get(1) {
                    self.events.push(TermEvent::Title(
                        String::from_utf8_lossy(title).into_owned(),
                    ));
                }
            }
            // OSC 7 = current working directory (a file:// URI).
            b"7" => {
                if let Some(&cwd) = params.get(1) {
                    self.events
                        .push(TermEvent::Cwd(String::from_utf8_lossy(cwd).into_owned()));
                }
            }
            _ => {} // OSC 8 (#26) and others are later slices
        }
    }
}
