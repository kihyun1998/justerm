//! #530 — a cell freed by a **structural repair** (it stopped being part of a glyph) is
//! reported as changed, and is a blank carrying the current background.
//!
//! A structural repair is not an erase. The app asked for something at *another* column;
//! freeing this one is the engine keeping its own no-orphan invariant. But it is still a
//! mutation, so it damages (ADR-0003), and — per the maintainer's decision on #530 — the blank
//! it leaves carries the pen's background, the same rule `clear_cells` already applies to BCE.
//!
//! The decision was B′ (pen's background) over B (the pen's *full* attributes, xterm.js — it can
//! create DECSCA protection on a cell the user never wrote) and C (the cell's own attributes,
//! alacritty — its `clear_wide` keeps `extra`, so a hyperlink outlives the destroyed glyph, which
//! is what #529 is filed against). See the issue for the full record.

use justerm_core::{Color, Engine, TermDamage};

/// The damaged column range for `row`, or `None` if that row is not damaged.
fn damaged(t: &Engine, row: usize) -> Option<(usize, usize)> {
    match t.damage() {
        TermDamage::Partial(lines) => lines
            .iter()
            .find(|d| d.line == row)
            .map(|d| (d.left, d.right)),
        TermDamage::Full => Some((0, usize::MAX)),
    }
}

/// Assert `col` is inside the damage recorded for `row`.
#[track_caller]
fn assert_damaged(t: &Engine, row: usize, col: usize, what: &str) {
    let (left, right) = damaged(t, row).unwrap_or_else(|| panic!("row {row} not damaged ({what})"));
    assert!(
        left <= col && col <= right,
        "{what}: column {col} changed but the damage span is {left}..={right}"
    );
}

/// A red run with a wide glyph in the middle: `ab한cd`, 한 occupying columns 2-3.
/// Returns an engine with the pen then moved to **green**, so "the cell's own colour" (red)
/// and "the pen's colour" (green) are distinguishable — with one pen the two candidate
/// answers are identical and every assertion below would pass vacuously.
fn red_run_green_pen() -> Engine {
    let mut t = Engine::new(8, 3);
    t.feed(b"\x1b[41m");
    t.feed("ab\u{D55C}cd".as_bytes());
    t.feed(b"\x1b[42m");
    t
}

// ---- the repair reports the cell it changed --------------------------------------------

#[test]
fn overwriting_a_wide_spacer_damages_the_lead_it_orphans() {
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;4H"); // onto 한's spacer
    t.reset_damage();
    t.feed(b"X");
    assert_eq!(t.grid().cell(0, 2).c(), ' ', "the lead was freed");
    assert_damaged(&t, 0, 2, "write_glyph repair of the orphaned lead");
}

#[test]
fn overwriting_a_wide_lead_damages_the_spacer_it_orphans() {
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;3H"); // onto 한's lead
    t.reset_damage();
    t.feed(b"X");
    assert!(
        !t.grid().cell(0, 3).is_wide_spacer(),
        "the spacer was freed"
    );
    assert_damaged(&t, 0, 3, "write_glyph repair of the orphaned spacer");
}

#[test]
fn erasing_from_a_wide_spacer_damages_the_lead_outside_the_range() {
    // EL starts at column 3 — the lead at column 2 is *outside* the erased range, so the
    // erase's own damage span cannot cover it.
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;4H");
    t.reset_damage();
    t.feed(b"\x1b[K");
    assert_eq!(t.grid().cell(0, 2).c(), ' ');
    assert_damaged(&t, 0, 2, "clear_cells repair below the erased range");
}

#[test]
fn erasing_up_to_a_wide_lead_damages_the_spacer_outside_the_range() {
    // ECH 3 from column 0 erases 0..=2, ending on 한's lead; its spacer at column 3 is
    // outside the range.
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;1H");
    t.reset_damage();
    t.feed(b"\x1b[3X");
    assert!(!t.grid().cell(0, 3).is_wide_spacer());
    assert_damaged(&t, 0, 3, "clear_cells repair above the erased range");
}

#[test]
fn inserting_at_a_wide_spacer_damages_the_lead_it_strands() {
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;4H");
    t.reset_damage();
    t.feed(b"\x1b[1@");
    assert_eq!(t.grid().cell(0, 2).c(), ' ');
    assert_damaged(&t, 0, 2, "insert_chars repair below the shifted range");
}

