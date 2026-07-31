//! #678 — a match column past the last one must not delete the match's own row.
//!
//! `Term::match_spans` bounds one end of a match and not the other:
//! `right` goes through `.min(last)` while `left` takes `m.start_col` raw, so
//! `if right >= left` fails on the start row and that row is silently dropped from
//! `frame().overlay.matches` and `overlay.active_match` — a multi-row match losing only
//! its *first* row, which is why it reads as "the highlight is fine" at a glance.
//!
//! **The column is consumer-supplied by design.** `Match` has four public fields, and
//! `Engine::set_active_search_match` is documented as taking one the consumer assembled
//! *outside* the engine's own set (the past-cap path, #436) — so "the engine produced
//! these coordinates" does not hold. `set_active_search_highlight(index)` is the safe
//! sibling: it resolves through the held set and cannot introduce a new coordinate.
//!
//! **#671 is the sibling but not the same shape.** It did not touch `selection_range`,
//! whose `left` is still unbounded today; it clamped selection's *producer*
//! (`Term::viewport_to_abs`), which made that read-site asymmetry unreachable. Search has
//! no producer to clamp — the coordinate **is** the consumer's — which is why the same
//! asymmetry stayed live here and why the bound sits at the read.
//!
//! **The references split 1–1 on the guard.** alacritty clamps a column unconditionally
//! (`Point::grid_clamp`, on both endpoints before any per-type arithmetic); xterm
//! **hides**, with a commented arm for exactly this input (*"exceeded the container width,
//! so hide"*) — which is justerm's *old* outcome. The tie breaks on the old outcome being
//! neither: it dropped one row and painted the rest. Recorded with its cost in
//! `docs/agents/reference-facts.md`, including the wide-pair spacer a clamp can land on
//! and a hide could not.

use justerm_core::{Engine, Match, SelectionSpan};

/// A 4×3 grid: `abcd` / `efgh` / `ijkl`. Column 4 is the first out-of-range one.
fn grid() -> Engine {
    let mut e = Engine::new(4, 3);
    e.feed(b"abcd\r\nefgh\r\nijkl");
    e
}

/// The control the rest is read against: an in-range consumer-supplied match projects
/// to exactly its own row.
#[test]
fn an_in_range_match_projects_its_row() {
    let mut term = grid();

    term.set_search_highlights(vec![Match {
        start_line: 0,
        start_col: 0,
        end_line: 0,
        end_col: 3,
    }]);

    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 0,
            left: 0,
            right: 3
        }]
    );
}

/// The defect: the same match one column past the end disappears entirely rather than
/// resolving onto the last column.
#[test]
fn an_out_of_range_start_column_does_not_delete_the_match_row() {
    let mut term = grid();

    term.set_search_highlights(vec![Match {
        start_line: 0,
        start_col: 80,
        end_line: 0,
        end_col: 3,
    }]);

    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 0,
            left: 3,
            right: 3
        }],
        "the row must still be painted, starting at the last column"
    );
}

/// The documented build-your-own-`Match` path (#436) reaches the same projection, so it
/// needs its own pin — it is the one whose doc invites a consumer-assembled coordinate.
#[test]
fn the_active_match_intake_is_bounded_too() {
    let mut term = grid();

    term.set_active_search_match(Some(Match {
        start_line: 1,
        start_col: 99,
        end_line: 1,
        end_col: 3,
    }));

    assert_eq!(
        term.frame().overlay.active_match,
        vec![SelectionSpan {
            row: 1,
            left: 3,
            right: 3
        }]
    );
}

/// A match spanning several rows loses only its *first* row today, because continuation
/// rows start at column 0 and never touch `start_col`. The partial symptom is the one
/// that reads as "the highlight is fine" at a glance.
#[test]
fn a_multi_row_match_keeps_every_row_including_the_first() {
    let mut term = grid();

    term.set_search_highlights(vec![Match {
        start_line: 0,
        start_col: 80,
        end_line: 2,
        end_col: 3,
    }]);

    assert_eq!(
        term.frame().overlay.matches,
        vec![
            SelectionSpan {
                row: 0,
                left: 3,
                right: 3
            },
            SelectionSpan {
                row: 1,
                left: 0,
                right: 3
            },
            SelectionSpan {
                row: 2,
                left: 0,
                right: 3
            },
        ]
    );
}

/// The bound is the row's own extent, and every row is `grid.cols()` wide — so a short
/// line does not shrink it. Pinned because `right`'s existing `.min(last)` reads as a
/// *content* bound and is in fact a grid bound; the two must not drift apart.
#[test]
fn the_bound_is_the_grid_width_not_the_printed_text() {
    let mut term = Engine::new(4, 3);
    term.feed(b"ab\r\ncdef"); // row 0 holds two characters on a four-wide grid

    term.set_search_highlights(vec![Match {
        start_line: 0,
        start_col: 80,
        end_line: 0,
        end_col: 99,
    }]);

    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 0,
            left: 3,
            right: 3
        }]
    );
}

