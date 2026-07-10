//! Decoder→renderer frame adapter (#277) — pure, host-testable.
//!
//! `JustermRenderer::apply_frame` (#261) consumes a **dense row-major** grid, but a decoded
//! **Partial** frame (the common case after the first, `justerm-core` term.rs) ships only the
//! damaged cells in *span order*, with a directory saying where each span sits. Feeding that
//! straight to the renderer misaligns `bg[row*cols+col]` and silently repaints undamaged cells
//! as Default. [`FrameGrid`] keeps a persistent dense grid and scatters each frame's damage
//! into it, so the renderer always packs a coherent full viewport.
//!
//! Ports the shipped, tested logic of justerm-web `cell-mirror.ts` (ADR-0011) to Rust: a Full
//! frame wipes the grid first, a scroll op shifts the region before spans, then each span's
//! span-ordered cells scatter into their `(line, left+i)` slots.

/// `u32`s per span in the flat span directory: `line, left, right, cell_offset, cell_count`
/// (mirrors `justerm-wasm-decode` `SPAN_STRIDE`).
pub const SPAN_STRIDE: usize = 5;

/// A decoded damage frame as the renderer receives it: the `kind`/`scroll` header plus the
/// span directory and the span-ordered cell columns. Borrowed — the caller owns the buffers
/// (the wasm decoder's views). `Default` is the empty frame (no damage), so a test can spread
/// `..Default::default()` for the columns it doesn't exercise.
#[derive(Default)]
pub struct DamageFrame<'a> {
    /// `0` = Full (whole viewport), `1` = Partial (damaged subset).
    pub kind: u8,
    /// `(top, bottom, count)` scroll op, applied before spans (`count > 0` scrolls up).
    pub scroll: Option<(u16, u16, i16)>,
    /// Span directory, [`SPAN_STRIDE`] `u32`s per span.
    pub spans: &'a [u32],
    /// Span-ordered cell columns (indexed by a span's `cell_offset + i`).
    pub codepoints: &'a [u32],
    pub fg: &'a [u32],
    pub bg: &'a [u32],
    pub flags: &'a [u16],
    /// Span-ordered combining-cluster index per cell (#285): `0` = none (use the base
    /// codepoint), else a 1-based index into `side_table` for this cell's trailing combining
    /// marks. Frame-local — resolved to text at scatter time so a later frame's differing
    /// `side_table` can't invalidate a stored index.
    pub extra: &'a [u16],
    /// This frame's combining-mark clusters, referenced by a cell's `extra - 1`. Each entry is
    /// only the trailing width-0 **marks** (e.g. `"\u{0301}"`) — justerm-core stores the base
    /// glyph in `codepoints`, and never unifies ZWJ / skin-tone / flag sequences (each of those
    /// is its own cell), so the base must be prepended to render the whole grapheme.
    pub side_table: &'a [String],
}

/// A persistent dense (row-major) copy of the viewport's cells, updated by scattering each
/// decoded frame's damage into it. The columns are what the renderer packs.
pub struct FrameGrid {
    cols: u32,
    rows: u32,
    codepoints: Vec<u32>,
    fg: Vec<u32>,
    bg: Vec<u32>,
    flags: Vec<u16>,
    /// Per-cell resolved grapheme-cluster text (#285): a non-empty string overrides the base
    /// codepoint at render time. Resolved from the frame's `extra`/`side_table` at scatter (the
    /// index is frame-local, so it must be dereferenced while its frame is current), then
    /// persists like the other columns.
    clusters: Vec<String>,
}

/// How many cells a `cols`×`rows` grid holds, or `None` if that does not fit a `u32` (#355).
///
/// The wire caps both at `u16` (`justerm-core`'s `serialize.rs`), so 65535×65535 — 4_294_836_225,
/// just under `u32::MAX` — is the largest a frame from core can name, and it never overflows. But
/// `apply_damage` reads `cols`/`rows` out of a JS-supplied header, and `apply_frame` takes them as
/// bare `u32` arguments, so nothing binds a caller that does not come through core.
pub fn cell_count(cols: u32, rows: u32) -> Option<usize> {
    cols.checked_mul(rows).map(|n| n as usize)
}

