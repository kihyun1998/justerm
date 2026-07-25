//! #540 — a row-shift verb (DL/SU/IL/SD, and the region paths in LF/RI) must not leave a wrap
//! link crossing the boundary of the region it shifted.
//!
//! The wrap flag says "this row continues into the **next** row", so it is a claim about
//! *adjacency*. justerm rotates whole `Row` structs, so every pair **inside** the region moves
//! together and its claim stays true — the flag is only falsified at the two seams, where a row's
//! next neighbour changed underneath it:
//!
//! - the row **above** the region (`top - 1`), whose continuation rotated away; and
//! - the row that loses its continuation to the blank: `bottom - 1` after an up-shift (the blank
//!   lands at `bottom`), `bottom` after a down-shift (its continuation rotated to `top` and was
//!   blanked there). The up-shift form applies only when a stationary row sits below the region
//!   (`bottom + 1 < rows`) — at the screen's bottom edge the scroll is what *serves* the wrap, so
//!   clearing there would split the logical line the linefeed exists to continue.
//!
//! No reference implements this rule, which is why it is derived here rather than ported (ADR-0004
//! — the spec is the authority for VT semantics, above any implementation):
//!
//! - **ghostty** clears the wrap on *every* row a full-width IL/DL touches
//!   (`terminal/Terminal.zig:2746-2752`, `:2906-2912` at `e6e26e1`) — the clear runs *before* the
//!   row swap at `:2936-2939`, so both ends stay false and an interior link is destroyed, not
//!   preserved. It still never touches the row above the shifted range.
//! - **alacritty** has no `WRAPLINE` clear on any scroll path at all (`852e971`).
//! - **xterm.js** splices whole line objects and never touches `isWrapped`
//!   (`common/InputHandler.ts:1345-1402` at `699f553`). Its opposite polarity ("I continue the
//!   *previous* row", `common/buffer/Buffer.ts:566-570`) does not make it immune — it moves the
//!   exposure to the mirrored seam, where a spliced-in line carries a continuation claim about a
//!   predecessor it never met.
//!
//! The interior-survives test below is the side condition that separates this rule from ghostty's:
//! a pair that moved together is still a pair.

use justerm_core::Engine;

/// `"abcdZ"` on a 4-column screen wraps row 0 into row 1.
fn wrapped_at_top(rows: usize) -> Engine {
    let mut t = Engine::new(4, rows);
    t.feed(b"abcdZ");
    assert!(t.grid().is_row_wrapped(0), "fixture: row 0 wraps");
    t
}

#[test]
fn dl_ends_the_wrap_on_the_row_above() {
    // The issue's headline repro: DL removes the continuation, so the row above must stop
    // claiming one — otherwise two unrelated lines merge in copy, search and accessible text.
    let mut t = Engine::new(4, 4);
    t.feed(b"abcdZ\r\nQRST");
    assert_eq!(t.accessible_text().trim_end(), "abcdZ\nQRST", "fixture");

    t.feed(b"\x1b[2;1H\x1b[M"); // DL 1 at row 1 — deletes "Z", pulls "QRST" up

    assert!(
        !t.grid().is_row_wrapped(0),
        "the continuation was deleted, so row 0 no longer wraps"
    );
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcd\nQRST",
        "two unrelated lines must not merge"
    );
}

#[test]
fn sd_ends_the_wrap_on_the_row_above_the_region() {
    // Same seam, reached by a *down* shift: the region's top is blanked, so the row above it
    // wraps into a blank. Nothing is visible until something is printed there — which is exactly
    // why the flag, not the text, is the assertion.
    let mut t = wrapped_at_top(5);
    t.feed(b"\x1b[2;4r\x1b[2;1H\x1b[T"); // region rows 2..4, SD 1

    assert!(
        !t.grid().is_row_wrapped(0),
        "row 0's continuation was pushed away, so it no longer wraps"
    );

    t.feed(b"\x1b[2;1HQQ"); // print into the blank the wrap used to point at
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcd\nQQ\nZ",
        "a later write into that row must not be swallowed into row 0's line"
    );
}

