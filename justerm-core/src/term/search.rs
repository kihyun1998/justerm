//! The search query surface: finding matches across the whole buffer, and holding
//! the highlight set the consumer hands back so it rides the frame.
//!
//! The engine-finds / consumer-navigates split this implements is stated once, in
//! [`crate::search`] — do not restate it here, and read it there.
//!
//! What is worth saying at *this* site is why one private method lives in a query
//! module but is called eight times from the write path. `invalidate_search_highlights`
//! drops the held set rather than re-anchoring it, because a held match is
//! **query-derived**: the engine keeps matches, not the query, and the set itself may
//! have changed under the mutation. The selection is the contrast, and it is not a
//! clean one — it is re-anchored where it can be, and *cleared* where it cannot (both
//! alt swaps set `selection = None` on the line above the call, since neither buffer's
//! coordinates mean anything in the other). So the rule is about what is
//! **reproducible by the consumer**, not about re-anchoring always being available.
//!
//! Visibility: the seven entry points are public API and stay `pub fn` — an inherent
//! impl's methods are reached through the type, not the module path, so a private child
//! module does not hide them. `invalidate_search_highlights` takes `pub(super)` because
//! its callers are the write path in `term.rs`; `word_bounded` stays private because its
//! only caller came with it.

use unicode_width::UnicodeWidthChar;

use crate::search::{Match, SearchOptions};
use crate::selection::SelectionSpan;

use super::Term;

impl Term {
    /// Literal search over the whole buffer (`[scrollback ++ screen]`), returning
    /// every non-overlapping match top-to-bottom in absolute coordinates. Matches
    /// cross soft-wrapped rows (one logical line) and skip wide-char spacers.
    /// Smart-case: a query with no uppercase matches case-insensitively.
    pub fn search(&self, query: &str) -> Vec<Match> {
        self.search_with(query, SearchOptions::default())
    }