/// Why a damage frame was refused (#355). Every variant is a *caller* error: the span directory or
/// the scroll region names cells outside the grid, or the frame does not carry the cells its spans
/// claim. `apply_damage` is wasm-exported, so these arrive from JS and are surfaced as thrown
/// errors — never as a wasm trap, which used to poison the renderer for good.
#[derive(Debug, PartialEq, Eq)]
pub enum DamageError {
    SpanOutsideGrid {
        line: usize,
        left: usize,
        count: usize,
        cols: usize,
        rows: usize,
    },
    SpanCellsMissing {
        cell_offset: usize,
        count: usize,
        cells: usize,
    },
    ScrollOutsideGrid {
        top: usize,
        bottom: usize,
        rows: usize,
    },
}

impl FrameGrid {
    /// A blank `cols`×`rows` grid (every cell codepoint `0` / colour ref `0` / no flags — the
    /// renderer resolves these as space / Default), or `None` if the grid has more cells than a
    /// `u32` can count — checked *before* any of the five per-cell vectors is reserved (#355).
    pub fn try_new(cols: u32, rows: u32) -> Option<Self> {
        let n = cell_count(cols, rows)?;
        Some(Self {
            cols,
            rows,
            codepoints: vec![0; n],
            fg: vec![0; n],
            bg: vec![0; n],
            flags: vec![0; n],
            clusters: vec![String::new(); n],
        })
    }

    pub fn cols(&self) -> u32 {
        self.cols
    }
    pub fn rows(&self) -> u32 {
        self.rows
    }
    pub fn codepoints(&self) -> &[u32] {
        &self.codepoints
    }
    pub fn fg(&self) -> &[u32] {
        &self.fg
    }
    pub fn bg(&self) -> &[u32] {
        &self.bg
    }
    pub fn flags(&self) -> &[u16] {
        &self.flags
    }
    pub fn clusters(&self) -> &[String] {
        &self.clusters
    }

    /// Check every index a scatter would produce, before it produces any of them (#355).
    ///
    /// `apply_damage` is wasm-exported: the span directory and the scroll region arrive as raw
    /// `u32`s from a JS caller, bound by nothing. They used to index `self.codepoints` directly, so
    /// a `line == rows` — an off-by-one, not an exotic value — trapped the module (`RuntimeError:
    /// unreachable`) and left it poisoned: every later call failed with "recursive use of an
    /// object". Reachable in a way the cell-count overflow never was.
    ///
    /// Validating up front, rather than checking as we go, is what makes a refusal *total*: the
    /// scatter wrote cells until it hit the bad index, so a rejected frame half-overwrote the grid.
    /// Same discipline as `resolve_frame`, which rasterises before it commits.
    fn validate(&self, frame: &DamageFrame) -> Result<(), DamageError> {
        let (cols, rows) = (self.cols as usize, self.rows as usize);

        // A Full frame ignores `scroll` (it repaints everything), so only a Partial can shift.
        // `top > bottom` is an empty region, not an error — `shift_region` simply does not iterate.
        // Only a `bottom` past the last row can walk off the grid.
        if let Some((top, bottom, _)) = frame.scroll
            && frame.kind != 0
            && top <= bottom
            && bottom as usize >= rows
        {
            return Err(DamageError::ScrollOutsideGrid {
                top: top as usize,
                bottom: bottom as usize,
                rows,
            });
        }

        // The span-ordered columns are read at `cell_offset + i`; the shortest one bounds them all.
        let cells = frame
            .codepoints
            .len()
            .min(frame.fg.len())
            .min(frame.bg.len())
            .min(frame.flags.len());
        let mut s = 0;
        while s + SPAN_STRIDE <= frame.spans.len() {
            let line = frame.spans[s] as usize;
            let left = frame.spans[s + 1] as usize;
            let cell_offset = frame.spans[s + 3] as usize;
            let count = frame.spans[s + 4] as usize;

            // `left + count` and `cell_offset + count` are `usize` sums of `u32`s: on wasm32 they
            // can wrap, so ask the checked question rather than the natural one.
            let past_right = left.checked_add(count).is_none_or(|r| r > cols);
            if line >= rows || past_right {
                return Err(DamageError::SpanOutsideGrid {
                    line,
                    left,
                    count,
                    cols,
                    rows,
                });
            }
            if cell_offset.checked_add(count).is_none_or(|e| e > cells) {
                return Err(DamageError::SpanCellsMissing {
                    cell_offset,
                    count,
                    cells,
                });
            }
            s += SPAN_STRIDE;
        }
        Ok(())
    }

