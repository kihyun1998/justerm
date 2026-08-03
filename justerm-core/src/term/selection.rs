//! The selection surface: the gesture entry points, the three fixups that keep an
//! anchor pointing at its content while the buffer moves under it, and two text
//! extractors — `selection_text` for the selected run, and `accessible_text`, which
//! reads the *active* buffer as one document (floored on the alt screen, like every
//! other absolute walk) and lives here only because it reuses the same extraction path,
//! not because it is a selection.
//!
//! The coordinate model — absolute `[scrollback ++ screen]` line indices, why they
//! survive an ordinary scroll, and the three places they do not — is stated in
//! [`crate::selection`]. Read it there; this module is the `Term` half of it.
//!
//! What is local to this site is the shape of that `Term` half. Three fixups
//! (`selection_shift_below_margin`, `selection_evict_oldest`, `selection_rotate_region`)
//! are `pub(super)` because the write path calls them from `term.rs` — each one beside
//! its decoration-marker counterpart, since both are absolute anchors and a buffer
//! motion moves them together. That pairing was weighed as a reason to merge the two
//! surfaces into one module and rejected in #584; the grounds and the counter-evidence
//! are recorded there, not re-argued here.
//!
//! `resolve` and `Resolved` stay private: every caller travelled into this module with
//! them. The remaining entry points are public API and keep `pub fn` — an inherent
//! impl's methods are reached through the type, not the module path, so a private child
//! module does not hide them.

use crate::selection::{Anchor, Selection, SelectionSpan, SelectionType, Side};

