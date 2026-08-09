//! The IME preedit run — pure, host-testable, in cells.
//!
//! An in-progress composition is **browser-owned state the engine never sees** (ADR-0028): it
//! arrives from `compositionupdate`, not from a frame, so nothing in the grid describes it and
//! nothing here may consult one. This module answers only *which cells the run occupies*; what is
//! painted into them is the pack-time pass.
//!
//! Ported from ghostty's `renderer/State.zig` `Preedit` (`e6e26e1`), including its two tests —
//! it is the only reference that separates the geometry from the drawing, and the right-edge rule
//! (shift the run left rather than clip it) keeps every codepoint the user is currently typing on
//! screen. alacritty instead shortens with an ellipsis (`display/mod.rs` `StrShortener`); neither
//! wraps.

use unicode_width::UnicodeWidthChar;

/// One codepoint of the run, with the width it occupies already decided.
///
/// `wide` is resolved by [`is_wide`] at the edge rather than re-asked per cell, exactly as ghostty
/// carries `Codepoint{ codepoint, wide }` from its apprt layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Codepoint {
    pub cp: u32,
    pub wide: bool,
}

/// Where the run lands: an inclusive cell span, plus how many leading codepoints had to be dropped
/// for it to fit (`cp_offset`, an index into the run — `run[cp_offset..]` is what is drawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: u32,
    pub end: u32,
    pub cp_offset: usize,
}

/// Whether a codepoint occupies two cells.
///
/// **Per-char `unicode-width`, the same crate and version `justerm-core` uses** — and the same
/// known defect, deliberately (ADR-0028): a VS16 or ZWJ sequence measures its parts, so an emoji
/// presentation selector reads width-1 here just as it does in the engine (#295/#297/#300). Both
/// references have it too, per codepoint (ghostty) or per char (alacritty). A preedit is re-drawn
/// on every keystroke and dies at commit, so a one-cell error self-corrects; a third answer at
/// this one site would be the divergence.
pub fn is_wide(cp: u32) -> bool {
    char::from_u32(cp)
        .and_then(UnicodeWidthChar::width)
        .is_some_and(|w| w >= 2)
}

/// The cells `run` occupies when it starts at `start`, with `max` the last usable column
/// (inclusive, i.e. `cols - 1`).
/// Two rules, in this order: **keep the tail** (the head is what the user finished typing, the tail
/// is what they are typing now), then **shift left** rather than clip, so a run that reaches the
/// right edge stays whole.
///
/// ⚠ **This corrects ghostty's loop rather than porting it.** Its own comment states the intent —
/// *"we need to adjust our codepoint start to a point where our width would be less than the number
/// of cells we have"* — but the loop breaks with the index of the codepoint that **overflowed**
/// (`State.zig` @ `e6e26e1`, `break :width .{ w, reverse_i }`), so the kept slice still includes it
/// and `w` still exceeds the room. A run wider than the screen therefore stays wider than the screen
/// and the shift saturates at column 0. Both of ghostty's own tests pass either way — neither
/// exercises a run that overruns — which is why the divergence is pinned here by a third test.
/// **Derived by walking the source, not executed**: no ghostty binary was run.
pub fn range(run: &[Codepoint], start: u32, max: u32) -> Range {
    let cells = |c: &Codepoint| if c.wide { 2 } else { 1 };
    // The room is the whole row, not just what is right of `start` — shifting left is exactly what
    // buys back the columns before it.
    let available = max.saturating_add(1);

    // Largest suffix that fits. `cp_offset == run.len()` means not even the last codepoint does,
    // which leaves an empty span rather than half a glyph.
    let mut w: u32 = 0;
    let mut cp_offset = run.len();
    for i in (0..run.len()).rev() {
        let c = cells(&run[i]);
        if w + c > available {
            break;
        }
        w += c;
        cp_offset = i;
    }

    let end = if w > 0 { start + (w - 1) } else { start };
    let shift = end.saturating_sub(max);
    Range {
        start: start.saturating_sub(shift),
        end: end.saturating_sub(shift),
        cp_offset,
    }
}

