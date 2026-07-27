//! #534 — the wide-wrap artefact marker has a lifetime, not just a birth.
//!
//! The marker records a **claim** about one column: *"the last column of this soft-wrapped row is
//! the blank a width-2 glyph vacated when it could not fit — skip it in text."* Two things have to
//! be true for that claim to hold, and both are ordinary VT commands away from going false:
//!
//! 1. **the row still soft-wraps** — a row-shift verb or any wrap-ending erase falsifies it;
//! 2. **its continuation still begins with a wide lead** — an overwrite, an erase, or an
//!    intra-row shift at column 0/1 of the next row falsifies it.
//!
//! Before this change nothing in the crate cleared the marker: `set_leading_spacer` had no
//! counterpart on any write path, so once the claim went false the column was dropped from every
//! text reader for good, and two visually separate runs merged in copy, search and accessible
//! text.
//!
//! The rule at every clear site is one sentence: **the record survives only an in-place same-width
//! overwrite.** A narrow write, an erase or a shift at columns 0/1 all end the pair the record was
//! *about*; a wide lead arriving later by some other route did not wrap from anywhere. Every site
//! therefore asks **before** its mutation — see
//! `insert_mode_does_not_clear_the_marker_it_is_in_the_middle_of_setting` for why the intuitive
//! post-mutation form is not merely equivalent-but-uglier.
//!
//! **References.** Both hold the print-path half, reaching *back* to the previous row, and both
//! gate on the state before the write:
//!
//! - alacritty `term/mod.rs:994` @ `852e971` — inside `write_at_cursor`'s
//!   `WIDE_CHAR | WIDE_CHAR_SPACER` block: `if point.column <= 1 && point.line !=
//!   self.topmost_line()` then remove `LEADING_WIDE_CHAR_SPACER` from `[line-1][last_column]`
//!   (`:1004-1008`). `topmost_line()` is `Line(-history_size)`, so this reaches into scrollback —
//!   which is why `the_row_above_may_be_the_last_scrollback_row` is reference-supported, not a
//!   justerm invention.
//! - ghostty `terminal/Terminal.zig:1484` @ `e6e26e1` — the whole wide-repair `switch` is gated on
//!   `cell.wide != wide`, so overwriting a wide glyph with **another** wide glyph deliberately
//!   repairs nothing; the reach-back then sits in the `.wide` (`:1501-1506`) and `.spacer_tail`
//!   (`:1529-1532`) arms. `keeps_the_marker_when_another_wide_glyph_takes_its_place` is that side
//!   condition; alacritty clears unconditionally there and is the outlier.
//!
//! The erase and `DCH` sites are **ported too** — ghostty's `Screen.splitCellBoundary`
//! (`Screen.zig:1831`, up-a-row branch at `:1873`) is called from `deleteChars`
//! (`Terminal.zig:3107-3109`) and `eraseChars` (`:3159-3160`). Only the `ICH` site has no
//! counterpart. The row-shift seams are the derived part: ghostty's `rowWillBeShifted` (`:2589`)
//! clears the marker on **every** shifted row, which it needs because it splits interior pairs and
//! supports left/right margins; justerm rotates whole `Row`s and has no DECSLRM, so it clears at
//! the seams — exactly the #540 derivation, and with the same validity condition.

use justerm_core::{Engine, SelectionType, Side};

/// `"abc한"` on a 4-column screen: `한` cannot fit in the last column, so it wraps and leaves the
/// artefact at `(0,3)`.
fn wrapped_wide(cols: usize, rows: usize) -> Engine {
    let mut t = Engine::new(cols, rows);
    t.feed("abc한".as_bytes());
    assert!(
        t.grid().cell(0, cols - 1).is_leading_spacer(),
        "fixture: artefact set"
    );
    assert_eq!(t.accessible_text().trim_end(), "abc한", "fixture");
    t
}

