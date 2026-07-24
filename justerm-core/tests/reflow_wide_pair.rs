//! #533 — reflow is a **producer** of the wide-wrap artefact, so it owes the artefact's marker.
//!
//! This is the reflow counterpart of `wide_wrap_vacate.rs` (#528, the *print*-path producer).
//! When a re-split would leave a row ending on a `WIDE_CHAR` lead, reflow drops the lead to the
//! next row and pads the vacated last column — but that padded cell is only an *artefact* if it
//! carries the leading-spacer marker the text extractors gate on. Without it the blank reads as a
//! real space, so a resize injects a phantom space into copy, search and the screen-reader text.
//!
//! alacritty marks the same cell at both of its equivalent sites — `grid/resize.rs:155-157` (grow)
//! and `:293-297` (shrink), the latter `mem::replace`-ing the last column with a
//! `LEADING_WIDE_CHAR_SPACER` — and ghostty sets `.wide = .spacer_head` (`PageList.zig:1767`).
//! xterm.js is the outlier: it writes a plain unmarked `nullCell` (`BufferReflow.ts:83`) and
//! re-infers the artefact heuristically each time (`:223-227`, "ends in null AND the following
//! line starts with a wide char"), which is strictly weaker than a marker. justerm follows the
//! two that mark it.
//!
//! The padding cell stays a **default** blank — reflow is a pure re-split of rows that already
//! exist and has no pen, unlike the print path, whose vacate is written with the current pen
//! (#528). That asymmetry is reproduced in all three references: each one's print path uses the
//! pen (alacritty `cursor.template`, ghostty `printCell`, xterm.js `curAttr`) while each one's
//! reflow path builds from defaults.
//!
//! **Out of scope here:** a width of one column, where a pair cannot be represented at all and
//! reflow leaves it split across rows. Both alacritty (`MIN_COLUMNS = 2`) and xterm.js
//! (`MINIMUM_COLS = 2`) forbid that width outright for exactly this reason, and ghostty destroys
//! the glyph; that is a contract decision, tracked separately, not a #533 conformance item.
//!
//! ADR-0025 D3 (position is part of a leading marker's definition) is the governing rule.

use justerm_core::Engine;

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

/// The marker is consumed on the way back, so a narrow→wide round trip is an identity. The join
/// half pops a trailing leading-spacer off a soft row, which is the symmetric operation to the
/// re-split's set — alacritty pairs the same two (`grid/resize.rs:136-142` removes on grow).
#[test]
fn a_narrow_then_widen_round_trip_restores_the_line() {
    let mut term = Engine::new(5, 4);
    term.feed("ab한cd".as_bytes());

    term.resize(3, 4);
    term.resize(5, 4);

    assert_eq!(term.accessible_text().replace('\n', ""), "ab한cd");
    assert_eq!(term.search("ab한cd").len(), 1);
    assert!(
        !term.grid().cell(0, 2).is_leading_spacer(),
        "the artefact is consumed on the join, not carried into the widened line"
    );
}