    /// Scatter a decoded frame's damage into the grid, or refuse it whole (#355).
    pub fn apply(&mut self, frame: &DamageFrame) -> Result<(), DamageError> {
        self.validate(frame)?;
        let cols = self.cols as usize;

        if frame.kind == 0 {
            // A Full frame is the whole viewport — wipe stale cells first, or content outside
            // the new spans resurrects as ghosts (cell-mirror.ts step 0). A Full is
            // authoritative (core ships every row as a span), so any `scroll` op is ignored: an
            // alt-screen switch can leave a stale scroll set on a Full frame (justerm-core
            // term.rs marks full-damage without clearing it), and shifting here would be
            // meaningless against a full repaint.
            self.codepoints.fill(0);
            self.fg.fill(0);
            self.bg.fill(0);
            self.flags.fill(0);
            for c in &mut self.clusters {
                c.clear();
            }
        } else if let Some((top, bottom, count)) = frame.scroll {
            // A Partial's scroll op precedes its spans: shift the stored region so retained
            // cells move with it; the spans then repaint the exposed line (core ships it as a
            // full-width span, with the BCE background — the shift's transient blank is
            // overwritten). An over-height `count` (scrolls accumulate unbounded between acks,
            // then narrow to i16) lands every row's source outside `[top, bottom]`, so
            // `shift_region` blanks the whole region — the spans repaint it (cell-mirror.ts
            // step 1).
            self.shift_region(top as usize, bottom as usize, count as isize);
        }

        let spans = frame.spans;
        let mut s = 0;
        while s + SPAN_STRIDE <= spans.len() {
            let line = spans[s] as usize;
            let left = spans[s + 1] as usize;
            let cell_offset = spans[s + 3] as usize;
            let count = spans[s + 4] as usize;
            for i in 0..count {
                let src = cell_offset + i;
                let dst = line * cols + left + i;
                self.codepoints[dst] = frame.codepoints[src];
                self.fg[dst] = frame.fg[src];
                self.bg[dst] = frame.bg[src];
                self.flags[dst] = frame.flags[src];
                // Resolve the frame-local grapheme index to its cluster text NOW (while this
                // frame's side_table is current) and store the text — a `0` index (or an empty
                // side_table) clears any stale cluster on the cell. justerm-core's side_table
                // holds ONLY the trailing width-0 combining marks (grid.rs `Combining`), not the
                // base glyph — that stays in `codepoints`. Assemble the full grapheme = base +
                // marks so the resolver rasterises e.g. "e\u{301}", not a lone floating accent.
                let ex = frame.extra.get(src).copied().unwrap_or(0) as usize;
                let cluster = &mut self.clusters[dst];
                match ex.checked_sub(1).and_then(|i| frame.side_table.get(i)) {
                    Some(marks) => {
                        cluster.clear();
                        cluster.push(char::from_u32(frame.codepoints[src]).unwrap_or(' '));
                        cluster.push_str(marks);
                    }
                    None => cluster.clear(),
                }
            }
            s += SPAN_STRIDE;
        }
        Ok(())
    }

    /// Shift rows `[top, bottom]` by `count` (`> 0` up, exposing blanks at the bottom; `< 0`
    /// down, exposing at the top). Iterates so the copy never reads an already-overwritten
    /// source: ascending for an up-shift (dst below its source), descending for a down-shift
    /// (cell-mirror.ts `shiftRegion`).
    fn shift_region(&mut self, top: usize, bottom: usize, count: isize) {
        if count > 0 {
            for y in top..=bottom {
                self.shift_row(y, top, bottom, count);
            }
        } else if count < 0 {
            for y in (top..=bottom).rev() {
                self.shift_row(y, top, bottom, count);
            }
        }
    }