    /// Search with explicit [`SearchOptions`] — regex, whole-word, and a case-sensitivity override
    /// on top of the literal + smart-case [`search`](Self::search) (#314). Same coordinates,
    /// soft-wrap join, spacer skip, and grapheme-mark inclusion (#304) as `search`.
    pub fn search_with(&self, query: &str, opts: SearchOptions) -> Vec<Match> {
        let q: Vec<char> = query.chars().collect();
        if q.is_empty() {
            return Vec::new();
        }
        // Smart-case unless overridden: case-insensitive iff the query has no uppercase.
        let ci = opts
            .case_sensitive
            .map_or_else(|| !q.iter().any(|c| c.is_uppercase()), |cs| !cs);
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
        // Regex mode: build the pattern once (case-insensitivity from the same smart-case/override
        // decision). An invalid pattern yields no matches rather than erroring (#314).
        let re = if opts.regex {
            match regex::RegexBuilder::new(query).case_insensitive(ci).build() {
                Ok(re) => Some(re),
                Err(_) => return Vec::new(),
            }
        } else {
            None
        };
        let total = self.scrollback.len() + self.grid.rows();

        // Floored: primary matches are unreachable on alt, and a primary WRAPLINE row
        // would otherwise soft-wrap-join into the alt grid and corrupt the haystack at
        // the boundary. (#144)
        let floor = self.abs_floor();
        let mut matches = Vec::new();
        let mut r = floor;
        while r < total {
            // Build the logical line at `r`: join soft-wrapped rows, recording
            // each char's source position and skipping wide-char spacers.
            let mut hay: Vec<char> = Vec::new();
            let mut pos: Vec<(usize, usize)> = Vec::new();
            let mut line = r;
            loop {
                let cells = self.abs_line(line);
                for (col, cell) in cells.iter().enumerate() {
                    if cell.is_spacer() {
                        continue;
                    }
                    // Build the haystack UNFOLDED (regex needs the original text; its own
                    // case-insensitive flag handles case). The literal path folds at compare time.
                    hay.push(cell.c());
                    pos.push((line, col));
                    // Include the cell's grapheme side-table marks — combining marks, and under
                    // mode 2027 the joined emoji scalars (2nd RI, ZWJ-joined emoji, skin tone) —
                    // so a clustered scalar is findable, not just the base (#304). Each maps to the
                    // same cell column, mirroring `append_cell`'s base+marks extraction.
                    if let Some(marks) = self.combining_at(line, col) {
                        for &m in marks {
                            hay.push(m);
                            pos.push((line, col));
                        }
                    }
                }
                let soft = self.abs_row(line).is_wrapped();
                if soft && line + 1 < total {
                    line += 1;
                } else {
                    break;
                }
            }
            // Trim trailing blank padding (only a logical line's tail can be blank), so a regex `$`
            // anchor or a greedy `.*` doesn't run into the grid's blank cells — mirrors
            // `viewport_logical_lines`'s trim (#314 Lens 1). Keeps hay/pos in lockstep.
            while hay.last().is_some_and(|c| c.is_whitespace()) {
                hay.pop();
                pos.pop();
            }

            // A match at char-index range [cs, ce) → a Match, whole-word-filtered and deduped.
            // (Marks map many hay entries to one column (#304), so a repeated in-cluster scalar can
            // yield consecutive identical Matches — collapse them.)
            let push_range = |cs: usize, ce: usize, matches: &mut Vec<Match>| {
                if opts.whole_word && !word_bounded(&hay, cs, ce - cs) {
                    return;
                }
                let m = Match {
                    start_line: pos[cs].0,
                    start_col: pos[cs].1,
                    end_line: pos[ce - 1].0,
                    end_col: pos[ce - 1].1,
                };
                if matches.last() != Some(&m) {
                    matches.push(m);
                }
            };

            if let Some(re) = &re {
                // Regex over the (unfolded) logical line; map each match's byte range to char indices.
                let hay_str: String = hay.iter().collect();
                for mat in re.find_iter(&hay_str) {
                    if mat.start() == mat.end() {
                        continue; // skip empty matches (e.g. `a*` between chars)
                    }
                    let cs = hay_str[..mat.start()].chars().count();
                    let ce = hay_str[..mat.end()].chars().count();
                    push_range(cs, ce, &mut matches);
                }
            } else {
                // Slide the literal needle non-overlapping, folding each hay char at compare time.
                let mut i = 0;
                while needle.len() <= hay.len() && i + needle.len() <= hay.len() {
                    let hit = hay[i..i + needle.len()]
                        .iter()
                        .enumerate()
                        .all(|(k, &c)| fold(c) == needle[k]);
                    if hit {
                        let before = matches.len();
                        push_range(i, i + needle.len(), &mut matches);
                        // Advance past a real (accepted) match; a whole-word-rejected run advances by
                        // one so a later, word-bounded position at an overlapping offset is still tried.
                        i += if matches.len() > before {
                            needle.len()
                        } else {
                            1
                        };
                    } else {
                        i += 1;
                    }
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

    /// Set the search highlights to paint (#108). The consumer owns the
    /// `Vec<Match>` (it drives next/prev); handing it back here lets `frame()`
    /// project the highlights onto the viewport. An empty vec clears them.
    pub fn set_search_highlights(&mut self, matches: Vec<Match>) {
        self.search_highlights = matches;
        // A new set voids the designation: a stale index could be accidentally
        // in range and light wrong content (#428). The consumer re-designates.
        self.active_search_highlight = None;
    }

    /// Designate which member of the held highlight set is the *active* match
    /// (#428) — the one the consumer's next/prev navigation currently points at.
    /// `frame()` projects it into `overlay.active_match` (it also stays in
    /// `overlay.matches`; the renderer's ranking resolves the overlap, #424).
    /// `None` or an out-of-range index projects nothing; the designation resets
    /// whenever a new set is passed to [`set_search_highlights`](Self::set_search_highlights).
    /// The index resolves to its span at call time (#436) — both designation
    /// APIs converge on one stored representation.
    pub fn set_active_search_highlight(&mut self, index: Option<usize>) {
        self.active_search_highlight = index.and_then(|i| self.search_highlights.get(i)).copied();
    }

    /// Designate the *active* match by its absolute span (#436), independent of
    /// the held highlight set — the past-cap path: a backend that caps its
    /// hand-over (the documented 1000, xterm's `highlightLimit`) can still give
    /// the current match its active emphasis, exactly as xterm creates the
    /// active decoration from the found result outside the capped list. The
    /// span projects through the same viewport math as any match (wrap-aware);
    /// it need not be a member of the held set, so past the cap the match
    /// paints the ACTIVE colour only (no plain highlight underneath). `None`
    /// clears. Same lifecycle as the index form: reset on every
    /// [`set_search_highlights`](Self::set_search_highlights) hand-over and on
    /// any coordinate-shifting invalidation.
    pub fn set_active_search_match(&mut self, m: Option<Match>) {
        self.active_search_highlight = m;
    }

    /// Invalidate the held search highlights (#108). Called wherever a buffer
    /// mutation shifts the *line* coordinates the matches were found at — cap
    /// eviction, in-screen region/RI/SU/SD/IL/DL scroll, the accrual
    /// sub-region scroll (#449 — which also re-anchors selection/markers below
    /// the margin, `selection_shift_below_margin`), reflow, both alt swaps.
    /// In-line *column* shifts (ICH/DCH, insert-mode print) and
    /// in-place erases (ED/EL/ECH, overwrite) deliberately do NOT funnel — the
    /// set stales in place there exactly like the selection sibling and
    /// xterm's decorations, healed by the consumer's debounced re-search on
    /// output (which those mutations are). Search matches are query-derived
    /// (the engine holds matches, not the query, and the *set* itself may have
    /// changed), so unlike the user-authored selection they are dropped rather
    /// than re-anchored. Clearing avoids painting wrong content for the frame
    /// between the mutation and the consumer's refresh.
    pub(super) fn invalidate_search_highlights(&mut self) {
        self.search_highlights.clear();
        // #436: the active designation is a stored SPAN, no longer structurally
        // tied to the set — clear it in the same funnel or it would keep
        // painting coordinates that now hold arbitrary other text.
        self.active_search_highlight = None;
    }
}

/// Whether the run `hay[i..i+len]` is bounded by non-word characters on both sides — the `\bword\b`
/// sense for whole-word search (#314). A word char is alphanumeric or `_` (the regex `\w` set),
/// deliberately distinct from `is_word_boundary`'s wider semantic-selection set.
fn word_bounded(hay: &[char], i: usize, len: usize) -> bool {
    // A word char is alphanumeric, `_`, OR a grapheme-extending mark (width 0: combining marks,
    // ZWJ, variation selectors) — so a mark attached to a base is never read as a word boundary,
    // matching the regex `\b` sense (`\w` includes `\p{M}`) and staying consistent across the
    // literal and regex paths on decomposed graphemes (#314 Lens 1).
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c.width() == Some(0);
    let left = i == 0 || !is_word(hay[i - 1]);
    let right = i + len == hay.len() || !is_word(hay[i + len]);
    left && right
}
