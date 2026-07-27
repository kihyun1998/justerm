//! #557 — a scroll that a soft wrap *asked for* must not clear the wrap it is serving.
//!
//! `wrapline` → `linefeed` scrolls the scroll region precisely so the continuation of a line that
//! ran off the last column has somewhere to land. #540 then taught `shift_region` to end the wraps
//! a shift falsifies, and its bottom-seam rule — *"the row that loses its continuation to the
//! blank"* — fires on exactly the row that just legitimately wrapped.
//!
//! The two cases are indistinguishable from inside `shift_region`, and the difference is **why**
//! the shift is happening:
//!
//! - a verb (`IL`/`DL`/`SU`/`SD`, `RI`) **displaces** a continuation that already existed and
//!   stayed put — the claim is falsified, clear it;
//! - a linefeed the auto-wrap asked for **creates** the continuation — the blank that lands at the
//!   region's bottom *is* where the wrapped text goes, so the claim is about to become true.
//!
//! That makes it the mirror of `evicts_to_scrollback`, which exempts the *top* seam for the same
//! shape of reason (a linefeed pushes row 0 into scrollback, so adjacency survives one row back).
//! Each seam has exactly one exemption, and each is a fact about the caller that the shift itself
//! cannot see.
//!
//! #540's own guard covers only the screen's bottom edge (`bottom + 1 == rows`). Its recorded
//! rationale — *"the link is only broken when there is a stationary row below the region"* — is
//! necessary but not sufficient: a stationary row below is required for the *displacing* case, and
//! says nothing about the serving one. `a_verb_that_displaces_a_real_continuation_still_clears`
//! below is the half that must keep working.

use justerm_core::Engine;

/// Rows as characters, with `~` marking a row that claims to continue into the next.
fn rows(t: &Engine) -> Vec<String> {
    (0..t.grid().rows())
        .map(|r| {
            let cells: String = (0..t.grid().cols())
                .map(|c| t.grid().cell(r, c).c())
                .collect();
            format!(
                "{cells}{}",
                if t.grid().is_row_wrapped(r) { "~" } else { "" }
            )
        })
        .collect()
}

// ---- the defect ------------------------------------------------------------------------------

#[test]
fn a_wrap_at_the_region_bottom_survives_the_scroll_that_serves_it() {
    // Region rows 1..3; the cursor is on its bottom row and the text runs past the last column.
    // The scroll exists to give the continuation a home.
    let mut t = Engine::new(4, 4);

    t.feed(b"\x1b[1;3r\x1b[3;1Habcdz");

    assert_eq!(
        t.accessible_text().trim_end(),
        "\n\nabcdz",
        "one logical line, not two"
    );
}

#[test]
fn the_same_holds_for_a_wide_glyph() {
    // The wide-at-boundary path reaches `wrapline` through a different branch of `write_glyph`,
    // so it is pinned separately rather than assumed.
    let mut t = Engine::new(4, 4);

    t.feed("\x1b[1;3r\x1b[3;1Habc한".as_bytes());

    assert_eq!(t.accessible_text().trim_end(), "\n\nabc한");
}

#[test]
fn a_top_anchored_sub_region_is_covered_too() {
    // `linefeed`'s three shift paths differ: a top-anchored sub-region on the primary screen also
    // evicts into scrollback, so it takes the branch that passes `evicts_to_scrollback`. The
    // bottom-seam exemption has to reach that one as well.
    let mut t = Engine::new(4, 4);

    t.feed(b"\x1b[1;3r\x1b[3;1Habcdz");

    assert_eq!(t.accessible_text().trim_end(), "\n\nabcdz");
    assert_eq!(
        t.scrollback_len(),
        1,
        "and this is what identifies the branch: a top-anchored region evicts row 0 into \
         scrollback, so the exemption had to reach the call that also passes evicts_to_scrollback"
    );
}

// ---- what must NOT change --------------------------------------------------------------------

#[test]
fn a_verb_that_displaces_a_real_continuation_still_clears() {
    // #540's case, arranged inside a region: the wrap and its continuation both exist *before* the
    // region is set, and the continuation then sits below the region, stationary. Scrolling the
    // region moves the lead away from it — the claim really is falsified, and clearing it is what
    // stops two unrelated lines merging in copy.
    let mut t = Engine::new(4, 5);
    t.feed(b"\x1b[2;1Habcdz"); // row 1 wraps into row 2
    assert_eq!(rows(&t)[1], "abcd~", "fixture: row 1 claims a continuation");

    t.feed(b"\x1b[1;2r"); // region rows 0..1 — the lead is now the region's bottom row
    t.feed(b"\x1b[2;1H\r\n"); // an ordinary line feed, not one a wrap asked for

    assert_eq!(
        t.accessible_text().trim_end(),
        "\nabcd\n\nz",
        "the continuation stayed put while the lead moved up, so the two are separate lines"
    );
}

#[test]
fn a_wrap_below_the_region_bottom_is_unaffected() {
    // The region is set but the wrap lands in its middle, so no seam is involved at all. This is
    // what identifies the defect as "the region's bottom row" rather than "a region".
    let mut t = Engine::new(4, 4);

    t.feed(b"\x1b[1;3r\x1b[2;1Habcdz");

    assert_eq!(t.accessible_text().trim_end(), "\nabcdz");
}

#[test]
fn no_region_at_all_is_unaffected() {
    // The control for every test above: identical input without DECSTBM.
    let mut t = Engine::new(4, 4);

    t.feed(b"\x1b[3;1Habcdz");

    assert_eq!(t.accessible_text().trim_end(), "\n\nabcdz");
}

#[test]
fn a_reverse_index_still_clears_its_seam() {
    // `RI` shifts *down* and never serves a wrap — it moves the cursor up. The exemption must not
    // reach it, or #540's down-shift seam regresses.
    let mut t = Engine::new(4, 5);
    t.feed(b"\x1b[2;1Habcdz");
    t.feed(b"\x1b[1;3r\x1b[1;1H\x1bM");

    assert!(
        !t.grid().is_row_wrapped(1),
        "the continuation was pushed out of the region, so the claim is false"
    );
}