/// Where the caret belongs while `run` is composing at `start`, with `max` the last usable column.
///
/// One past the run — except at the right edge, where there IS no cell past it. Clamping to `max`
/// there would drop the caret onto the run's own last cell, and for a wide tail that cell is the
/// **spacer**: a block caret spans one column there (`is_wide_lead` is false on a spacer), so it
/// inverts the right half of the glyph the user is composing. Measured before fixing: on a
/// 106-column grid, asking for column 104 *or* 105 returned caret 105, the spacer of the run's own
/// last pair.
///
/// So at the edge the caret goes to the last glyph's **lead** instead. It is inside the run, which
/// D5's "one past the end" does not describe — but a caret covering a whole composed glyph is the
/// nearest thing to the rule that a full row can express, and it is the only option that never
/// shows half a glyph. ghostty avoids the question by drawing no caret at all during a preedit.
pub fn caret_col(run: &[Codepoint], start: u32, max: u32) -> u32 {
    if run.is_empty() {
        return start.min(max);
    }
    let r = range(run, start, max);
    if r.end < max {
        return r.end + 1;
    }
    // No cell past the run. Step back onto the last glyph's lead, so a block caret covers the pair.
    let tail_is_wide = run.last().is_some_and(|c| c.wide);
    if tail_is_wide {
        r.end.saturating_sub(1)
    } else {
        r.end
    }
}

/// The cells an open composition covers, as the packer needs to see them: one inclusive span on one
/// row (a preedit never wraps — no reference wraps one, and `range` shifts rather than spilling).
///
/// This exists because the pass is **not a layer** (ADR-0019's amendment): every stage after glyph
/// resolution — the selection / match / active-match composite, `selectionForeground`, both
/// decoration layers — is keyed on `(row, col)` and would otherwise paint straight over the
/// composed cells. Measured before this existed: a selection covering the run raised every composed
/// cell's mean channel value by ~90, i.e. the tint was applied to the text the user was typing,
/// which is exactly what ADR-0028 D2 says the pass prevents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub row: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Whether `(row, col)` is inside the composition.
    pub fn covers(&self, row: u32, col: u32) -> bool {
        row == self.row && col >= self.start && col <= self.end
    }
}

/// One cell the pass takes over: a flat grid index, the codepoint to draw there, and the flags that
/// describe it. `cp == 0` marks a wide glyph's trailing half, which draws no codepoint of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellWrite {
    pub idx: usize,
    pub cp: u32,
    pub flags: u16,
    pub kind: WriteKind,
}

/// Whether a write is the composition's own cell or the repair it owes a pair it cut in half.
///
/// The two are not interchangeable and the difference is *whose* cell it is: a [`Run`](Self::Run)
/// cell belongs to the composition, so ADR-0028 D2 has the pass re-supply its colours; a
/// [`Repair`](Self::Repair) cell is still the **application's**, outside the composition, and the
/// pass touches it only because ADR-0025 forbids leaving half a glyph behind. [`Span`] excludes the
/// repairs for exactly this reason, and until #715 the patch did not, so one half of the pass
/// treated a repair as composed and the other did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    /// A cell of the composition itself.
    Run,
    /// The other half of a pair the run cut, blanked so it stops drawing half a glyph.
    Repair,
}