    /// Move row `src = y + count` into row `y` (all four columns), or blank `y` when the source
    /// is outside `[top, bottom]` (a newly-exposed line).
    fn shift_row(&mut self, y: usize, top: usize, bottom: usize, count: isize) {
        let cols = self.cols as usize;
        let src = y as isize + count;
        let in_range = src >= top as isize && src <= bottom as isize;
        for x in 0..cols {
            let dst = y * cols + x;
            if in_range {
                let s = src as usize * cols + x;
                self.codepoints[dst] = self.codepoints[s];
                self.fg[dst] = self.fg[s];
                self.bg[dst] = self.bg[s];
                self.flags[dst] = self.flags[s];
                self.clusters[dst] = self.clusters[s].clone();
            } else {
                self.codepoints[dst] = 0;
                self.fg[dst] = 0;
                self.bg[dst] = 0;
                self.flags[dst] = 0;
                self.clusters[dst].clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal Partial frame. `flags` must be as long as the other columns: the scatter reads
    /// `frame.flags[src]` raw, exactly like `codepoints`.
    fn damage<'a>(
        spans: &'a [u32],
        cps: &'a [u32],
        flags: &'a [u16],
        scroll: Option<(u16, u16, i16)>,
    ) -> DamageFrame<'a> {
        DamageFrame {
            kind: 1,
            scroll,
            spans,
            codepoints: cps,
            fg: cps,
            bg: cps,
            flags,
            ..Default::default()
        }
    }

    #[test]
    fn a_span_pointing_outside_the_grid_is_refused_and_changes_nothing() {
        // #355 (found by the sibling lens, reproduced in Chromium): `dst = line * cols + left + i`
        // indexed `self.codepoints` raw. `line = rows` walked off the end -> `RuntimeError:
        // unreachable`, and the wasm instance stayed poisoned ("recursive use of an object") for
        // every later call. `apply_frame`'s guards return a JsValue; this one killed the renderer.
        //
        // Refusal must also be TOTAL: the scatter used to write cells before reaching the bad
        // index, so a rejected frame half-overwrote the grid.
        let mut g = FrameGrid::try_new(4, 2).unwrap();
        g.codepoints[0] = 0x41;

        // span = [line, left, _, cell_offset, count]; line 2 is one past the last row.
        let err = g.apply(&damage(&[2, 0, 0, 0, 1], &[0x42], &[0], None));
        assert!(matches!(err, Err(DamageError::SpanOutsideGrid { .. })));
        assert_eq!(
            g.codepoints[0], 0x41,
            "a refused frame must not have written anything"
        );
    }

    #[test]
    fn a_span_running_past_the_last_column_is_refused() {
        let mut g = FrameGrid::try_new(4, 2).unwrap();
        // left 3 + count 2 = 5 > cols 4: the second cell would spill onto the next row.
        let err = g.apply(&damage(&[0, 3, 0, 0, 2], &[0x41, 0x42], &[0, 0], None));
        assert!(matches!(err, Err(DamageError::SpanOutsideGrid { .. })));
    }

    #[test]
    fn a_span_claiming_more_cells_than_the_columns_carry_is_refused() {
        let mut g = FrameGrid::try_new(4, 2).unwrap();
        // count 3 but only one codepoint was sent: `src = cell_offset + i` used to index raw.
        let err = g.apply(&damage(&[0, 0, 0, 0, 3], &[0x41], &[0], None));
        assert!(matches!(err, Err(DamageError::SpanCellsMissing { .. })));
    }

    #[test]
    fn a_scroll_region_outside_the_grid_is_refused() {
        let mut g = FrameGrid::try_new(4, 2).unwrap();
        // bottom = 5 on a 2-row grid: `shift_row`'s `dst = y * cols + x` walked off the end.
        let err = g.apply(&damage(&[], &[], &[], Some((0, 5, 1))));
        assert!(matches!(err, Err(DamageError::ScrollOutsideGrid { .. })));
        // top > bottom is an EMPTY region, not an error — the existing loop simply does not run.
        assert!(g.apply(&damage(&[], &[], &[], Some((1, 0, 1)))).is_ok());
    }

    #[test]
    fn an_in_bounds_partial_frame_still_applies() {
        // The control. A guard that refused everything would satisfy the four tests above.
        let mut g = FrameGrid::try_new(4, 2).unwrap();
        assert!(
            g.apply(&damage(&[1, 2, 0, 0, 2], &[0x41, 0x42], &[0, 0], None))
                .is_ok()
        );
        let cell = |row: usize, col: usize| g.codepoints[row * 4 + col];
        assert_eq!(cell(1, 2), 0x41);
        assert_eq!(cell(1, 3), 0x42);
    }

    #[test]
    fn a_grid_whose_cell_count_overflows_is_refused_before_it_allocates() {
        // RED (#355). `cols * rows` is a u32 multiply, evaluated BEFORE `resolve_frame`'s guard on
        // the `apply_damage` path — so this is the first thing to blow up, not the last. The wire
        // caps both at u16, but `apply_damage` reads them out of a JS-supplied header.
        // The arithmetic is tested through `cell_count`, not the constructor: the largest grid the
        // wire can express is 65535^2 cells, and *allocating* it here would ask for ~100 GB (the
        // per-cell `String` alone is 24 bytes). Test the property, not the allocator.
        assert_eq!(cell_count(u32::MAX, 2), None);
        assert_eq!(cell_count(65_535, 65_535), Some(4_294_836_225)); // fits u32, just
        assert_eq!(cell_count(3, 2), Some(6));
        // And the constructor refuses before it reserves anything.
        assert!(FrameGrid::try_new(u32::MAX, 2).is_none());
        assert!(FrameGrid::try_new(3, 2).is_some());
    }

    #[test]
    fn partial_span_scatters_into_the_dense_grid() {
        // 3x2 grid. A Partial frame: one span at line 1, cols 1..=2 (2 cells 'A','B').
        let mut g = FrameGrid::try_new(3, 2).unwrap();
        let f = DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[1, 1, 2, 0, 2], // line=1, left=1, right=2, cell_offset=0, count=2
            codepoints: &[0x41, 0x42],
            fg: &[10, 11],
            bg: &[20, 21],
            flags: &[0, 0],
            ..Default::default()
        };
        g.apply(&f).unwrap();
        // idx = row*cols + col. (1,1) = 4, (1,2) = 5.
        assert_eq!(g.codepoints()[4], 0x41);
        assert_eq!(g.codepoints()[5], 0x42);
        assert_eq!(g.fg()[4], 10);
        assert_eq!(g.bg()[5], 21);
        // Untouched cells stay blank.
        assert_eq!(g.codepoints()[0], 0);
        assert_eq!(g.codepoints()[3], 0);
    }