// ---- 1. the continuation's wide lead is destroyed --------------------------------------------

#[test]
fn overwriting_the_wrapped_glyph_clears_the_marker_above() {
    let mut t = wrapped_wide(4, 4);

    t.feed(b"\x1b[2;1Hx"); // overwrite 한 with a narrow char

    assert!(
        !t.grid().cell(0, 3).is_leading_spacer(),
        "no wide glyph wrapped from here any more"
    );
    assert_eq!(
        t.accessible_text().trim_end(),
        "abc x",
        "the column stopped being an artefact, so it is an ordinary blank again"
    );
}

#[test]
fn overwriting_the_spacer_half_clears_the_marker_above() {
    // Writing at column 1 destroys the pair from the other end: the spacer is overwritten and the
    // existing no-orphan repair frees the lead at column 0.
    let mut t = wrapped_wide(4, 4);

    t.feed(b"\x1b[2;2Hx");
    t.feed(b"\x1b[2;1Hy"); // fill the freed lead so the defect is visible in the text

    assert!(!t.grid().cell(0, 3).is_leading_spacer());
    assert_eq!(t.accessible_text().trim_end(), "abc yx");
}

#[test]
fn keeps_the_marker_when_another_wide_glyph_takes_its_place() {
    // The side condition that separates this rule from alacritty's unconditional clear: a wide
    // glyph still wrapped from that column, so the blank is still an artefact.
    let mut t = wrapped_wide(4, 4);

    t.feed("\x1b[2;1H日".as_bytes());

    assert!(
        t.grid().cell(0, 3).is_leading_spacer(),
        "the claim is still true"
    );
    assert_eq!(t.accessible_text().trim_end(), "abc日");
}

#[test]
fn a_shift_that_substitutes_a_different_pair_still_voids_the_record() {
    // The case that separates "is *the* wrapped pair still here" from "is *a* wide lead standing
    // at column 0". `DCH` deletes 한 — the glyph that actually wrapped — and pulls 日 left into
    // its place; 日 was typed on the continuation row and never wrapped from anywhere. Asking
    // after the shift would see a wide lead and keep a record whose subject is deleted.
    let mut t = Engine::new(4, 3);
    t.feed("abc한日".as_bytes());
    assert!(t.grid().cell(0, 3).is_leading_spacer(), "fixture");

    t.feed(b"\x1b[2;1H\x1b[2P");

    assert!(!t.grid().cell(0, 3).is_leading_spacer());
    assert_eq!(t.accessible_text().trim_end(), "abc 日");
}

#[test]
fn insert_mode_does_not_clear_the_marker_it_is_in_the_middle_of_setting() {
    // `write_glyph`'s wide-at-boundary path is a composite: `vacate_for_wrap` SETS the marker,
    // then `wrapline()`, then — under IRM — `insert_chars` opens the gap, then the glyph lands.
    // A repair that ran *after* the shift would look at the freshly blanked column 0 and clear
    // the marker inside its own set site's critical section. This is why every call site asks
    // before its mutation, not after.
    let mut t = Engine::new(4, 3);
    t.feed(b"\x1b[4h"); // IRM on
    t.feed("abc한".as_bytes());

    assert!(
        t.grid().cell(0, 3).is_leading_spacer(),
        "the marker was just set, not falsified"
    );
    assert_eq!(t.accessible_text().trim_end(), "abc한");
    assert_eq!(t.search("abc한").len(), 1);
}

#[test]
fn insert_mode_at_column_zero_shifts_the_pair_and_voids_the_record() {
    // The mirror of the test above, and of the in-place overwrite below: IRM *inserts*, so the
    // wrapped pair moves off column 0 rather than being replaced there. Same glyph, same column,
    // opposite answer — which is the whole content of "only an in-place same-width overwrite
    // survives".
    let mut t = wrapped_wide(4, 3);
    t.feed(b"\x1b[4h");

    t.feed("\x1b[2;1H日".as_bytes());

    assert!(!t.grid().cell(0, 3).is_leading_spacer());
    assert_eq!(t.accessible_text().trim_end(), "abc 日한");
}

