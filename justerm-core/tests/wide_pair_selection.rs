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
//! **Block selection is covered too, and it is the case that decides where the rule goes.** A
//! rectangle is one column range for *every* row, so the widening cannot be hoisted out of the row
//! loop the way the rectangle's own bound can — it fires on whichever rows meet a pair, leaving those
//! rows one column wider than the rectangle. That is the price of never painting half a glyph, and
//! all three references pay it: none of them exempts a rectangular selection from its pair rule
//! (alacritty's `contains_cell` applies the wide arm after its `is_block` early return; xterm's two
//! fixups sit outside the `SelectionMode.COLUMN` guard). It also closes the hole
//! `docs/map/territory/selection.md` records as *"Block selection over wide characters is
//! unspecified"* — on both observables, which is the point: a rule applied to the highlight alone
//! would rebuild this defect on the copy.

use justerm_core::{Engine, Match, SelectionType, Side};

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

#[test]
fn a_block_rectangle_widens_per_row_on_both_observables() {
    // A rectangle is one column range for every row, but it meets a pair at a different place on
    // each — so the widening cannot be hoisted out of the row loop. Two rows, the pair on opposite
    // sides:
    //   row 0  "漢ab"  -> lead 0, spacer 1, 'a' 2, 'b' 3
    //   row 1  "ab漢"  -> 'a' 0, 'b' 1, lead 2, spacer 3
    let mut e = Engine::new(4, 2);
    e.feed("漢ab".as_bytes());
    e.feed(&[0x0d, 0x0a]);
    e.feed("ab漢".as_bytes());

    // Columns 1..=2 on both rows: row 0 starts on a spacer, row 1 ends on a lead.
    e.selection_begin(0, 1, Side::Left, SelectionType::Block);
    e.selection_extend(1, 2, Side::Right);

    assert_eq!(
        spans(&e),
        vec![(0, 0, 2), (1, 1, 3)],
        "each row widens on its own side of the rectangle"
    );
    // Written as an escape, never as a literal line break: this file is checked out with CRLF, so a
    // multi-line string literal would carry a `\r` the engine never emitted — the same mangling that
    // made this fixture's `feed` wrong twice while it was being written.
    assert_eq!(
        e.selection_text().as_deref(),
        Some("漢a\nb漢"),
        "and the copy widens with it -- the two observables cannot disagree"
    );
}

#[test]
fn a_match_clamped_onto_a_trailing_spacer_covers_the_whole_glyph() {
    // #678 bounds an out-of-range match column onto the last cell (ADR-0026 D3). Where that cell is
    // a wide glyph's trailing half, the projected span used to be the right half alone: measured on
    // a 6-column row holding `abcd한`, `start_col: 99` gave `left: 5, right: 5`.
    let mut e = Engine::new(6, 2);
    e.feed("abcd한".as_bytes());

    let spans = e.match_spans(&Match {
        start_line: 0,
        start_col: 99,
        end_line: 0,
        end_col: 99,
    });

    assert_eq!(
        spans
            .iter()
            .map(|s| (s.row, s.left, s.right))
            .collect::<Vec<_>>(),
        vec![(0, 4, 5)],
        "the clamp lands on the spacer; the span takes the pair"
    );
}
