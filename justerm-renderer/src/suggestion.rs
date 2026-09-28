//! The consumer's suggestion run (#972) — pure, host-testable, in cells.
//!
//! A suggestion is text the consumer draws after the cursor (an autosuggestion at a shell prompt).
//! It is renderer state only: no engine cell holds it, so it never reaches copy or search. This
//! module answers which cells the run takes and what they carry; the preedit's pass is a separate
//! writer (ADR-0028 D4) and shares nothing with it but the per-codepoint width.
//!
//! Rules, in the order they apply (`docs/map/territory/cell-compositing.md` holds the reasons):
//! the run starts at the cursor cell and keeps its head; it stops at the first cell that is not
//! blank, and at the right edge; a wide codepoint that does not fit is dropped whole and ends the
//! run; a cell covered by a highlight, a decoration or the hovered link keeps the engine's cell.

use crate::attrs::{
    DIM, INVERSE, STRIKETHROUGH, UNDERLINE, USTYLE_MASK, WIDE_CHAR, WIDE_CHAR_SPACER,
};
use crate::glyph_resolve::Cells;
use crate::preedit::{Codepoint, Patch};
use std::borrow::Cow;

/// One cell the suggestion writes: a flat grid index, the codepoint (`0` for a wide glyph's
/// trailing half) and the flags that replace the cell's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellWrite {
    pub idx: usize,
    pub cp: u32,
    pub flags: u16,
}

/// Whether the cell at `idx` draws nothing but its background: a space or empty codepoint, no
/// grapheme override, not half of a wide pair, and no flag that puts ink on an empty cell.
pub fn is_blank(cells: &Cells<'_>, idx: usize) -> bool {
    let cp = cells.codepoints.get(idx).copied().unwrap_or(0);
    let flags = cells.flags.get(idx).copied().unwrap_or(0);
    let cluster = cells.clusters.get(idx).is_some_and(|s| !s.is_empty());
    let inks = INVERSE | UNDERLINE | USTYLE_MASK | STRIKETHROUGH | WIDE_CHAR | WIDE_CHAR_SPACER;
    (cp == 0 || cp == u32::from(b' ')) && !cluster && flags & inks == 0
}

/// The cells `run` writes when the cursor is at `(cursor_col, cursor_row)`. `withheld(col)` is
/// true where something already describes the engine's cell on the cursor row (a highlight, a
/// decoration, the hovered link); such a cell is skipped but still counted, and a wide codepoint
/// with either half withheld is skipped whole.
pub fn writes(
    run: &[Codepoint],
    cursor_col: u32,
    cursor_row: u32,
    cells: &Cells<'_>,
    dim: bool,
    withheld: impl Fn(u32) -> bool,
) -> Vec<CellWrite> {
    let mut out = Vec::new();
    if run.is_empty() || cells.cols == 0 || cursor_row >= cells.rows {
        return out;
    }
    let base = cursor_row as usize * cells.cols as usize;
    let dim = if dim { DIM } else { 0 };
    let mut col = cursor_col;
    for c in run {
        let width = if c.wide { 2 } else { 1 };
        if col.checked_add(width).is_none_or(|end| end > cells.cols) {
            break;
        }
        if (col..col + width).any(|x| !is_blank(cells, base + x as usize)) {
            break;
        }
        if !(col..col + width).any(&withheld) {
            let idx = base + col as usize;
            if c.wide {
                out.push(CellWrite {
                    idx,
                    cp: c.cp,
                    flags: WIDE_CHAR | dim,
                });
                out.push(CellWrite {
                    idx: idx + 1,
                    cp: 0,
                    flags: WIDE_CHAR_SPACER | dim,
                });
            } else {
                out.push(CellWrite {
                    idx,
                    cp: c.cp,
                    flags: dim,
                });
            }
        }
        col += width;
    }
    out
}