#[test]
fn deleting_at_a_wide_spacer_damages_the_lead_it_strands() {
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;4H");
    t.reset_damage();
    t.feed(b"\x1b[1P");
    assert_eq!(t.grid().cell(0, 2).c(), ' ');
    assert_damaged(&t, 0, 2, "delete_chars repair below the shifted range");
}

// ---- the freed cell carries the pen's background (#530: B′) -----------------------------

#[test]
fn a_freed_lead_carries_the_pens_background_not_the_default() {
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;4H");
    t.feed(b"X");
    assert_eq!(
        t.grid().cell(0, 2).bg(),
        Color::Indexed(2),
        "the freed cell is a blank in the CURRENT background, not an uncoloured notch"
    );
    // Right reason: it is the *pen's* colour, not the cell's own — the two differ here.
    assert_eq!(
        t.grid().cell(0, 1).bg(),
        Color::Indexed(1),
        "its untouched neighbour still has the run's red"
    );
}

#[test]
fn a_freed_spacer_carries_the_pens_background() {
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;3H");
    t.feed(b"X");
    assert_eq!(t.grid().cell(0, 3).bg(), Color::Indexed(2));
}

#[test]
fn the_vs15_demotion_frees_its_spacer_into_the_pens_background() {
    // The mode-2027 demotion (⌚ + VS15 → width 1) frees the spacer it no longer needs.
    let mut t = Engine::new(8, 3);
    t.feed(b"\x1b[?2027h\x1b[41m");
    t.feed("\u{231A}".as_bytes());
    t.feed(b"\x1b[42m"); // pen moves before the demotion
    t.feed("\u{FE0E}".as_bytes());
    assert!(!t.grid().cell(0, 0).is_wide(), "demoted to width 1");
    assert_eq!(
        t.grid().cell(0, 1).bg(),
        Color::Indexed(2),
        "the freed spacer takes the pen's background"
    );
    assert_damaged(&t, 0, 1, "demote frees its spacer");
}

#[test]
fn a_freed_cell_keeps_no_glyph_and_no_riders() {
    // B′ is the pen's *background* — not the pen's full attributes (which would plant the
    // pen's hyperlink and underline colour on a cell nobody wrote), and not the cell's own
    // attributes (which would leave the destroyed glyph's link alive — the defect #529 is
    // filed against).
    let mut t = Engine::new(8, 3);
    t.feed(b"\x1b]8;;https://example.com\x07"); // link open while the wide glyph is written
    t.feed(b"\x1b[4m\x1b[58:5:1m\x1b[41m");
    t.feed("ab\u{D55C}cd".as_bytes());
    t.feed(b"\x1b[42m"); // pen: green, link STILL open, colour still armed
    t.feed(b"\x1b[1;4H");
    t.feed(b"X");

    let freed = t.grid().cell(0, 2);
    assert_eq!(freed.c(), ' ', "no glyph");
    assert_eq!(freed.bg(), Color::Indexed(2), "the pen's background");
    assert_eq!(
        t.link_at(0, 2),
        None,
        "neither the destroyed glyph's link nor the pen's is planted on a freed cell"
    );
    assert_eq!(t.underline_color_at(0, 2), Color::Default);
    assert!(
        !freed.flags().contains(justerm_core::CellFlags::UNDERLINE),
        "and no glyph-ish attribute survives — a blank draws no underline"
    );
}

#[test]
fn the_freed_cell_reaches_a_frame_mode_consumer() {
    // The whole point of the damage half: without it the colour above is invisible on the
    // wire, and any assertion made through `frame()` passes vacuously for every candidate.
    let mut t = red_run_green_pen();
    t.feed(b"\x1b[1;4H");
    t.reset_damage();
    t.feed(b"X");

    let frame = t.frame();
    let span = frame
        .spans
        .iter()
        .find(|s| s.line == 0 && s.left as usize <= 2 && s.right as usize >= 2)
        .expect("the freed column is shipped to the consumer");
    let cell = &span.cells[2 - span.left as usize];
    assert_eq!(cell.c(), ' ');
    assert_eq!(cell.bg(), Color::Indexed(2));
}