#[test]
fn keeps_the_marker_when_the_continuation_is_written_elsewhere() {
    // A write on the continuation row that leaves column 0 alone must not disturb the claim.
    let mut t = wrapped_wide(4, 4);

    t.feed(b"\x1b[2;4HZ");

    assert!(t.grid().cell(0, 3).is_leading_spacer());
    assert_eq!(t.accessible_text().trim_end(), "abc한 Z");
}

#[test]
fn the_row_above_may_be_the_last_scrollback_row() {
    // `top == 0` does not mean "no row above": the text readers walk `[scrollback ++ grid]` as one
    // buffer, so the marker can sit on the last scrollback row while its continuation is grid row
    // 0. The same seam #540 found for the wrap flag.
    let mut t = Engine::new(4, 2);
    t.feed("abc한".as_bytes());
    t.feed(b"\r\n"); // evict row 0 into scrollback; 한 rides up to grid row 0
    assert_eq!(t.accessible_text().trim_end(), "abc한", "fixture");

    t.feed(b"\x1b[1;1Hx");

    assert_eq!(
        t.accessible_text().trim_end(),
        "abc x",
        "the marker on the scrollback row describes a glyph that is gone"
    );
}

#[test]
fn erasing_the_wrapped_glyph_clears_the_marker_above() {
    // Neither reference repairs this one — their reach-back lives in the print path only.
    let mut t = wrapped_wide(4, 4);

    t.feed(b"\x1b[2;1H\x1b[2K"); // EL 2 destroys the wrapped glyph
    t.feed(b"\x1b[2;1Hx");

    assert_eq!(t.accessible_text().trim_end(), "abc x");
}

#[test]
fn ich_at_column_zero_clears_the_marker_above() {
    // ICH shifts the pair right, so column 0 of the continuation is a blank the app inserted — the
    // artefact column stopped describing it.
    let mut t = wrapped_wide(4, 4);

    t.feed(b"\x1b[2;1H\x1b[1@");

    assert_eq!(
        t.accessible_text().trim_end(),
        "abc  한",
        "two real blanks: the ex-artefact and the inserted one"
    );
}

#[test]
fn dch_at_column_zero_clears_the_marker_above() {
    let mut t = wrapped_wide(4, 4);

    t.feed(b"\x1b[2;1H\x1b[1P"); // deletes the lead; the no-orphan repair frees the spacer
    t.feed(b"\x1b[2;1HX");

    assert_eq!(t.accessible_text().trim_end(), "abc X");
}

// ---- 2. the marker is carried off its defining position -------------------------------------

#[test]
fn dch_does_not_strand_the_marker_mid_row() {
    // DCH moves whole cells, marker included, and a marker mid-row describes nothing (ADR-0025
    // D3). The stranded one is dropped from the text, merging two visually separate runs — the
    // exact measurement recorded in `is_walk_transparent_spacer`'s doc comment.
    let mut t = Engine::new(6, 4);
    t.feed("abcde한".as_bytes());

    t.feed(b"\x1b[1;1H\x1b[2P"); // row 0 becomes "cde" + the marker's cell + blanks
    t.feed(b"\x1b[1;5HXY");

    assert!(
        (0..6).all(|c| !t.grid().cell(0, c).is_leading_spacer()),
        "no marker anywhere on a row that no longer wraps"
    );
    assert_eq!(
        t.accessible_text().lines().next().unwrap().trim_end(),
        "cde XY"
    );
    assert_eq!(t.search("cdeXY").len(), 0, "the blank between them is real");
    assert_eq!(t.search("cde XY").len(), 1);
}

