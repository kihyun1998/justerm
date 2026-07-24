//! #533 — reflow is a **producer** of the wide-wrap artefact, so it owes the artefact's marker,
//! and it must not split a wide pair across rows.
//!
//! This is the reflow counterpart of `wide_wrap_vacate.rs` (#528, the *print*-path producer).
//! When a re-split would leave a row ending on a `WIDE_CHAR` lead, reflow drops the lead to the
//! next row and pads the vacated last column — but that padded cell is only an *artefact* if it
//! carries the leading-spacer marker the text extractors gate on. Without it the blank reads as a
//! real space, so a resize injects a phantom space into copy, search and the screen-reader text.
//!
//! alacritty does exactly this at both of its equivalent sites — `grid/resize.rs:155-157` (grow)
//! and `:293-297` (shrink), the latter `mem::replace`-ing the last column with a
//! `LEADING_WIDE_CHAR_SPACER` cell and moving the wide char to the wrapped row; ghostty sets
//! `.wide = .spacer_head` in `PageList.zig:1767`.
//!
//! The padding cell stays a **default** cell (no background): reflow is a pure re-split of rows
//! that already exist and has no pen, unlike the print path, whose vacate is written with the
//! current pen (#528). All three references build this cell from defaults.
//!
//! ADR-0025 D3 (position is part of a leading marker's definition) and D4 (both halves of a pair
//! move together) are the rules; these are conformance tests under them.

use justerm_core::{Engine, SelectionType, Side};

/// The artefact reflow creates carries its marker, so the text readers skip it.
#[test]
fn reflow_marks_the_wide_wrap_artefact_it_creates() {
    let mut term = Engine::new(5, 4);
    term.feed("ab한cd".as_bytes()); // 한 is cols 2..=3, "cd" is col 4 + wrap
    assert_eq!(
        term.accessible_text().replace('\n', ""),
        "ab한cd",
        "fixture"
    );
    assert_eq!(term.search("ab한cd").len(), 1, "fixture");

    term.resize(3, 4); // 한 no longer fits at the end of row 0 → drops to row 1

    assert!(
        term.grid().cell(0, 2).is_leading_spacer(),
        "the column the wide glyph vacated is marked as the artefact, not left a bare blank"
    );
    assert_eq!(
        term.accessible_text().replace('\n', ""),
        "ab한cd",
        "no phantom space: the artefact is skipped by the text readers"
    );
    assert_eq!(
        term.search("ab한cd").len(),
        1,
        "the line is still findable by the text that is on screen"
    );
}

/// Reflowing to a single column must not leave a spacer stranded without its lead. A width-2
/// glyph cannot be represented with its spacer at one column, and the crate already decided what
/// that looks like: `write_glyph` writes the lead **alone** there (`term.rs`, the spacer is gated
/// on `col + 1 < cols`). Reflow follows the same rule rather than splitting the pair across two
/// rows, which would strand a `WIDE_CHAR_SPACER` at column 0 with a live lead on the row above.
#[test]
fn reflow_to_one_column_does_not_split_a_wide_pair() {
    let mut term = Engine::new(8, 6);
    term.feed("ab한cd".as_bytes());

    term.resize(1, 8);

    let grid = term.grid();
    for row in 0..grid.rows() {
        assert!(
            !grid.cell(row, 0).is_wide_spacer(),
            "row {row}: a lead-less spacer was stranded at column 0 — the pair was split"
        );
    }
    assert_eq!(
        term.accessible_text().replace('\n', ""),
        "ab한cd",
        "the logical line survives the degenerate width"
    );
}

/// The user-visible consequence of the split: the word walk stops at the stranded spacer, so a
/// double-click reports less than the text readers do. Asserted separately from the structural
/// check above so a regression says *which* invariant broke.
#[test]
fn a_word_survives_reflow_to_one_column() {
    let mut term = Engine::new(8, 6);
    term.feed("ab한cd".as_bytes());

    term.resize(1, 8);
    term.selection_begin(0, 0, Side::Left, SelectionType::Word);

    assert_eq!(
        term.selection_text().as_deref(),
        Some("ab한cd"),
        "selection agrees with accessible_text instead of stopping mid-glyph"
    );
}