#[test]
fn sd_ends_the_wrap_at_the_regions_bottom_edge() {
    // The seam the issue does not mention, and the one that merges *visible* text: after a down
    // shift the row now at `bottom` lost its continuation (it rotated to `top` and was blanked),
    // so its stale claim reaches across the region boundary into a row that never moved.
    let mut t = Engine::new(4, 5);
    t.feed(b"ZERO\r\nAAAA\r\nabcdE\x1b[5;1HOUT");
    assert!(
        t.grid().is_row_wrapped(2),
        "fixture: row 2 wraps into row 3"
    );

    t.feed(b"\x1b[2;4r\x1b[2;1H\x1b[T"); // region rows 2..4, SD 1

    assert!(
        !t.grid().is_row_wrapped(3),
        "the row at the region's bottom edge lost its continuation"
    );
    assert_eq!(
        t.accessible_text().trim_end(),
        "ZERO\n\nAAAA\nabcd\nOUT",
        "the wrap must not reach across the region boundary into untouched content"
    );
}

#[test]
fn su_ends_the_wrap_above_the_blank_it_exposed() {
    // The up-shift form of the bottom seam: the blank lands at `bottom`, so the row above it is
    // the one whose continuation left.
    let mut t = Engine::new(4, 5);
    t.feed(b"ZERO\r\nAAAA\r\nBBBB\r\nabcdE");
    assert!(
        t.grid().is_row_wrapped(3),
        "fixture: row 3 wraps into row 4"
    );

    t.feed(b"\x1b[2;4r\x1b[2;1H\x1b[S"); // region rows 2..4, SU 1

    assert!(
        !t.grid().is_row_wrapped(2),
        "row 2's continuation is now the blank the shift exposed"
    );

    t.feed(b"\x1b[4;1HQQ");
    assert_eq!(
        t.accessible_text().trim_end(),
        "ZERO\nBBBB\nabcd\nQQ\nE",
        "a later write into the exposed row must not join row 2's line"
    );
}

#[test]
fn ri_ends_the_wrap_on_the_row_above_the_region() {
    // RI at the top margin scrolls the region down through the same grid primitive, so it owes
    // the same clear — this pins the `reverse_index` call site, not just the CSI verbs.
    let mut t = wrapped_at_top(5);
    t.feed(b"\x1b[2;4r\x1b[2;1H\x1bM"); // region rows 2..4, cursor at its top, RI

    assert!(
        !t.grid().is_row_wrapped(0),
        "RI shifted row 0's continuation away"
    );
}

#[test]
fn a_full_screen_shift_ends_the_wrap_on_the_scrollback_row_above_the_grid() {
    // `top == 0` does not mean "no row above". The text readers walk `[scrollback ++ grid]` as one
    // buffer on the primary screen, so the seam moves *out of the grid* — and this is the ordinary
    // case, not an exotic one: any logical line long enough to have scrolled, plus a default
    // full-screen region. Found by the #540 completeness pass.
    let mut t = Engine::new(4, 3);
    t.feed(b"abcdefghijklmnop\r\nQRST");
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcdefghijklmnop\nQRST",
        "fixture: one logical line spanning scrollback and grid"
    );

    t.feed(b"\x1b[1;1H\x1b[M"); // DL 1 at row 0 — the region's top is the grid's top

    assert_eq!(
        t.accessible_text().trim_end(),
        "abcdefgh\nmnop\nQRST",
        "the scrollback tail must stop wrapping into the grid"
    );
}

#[test]
fn a_scroll_that_evicts_into_scrollback_keeps_the_wrap_it_preserved() {
    // The exemption that makes the rule above safe: `linefeed` pushes grid row 0 *into* scrollback,
    // so the continuation is re-attached one row further back and the claim stays true. Clearing
    // there would split a logical line the scroll deliberately preserved.
    let mut t = Engine::new(4, 3);
    t.feed(b"abcdefghijklmnop"); // one logical line, already spilling into scrollback
    t.feed(b"\r\nQRST"); // a linefeed at the bottom row evicts row 0 into scrollback

    assert!(
        t.accessible_text().contains("abcdefghijklmnop"),
        "the logical line must survive the eviction whole: {:?}",
        t.accessible_text()
    );
}

