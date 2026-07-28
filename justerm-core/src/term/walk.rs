//! Buffer-walk primitives: absolute-index access, logical-line stepping, word
//! extent, and text materialisation over the single `[scrollback ++ grid]` buffer.
//!
//! These are the shared floor the *read* surfaces stand on — search, selection,
//! decoration markers and `accessible_text` reach the buffer's cells through
//! `abs_line` / `abs_row` here rather than indexing it themselves.
//!
//! The alt-screen floor contract is **not** yet centralised, and this module is
//! not where that finishes: on the alt screen `scrollback` holds the *primary*
//! buffer's history, so an absolute-index walk has to floor at `scrollback.len()`
//! — a defect this crate has shipped three separate times (#113, #144, #207).
//! `abs_floor` states that rule and the two logical-line walkers here obey it,
//! but `viewport_logical_lines`, `search` and `accessible_text` still open-code
//! the same `if on_alt { scrollback.len() } else { 0 }` expression back in
//! `term.rs`. Do not read this module's existence as "the floor is handled".
//!
//! Visibility is `pub(super)` for what `term.rs` actually calls, private for what
//! only this module walks with — `abs_floor` included, since its only callers are
//! the two walkers below. `pub(super)` is not a widening: an item private to
//! `term` was already visible to `term` and all of its descendants, which is
//! exactly what `pub(super)` in a child restores.

use crate::cell::Cell;
use crate::grid::{Grid, Row};
use crate::selection::BufferPoint;

use super::Term;

impl Term {
    /// The cells of absolute buffer line `line`, reading the screen portion from
    /// `grid` (scrollback is shared). Callers pick the active grid (`abs_line`) or
    /// the primary grid (`primary_grid`, for command-mark text on the alt screen).
    pub(super) fn line_in<'a>(&'a self, grid: &'a Grid, line: usize) -> &'a [Cell] {
        if line < self.scrollback.len() {
            &self.scrollback[line]
        } else {
            grid.row(line - self.scrollback.len())
        }
    }

    /// The whole row of absolute buffer line `line` from `grid` (see `line_in`).
    pub(super) fn row_in<'a>(&'a self, grid: &'a Grid, line: usize) -> &'a Row {
        if line < self.scrollback.len() {
            &self.scrollback[line]
        } else {
            grid.row_ref(line - self.scrollback.len())
        }
    }

    /// The cells of absolute buffer line `line` on the *active* screen.
    pub(super) fn abs_line(&self, line: usize) -> &[Cell] {
        self.line_in(&self.grid, line)
    }

    /// The whole row of absolute buffer line `line` on the *active* screen.
    pub(super) fn abs_row(&self, line: usize) -> &Row {
        self.row_in(&self.grid, line)
    }

    /// The combining marks at absolute `(line, col)` reading `grid`, or `None` —
    /// flag-gated through the row's map, so a stale entry is never surfaced.
    fn combining_in<'a>(&'a self, grid: &'a Grid, line: usize, col: usize) -> Option<&'a [char]> {
        self.row_in(grid, line).combining_at(col)
    }

    /// The combining marks at absolute `(line, col)` on the *active* screen.
    pub(super) fn combining_at(&self, line: usize, col: usize) -> Option<&[char]> {
        self.combining_in(&self.grid, line, col)
    }

    /// Append the text at absolute `(line, col)` — its base glyph plus any
    /// combining marks from the row's map — to `out`. Wide-char spacers
    /// contribute nothing.
    pub(super) fn append_cell(&self, grid: &Grid, out: &mut String, line: usize, col: usize) {
        let cell = &self.line_in(grid, line)[col];
        if cell.is_spacer() {
            return;
        }
        out.push(cell.c());
        if let Some(marks) = self.combining_in(grid, line, col) {
            out.extend(marks);
        }
    }