use super::Term;

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

    /// Shift selection endpoints anchored at absolute line `>= from` down by
    /// one (#449): a top-anchored sub-region scroll grew scrollback while the
    /// rows below the margin stayed fixed on screen, so their content's
    /// absolute index rose +1 and the anchors must follow it. Endpoints above
    /// `from` (in-region / scrollback content, whose indices are stable) are
    /// untouched — per endpoint, so a selection straddling the margin keeps
    /// both ends on their content.
    pub(super) fn selection_shift_below_margin(&mut self, from: usize) {
        if let Some(sel) = &mut self.selection {
            if sel.anchor.point.line >= from {
                sel.anchor.point.line += 1;
            }
            if sel.focus.point.line >= from {
                sel.focus.point.line += 1;
            }
        }
    }

    /// Shift the selection up by one absolute line after the oldest history line
    /// is evicted by the scrollback cap. An endpoint clamps to the new top; if
    /// the whole selection was on the evicted line, it is cleared.
    pub(super) fn selection_evict_oldest(&mut self) {
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
    /// otherwise down (dropped at `bottom`). Called once per scrolled line (delta
    /// 1) by linefeed/RI/SU/SD/IL/DL.
    ///
    /// Mirrors alacritty `Selection::rotate`: an endpoint pushed past the region
    /// edge is *clamped* to that edge (upper → `top`/col 0/Left, lower →
    /// `bottom`/last col/Right; columns/side kept for Block), preserving the part
    /// of the selection still in the buffer. The whole selection clears only on a
    /// true *overtake* — the upper endpoint crossing the bottom while the lower
    /// stays inside, or the lower falling above the upper (a selection wholly on
    /// the dropped line). (#174: this replaced a policy that cleared on any
    /// endpoint touching the dropped edge, dropping still-valid content.)
    pub(super) fn selection_rotate_region(&mut self, top: usize, bottom: usize, up: bool) {
        let (ty, anchor, focus) = match self.selection.as_ref() {
            Some(s) => (s.ty, s.anchor, s.focus),
            None => return,
        };
        let last_col = self.grid.cols().saturating_sub(1);
        // Order the endpoints by buffer position; the upper (`start`) clamps to
        // the region top, the lower (`end`) to the bottom. Remember which is the
        // anchor so the result writes back to the right field.
        let anchor_is_start = anchor.point <= focus.point;
        let (mut start, mut end) = if anchor_is_start {
            (anchor, focus)
        } else {
            (focus, anchor)
        };

        let (top_i, bottom_i) = (top as isize, bottom as isize);
        // The endpoint's line after the one-line scroll, or `None` if it's outside
        // the region (untouched). The dropped-edge line shifts *past* the edge (to
        // be clamped/overtaken below), matching alacritty's `line - delta`.
        let shift = |line: usize| -> Option<isize> {
            if line < top || line > bottom {
                None
            } else if up {
                Some(line as isize - 1)
            } else {
                Some(line as isize + 1)
            }
        };

        // Upper endpoint: clamp to the region top when pushed above it; clear if it
        // overtook the region bottom (down-scroll) while the lower stays inside.
        if let Some(nl) = shift(start.point.line) {
            if nl > bottom_i && (end.point.line as isize) <= bottom_i {
                self.selection = None;
                return;
            }
            if nl < top_i {
                start.point.line = top;
                if ty != SelectionType::Block {
                    start.point.col = 0;
                    start.side = Side::Left;
                }
            } else {
                start.point.line = nl as usize;
            }
        }
        // Lower endpoint: clear if it fell above the (rotated) upper endpoint;
        // else clamp to the region bottom when pushed below it.
        if let Some(nl) = shift(end.point.line) {
            if nl < start.point.line as isize {
                self.selection = None;
                return;
            }
            if nl > bottom_i {
                end.point.line = bottom;
                if ty != SelectionType::Block {
                    end.point.col = last_col;
                    end.side = Side::Right;
                }
            } else {
                end.point.line = nl as usize;
            }
        }

        if let Some(sel) = &mut self.selection {
            if anchor_is_start {
                (sel.anchor, sel.focus) = (start, end);
            } else {
                (sel.anchor, sel.focus) = (end, start);
            }
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
                    // Bound before reading, not after (#660). `abs_line` indexes the grid
                    // unguarded, so a line past the last visible row panics here — and the
                    // `push` closure below, which does apply the bound, never gets to run.
                    // The sibling projection already has this ordering right:
                    // `Term::match_spans` (`term/search.rs`) does `if row >= rows { break }`
                    // *before* its own `abs_line`, so this loop was the local outlier.
                    //
                    // This is not a clamp and truncates nothing observable: the function
                    // already drops off-screen rows silently and says so ("Empty when … the
                    // selection is fully scrolled off-screen"), so making the existing
                    // filter total converts a panic into the drop the contract promises.
                    // All three references bound at read time too — alacritty's
                    // `Selection::to_range` goes through `grid_clamp`, xterm.js's
                    // `translateBufferLineToString` returns `''` for a missing line, and
                    // ghostty clamps a pin's column against its own page.
                    //
                    // **Unreachable as this crate stands, and that is recorded rather than
                    // enjoyed.** With the anchor clamped at `viewport_to_abs` and the alt
                    // drop in `resize`, no path is known that reaches this loop with a line
                    // past the last row — measured: removing this bound reds no test in the
                    // suite. It is kept for the reason the `right - left + 1` widening in
                    // `serialize.rs` is kept (#582): it costs nothing, it makes the function
                    // total on its own rather than by trusting a guard two files away, and
                    // the walk it protects is one careless edit from an out-of-bounds index.
                    if line < top {
                        continue;
                    }
                    if line - top >= rows {
                        break;
                    }
                    let len = self.abs_line(line).len();
                    // Both ends, not one (ADR-0026 D3). `right_excl` was bounded and `left`
                    // was not, which is not half a guard: the raw end survives into the
                    // `right_excl > left` test below and drops the row instead of shortening
                    // it — silently, and only ever the *start* row, so a multi-row selection
                    // looks intact. Unreachable as this crate stands (#671 clamps the
                    // producer, `resize` re-clamps reflowed points, alt drops the selection),
                    // and kept for the reason the bound above it is kept: it makes the
                    // function total on its own rather than by trusting a guard two files
                    // away. `match_spans` is the same expression in `term/search.rs`, where
                    // the coordinate IS the consumer's and this is the only guard there is.
                    let left = if line == start_line { from.min(len) } else { 0 };
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
                // The Block arm bounds against the grid rather than each line, because a
                // rectangle is the same columns on every row by definition — `resolve`
                // already clipped `to` with `.min(cols)`, so only `from` was open
                // (ADR-0026 D3/D4). Same reachability as the Linear arm above.
                let cols = self.grid.cols();
                let from = from.min(cols);
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
            } => Some(self.extract_lines(&self.grid, start_line, from, end_line, to)),
            Resolved::Block {
                line0,
                line1,
                from,
                to,
            } => {
                // Each row independently — no soft-wrap joining.
                let mut out = String::new();
                for line in line0..=line1 {
                    let hi = to.min(self.abs_line(line).len());
                    let mut seg = String::new();
                    for col in from..hi {
                        self.append_cell(&self.grid, &mut seg, line, col);
                    }
                    out.push_str(seg.trim_end_matches(' '));
                    if line != line1 {
                        out.push('\n');
                    }
                }
                Some(out)
            }
        }
    }

    /// The whole buffer as one text document (#150): scrollback + screen assembled
    /// into logical lines (soft-wrap joined, wide-spacers skipped, trailing blanks
    /// trimmed at the logical end) — the accessible-view a screen reader reads as
    /// a document, distinct from the viewport row tree (#119). Reuses the
    /// selection extraction (`extract_lines`) over the full
    /// range. On the alt screen only the alt buffer is shown — its "scrollback" is
    /// the *primary* buffer's, not this app's — mirroring `viewport_logical_lines`'
    /// alt floor.
    pub fn accessible_text(&self) -> String {
        let total = self.scrollback.len() + self.grid.rows();
        if total == 0 {
            return String::new();
        }
        let start = self.abs_floor();
        let mut doc = self.extract_lines(&self.grid, start, 0, total - 1, usize::MAX);
        // Trim *trailing* empty lines (blank screen rows below the content) — pure
        // noise to a listener, and what a fresh screen would otherwise emit. Keep
        // *internal* blank lines (paragraph breaks between command outputs) — a
        // document wants those, unlike the viewport tree which drops all empties.
        doc.truncate(doc.trim_end_matches('\n').len());
        doc
    }
}