#[test]
fn ich_discards_the_marker_off_the_edge() {
    // The other half of the issue's "ICH / DCH" claim, measured: a right shift always pushes the
    // last column off the edge, so ICH cannot strand a marker. Pinned so a future change to the
    // shift does not quietly start stranding one.
    let mut t = Engine::new(6, 4);
    t.feed("abcde한".as_bytes());

    t.feed(b"\x1b[1;1H\x1b[2@");

    assert!((0..6).all(|c| !t.grid().cell(0, c).is_leading_spacer()));
    assert!(t.grid().is_row_wrapped(0), "ICH does not end the wrap");
    assert_eq!(t.accessible_text().trim_end(), "  abcd한");
}

// ---- 3. the row stops wrapping ---------------------------------------------------------------

/// `"ab한"` on a 3-column screen, then `DL` at row 1 removes the continuation. #540 already ends
/// the wrap here; what is left is the marker the shift orphaned.
fn orphaned_by_dl() -> Engine {
    let mut t = Engine::new(3, 4);
    t.feed("ab한".as_bytes());
    t.feed(b"\x1b[2;1H\x1b[M");
    assert!(!t.grid().is_row_wrapped(0), "fixture: #540 ended the wrap");
    t
}

#[test]
fn a_row_shift_drops_the_marker_it_orphans() {
    let t = orphaned_by_dl();

    assert!(
        !t.grid().cell(0, 2).is_leading_spacer(),
        "a row that does not wrap cannot hold a wrap artefact (ADR-0025 D3)"
    );
}

#[test]
fn an_orphaned_marker_does_not_widen_a_word_selection() {
    // `is_wrap_artefact` makes the marked column transparent to the word walk, so a stale marker
    // extends the highlight over a blank that is not part of the word. The control below differs
    // in exactly one thing: no wide glyph was ever involved.
    let mut t = orphaned_by_dl();
    t.selection_begin(0, 0, Side::Left, SelectionType::Word);
    let stale = t.selection_range();

    let mut control = Engine::new(3, 4);
    control.feed(b"ab");
    control.selection_begin(0, 0, Side::Left, SelectionType::Word);

    assert_eq!(
        stale.first().map(|s| s.right),
        control.selection_range().first().map(|s| s.right),
        "the word is \"ab\" in both; the wide glyph's history must not widen the highlight"
    );
}

#[test]
fn the_row_shift_seam_above_the_grid_is_a_scrollback_row() {
    // `shift_region`'s `top == 0` branch is the one wrap-ending path whose row is not a grid row,
    // so it does not go through `end_wrap` and has to couple the marker clear itself. Without it
    // the whole defect survives one row above the grid — reachable from every `DL`/`IL`/`SU`/`SD`
    // and region `RI`, since `scroll_region_lines` always passes `evicts_to_scrollback: false`.
    // Same seam #540 found for the wrap flag.
    let mut t = Engine::new(4, 2);
    t.feed("abc한".as_bytes());
    t.feed(b"\r\n"); // evict row 0 (marker + wrap) into scrollback
    t.feed(b"\x1b[1;1H\x1b[M"); // full-screen DL -> shift_region(top = 0)

    t.resize(8, 2); // reflow would bake a surviving marker in mid-row
    t.feed(b"\x1b[1;5HZ");

    assert_eq!(
        t.accessible_text().lines().next().unwrap().trim_end(),
        "abc Z"
    );
    assert_eq!(t.search("abcZ").len(), 0, "the blank between them is real");
}

#[test]
fn reflow_does_not_carry_an_orphaned_marker_inward() {
    // A marker cell is not `is_blank()` (the marker is one of the content bits), so reflow's
    // hard-line trim keeps it and the re-split lands it mid-row — where it silently swallows the
    // blank between two runs.
    let mut t = orphaned_by_dl();

    t.resize(5, 4);
    t.feed(b"\x1b[1;4HZ");

    assert_eq!(
        t.accessible_text().lines().next().unwrap().trim_end(),
        "ab Z"
    );
}
