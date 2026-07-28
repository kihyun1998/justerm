//! When the cursor lands past everything the reflow emitted, the seam spends a row — if the pane
//! can pay for it (#567 ①).
//!
//! A cursor sitting just after the content is an ordinary shell state: the prompt is at the bottom
//! of a full screen, waiting for the next byte. Narrow the window and the content re-splits into
//! more rows than it had, so "just after it" names a row the screen does not contain. #562 made
//! `reflow` say so honestly instead of inventing a position, and the caller's row fit supplies that
//! row **while the pane is shorter than the screen**. When the content already fills the screen
//! there is nothing to supply, and the cursor was pulled back onto the last glyph — so the next byte
//! destroyed a character.
//!
//! Having it costs one row, and the whole question is who pays. Five earlier designs made `reflow`
//! itself materialise rows and were rejected on measurements (a cursor at column 59 resized to width
//! 4 emptied the buffer; a blank-line exemption turned 22 alt lines into 21) — `reflow` cannot see
//! the pane's budget, so it spends what it does not have. The seam can: a pane with history pays by
//! **scrolling**, which is what a terminal does when content grows past the bottom, and a pane with
//! none cannot pay at all and keeps clamping.
//!
//! The gate is therefore `limit > 0`, not "is this the alt screen". Since #567 the alt panes pass
//! `limit: 0` because that is what an alt screen's history is, so they are excluded by the budget
//! rather than by a branch — the permanent `if alt` this design was rejected for needing.
//!
//! ADR-0025's rule is **amended** by this, not read narrowly: `reflow` does not create rows; the
//! seam may, when the pane can pay. The record rejected materialising *unconditionally*, which is
//! what was measured.

use justerm_core::Engine;

#[test]
fn the_cursor_below_a_full_screen_scrolls_instead_of_overwriting() {
    // The ordinary shell shape. Two 6-column lines fill a 3-row screen exactly once they re-split at
    // width 3, so the cursor's row is one past the last — and history is there to take the top line.
    let mut t = Engine::with_scrollback(6, 3, 100);
    t.feed(b"abcdef\r\nghijkl\r\n");
    assert_eq!(t.scrollback_len(), 0, "fixture");

    t.resize(3, 3);
    t.feed(b"Z");

    assert_eq!(
        t.accessible_text().trim_end(),
        "abcdef\nghijkl\nZ",
        "the next byte follows the content instead of landing on top of it"
    );
    assert_eq!(
        t.scrollback_len(),
        2,
        "the row it needed came from scrolling, which is what history is for"
    );
}

#[test]
fn the_same_when_the_content_fills_the_screen_exactly() {
    // One row of apparent slack is not slack: the re-split consumes it, and the cursor still needs
    // the row after. Pinned separately because "the screen is full" is the easy case to reason about
    // and this is the one that looks like it should already work.
    let mut t = Engine::with_scrollback(6, 4, 100);
    t.feed(b"abcdef\r\nghijkl\r\n");

    t.resize(3, 4);
    t.feed(b"Z");

    assert_eq!(t.accessible_text().trim_end(), "abcdef\nghijkl\nZ");
}

#[test]
fn a_pane_that_cannot_pay_still_clamps() {
    // The control that keeps this from being the rejected design. With no history the displaced row
    // is destroyed rather than archived, so the seam does not spend one — the cursor keeps landing
    // on the last row and a character is lost. That is worse than the fix and better than losing a
    // whole line, and it is the same judgement `reflow` was never able to make.
    let mut t = Engine::with_scrollback(6, 3, 0);
    t.feed(b"abcdef\r\nghijkl\r\n");

    t.resize(3, 3);
    t.feed(b"Z");

    assert_eq!(
        t.scrollback_len(),
        0,
        "nothing was archived, because there is nowhere to archive it"
    );
    assert_eq!(
        t.accessible_text().lines().count(),
        2,
        "and no line was destroyed to make room: {:?}",
        t.accessible_text()
    );
}

#[test]
fn a_short_pane_still_needs_no_scroll() {
    // The case #562 already fixed, kept as a side condition: when the fit is going to create the row
    // anyway, the seam must not also scroll for it — that would cost a row of history for nothing.
    let mut t = Engine::with_scrollback(6, 10, 100);
    t.feed(b"ab\r\n");

    t.resize(3, 10);
    t.feed(b"Z");

    assert_eq!(t.accessible_text().trim_end(), "ab\nZ");
    assert_eq!(t.scrollback_len(), 0, "no row was spent");
}

#[test]
fn the_alt_screen_is_excluded_by_its_budget_not_by_a_branch() {
    // #567's dividend. The alt pane has no history, so the same `limit > 0` gate that lets the
    // primary scroll refuses here — no `if alt` anywhere in the seam. Every line must survive.
    let mut t = Engine::new(6, 4);
    t.feed(b"\x1b[?1049h");
    t.feed(b"aaaaaa\r\nbbbbbb\r\n");
    let before = t.accessible_text().lines().count();

    t.resize(3, 4);

    // The *text* must change — narrowing truncates each row to the new width, which is what a
    // non-reflowing resize does (`Row::resize`, and the same in all three references). What must
    // not change is how many lines there are: spending a row here would destroy one.
    assert_eq!(
        t.accessible_text().trim_end(),
        "aaa\nbbb",
        "rows are truncated to the new width, not re-split"
    );
    assert_eq!(
        t.accessible_text().lines().count(),
        before,
        "and no line was spent on the cursor"
    );
}

#[test]
fn a_bce_tail_still_costs_no_row() {
    // #530 unchanged. A cursor parked in a colour-erased tail is the ordinary prompt redraw, and the
    // trim decides where the line *ends* — the cursor being past that end must not buy a row.
    let mut t = Engine::with_scrollback(8, 4, 100);
    t.feed(b"abcde\x1b[1;6H\x1b[41m\x1b[K");

    t.resize(5, 4);

    assert_eq!(t.accessible_text().trim_end(), "abcde");
    assert_eq!(t.scrollback_len(), 0, "no row was spent for the tail");
    assert!(!t.grid().is_row_wrapped(0));
}
