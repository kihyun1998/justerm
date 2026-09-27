//! Settled-cell paint (#967): the `Term` half of [`crate::paint`] — the change record the write
//! path keeps, the query that reports it as logical lines, and the colour write into the cells of
//! a reported line. Needs the whole buffer (the wrap join and the absolute coordinates), so it is
//! core mechanism; the rule that picks the spans is the consumer's
//! ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)).

use crate::cell::Cell;
use crate::grid::Row;
use crate::paint::{ChangedLine, LineRef, PaintSpan};

use super::{ChangeWatch, Term};

impl ChangeWatch {
    /// Both lines as reflow points, unset ones as `(0, 0)` — [`Self::reflowed`] reads them back
    /// by position.
    pub(super) fn points(&self) -> [(usize, usize); 2] {
        [
            (self.changed_from.unwrap_or(0), 0),
            (self.answered_from.unwrap_or(0), 0),
        ]
    }

    /// Re-anchor after a reflow, from the two points [`Self::points`] gave it. The lines answered
    /// since the previous reflow join the changed ones, and the answered record starts over.
    pub(super) fn reflowed(&mut self, mapped: &[(usize, usize)], evicted: usize) {
        let at = |i: usize| mapped[i].0.saturating_sub(evicted);
        let changed = self.changed_from.map(|_| at(0));
        let answered = self.answered_from.map(|_| at(1));
        self.changed_from = match (changed, answered) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.answered_from = None;
    }

    fn evict_oldest(&mut self, n: usize) {
        self.changed_from = self.changed_from.map(|l| l.saturating_sub(n));
        self.answered_from = self.answered_from.map(|l| l.saturating_sub(n));
    }
}

impl Term {
    /// The active buffer's change record.
    fn watch_mut(&mut self) -> &mut ChangeWatch {
        if self.on_alt {
            &mut self.alt_watch
        } else {
            &mut self.normal_watch
        }
    }

    /// Record that the content of absolute line `line` of the active buffer changed.
    pub(super) fn content_changed_at(&mut self, line: usize) {
        let watch = self.watch_mut();
        watch.changed_from = Some(watch.changed_from.map_or(line, |from| from.min(line)));
    }

    /// Shift both buffers' change records after `n` lines left the front of the buffer.
    pub(super) fn watches_evict_oldest(&mut self, n: usize) {
        self.normal_watch.evict_oldest(n);
        self.alt_watch.evict_oldest(n);
    }

    /// The active buffer's logical lines whose content changed since this last answered, in
    /// buffer order, each with the [`LineRef`] that paints it. Empty lines are left out.
    ///
    /// A line is reported whole — from the first row of its soft-wrap run — when any of its rows
    /// changed, and again each time it changes, so a line still being written is re-reported
    /// until it stops. A line is also reported when the text did not change but its colours or
    /// attributes did, and when the screen was repainted whole (a resize, an alt-screen switch,
    /// a user scroll): lines are reported from the lowest one that changed to the end of the
    /// buffer, so a line that did not change may be reported alongside ones that did. Painting
    /// reports nothing.
    pub fn changed_logical_lines(&mut self) -> Vec<ChangedLine> {
        let Some(from) = self.watch_mut().changed_from.take() else {
            return Vec::new();
        };
        let floor = self.abs_floor();
        let total = self.scrollback.len() + self.grid.rows();
        let mut start = from.clamp(floor, total);
        while start > floor && self.abs_row(start - 1).is_wrapped() {
            start -= 1;
        }
        let watch = self.watch_mut();
        watch.answered_from = Some(watch.answered_from.map_or(start, |from| from.min(start)));

        let mut out = Vec::new();
        let mut line = start;
        while line < total {
            let mut text = String::new();
            let last = self.walk_logical_line(&self.grid, line, |ch, _, _| text.push(ch));
            text.truncate(text.trim_end_matches(' ').len());
            if !text.is_empty() {
                out.push(ChangedLine {
                    at: LineRef {
                        line,
                        evicted_total: self.evicted_total,
                        alt: self.on_alt,
                    },
                    text,
                });
            }
            line = last + 1;
        }
        out
    }