/// The cells the preedit takes over, given the cursor it is anchored to.
///
/// Returns nothing when the run is empty or the cursor's row is outside the grid — ghostty's rule
/// (*"If the cursor isn't visible in the viewport we don't show it"*, `generic.zig` @ `e6e26e1`).
/// The caller owns the copy-on-write: it applies these writes to its own columns, forcing the
/// default background and foreground for the same indices, because the pass **replaces** the cell
/// rather than layering over it (ADR-0028 D2 — otherwise a selection tint underneath reads as
/// *selected text*).
///
/// Every covered cell carries [`UNDERLINE`](crate::attrs::UNDERLINE), including a wide glyph's
/// trailing half: both grid-drawing references underline the whole run, and ghostty underlines the
/// pair's second cell explicitly (`addPreeditCell`).
pub fn writes(
    run: &[Codepoint],
    cursor_col: u32,
    cursor_row: u32,
    cols: u32,
    rows: u32,
    grid_flags: &[u16],
) -> Vec<CellWrite> {
    if run.is_empty() || cols == 0 || cursor_row >= rows {
        return Vec::new();
    }
    let r = range(run, cursor_col, cols - 1);
    let mut out = Vec::new();
    let mut col = r.start;
    for c in &run[r.cp_offset.min(run.len())..] {
        if col >= cols {
            break; // the shift could not buy enough room; draw what fits, never half a pair
        }
        let lead = if c.wide { crate::attrs::WIDE_CHAR } else { 0 };
        let idx = (cursor_row as usize) * (cols as usize) + col as usize;
        out.push(CellWrite {
            idx,
            cp: c.cp,
            flags: lead | crate::attrs::UNDERLINE,
            kind: WriteKind::Run,
        });
        if c.wide {
            if col + 1 >= cols {
                out.pop(); // no room for the trailing half — the pair is indivisible
                break;
            }
            out.push(CellWrite {
                idx: idx + 1,
                cp: 0,
                flags: crate::attrs::WIDE_CHAR_SPACER | crate::attrs::UNDERLINE,
                kind: WriteKind::Run,
            });
        }
        col += if c.wide { 2 } else { 1 };
    }

    // Repair the pairs the run cut in half. A wide glyph's two cells are one fact with one owner
    // (ADR-0025), so a write that lands on one half owes the other: the resolver keeps a lead's
    // `WIDE_CHAR` and drops its right half when the spacer is gone, and says so in its own comment —
    // *"a lead drawn as its left half only"*. That is a declared legal state there (core's resize
    // truncates through pairs), but a preedit has no business creating it, and the right-edge shift
    // puts the run on arbitrary columns so it is reachable in ordinary CJK typing.
    let row_base = (cursor_row as usize) * (cols as usize);
    let flag_at = |c: u32| grid_flags.get(row_base + c as usize).copied().unwrap_or(0);
    // A repair keeps the cell's own SGR and loses only its claim to be half of a pair — the pair is
    // what the run destroyed, the rest is the application's (#715). Both bits, not just the one this
    // side carries: a `WIDE_CHAR_SPACER` left in place would draw the right half of whatever the
    // preedit put in the cell before it, which is the same artefact one column over.
    let unpaired = |flags: u16| flags & !(crate::attrs::WIDE_CHAR | crate::attrs::WIDE_CHAR_SPACER);
    if let (Some(first), Some(last)) = (out.first().map(|w| w.idx), out.last().map(|w| w.idx)) {
        let first_col = (first - row_base) as u32;
        let last_col = (last - row_base) as u32;
        // The run starts on a spacer: its lead sits outside the run and would keep half a glyph.
        if crate::attrs::is_wide_spacer(flag_at(first_col)) && first_col > 0 {
            out.push(CellWrite {
                idx: first - 1,
                cp: u32::from(b' '),
                flags: unpaired(flag_at(first_col - 1)),
                kind: WriteKind::Repair,
            });
        }
        // The run ends on a lead: its spacer sits outside the run and would draw a stale right half.
        if crate::attrs::is_wide_lead(flag_at(last_col)) && last_col + 1 < cols {
            out.push(CellWrite {
                idx: last + 1,
                cp: u32::from(b' '),
                flags: unpaired(flag_at(last_col + 1)),
                kind: WriteKind::Repair,
            });
        }
    }
    out
}

/// The per-cell columns a composition rewrites, owned — the copy-on-write the pass makes before
/// anything resolves the frame.
///
/// Five of the frame's six columns are here. The sixth, `underline_colors`, is answered in the
/// packer instead (ADR-0028 D2, #711): `0` there already means *follow the fg the pass supplied*,
/// so both halves write the same declaration and the split is a gate artifact rather than a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub codepoints: Vec<u32>,
    pub flags: Vec<u16>,
    pub clusters: Vec<String>,
    pub bg: Vec<u32>,
    pub fg: Vec<u32>,
}