    #[test]
    fn full_frame_wipes_stale_cells_before_scattering() {
        // cell-mirror.ts step 0: a Full frame is the whole viewport, so stale cells outside
        // the new spans must be wiped or they resurrect as ghosts.
        let mut g = FrameGrid::try_new(3, 2).unwrap();
        // A Partial sets (0,0).
        g.apply(&DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[0, 0, 0, 0, 1],
            codepoints: &[0x58],
            fg: &[1],
            bg: &[2],
            flags: &[0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.codepoints()[0], 0x58);
        // A Full frame whose only span covers (1,1): (0,0) must be wiped, not resurrected.
        g.apply(&DamageFrame {
            kind: 0,
            scroll: None,
            spans: &[1, 1, 1, 0, 1],
            codepoints: &[0x59],
            fg: &[3],
            bg: &[4],
            flags: &[0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.codepoints()[0], 0, "Full wipes the stale (0,0)");
        assert_eq!(g.fg()[0], 0, "wiped cell's colour ref reset too");
        assert_eq!(
            g.codepoints()[4],
            0x59,
            "Full's span at (1,1) still scatters"
        ); // 1*3+1
    }

    /// Fill a 1-column grid's rows with the given codepoints via a Full frame (one span/row).
    fn fill_col(codes: &[u32]) -> FrameGrid {
        let rows = codes.len() as u32;
        let mut g = FrameGrid::try_new(1, rows).unwrap();
        let mut spans = Vec::new();
        for (row, _) in codes.iter().enumerate() {
            spans.extend_from_slice(&[row as u32, 0, 0, row as u32, 1]);
        }
        let zeros_u32 = vec![0u32; codes.len()];
        let zeros_u16 = vec![0u16; codes.len()];
        g.apply(&DamageFrame {
            kind: 0,
            scroll: None,
            spans: &spans,
            codepoints: codes,
            fg: &zeros_u32,
            bg: &zeros_u32,
            flags: &zeros_u16,
            ..Default::default()
        })
        .unwrap();
        g
    }

    #[test]
    fn scroll_up_shifts_the_region_before_spans() {
        // Rows A,B,C,D; scroll up by 1 over [0,3] with no spans → B,C,D,blank.
        let mut g = fill_col(&[0x41, 0x42, 0x43, 0x44]);
        g.apply(&DamageFrame {
            kind: 1,
            scroll: Some((0, 3, 1)), // top=0, bottom=3, count=+1 (up)
            spans: &[],
            codepoints: &[],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.codepoints(), &[0x42, 0x43, 0x44, 0]);
    }

    #[test]
    fn scroll_down_shifts_the_region_and_exposes_the_top() {
        // Rows A,B,C,D; scroll down by 1 over [0,3] → blank,A,B,C (descending copy order).
        let mut g = fill_col(&[0x41, 0x42, 0x43, 0x44]);
        g.apply(&DamageFrame {
            kind: 1,
            scroll: Some((0, 3, -1)), // count = -1 (down)
            spans: &[],
            codepoints: &[],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.codepoints(), &[0, 0x41, 0x42, 0x43]);
    }

    #[test]
    fn scroll_then_span_repaints_the_exposed_line() {
        // Scroll up exposes a blank bottom row; the same frame's span repaints it (the common
        // shape for a new line at the bottom). Scroll is applied before spans.
        let mut g = fill_col(&[0x41, 0x42, 0x43, 0x44]);
        g.apply(&DamageFrame {
            kind: 1,
            scroll: Some((0, 3, 1)),
            spans: &[3, 0, 0, 0, 1], // repaint row 3 with 'Z'
            codepoints: &[0x5A],
            fg: &[0],
            bg: &[0],
            flags: &[0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.codepoints(), &[0x42, 0x43, 0x44, 0x5A]);
    }

    #[test]
    fn partial_frames_accumulate_across_calls() {
        // A Partial only carries its damage; prior cells persist (the whole reason the grid is
        // stateful — cell-mirror.ts / ADR-0011).
        let mut g = FrameGrid::try_new(2, 1).unwrap();
        g.apply(&DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[0, 0, 0, 0, 1],
            codepoints: &[0x41],
            fg: &[0],
            bg: &[0],
            flags: &[0],
            ..Default::default()
        })
        .unwrap();
        g.apply(&DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[0, 1, 1, 0, 1], // only col 1 damaged
            codepoints: &[0x42],
            fg: &[0],
            bg: &[0],
            flags: &[0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            g.codepoints(),
            &[0x41, 0x42],
            "col 0 persisted, col 1 updated"
        );
    }

    #[test]
    fn scroll_sub_region_leaves_rows_outside_the_margins_untouched() {
        // DECSTBM margins: scroll only rows [1,2] up by 1. Rows 0 and 3 must not move.
        let mut g = fill_col(&[0x41, 0x42, 0x43, 0x44]);
        g.apply(&DamageFrame {
            kind: 1,
            scroll: Some((1, 2, 1)),
            spans: &[],
            codepoints: &[],
            ..Default::default()
        })
        .unwrap();
        // row0 (A) untouched; [1,2] shift up → row1=C, row2=blank; row3 (D) untouched.
        assert_eq!(g.codepoints(), &[0x41, 0x43, 0, 0x44]);
    }

    #[test]
    fn scroll_count_exceeding_region_height_blanks_it() {
        // Scrolls accumulate unbounded between acks (justerm-core term.rs); a count larger than
        // the region height must blank the whole region (every row's source falls outside it),
        // not over-read — the frame's spans then repaint it.
        let mut g = fill_col(&[0x41, 0x42, 0x43, 0x44]);
        g.apply(&DamageFrame {
            kind: 1,
            scroll: Some((0, 3, 9)), // count 9 >> region height 4
            spans: &[],
            codepoints: &[],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.codepoints(), &[0, 0, 0, 0]);
    }

    #[test]
    fn full_frame_ignores_a_stale_scroll_op() {
        // An alt-screen switch can leave a scroll op set on a Full frame (justerm-core marks
        // full-damage without clearing scroll). The Full's spans are authoritative — the stale
        // scroll must NOT shift the grid. Here the Full repaints both cells; a wrongly-applied
        // scroll would have shuffled them first.
        let mut g = FrameGrid::try_new(1, 2).unwrap();
        g.apply(&DamageFrame {
            kind: 0,
            scroll: Some((0, 1, 1)), // stale scroll — must be ignored on a Full
            spans: &[0, 0, 0, 0, 1, 1, 0, 0, 1, 1],
            codepoints: &[0x41, 0x42],
            fg: &[0, 0],
            bg: &[0, 0],
            flags: &[0, 0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            g.codepoints(),
            &[0x41, 0x42],
            "Full spans authoritative, scroll ignored"
        );
    }

    #[test]
    fn scatter_assembles_base_plus_marks_and_persists() {
        // #285: a cell's `extra` is a 1-based index into THIS frame's side_table, which holds
        // only the trailing combining MARKS (justerm-core grid.rs). The base glyph is in
        // `codepoints`, so the grid stores base + marks. Resolve at scatter (while the frame is
        // current) so a later frame's different side_table can't invalidate a stored index.
        let mut g = FrameGrid::try_new(2, 1).unwrap();
        g.apply(&DamageFrame {
            kind: 1,
            spans: &[0, 0, 1, 0, 2],   // both cells on row 0
            codepoints: &[0x65, 0x41], // base 'e' (cell0), 'A' (cell1)
            fg: &[0, 0],
            bg: &[0, 0],
            flags: &[0, 0],
            extra: &[1, 0], // cell0 → side_table[0]; cell1 no cluster
            side_table: &["\u{0301}".to_string()], // combining acute (marks only, as core emits)
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.clusters()[0], "e\u{0301}", "grid assembles base + marks");
        assert_eq!(g.clusters()[1], "", "cell1 has no cluster");
        // A later Partial (its own, cluster-free side_table) touching only cell1 must leave
        // cell0's stored cluster intact — the resolved text doesn't depend on the new frame.
        g.apply(&DamageFrame {
            kind: 1,
            spans: &[0, 1, 1, 0, 1],
            codepoints: &[0x42],
            fg: &[0],
            bg: &[0],
            flags: &[0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            g.clusters()[0],
            "e\u{0301}",
            "cluster persists across Partials"
        );
    }

    #[test]
    fn full_wipes_and_scroll_moves_the_cluster_column() {
        let mut g = FrameGrid::try_new(1, 2).unwrap();
        g.apply(&DamageFrame {
            kind: 1,
            spans: &[0, 0, 0, 0, 1],
            codepoints: &[0x61], // base 'a'
            fg: &[0],
            bg: &[0],
            flags: &[0],
            extra: &[1],
            side_table: &["\u{0308}".to_string()], // combining diaeresis → "a\u{0308}"
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.clusters()[0], "a\u{0308}");
        // Scroll down 1 over [0,1]: the cluster moves with its row; the exposed top clears.
        g.apply(&DamageFrame {
            kind: 1,
            scroll: Some((0, 1, -1)),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.clusters()[1], "a\u{0308}", "cluster shifts with the row");
        assert_eq!(g.clusters()[0], "", "exposed row's cluster cleared");
        // A Full frame with no cluster wipes the column.
        g.apply(&DamageFrame {
            kind: 0,
            spans: &[0, 0, 0, 0, 1, 1, 0, 0, 1, 1],
            codepoints: &[0x41, 0x42],
            fg: &[0, 0],
            bg: &[0, 0],
            flags: &[0, 0],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            g.clusters(),
            &["".to_string(), "".to_string()],
            "Full wipes clusters"
        );
    }
}