    /// Write colour references into the cells behind `spans` of the logical line `at` names,
    /// if that line still reads `text`; returns whether it was painted.
    ///
    /// `at` is rebased by the lines evicted since it was reported. The paint is refused, and
    /// nothing changes, when the line has left the buffer, when it was on an alternate screen that
    /// has since closed, when `at` no longer names the start of a logical line, or when the line's
    /// text is no longer `text` — output since the report rewrote it, or a resize or a region
    /// scroll moved another line under the reference. A line that reads the same text is painted
    /// wherever it now is. A primary-screen line is painted even while the alternate screen is up.
    ///
    /// Only the channels a span names change, on the cells its `char`s came from; a wide glyph's
    /// spacer takes its lead's colour. Painting damages the painted cells like any cell write, and
    /// is not reported by [`Self::changed_logical_lines`].
    pub fn paint_logical_line(&mut self, at: LineRef, text: &str, spans: &[PaintSpan]) -> bool {
        let Some(evicted) = self.evicted_total.checked_sub(at.evicted_total) else {
            return false;
        };
        let Some(line) = usize::try_from(evicted)
            .ok()
            .and_then(|evicted| at.line.checked_sub(evicted))
        else {
            return false;
        };
        if at.alt && !self.on_alt {
            return false;
        }
        let active = at.alt == self.on_alt;
        let grid = if active { &self.grid } else { &self.alt_grid };
        let floor = if at.alt { self.scrollback.len() } else { 0 };
        let total = self.scrollback.len() + grid.rows();
        if line < floor || line >= total {
            return false;
        }
        if line > floor && self.row_in(grid, line - 1).is_wrapped() {
            return false;
        }

        let mut now = String::new();
        let mut cells: Vec<(usize, usize)> = Vec::new();
        self.walk_logical_line(grid, line, |ch, abs, col| {
            now.push(ch);
            cells.push((abs, col));
        });
        let trimmed = now.trim_end_matches(' ');
        if trimmed != text {
            return false;
        }
        cells.truncate(trimmed.chars().count());

        for span in spans {
            let end = span.end.min(cells.len());
            for &(abs, col) in cells.get(span.start..end).unwrap_or_default() {
                let row = self.row_for_paint(at.alt, abs);
                let pair = row[col].is_wide() && row.get(col + 1).is_some_and(Cell::is_wide_spacer);
                for c in col..=col + usize::from(pair) {
                    if let Some(fg) = span.fg {
                        row[c].set_fg(fg);
                    }
                    if let Some(bg) = span.bg {
                        row[c].set_bg(bg);
                    }
                }
                if active {
                    self.damage_painted(abs, col, col + usize::from(pair));
                }
            }
        }
        true
    }

    /// The row of absolute `line` in the buffer `alt` names, for writing.
    fn row_for_paint(&mut self, alt: bool, line: usize) -> &mut Row {
        let base = self.scrollback.len();
        if line < base {
            &mut self.scrollback[line]
        } else if alt == self.on_alt {
            self.grid.row_mut(line - base)
        } else {
            self.alt_grid.row_mut(line - base)
        }
    }

    /// Damage painted columns `[left, right]` of active-buffer line `line` — without recording a
    /// content change, since a paint changes none. While scrolled up only a full repaint reaches
    /// the view, so a visible line takes one.
    fn damage_painted(&mut self, line: usize, left: usize, right: usize) {
        let base = self.scrollback.len();
        let top = base - self.display_offset;
        if self.display_offset > 0 {
            if (top..top + self.grid.rows()).contains(&line) {
                self.full_damage = true;
            }
        } else if line >= base {
            let last = self.grid.cols() - 1;
            self.line_damage[line - base].expand(left.min(last), right.min(last));
        }
    }
}