/// The frame's columns with the suggestion written in, or `None` when it writes no cell. Each
/// written cell takes the suggestion's codepoint, flags and `fg` (a tagged colour reference) and
/// keeps its own background. The clusters and backgrounds are borrowed, never copied: a written cell
/// has no cluster to clear, since a cell with one is not blank.
#[allow(clippy::too_many_arguments)]
pub fn patch<'a>(
    run: &[Codepoint],
    cursor_col: u32,
    cursor_row: u32,
    suggestion_fg: u32,
    dim: bool,
    cells: &Cells<'a>,
    bg: &'a [u32],
    fg: &[u32],
    withheld: impl Fn(u32) -> bool,
) -> Option<Patch<'a>> {
    let w = writes(run, cursor_col, cursor_row, cells, dim, withheld);
    if w.is_empty() {
        return None;
    }
    let mut p = Patch {
        codepoints: cells.codepoints.to_vec(),
        flags: cells.flags.to_vec(),
        clusters: Cow::Borrowed(cells.clusters),
        bg: Cow::Borrowed(bg),
        fg: fg.to_vec(),
    };
    for cw in w {
        if let Some(slot) = p.codepoints.get_mut(cw.idx) {
            *slot = cw.cp;
        }
        if let Some(slot) = p.flags.get_mut(cw.idx) {
            *slot = cw.flags;
        }
        if let Some(slot) = p.fg.get_mut(cw.idx) {
            *slot = suggestion_fg;
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GA: u32 = 0xAC00; // U+AC00 HANGUL SYLLABLE GA, width 2
    const RGB_RED: u32 = (2 << 24) | 0xFF0000; // an Rgb colour reference
    const BG_BLUE: u32 = (1 << 24) | 4; // Indexed(4)

    fn narrow(s: &str) -> Vec<Codepoint> {
        s.chars()
            .map(|c| Codepoint {
                cp: c as u32,
                wide: false,
            })
            .collect()
    }

    struct Grid {
        cols: u32,
        rows: u32,
        cps: Vec<u32>,
        flags: Vec<u16>,
        clusters: Vec<String>,
    }

    impl Grid {
        fn blank(cols: u32, rows: u32) -> Self {
            let n = (cols * rows) as usize;
            Grid {
                cols,
                rows,
                cps: vec![u32::from(b' '); n],
                flags: vec![0; n],
                clusters: vec![String::new(); n],
            }
        }
        fn put(&mut self, row: u32, col: u32, s: &str) {
            for (i, c) in s.chars().enumerate() {
                self.cps[(row * self.cols + col) as usize + i] = c as u32;
            }
        }
        fn cells(&self) -> Cells<'_> {
            Cells {
                cols: self.cols,
                rows: self.rows,
                codepoints: &self.cps,
                flags: &self.flags,
                clusters: &self.clusters,
            }
        }
    }

    fn drawn(w: &[CellWrite], cols: u32) -> Vec<(u32, u32)> {
        w.iter().map(|c| ((c.idx as u32) % cols, c.cp)).collect()
    }

    #[test]
    fn a_narrow_run_draws_from_the_cursor_cell_onward() {
        let g = Grid::blank(10, 2);
        let w = writes(&narrow("abc"), 3, 1, &g.cells(), true, |_| false);
        assert_eq!(
            drawn(&w, 10),
            vec![(3, 'a' as u32), (4, 'b' as u32), (5, 'c' as u32)]
        );
        assert!(w.iter().all(|c| c.idx / 10 == 1), "all on the cursor row");
        assert!(w.iter().all(|c| c.flags == DIM));
    }

    #[test]
    fn without_dim_the_flags_are_empty() {
        let g = Grid::blank(10, 1);
        let w = writes(&narrow("a"), 0, 0, &g.cells(), false, |_| false);
        assert_eq!(w[0].flags, 0);
    }

    #[test]
    fn the_run_stops_at_the_first_cell_that_is_not_blank() {
        // zsh's RPROMPT sits at columns 6..: the suggestion must not draw over it.
        let mut g = Grid::blank(12, 1);
        g.put(0, 6, "12:00");
        let w = writes(&narrow("status --short"), 2, 0, &g.cells(), true, |_| false);
        assert_eq!(
            drawn(&w, 12).iter().map(|d| d.0).collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
    }

    #[test]
    fn at_pending_wrap_the_cursor_sits_on_the_last_typed_character_and_nothing_is_drawn() {
        // The cursor column is `cols - 1` and the character just typed is still in it.
        let mut g = Grid::blank(10, 1);
        g.put(0, 0, "git statu");
        g.put(0, 9, "s");
        let w = writes(&narrow("h"), 9, 0, &g.cells(), true, |_| false);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn the_run_is_clipped_at_the_right_edge_keeping_its_head() {
        let g = Grid::blank(6, 1);
        let w = writes(&narrow("abcdef"), 3, 0, &g.cells(), true, |_| false);
        assert_eq!(
            drawn(&w, 6),
            vec![(3, 'a' as u32), (4, 'b' as u32), (5, 'c' as u32)]
        );
    }

    #[test]
    fn a_wide_codepoint_takes_two_cells_as_a_lead_and_a_spacer() {
        let g = Grid::blank(10, 1);
        let run = [Codepoint { cp: GA, wide: true }];
        let w = writes(&run, 2, 0, &g.cells(), true, |_| false);
        assert_eq!(
            w,
            vec![
                CellWrite {
                    idx: 2,
                    cp: GA,
                    flags: WIDE_CHAR | DIM
                },
                CellWrite {
                    idx: 3,
                    cp: 0,
                    flags: WIDE_CHAR_SPACER | DIM
                },
            ]
        );
    }

    #[test]
    fn a_wide_codepoint_that_does_not_fit_at_the_edge_is_dropped_whole() {
        let g = Grid::blank(4, 1);
        let run = [
            Codepoint {
                cp: 'a' as u32,
                wide: false,
            },
            Codepoint { cp: GA, wide: true },
        ];
        let w = writes(&run, 2, 0, &g.cells(), true, |_| false);
        assert_eq!(drawn(&w, 4), vec![(2, 'a' as u32)]);
    }

    #[test]
    fn a_wide_codepoint_that_would_reach_a_non_blank_cell_is_dropped_whole() {
        let mut g = Grid::blank(10, 1);
        g.put(0, 3, "X");
        let run = [
            Codepoint {
                cp: 'a' as u32,
                wide: false,
            },
            Codepoint { cp: GA, wide: true },
        ];
        let w = writes(&run, 1, 0, &g.cells(), true, |_| false);
        assert_eq!(drawn(&w, 10), vec![(1, 'a' as u32)]);
    }

    #[test]
    fn half_of_a_wide_pair_counts_as_not_blank() {
        let mut g = Grid::blank(10, 1);
        g.flags[4] = WIDE_CHAR_SPACER;
        g.cps[4] = 0;
        let w = writes(&narrow("abcd"), 2, 0, &g.cells(), true, |_| false);
        assert_eq!(
            drawn(&w, 10).iter().map(|d| d.0).collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[test]
    fn a_blank_cell_that_draws_ink_counts_as_not_blank() {
        for ink in [INVERSE, UNDERLINE, STRIKETHROUGH] {
            let mut g = Grid::blank(10, 1);
            g.flags[3] = ink;
            let w = writes(&narrow("abc"), 2, 0, &g.cells(), true, |_| false);
            assert_eq!(drawn(&w, 10), vec![(2, 'a' as u32)], "ink flag {ink:#x}");
        }
    }

    #[test]
    fn a_grapheme_override_counts_as_not_blank() {
        let mut g = Grid::blank(10, 1);
        g.clusters[3] = "e\u{301}".to_string();
        let w = writes(&narrow("abc"), 2, 0, &g.cells(), true, |_| false);
        assert_eq!(drawn(&w, 10), vec![(2, 'a' as u32)]);
    }

    #[test]
    fn a_withheld_cell_keeps_the_engine_cell_and_the_run_continues_after_it() {
        let g = Grid::blank(10, 1);
        let w = writes(&narrow("abc"), 2, 0, &g.cells(), true, |c| c == 3);
        assert_eq!(drawn(&w, 10), vec![(2, 'a' as u32), (4, 'c' as u32)]);
    }

    #[test]
    fn a_wide_codepoint_with_either_half_withheld_is_withheld_whole() {
        let g = Grid::blank(10, 1);
        let run = [Codepoint { cp: GA, wide: true }];
        for held in [2, 3] {
            let w = writes(&run, 2, 0, &g.cells(), true, |c| c == held);
            assert!(w.is_empty(), "held {held}: {w:?}");
        }
    }

    #[test]
    fn an_anchor_at_or_past_the_right_edge_writes_nothing_and_never_overflows() {
        let g = Grid::blank(10, 1);
        for col in [10, u32::MAX - 1, u32::MAX] {
            let w = writes(&narrow("ab"), col, 0, &g.cells(), true, |_| false);
            assert!(w.is_empty(), "col {col}: {w:?}");
        }
        let run = [Codepoint { cp: GA, wide: true }];
        assert!(writes(&run, u32::MAX, 0, &g.cells(), true, |_| false).is_empty());
    }

    #[test]
    fn nothing_is_written_for_an_empty_run_or_an_off_grid_row() {
        let g = Grid::blank(10, 2);
        assert!(writes(&[], 0, 0, &g.cells(), true, |_| false).is_empty());
        assert!(writes(&narrow("a"), 0, 2, &g.cells(), true, |_| false).is_empty());
    }

    #[test]
    fn the_patch_writes_fg_and_flags_and_keeps_the_cells_own_background() {
        // A powerline-coloured row: the suggestion keeps its background.
        let mut g = Grid::blank(6, 1);
        g.flags[2] = crate::attrs::BLINK | crate::attrs::HIDDEN; // replaced, never inherited
        let bg = vec![BG_BLUE; 6];
        let fg = vec![0; 6];
        let p = patch(
            &narrow("ab"),
            2,
            0,
            RGB_RED,
            true,
            &g.cells(),
            &bg,
            &fg,
            |_| false,
        )
        .expect("writes two cells");
        assert_eq!(&p.codepoints[2..4], &['a' as u32, 'b' as u32]);
        assert_eq!(&p.flags[2..4], &[DIM, DIM]);
        assert_eq!(&p.fg[2..4], &[RGB_RED, RGB_RED]);
        assert_eq!(p.bg, bg, "every background is the cell's own");
        assert_eq!(p.fg[1], 0, "an unwritten cell keeps its fg");
    }

    #[test]
    fn the_patch_is_none_when_nothing_is_written() {
        let mut g = Grid::blank(4, 1);
        g.put(0, 0, "abcd");
        let bg = vec![0; 4];
        assert!(
            patch(
                &narrow("x"),
                1,
                0,
                RGB_RED,
                true,
                &g.cells(),
                &bg,
                &bg,
                |_| false
            )
            .is_none()
        );
    }
}