#[test]
fn the_seam_clear_reaches_the_wire_not_just_the_model() {
    // `end_wrap` damages the seam row's last column so a `Partial` frame re-ships the derived
    // WRAPLINE bit. Damage is indexed by row *position*, so the clear has to be recorded after
    // `record_scroll` rotates `line_damage` with the content — otherwise the model splits the rows
    // and the wire never says so, leaving a frame-mode consumer joined forever.
    let last_col = 3;

    let mut t = Engine::new(4, 5);
    t.feed(b"ZERO\r\nAAAA\r\nBBBB\r\nabcdE");
    t.feed(b"\x1b[2;4r");
    t.reset_damage();
    t.feed(b"\x1b[2;1H\x1b[S"); // SU 1 — the seam lands on row 2
    assert!(
        damaged_columns(&t, 2).contains(&last_col),
        "up-shift: row 2's last column must be damaged, got {:?}",
        t.damage()
    );

    let mut t = Engine::new(4, 5);
    t.feed(b"ZERO\r\nAAAA\r\nabcdE\x1b[5;1HOUT");
    t.feed(b"\x1b[2;4r");
    t.reset_damage();
    t.feed(b"\x1b[2;1H\x1b[T"); // SD 1 — the seam lands on row 3
    assert!(
        damaged_columns(&t, 3).contains(&last_col),
        "down-shift: row 3's last column must be damaged, got {:?}",
        t.damage()
    );
}

#[test]
fn a_row_that_cannot_advance_never_claims_a_wrap() {
    // Surfaced by this issue's completeness pass, but the fix site is the *set* site, not a shift:
    // parked below a DECSTBM region on the last row, `wrapline` -> `linefeed` advances nothing and
    // the glyph overwrites this same row from column 0. A wrap claimed there is permanently false —
    // the cursor never leaves, so nothing ever clears it — and a later row shift inherits it and
    // merges two unrelated logical lines. `write_glyph` now asks `wrapline_advances()`, the
    // predicate both wide-at-boundary paths already ask.
    let mut t = Engine::new(4, 3);
    t.feed(b"\x1b[1;2r\x1b[3;1HabcdX"); // region rows 1..2; print on row 3, below the margin

    assert!(
        !t.grid().is_row_wrapped(2),
        "nothing advanced, so the row did not wrap — it was overwritten from column 0"
    );

    t.feed(b"\x1b[r"); // margins back to full screen
    t.feed(b"\x1b[3;1H\n"); // LF at the last row shifts the region under the stale claim
    t.feed(b"\x1b[3;1HQQ");
    assert_eq!(
        t.accessible_text().trim_end(),
        "\n\nXbcd\nQQ",
        "a later write must not be swallowed into a line that never wrapped"
    );
}

/// The damaged column span recorded for `row`, or empty if the row is not in the damage set.
fn damaged_columns(t: &Engine, row: usize) -> Vec<usize> {
    match t.damage() {
        justerm_core::TermDamage::Full => (0..t.grid().cols()).collect(),
        justerm_core::TermDamage::Partial(lines) => lines
            .iter()
            .filter(|l| l.line == row)
            .flat_map(|l| l.left..=l.right)
            .collect(),
    }
}

#[test]
fn a_wrap_at_the_screens_bottom_edge_survives_the_scroll_that_serves_it() {
    // The rule's precondition, and the case that caught the first draft of it: a row at the
    // screen's bottom edge that wraps is the ordinary soft-wrap-at-the-last-row state — the
    // linefeed scrolls *so that* the continuation has somewhere to land, so the shift makes that
    // claim true rather than false. Clearing it here splits the logical line the scroll exists to
    // serve. Driven on the alt screen because that is the branch that region-scrolls in place.
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[?1049h");
    t.feed(b"\x1b[2;1Habcdefgh"); // start on the last row: "abcde" wraps, the scroll serves it

    assert!(
        t.grid().is_row_wrapped(0),
        "the row scrolled up still continues into the row the scroll exposed"
    );
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcdefgh",
        "the logical line must stay joined across the scroll"
    );
}

#[test]
fn a_wrap_inside_the_region_survives_the_shift() {
    // The side condition that separates this rule from ghostty's blunt one: both halves of the
    // pair moved together, so the claim is still true and must be left alone. Clearing every
    // touched row (ghostty `Terminal.zig:2746-2752`) would split this logical line.
    let mut t = Engine::new(4, 5);
    t.feed(b"ZERO\r\nAAAA\r\nabcdE\x1b[5;1HOUT");
    assert!(t.grid().is_row_wrapped(2), "fixture");

    t.feed(b"\x1b[2;4r\x1b[2;1H\x1b[S"); // region rows 2..4, SU 1 — the pair moves up together

    assert!(
        t.grid().is_row_wrapped(1),
        "the pair moved together, so the wrap is still true"
    );
    assert!(
        t.accessible_text().contains("abcdE"),
        "the logical line must stay joined: {:?}",
        t.accessible_text()
    );
}
