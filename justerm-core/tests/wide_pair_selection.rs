//! #454 — a selection endpoint that lands inside a wide-glyph pair takes the whole pair.
//!
//! The pair is one thing: `selection_text` already refuses to hand back half a glyph (a spacer
//! extracts as nothing), so a range whose end falls between the two halves makes the highlight and
//! the copy describe different text. Measured before the fix, on `"漢ab"` in a 4-column grid:
//!
//! ```text
//! anchored on the spacer, dragged right   range=[(0,1,2)]   text="a"
//! anchored on the lead,   dragged right   range=[(0,0,2)]   text="漢a"
//! ```
//!
//! The first row is the everyday gesture — press on the right half of a CJK glyph and drag — and it
//! highlights `漢a` worth of cells while copying `a`.
//!
//! **Named prior art, and it splits on direction while agreeing that the two must agree.**
//! alacritty widens the *copy* the same way this does (`alacritty_terminal/src/term/mod.rs:583-585`
//! @ `852e971`, *"Include wide char when trailing spacer is selected"*, `cols.start -= 1`) and pulls
//! the lead into the paint from `Selection::contains_cell`. xterm.js instead moves a spacer endpoint
//! **right** (`SelectionService.ts:573-577`, `:667-678` @ `699f553`), excluding the glyph from both.
//! Either is coherent; what justerm cannot be is the third thing, where the two disagree. The
//! direction follows from the renderer's own rule (`justerm-renderer` `pair.rs`): a span covering
//! either half covers both, so the copy must contain what the highlight claims.
//!
//! Block selection is deliberately **not** covered here: its rectangle is one column range for every
//! row, so a per-row widening is not expressible in the resolved shape. Its highlight is made whole
//! at the paint site instead, and the copy residue is the map's existing
//! `docs/map/territory/selection.md` hole ("Block selection over wide characters is unspecified").

use justerm_core::{Engine, SelectionType, Side};

/// `(row, left, right)` triples — the shape `SelectionSpan` projects.
fn spans(e: &Engine) -> Vec<(usize, usize, usize)> {
    e.selection_range()
        .iter()
        .map(|s| (s.row, s.left, s.right))
        .collect()
}

#[test]
fn an_endpoint_anchored_on_a_spacer_takes_the_whole_glyph() {
    // cols: 0 = 漢's lead, 1 = its spacer, 2 = 'a', 3 = 'b'
    let mut e = Engine::new(4, 2);
    e.feed("漢ab".as_bytes());

    e.selection_begin(0, 1, Side::Left, SelectionType::Char);
    e.selection_extend(0, 2, Side::Right);

    assert_eq!(
        spans(&e),
        vec![(0, 0, 2)],
        "the highlight starts at the lead"
    );
    assert_eq!(
        e.selection_text().as_deref(),
        Some("漢a"),
        "and the copy contains what the highlight claims"
    );
}

#[test]
fn an_endpoint_landing_on_a_lead_takes_its_spacer() {
    // The mirror direction: cols 0 = 'a', 1 = 'b', 2 = 漢's lead, 3 = its spacer.
    let mut e = Engine::new(4, 2);
    e.feed("ab漢".as_bytes());

    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(0, 2, Side::Right);

    assert_eq!(
        spans(&e),
        vec![(0, 0, 3)],
        "the highlight reaches the spacer"
    );
    assert_eq!(e.selection_text().as_deref(), Some("ab漢"));
}

#[test]
fn a_selection_of_narrow_cells_is_not_widened() {
    // The side condition that says this reads the WIDE bits rather than growing every range: 'a'
    // alone, with the pair immediately to its left, must not pull the glyph in.
    let mut e = Engine::new(4, 2);
    e.feed("漢ab".as_bytes());

    e.selection_begin(0, 2, Side::Left, SelectionType::Char);
    e.selection_extend(0, 2, Side::Right);

    assert_eq!(spans(&e), vec![(0, 2, 2)]);
    assert_eq!(e.selection_text().as_deref(), Some("a"));
}

#[test]
fn a_lead_whose_spacer_was_truncated_away_is_not_widened_past_the_row() {
    // `Row::resize` narrows straight through a pair and leaves the lead alone in the last column —
    // ADR-0025 D4's scope records that as a legal buffer state, so the widening must have an answer
    // for it rather than reading past the row.
    let mut e = Engine::new(4, 2);
    e.feed("ab漢".as_bytes());
    e.resize(3, 2);

    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(0, 2, Side::Right);

    assert_eq!(
        spans(&e),
        vec![(0, 0, 2)],
        "the stranded lead ends the span; nothing is invented past the row"
    );
}