/// Apply an open composition to the frame's columns, or `None` when nothing is composing and when
/// the anchor is off-grid (ghostty draws nothing rather than guessing).
///
/// **This is pure on purpose, and the reason is a gate rather than taste.** `webgl.rs` is
/// `#[cfg(target_arch = "wasm32")]` and 0-compiles on host, so a rule written there is unreachable
/// by `cargo test --manifest-path justerm-renderer/Cargo.toml`, where every test of this behaviour
/// lives — the same axis #711's fix was decided on, which had a pure home already (the packer) and
/// so did not need this move. #715 has none: the patch is where the application's background is
/// overwritten, so by the time the packer runs the value it would have to restore is gone.
///
/// The two kinds of write are not treated alike, which is the whole of #715 — see [`WriteKind`].
pub fn patch(
    run: &[Codepoint],
    cursor_col: u32,
    cursor_row: u32,
    cells: &crate::glyph_resolve::Cells<'_>,
    bg: &[u32],
    fg: &[u32],
) -> Option<Patch> {
    if run.is_empty() {
        return None;
    }
    let w = writes(
        run,
        cursor_col,
        cursor_row,
        cells.cols,
        cells.rows,
        cells.flags,
    );
    if w.is_empty() {
        return None; // off-grid anchor, or no room — ghostty draws nothing rather than guessing
    }
    let mut p = Patch {
        codepoints: cells.codepoints.to_vec(),
        flags: cells.flags.to_vec(),
        clusters: cells.clusters.to_vec(),
        bg: bg.to_vec(),
        fg: fg.to_vec(),
    };
    for cw in w {
        if let Some(slot) = p.codepoints.get_mut(cw.idx) {
            *slot = cw.cp;
        }
        if let Some(slot) = p.flags.get_mut(cw.idx) {
            *slot = cw.flags; // REPLACE, not or-in: the cell's own SGR is not the preedit's
        }
        // …but only for a cell the composition actually took. A repair is the application's cell
        // with half a glyph removed, so re-supplying its colours would erase a background nobody
        // asked the pass to touch (#715). `Span` already draws this line — the packer's stand-downs
        // apply to the run and never to a repair — and this is the other half of that same line.
        if cw.kind == WriteKind::Run {
            if let Some(slot) = p.bg.get_mut(cw.idx) {
                *slot = 0;
            }
            if let Some(slot) = p.fg.get_mut(cw.idx) {
                *slot = 0;
            }
        }
        // A grapheme override belonging to the cell underneath would otherwise be rasterised in
        // place of the preedit's codepoint — `resolve_frame` prefers a non-empty cluster.
        if let Some(slot) = p.clusters.get_mut(cw.idx) {
            slot.clear();
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GA: u32 = 0xAC00; // U+AC00 HANGUL SYLLABLE GA — ghostty's own fixture

    fn narrow(cp: u32) -> Codepoint {
        Codepoint { cp, wide: false }
    }
    fn wide(cp: u32) -> Codepoint {
        Codepoint { cp, wide: true }
    }

    // --- ported verbatim from ghostty `State.zig` "preedit range covers exact cell width" ---

    #[test]
    fn narrow_run_covers_one_cell() {
        assert_eq!(
            range(&[narrow(b'a' as u32)], 2, 9),
            Range {
                start: 2,
                end: 2,
                cp_offset: 0
            }
        );
    }

    #[test]
    fn wide_run_covers_two_cells() {
        assert_eq!(
            range(&[wide(GA)], 2, 9),
            Range {
                start: 2,
                end: 3,
                cp_offset: 0
            }
        );
    }

    // --- ported from ghostty "preedit range shifts left at right edge" ---

    #[test]
    fn wide_run_shifts_left_at_the_right_edge() {
        assert_eq!(
            range(&[wide(GA)], 9, 9),
            Range {
                start: 8,
                end: 9,
                cp_offset: 0
            }
        );
    }

    // --- justerm's own, for states ghostty's two tests do not reach ---

    #[test]
    fn an_empty_run_occupies_nothing_at_its_start() {
        assert_eq!(
            range(&[], 4, 9),
            Range {
                start: 4,
                end: 4,
                cp_offset: 0
            }
        );
    }

    #[test]
    fn a_run_longer_than_the_screen_drops_leading_codepoints() {
        // 6 wide codepoints = 12 cells into a 10-column screen (max = 9): the tail is what the
        // user is still typing, so the head is what goes.
        let run: Vec<Codepoint> = (0..6).map(|i| wide(GA + i)).collect();
        let r = range(&run, 0, 9);
        assert!(
            r.cp_offset > 0,
            "expected leading codepoints to be dropped, got {r:?}"
        );
        assert!(
            r.end <= 9,
            "the run must not overrun the last column: {r:?}"
        );
        assert_eq!(
            r.start, 0,
            "a dropped head means the run starts at the left edge: {r:?}"
        );
    }

    #[test]
    fn a_run_that_exactly_fills_the_row_keeps_every_codepoint() {
        // The boundary the drop rule turns on. Without this, `w + c > available` and the
        // off-by-one `>=` are indistinguishable — the over-long run above passes under both.
        let run: Vec<Codepoint> = (0..5).map(|i| wide(GA + i)).collect(); // 10 cells
        assert_eq!(
            range(&run, 0, 9),
            Range {
                start: 0,
                end: 9,
                cp_offset: 0
            }
        );
    }

    #[test]
    fn a_wide_codepoint_with_one_cell_left_shifts_rather_than_splitting() {
        // Starting at the last column, a two-cell glyph cannot render half of itself.
        let r = range(&[wide(GA)], 9, 9);
        assert_eq!(r.end - r.start, 1, "the pair stays a pair: {r:?}");
    }

    // --- writes(): which cells the pass takes over ---

    use crate::attrs::{UNDERLINE, WIDE_CHAR, WIDE_CHAR_SPACER};

    #[test]
    fn a_narrow_run_takes_one_cell_per_codepoint_and_underlines_it() {
        let w = writes(
            &[narrow(b'a' as u32), narrow(b'b' as u32)],
            2,
            1,
            10,
            5,
            &[],
        );
        assert_eq!(
            w,
            vec![
                CellWrite {
                    idx: 12,
                    cp: b'a' as u32,
                    flags: UNDERLINE,
                    kind: WriteKind::Run
                },
                CellWrite {
                    idx: 13,
                    cp: b'b' as u32,
                    flags: UNDERLINE,
                    kind: WriteKind::Run
                },
            ]
        );
    }

    #[test]
    fn a_wide_codepoint_writes_a_lead_and_a_spacer_both_underlined() {
        let w = writes(&[wide(GA)], 2, 0, 10, 5, &[]);
        assert_eq!(
            w,
            vec![
                CellWrite {
                    idx: 2,
                    cp: GA,
                    flags: WIDE_CHAR | UNDERLINE,
                    kind: WriteKind::Run
                },
                CellWrite {
                    idx: 3,
                    cp: 0,
                    flags: WIDE_CHAR_SPACER | UNDERLINE,
                    kind: WriteKind::Run
                },
            ],
            "the trailing half draws no codepoint of its own but is still part of the run"
        );
    }

    #[test]
    fn a_run_at_the_right_edge_lands_on_the_shifted_cells() {
        // range() shifts a wide run left; the writes must follow it rather than the raw cursor.
        let w = writes(&[wide(GA)], 9, 0, 10, 5, &[]);
        assert_eq!(w.first().map(|c| c.idx), Some(8));
        assert_eq!(w.last().map(|c| c.idx), Some(9));
    }

    #[test]
    fn nothing_is_written_when_the_cursor_row_is_off_the_grid() {
        // ghostty's rule: a preedit whose row is not in the viewport is not drawn at all.
        assert!(writes(&[wide(GA)], 2, 5, 10, 5, &[]).is_empty());
        assert!(writes(&[wide(GA)], 2, 99, 10, 5, &[]).is_empty());
    }

    #[test]
    fn nothing_is_written_for_an_empty_run_or_a_zero_width_grid() {
        assert!(writes(&[], 2, 0, 10, 5, &[]).is_empty());
        assert!(
            writes(&[wide(GA)], 0, 0, 0, 5, &[]).is_empty(),
            "must not index a zero-column grid"
        );
    }

    #[test]
    fn a_pair_is_never_split_across_the_last_column() {
        // A one-column grid cannot hold a wide glyph; half a pair would render half a glyph.
        assert!(writes(&[wide(GA)], 0, 0, 1, 5, &[]).is_empty());
    }

    // --- the pairs the run cuts in half (ADR-0025: one pair, one owner) ---

    #[test]
    fn a_run_starting_on_a_spacer_blanks_the_lead_it_orphans() {
        // grid: [wide lead, spacer, ...] and the run lands on the spacer at col 1.
        let flags = [WIDE_CHAR, WIDE_CHAR_SPACER, 0, 0, 0, 0, 0, 0, 0, 0];
        let w = writes(&[narrow(b'a' as u32)], 1, 0, 10, 5, &flags);
        assert!(
            w.iter().any(|c| c.idx == 0
                && c.cp == u32::from(b' ')
                && c.flags & (WIDE_CHAR | WIDE_CHAR_SPACER) == 0),
            "the orphaned lead must be blanked, else it draws its left half only: {w:?}"
        );
    }

    #[test]
    fn a_run_ending_on_a_lead_blanks_the_spacer_it_orphans() {
        // grid: [..., wide lead at 2, spacer at 3] and the run's last cell is that lead.
        let flags = [0, 0, WIDE_CHAR, WIDE_CHAR_SPACER, 0, 0, 0, 0, 0, 0];
        let w = writes(
            &[narrow(b'a' as u32), narrow(b'b' as u32)],
            1,
            0,
            10,
            5,
            &flags,
        );
        assert!(
            w.iter().any(|c| c.idx == 3
                && c.cp == u32::from(b' ')
                && c.flags & (WIDE_CHAR | WIDE_CHAR_SPACER) == 0),
            "the orphaned spacer must be blanked, else it draws a stale right half: {w:?}"
        );
    }

    #[test]
    fn a_run_that_breaks_no_pair_writes_only_its_own_cells() {
        let flags = [0u16; 10];
        let w = writes(&[narrow(b'a' as u32)], 1, 0, 10, 5, &flags);
        assert_eq!(w.len(), 1, "no repair where nothing was broken: {w:?}");
    }

    // --- what a repair may touch: the glyph, never the pen (#715, ADR-0028 D2) ---
    //
    // The run's own cells leave the composition stack and come back with bg, fg and glyph together
    // (D2). A repair cell never entered it — `Span` says so, and it is the application's cell, on
    // screen, outside the composition — so the pass owes it exactly one thing: stop drawing half a
    // glyph. Everything else there was declared by the application and stays.

    /// The `#715` fixture as measured in the browser: every cell blue, a wide pair at columns 2-3
    /// with the lead carrying an SGR underline of its own, and a one-cell run anchored on the
    /// pair's **spacer** — which orphans the lead at column 2.
    fn orphaning_fixture() -> (Vec<u32>, Vec<u16>, Vec<String>, Vec<u32>, Vec<u32>) {
        const N: usize = 6;
        let codepoints = vec![u32::from(b'x'); N];
        let mut flags = vec![0u16; N];
        flags[2] = WIDE_CHAR | UNDERLINE;
        flags[3] = WIDE_CHAR_SPACER;
        let mut clusters = vec![String::new(); N];
        clusters[2] = "가".to_string();
        (codepoints, flags, clusters, vec![BLUE; N], vec![RED; N])
    }

    const BLUE: u32 = 0x0000_00FF;
    const RED: u32 = 0x00FF_0000;

    #[test]
    fn a_repair_keeps_the_background_and_foreground_the_application_painted() {
        let (codepoints, flags, clusters, bg, fg) = orphaning_fixture();
        let cells = crate::glyph_resolve::Cells {
            cols: 6,
            rows: 1,
            codepoints: &codepoints,
            flags: &flags,
            clusters: &clusters,
        };
        let p = patch(&[narrow(b'a' as u32)], 3, 0, &cells, &bg, &fg).expect("the run is on grid");

        assert_eq!(
            p.bg[2], BLUE,
            "the orphaned lead is not the composition's cell"
        );
        assert_eq!(p.fg[2], RED, "…and neither is its foreground");
        // Side conditions, so a patch that simply stopped writing anything cannot pass this:
        assert_eq!(p.bg[3], 0, "the RUN's cell is still re-supplied (D2)");
        assert_eq!(p.fg[3], 0, "…in both channels");
        assert_eq!(p.bg[5], BLUE, "an untouched cell is untouched");
    }

    #[test]
    fn a_repair_blanks_the_glyph_channel_and_only_that() {
        let (codepoints, flags, clusters, bg, fg) = orphaning_fixture();
        let cells = crate::glyph_resolve::Cells {
            cols: 6,
            rows: 1,
            codepoints: &codepoints,
            flags: &flags,
            clusters: &clusters,
        };
        let p = patch(&[narrow(b'a' as u32)], 3, 0, &cells, &bg, &fg).expect("the run is on grid");

        assert_eq!(p.codepoints[2], u32::from(b' '), "no half glyph is drawn");
        assert!(
            p.clusters[2].is_empty(),
            "a cluster override would redraw the very glyph the repair just blanked"
        );
        assert_eq!(
            p.flags[2] & (WIDE_CHAR | WIDE_CHAR_SPACER),
            0,
            "the pair is gone, so the cell may no longer claim to be half of one"
        );
        assert_eq!(
            p.flags[2] & UNDERLINE,
            UNDERLINE,
            "the application's own SGR is not the composition's to erase"
        );
    }

    #[test]
    fn the_spacer_a_run_orphans_is_repaired_the_same_way() {
        // The mirror end: the run's last cell is a wide LEAD, so its spacer at column 3 is orphaned.
        let (codepoints, flags, clusters, bg, fg) = orphaning_fixture();
        let cells = crate::glyph_resolve::Cells {
            cols: 6,
            rows: 1,
            codepoints: &codepoints,
            flags: &flags,
            clusters: &clusters,
        };
        let p = patch(
            &[narrow(b'a' as u32), narrow(b'b' as u32)],
            1,
            0,
            &cells,
            &bg,
            &fg,
        )
        .expect("the run is on grid");

        assert_eq!(
            p.bg[3], BLUE,
            "the orphaned spacer keeps the application's bg"
        );
        assert_eq!(p.codepoints[3], u32::from(b' '), "and draws no stale half");
        assert_eq!(p.flags[3] & WIDE_CHAR_SPACER, 0, "it is no longer a spacer");
        assert_eq!(p.bg[2], 0, "the cell the run took IS re-supplied");
    }

    #[test]
    fn a_run_cell_that_is_itself_a_space_is_still_the_compositions_cell() {
        // The codepoint cannot be the discriminator, and a plausible fix would have made it one: a
        // repair writes a space, and so does a user who types one mid-composition. Only *whose
        // cell it is* separates the two, which is why `WriteKind` exists rather than a `cp == ' '`
        // test at the point of use.
        let (codepoints, flags, clusters, bg, fg) = orphaning_fixture();
        let cells = crate::glyph_resolve::Cells {
            cols: 6,
            rows: 1,
            codepoints: &codepoints,
            flags: &flags,
            clusters: &clusters,
        };
        let p =
            patch(&[narrow(u32::from(b' '))], 3, 0, &cells, &bg, &fg).expect("the run is on grid");

        assert_eq!(p.bg[3], 0, "the run took this cell, space or not");
        assert_eq!(p.bg[2], BLUE, "…and did not take the one it only repaired");
    }

    #[test]
    fn a_composition_that_breaks_no_pair_leaves_every_other_cell_alone() {
        // The `None`-shaped control: without a repair, a patch touches exactly the run's own cells.
        let codepoints = vec![u32::from(b'x'); 6];
        let flags = vec![0u16; 6];
        let clusters = vec![String::new(); 6];
        let (bg, fg) = (vec![BLUE; 6], vec![RED; 6]);
        let cells = crate::glyph_resolve::Cells {
            cols: 6,
            rows: 1,
            codepoints: &codepoints,
            flags: &flags,
            clusters: &clusters,
        };
        let p = patch(&[narrow(b'a' as u32)], 3, 0, &cells, &bg, &fg).expect("the run is on grid");

        assert_eq!(p.bg, vec![BLUE, BLUE, BLUE, 0, BLUE, BLUE]);
        assert!(
            patch(&[], 3, 0, &cells, &bg, &fg).is_none(),
            "nothing composing"
        );
    }

    #[test]
    fn a_repair_never_reaches_off_the_row() {
        // A spacer in column 0 has no lead to its left on this row — and the cell before it belongs
        // to the previous row, which this run must not touch.
        let mut flags = [0u16; 10];
        flags[0] = WIDE_CHAR_SPACER;
        let w = writes(&[narrow(b'a' as u32)], 0, 1, 10, 5, &flags);
        assert!(
            w.iter().all(|c| c.idx >= 10),
            "stayed on its own row: {w:?}"
        );
    }

    // --- caret_col(): D5's position rule, including the edge D5's phrasing does not cover ---

    #[test]
    fn the_caret_sits_one_past_the_run() {
        assert_eq!(caret_col(&[wide(GA)], 2, 9), 4);
        assert_eq!(caret_col(&[narrow(b'a' as u32)], 2, 9), 3);
        assert_eq!(caret_col(&[wide(GA), wide(GA)], 0, 9), 4);
    }

    #[test]
    fn at_the_right_edge_the_caret_takes_the_last_glyph_s_lead_not_its_spacer() {
        // Measured on the real renderer before this existed: a 106-column grid returned 105 — the
        // spacer of the run's own last pair — for both column 104 and column 105.
        assert_eq!(
            caret_col(&[wide(GA)], 9, 9),
            8,
            "the pair occupies 8..9; 9 is its spacer"
        );
        assert_eq!(caret_col(&[wide(GA)], 8, 9), 8);
        // A narrow tail has no half to bisect, so it keeps the cell it ends on.
        assert_eq!(caret_col(&[narrow(b'a' as u32)], 9, 9), 9);
    }

    #[test]
    fn an_empty_run_leaves_the_caret_at_the_anchor() {
        assert_eq!(caret_col(&[], 4, 9), 4);
        assert_eq!(caret_col(&[], 99, 9), 9, "and never off the grid");
    }

    // --- is_wide: the width contract, including the defect it inherits on purpose ---

    #[test]
    fn hangul_and_kanji_are_wide_ascii_is_not() {
        assert!(is_wide(GA));
        assert!(is_wide(0x6F22)); // 漢
        assert!(!is_wide(b'a' as u32));
        assert!(!is_wide(0x00E9)); // é
    }

    #[test]
    fn a_variation_selector_reads_narrow_and_that_is_the_engine_s_answer_too() {
        // U+FE0F alone is width-0/1 per char; the emoji it promotes stays width-1 here. Pinned so
        // the inherited defect is a recorded contract (ADR-0028) rather than a surprise.
        assert!(!is_wide(0xFE0F));
        assert!(!is_wide(0x2764)); // ❤ — wide only WITH the selector, which per-char cannot see
    }

    #[test]
    fn an_unpaired_surrogate_or_out_of_range_scalar_is_narrow() {
        assert!(!is_wide(0xD800)); // not a scalar value
        assert!(!is_wide(0x11_0000)); // past the last codepoint
    }
}