    /// Concatenate the selected cells from `(start_line, from)` to
    /// `(end_line, to_end)` (half-open columns on the first/last line, whole
    /// lines between). Soft-wrapped rows (WRAPLINE) accumulate into one *logical*
    /// line so trailing-blank trimming happens only at the logical end — spaces
    /// at a wrap boundary are real content. A hard line-end flushes with `\n`.
    pub(super) fn extract_lines(
        &self,
        grid: &Grid,
        start_line: usize,
        from: usize,
        end_line: usize,
        to_end: usize,
    ) -> String {
        // The two column bounds are **not** symmetric: `to_end` is exclusive and absorbs a
        // one-past column through `.min(cells.len())` below, but `from` is inclusive and cannot.
        // A `from` of `cells.len()` — a command-start mark on a row its prompt exactly filled
        // (#562) — names the first cell of the *next* row, so step there. Left in place it selects
        // an empty run on this row and then, because the row is hard-ended, flushes that run with a
        // `\n` the command never contained; `doc_line_of` reported the wrong document line beside
        // it, sending an a11y "jump to previous command" one row off.
        let (start_line, from) =
            if from >= self.line_in(grid, start_line).len() && start_line < end_line {
                (start_line + 1, 0)
            } else {
                (start_line, from)
            };

        let mut out = String::new();
        let mut current = String::new();
        for line in start_line..=end_line {
            let cells = self.line_in(grid, line);
            let left = if line == start_line { from } else { 0 };
            let right = if line == end_line {
                to_end.min(cells.len())
            } else {
                cells.len()
            };
            // A degenerate range (sides inverting one cell) gives left > right;
            // clamp to empty rather than panic on the slice.
            let right = right.max(left);
            for col in left..right {
                self.append_cell(grid, &mut current, line, col);
            }

            let is_last = line == end_line;
            let soft = self.row_in(grid, line).is_wrapped();
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

    /// The lowest absolute line a soft-wrap buffer walk may reach. On the alt screen
    /// `scrollback` holds the *primary* buffer's history — a separate logical space —
    /// so a walk floors at `scrollback.len()` (the alt grid's first line) and must not
    /// join across it. Mirrors the `search()` (#144) and `viewport_logical_lines`
    /// (#113) floors: justerm's single `[scrollback ++ grid]` buffer reproduces the
    /// primary↔alt isolation xterm gets from separate `Buffer` objects.
    fn abs_floor(&self) -> usize {
        if self.on_alt {
            self.scrollback.len()
        } else {
            0
        }
    }

    /// The cell position before `(line, col)` in the *logical* line — the column
    /// to the left, or the end of the previous row if it soft-wrapped into this
    /// one. `None` at the buffer start or across a hard line-end.
    fn prev_pos(&self, line: usize, col: usize) -> Option<(usize, usize)> {
        if col > 0 {
            return Some((line, col - 1));
        }
        // Only step up while the previous row is still on *this* buffer (>= floor):
        // on alt, row 0 (`line == scrollback.len()`) must not join the primary
        // scrollback row below it, even when that row carries WRAPLINE (#207).
        if line > self.abs_floor() {
            let prev = self.abs_line(line - 1);
            if self.abs_row(line - 1).is_wrapped() {
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
        // Symmetric floor guard (#207): a row below the floor (primary scrollback on
        // alt) must not soft-wrap-join down into the alt grid. `line >= floor` holds
        // for any position reachable on alt once `prev_pos` is floored; kept explicit
        // so no future caller can cross from a primary row.
        if line >= self.abs_floor() && line + 1 < total && self.abs_row(line).is_wrapped() {
            return Some((line + 1, 0));
        }
        None
    }

    /// Is `(line, col)` the wide-wrap artefact — the blank a width-2 glyph left behind when it
    /// did not fit, which the text extractors drop entirely?
    ///
    /// Position is part of the definition, not a shortcut: the artefact is only ever the **last**
    /// column of a soft-wrapped row. The marker alone is not enough, because a row-shift verb
    /// moves whole cells and can carry it inward, where it describes nothing — and a stale
    /// marker mid-row must stay an ordinary blank, or two visually separate words silently
    /// become one in the clipboard (#528).
    ///
    /// Since #534 the write sites keep that from arising: `DCH` ends the wrap **before** its
    /// shift, so the marker is cleared while it is still at the last column, and `ICH` pushes it
    /// off the edge. So this clause is now defence in depth rather than the only defence —
    /// deliberately kept, because it is one comparison and it is the read side's own statement of
    /// ghostty's write-side page invariant (*"Spacer heads must be at the end"*,
    /// `terminal/page.zig:537` @ `e6e26e1`), which that engine asserts rather than tolerates.
    fn is_wrap_artefact(&self, line: usize, col: usize) -> bool {
        let cells = self.abs_line(line);
        cells[col].is_leading_spacer() && col + 1 == cells.len()
    }

    /// A `' '` cell the word walk passes *through* rather than stopping on, because it
    /// belongs to a glyph and is not a gap between words. Two kinds, gated differently
    /// (ADR-0025 D3 — position is part of the definition):
    /// - a wide glyph's **trailing** spacer (`is_wide_spacer`) — transparent where it actually
    ///   continues a wide lead whose character is not itself a boundary. A trailing spacer
    ///   carries no character of its own; it *stands for its lead* (ADR-0025 D4 — the pair is
    ///   one unit), so both halves of that test read the lead, never the spacer;
    /// - the **leading**-spacer wrap artefact — transparent *only* at the last column of a
    ///   wrapped row (`is_wrap_artefact`); a marker a row-shift carried inward describes
    ///   nothing and must still end a word (#528).
    ///
    /// Reading the spacer cell alone was wrong twice, both caught by the two-lens pass and
    /// both measured: **U+3000** IDEOGRAPHIC SPACE is wide *and* `is_whitespace()`, so the walk
    /// stepped onto its second cell before breaking on the lead and started the highlight on
    /// half a glyph (`　abc`, double-click `a` → `1..=4` instead of `2..=4`); and a **lead-less**
    /// trailing spacer (#529's orphan) claimed to continue a glyph that is not there, merging
    /// two words in the clipboard. Requiring a wide, non-boundary lead answers both. Note the
    /// boundary clause has exactly **one** live trigger under the current fixed set — U+3000;
    /// `│` is East-Asian-Ambiguous (width 1) and the rest of the set is ASCII — so it is a real
    /// case, not a class.
    ///
    /// Resolving a spacer through its lead is alacritty's idiom, in a walk:
    /// `term/search.rs:457-460` (`skip_wide`) *replaces* a `WIDE_CHAR_SPACER` cell with
    /// `iter.prev()` and matches on that. Ghostty states the same principle in its print path
    /// (`Terminal.zig:1132-1142`, *"the previous cell is a wide spacer tail, so we actually want
    /// the cell before that because that has the actual content"*), and its **write-side**
    /// integrity rule is this predicate clause for clause — `page.zig:514-534` rejects a spacer
    /// tail at `x == 0` and one whose `cells[x-1].wide != .wide`. What ghostty does *not* do is
    /// model this in its own word selection: `Screen.zig:3204` `selectWord` breaks on
    /// `!hasText()`, i.e. it classifies the spacer cell and stops there — the pre-#535 justerm
    /// behaviour. Do not read ghostty's selection code as precedent for this.
    ///
    /// Gating on the char alone cut every CJK word at its first glyph, because a trailing
    /// spacer's char is a space and read as a boundary (#535). For that **trailing** kind the
    /// walk was the crate's outlier — `append_cell`, `viewport_logical_lines` and `search`
    /// already skipped it. It now matches alacritty, which gates the flag in both inline walkers
    /// (`search.rs:556`, `:580`) plus a forward normalizer in `semantic_search_left` (`:521-525`)
    /// — `semantic_search_right` has no gate at all. xterm.js converges on the *transparency*
    /// only (`_isCharWordSeparator` returns early on a width-0 cell): its stated reason is that
    /// such cells *"are always to the right of wide characters"*, which is precisely the
    /// assumption the lead-less clause above exists to reject.
    ///
    /// For the **leading** kind the comparison used to invert, and not in this walk's favour:
    /// those same extractors gate on `is_spacer()` with *no* position test, so a marker stranded
    /// mid-row was dropped from the text and two visually separate words merged. Measured on a
    /// 6-column screen — `"cde"` + a DCH-stranded marker + `"XY"` rendered as `cde XY` and copied
    /// as `"cdeXY"`, with `search("cdeXY")` matching. The position rule here bounds the selection
    /// *range*; it never could fix that *text*, which is why the direction was to fix the write
    /// site rather than to widen the extractors. **#534 did that**, so the extractors' bare
    /// `is_spacer()` is now safe by construction: no path leaves a marker whose claim is false.
    /// Do not add a position test to them on this reasoning — the invariant is upstream.
    ///
    /// Note what that leaves the position clause here: a read-site echo of the invariant ghostty
    /// enforces when writing (a mid-row spacer head is a page-integrity violation there, because
    /// a row shift clears it). Alacritty, which gates leading-everywhere, still does *not* clear
    /// on `delete_chars` — so its unconditional gate carries the merge defect justerm no longer
    /// has.
    fn is_walk_transparent_spacer(&self, line: usize, col: usize) -> bool {
        if self.is_wrap_artefact(line, col) {
            return true;
        }
        let cells = self.abs_line(line);
        cells[col].is_wide_spacer()
            && col > 0
            && cells[col - 1].is_wide()
            && !is_word_boundary(cells[col - 1].c())
    }

    /// Walk left to the first cell of `p`'s word (a maximal run of non-boundary
    /// chars), following a soft wrap into the previous row.
    pub(super) fn word_start(&self, p: BufferPoint) -> BufferPoint {
        let cells = self.abs_line(p.line);
        let (mut line, mut col) = (p.line, p.col.min(cells.len().saturating_sub(1)));
        while let Some((pl, pc)) = self.prev_pos(line, col) {
            // The wrap artefact represents no column of text, so the walk passes *through* it
            // rather than stopping — else a word that wrapped only because a wide glyph did not
            // fit would be cut in half (#528).
            if !self.is_walk_transparent_spacer(pl, pc)
                && is_word_boundary(self.abs_line(pl)[pc].c())
            {
                break;
            }
            line = pl;
            col = pc;
        }
        BufferPoint { line, col }
    }

    /// Walk right to the last cell of `p`'s word, following a soft wrap into the
    /// next row.
    pub(super) fn word_end(&self, p: BufferPoint) -> BufferPoint {
        let cells = self.abs_line(p.line);
        let (mut line, mut col) = (p.line, p.col.min(cells.len().saturating_sub(1)));
        while let Some((nl, nc)) = self.next_pos(line, col) {
            if !self.is_walk_transparent_spacer(nl, nc)
                && is_word_boundary(self.abs_line(nl)[nc].c())
            {
                break; // (see `word_start`: a glyph's spacer is transparent to the walk)
            }
            line = nl;
            col = nc;
        }
        BufferPoint { line, col }
    }
}

/// Whether `c` ends a word for Word (semantic) selection. Whitespace plus a
/// punctuation set mirroring Alacritty's default `semantic_escape_chars`, so
/// path/URL-ish runs (`.`, `/`, `-`) stay one word.
fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || ",│`|:\"'()[]{}<>".contains(c)
}