// ===========================================================================
// Controls — what must NOT change
// ===========================================================================

/// `set_active_search_highlight(index)` resolves through the held set, so it cannot
/// introduce a coordinate the set did not already carry. An out-of-range *index*
/// designates nothing, and that is unrelated to this fix.
#[test]
fn an_out_of_range_index_still_designates_nothing() {
    let mut term = grid();
    term.set_search_highlights(vec![Match {
        start_line: 0,
        start_col: 0,
        end_line: 0,
        end_col: 3,
    }]);

    term.set_active_search_highlight(Some(99));

    assert!(term.frame().overlay.active_match.is_empty());
}

/// The row axis is bounded already and stays that way: a match declaring an end line
/// past the buffer clips to the visible rows rather than being dropped, and an inverted
/// pair yields nothing. Measured before the fix and pinned unchanged, so the column
/// bound cannot be credited with — or blamed for — the row behaviour.
#[test]
fn the_row_axis_is_unchanged() {
    let mut past_end = grid();
    past_end.set_search_highlights(vec![Match {
        start_line: 0,
        start_col: 0,
        end_line: 99,
        end_col: 3,
    }]);
    assert_eq!(
        past_end.frame().overlay.matches.len(),
        3,
        "an end line past the buffer clips to the visible rows"
    );

    let mut inverted = grid();
    inverted.set_search_highlights(vec![Match {
        start_line: 99,
        start_col: 0,
        end_line: 0,
        end_col: 3,
    }]);
    assert!(
        inverted.frame().overlay.matches.is_empty(),
        "an inverted line pair yields nothing"
    );
}

/// Nothing out of range reaches the wire, before or after — the emitted span is what is
/// serialized, and `if right >= left` was already total. Pinned so the fix is not read as
/// having closed a wire hazard it never had.
#[test]
fn no_span_escapes_the_grid_through_the_wire() {
    let mut term = grid();
    term.set_search_highlights(vec![
        Match {
            start_line: 0,
            start_col: 80,
            end_line: 0,
            end_col: 3,
        },
        Match {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 99,
        },
    ]);
    term.set_active_search_match(Some(Match {
        start_line: 2,
        start_col: 77,
        end_line: 2,
        end_col: 3,
    }));

    let frame = term.frame();
    let bytes = justerm_core::encode(&frame);
    let decoded = justerm_core::decode(&bytes).expect("an out-of-range match still round-trips");

    for span in decoded
        .overlay
        .matches
        .iter()
        .chain(decoded.overlay.active_match.iter())
    {
        assert!(
            span.left < 4 && span.right < 4 && span.row < 3,
            "escaped to the wire: {span:?}"
        );
    }
}

// ===========================================================================
// Step 4 — a real match, on bytes a real application produced
// ===========================================================================

/// Recorded on a real PTY: htop drawing an 80×24 screen (the #660 fixture). The engine
/// finds `CPU` at column 48 of line 9 — a real match at a real coordinate, not a number
/// chosen to be out of range.
///
/// The gesture is the one the API exists for and the one #437 is about: the consumer
/// holds that match, the grid narrows under it, and it hands the *same* match back
/// through `set_active_search_match` because it re-designates by **position**. The engine
/// invalidates its own query-derived set on resize (it cannot re-run a query it does not
/// keep), so nothing upstream re-clamps that coordinate for the consumer — column 48 is
/// simply past the end of a 40-column grid now.
///
/// Before the fix the emphasis vanished silently. After it, it lands on the last column:
/// wrong content, but visibly *somewhere*, which is what lets a consumer notice.
#[test]
fn a_stale_real_match_after_a_narrowing_resize_still_paints() {
    let mut term = Engine::new(80, 24);
    term.feed(include_bytes!("fixtures/alt_resize_htop.pre.raw"));

    let found = term.search("CPU");
    let stale = *found.first().expect("fixture: htop's header contains CPU");
    assert!(
        stale.start_col >= 40,
        "fixture: the match must sit past the narrowed width to exercise this ({stale:?})"
    );

    term.resize(40, 24);
    term.set_active_search_match(Some(stale));

    let spans = term.frame().overlay.active_match;
    assert_eq!(
        spans.len(),
        1,
        "the stale designation must still paint one row, not disappear"
    );
    assert_eq!(
        spans[0].left, 39,
        "clamped onto the last column of the narrowed grid"
    );
}
