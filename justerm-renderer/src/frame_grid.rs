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
/// (the wasm decoder's views).
pub struct DamageFrame<'a> {
    /// `0` = Full (whole viewport), `1` = Partial (damaged subset).
    pub kind: u8,
    /// `(top, bottom, count)` scroll op, applied before spans (`count > 0` scrolls up).
    pub scroll: Option<(u16, u16, i16)>,
    /// Span directory, [`SPAN_STRIDE`] `u32`s per span.
    pub spans: &'a [u32],
    /// Span-ordered cell columns (indexed by a span's `cell_offset + i`). Each cell carries its
    /// **base** codepoint only; the decoder's `extra`/`side_table` grapheme-cluster columns are
    /// not scattered here — the renderer's glyph resolver is single-codepoint today, so wiring
    /// combining clusters / ZWJ sequences through is deferred to #285.
    pub codepoints: &'a [u32],
    pub fg: &'a [u32],
    pub bg: &'a [u32],
    pub flags: &'a [u16],
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
}

impl FrameGrid {
    /// A blank `cols`×`rows` grid (every cell codepoint `0` / colour ref `0` / no flags — the
    /// renderer resolves these as space / Default).
    pub fn new(cols: u32, rows: u32) -> Self {
        let n = (cols * rows) as usize;
        Self {
            cols,
            rows,
            codepoints: vec![0; n],
            fg: vec![0; n],
            bg: vec![0; n],
            flags: vec![0; n],
        }
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

    /// Scatter a decoded frame's damage into the grid.
    pub fn apply(&mut self, frame: &DamageFrame) {
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
            }
            s += SPAN_STRIDE;
        }
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
            } else {
                self.codepoints[dst] = 0;
                self.fg[dst] = 0;
                self.bg[dst] = 0;
                self.flags[dst] = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_span_scatters_into_the_dense_grid() {
        // 3x2 grid. A Partial frame: one span at line 1, cols 1..=2 (2 cells 'A','B').
        let mut g = FrameGrid::new(3, 2);
        let f = DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[1, 1, 2, 0, 2], // line=1, left=1, right=2, cell_offset=0, count=2
            codepoints: &[0x41, 0x42],
            fg: &[10, 11],
            bg: &[20, 21],
            flags: &[0, 0],
        };
        g.apply(&f);
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
        let mut g = FrameGrid::new(3, 2);
        // A Partial sets (0,0).
        g.apply(&DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[0, 0, 0, 0, 1],
            codepoints: &[0x58],
            fg: &[1],
            bg: &[2],
            flags: &[0],
        });
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
        });
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
        let mut g = FrameGrid::new(1, rows);
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
        });
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
            fg: &[],
            bg: &[],
            flags: &[],
        });
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
            fg: &[],
            bg: &[],
            flags: &[],
        });
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
        });
        assert_eq!(g.codepoints(), &[0x42, 0x43, 0x44, 0x5A]);
    }

    #[test]
    fn partial_frames_accumulate_across_calls() {
        // A Partial only carries its damage; prior cells persist (the whole reason the grid is
        // stateful — cell-mirror.ts / ADR-0011).
        let mut g = FrameGrid::new(2, 1);
        g.apply(&DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[0, 0, 0, 0, 1],
            codepoints: &[0x41],
            fg: &[0],
            bg: &[0],
            flags: &[0],
        });
        g.apply(&DamageFrame {
            kind: 1,
            scroll: None,
            spans: &[0, 1, 1, 0, 1], // only col 1 damaged
            codepoints: &[0x42],
            fg: &[0],
            bg: &[0],
            flags: &[0],
        });
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
            fg: &[],
            bg: &[],
            flags: &[],
        });
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
            fg: &[],
            bg: &[],
            flags: &[],
        });
        assert_eq!(g.codepoints(), &[0, 0, 0, 0]);
    }

    #[test]
    fn full_frame_ignores_a_stale_scroll_op() {
        // An alt-screen switch can leave a scroll op set on a Full frame (justerm-core marks
        // full-damage without clearing scroll). The Full's spans are authoritative — the stale
        // scroll must NOT shift the grid. Here the Full repaints both cells; a wrongly-applied
        // scroll would have shuffled them first.
        let mut g = FrameGrid::new(1, 2);
        g.apply(&DamageFrame {
            kind: 0,
            scroll: Some((0, 1, 1)), // stale scroll — must be ignored on a Full
            spans: &[0, 0, 0, 0, 1, 1, 0, 0, 1, 1],
            codepoints: &[0x41, 0x42],
            fg: &[0, 0],
            bg: &[0, 0],
            flags: &[0, 0],
        });
        assert_eq!(
            g.codepoints(),
            &[0x41, 0x42],
            "Full spans authoritative, scroll ignored"
        );
    }
}
