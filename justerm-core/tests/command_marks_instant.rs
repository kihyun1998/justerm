//! #742 — `Engine::command_marks` answers *now*, and the answer carries nothing that
//! dates it. That is the contract, not an oversight, so this file pins the two facts
//! the contract rests on rather than the absence of the fields.
//!
//! Its sibling `Engine::marker_index` carries a basis and an epoch because a consumer
//! *must* hold that answer — an overview ruler has to be current in every frame, and
//! re-pulling per frame is the `O(M)`-per-frame payload ADR-0020 R3 forbids. This query
//! is consumed when a user acts, so re-asking is the natural act. What makes re-asking
//! a sufficient answer is measured below and is not true of the sibling:
//!
//! - the frame of reference **never changes** (always `[scrollback ++ primary]`), so a
//!   re-ask always answers and an empty answer can only mean disposal;
//! - and the lines really do move on **both** of the sibling's axes, so a caller that
//!   kept one would be wrong on two independent counts.
//!
//! Prose alone would not hold this: #737's defect was a *doc* asserting a rebasing rule
//! that did not hold, and the consumer was written against it. These tests are what a
//! future change to `command_marks`'s scope or dating has to walk past.

use justerm_core::{Engine, MarkerKind};

fn lines(e: &Engine) -> Vec<usize> {
    e.command_marks().into_iter().map(|(_, l, _)| l).collect()
}

/// Axis 1 — scrollback eviction shifts every mark by the same amount, and neither the
/// tuple nor anything else the caller receives from this query expresses the delta.
#[test]
fn eviction_moves_every_line_and_the_answer_does_not_say_by_how_much() {
    let mut e = Engine::with_scrollback(20, 5, 4);
    for _ in 0..14 {
        e.feed(b"x\r\n");
    }
    e.feed(b"\x1b]133;A\x07p$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");

    let before = lines(&e);
    assert_eq!(before, vec![7, 7, 7, 8], "the marks as first answered");

    e.feed(b"x\r\nx\r\n");
    assert_eq!(
        lines(&e),
        vec![5, 5, 5, 6],
        "a uniform -2 the caller can see the effect of and cannot compute: the delta \
         lives in `evicted_total`, which this query does not return"
    );
}

/// Axis 2 — the epoch axis, reachable from the byte stream alone. A top-anchored
/// `DECSTBM` region leaves a static footer; a mark inside it is shifted once per output
/// line by `markers_shift_below_margin`. No resize, one `feed` per line, and
/// `evicted_total` never moves — so a caller holding the eviction delta would still be
/// wrong here. This is the axis #742's own body did not measure.
#[test]
fn a_static_footer_moves_the_marks_with_no_eviction_and_no_resize() {
    let mut e = Engine::new(20, 8);
    e.feed(b"\x1b[1;6r"); // region rows 1..=6, footer below it
    e.feed(b"\x1b[8;1H\x1b]133;A\x07f");
    assert_eq!(lines(&e), vec![7], "born in the footer");

    let basis_before = e.marker_index().evicted_total;
    e.feed(b"\x1b[6;1H");
    for _ in 0..3 {
        e.feed(b"line\r\n");
    }

    assert_eq!(lines(&e), vec![10], "shifted once per output line");
    assert_eq!(
        e.marker_index().evicted_total,
        basis_before,
        "and nothing was evicted, so the move is invisible to the other axis"
    );
}

/// The first property re-asking rests on: the lines are `[scrollback ++ primary]`
/// **always**, so the answer survives an alt-screen excursion unchanged — and the same
/// integer in `marker_index` names alt content at the same time. Nothing on either
/// carries the distinction; the doc comment is where it is stated.
#[test]
fn the_lines_are_primary_even_while_the_alt_screen_is_up() {
    let mut e = Engine::with_scrollback(16, 6, 40);
    e.feed(b"a\r\nb\r\nc\r\n");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n");
    let on_primary = lines(&e);
    assert_eq!(on_primary, vec![3, 3, 3]);

    e.feed(b"\x1b[?1049h");
    assert_eq!(
        lines(&e),
        on_primary,
        "unchanged on alt: this query is primary-scoped, it does not follow the \
         active buffer"
    );

    // The collision, stated as an assertion so it cannot be read as hypothetical.
    e.add_marker(3);
    let alt_line = e.marker_index().markers[0].line as usize;
    assert_eq!(
        alt_line, on_primary[0],
        "the same integer: 3 here names an ALT row, 3 above names PRIMARY content — \
         the two buffers share absolute indices, which is why `tracked_point` answers \
         None rather than a number (ADR-0026 D2/D3)"
    );
}

/// The second property: a re-ask always answers, and absence is unambiguous. The
/// sibling can promise neither — an id missing from `marker_index` may be disposed *or*
/// may simply belong to the other screen.
#[test]
fn a_re_ask_always_answers_and_an_empty_answer_can_only_mean_disposal() {
    let mut e = Engine::with_scrollback(16, 6, 2);
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n");
    let first = e.command_marks()[0].0;

    e.feed(b"\x1b[?1049h");
    let found = e.command_marks().into_iter().find(|m| m.0 == first);
    assert_eq!(
        found.map(|(_, _, k)| k),
        Some(MarkerKind::PromptStart),
        "re-asking on the alt screen still answers"
    );
    assert!(
        !e.marker_index().markers.iter().any(|m| m.id == first),
        "while the sibling pull cannot: on alt it reports the alt population, so this \
         id's absence there says nothing about whether it is alive"
    );

    e.feed(b"\x1b[?1049l");
    for _ in 0..12 {
        e.feed(b"x\r\n");
    }
    assert!(
        e.command_marks().is_empty(),
        "evicted off the top: empty means disposed, and cannot mean anything else"
    );
}
