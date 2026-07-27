//! Setting the soft-wrap flag owes damage, exactly as clearing it does.
//!
//! The flag lives on the `Row` (#538) and reaches a frame-mode consumer only as the last cell's
//! derived `WRAPLINE` bit. Every other cell-carried fact changes when that cell is written, so
//! damage covers it for free — this one does not. `Term::end_wrap` has carried the matching
//! `damage_span` since #540, with the reason in its doc comment:
//!
//! > a `Partial` frame would never re-ship the bit — leaving a frame-mode consumer with two rows
//! > joined forever
//!
//! The **set** side never took the symmetric obligation. `write_glyph`'s pending-wrap resolve marks
//! the row and writes the *next* row's first cell, so the frame ships a span for the continuation
//! and none for the row that now claims it.
//!
//! Usually invisible, because the cursor moves off the marked row and `frame_damage` tops the frame
//! up with the old cursor cell. It stops being invisible exactly when a **scroll serves the wrap**
//! (#557): the cursor stays on the same row index, nothing tops it up, and the model and the wire
//! disagree — the model says wrapped, the wire says not.
//!
//! Pre-existing and not region-specific: the last-row control below reproduces it without DECSTBM.

use justerm_core::{CellFlags, Engine};

/// Does the frame carry `WRAPLINE` on `row`'s last cell — i.e. would a consumer rebuilding logical
/// lines from this frame join it to the next row?
fn frame_says_wrapped(t: &Engine, row: usize) -> bool {
    let last = (t.grid().cols() - 1) as u16;
    let row = row as u16;
    t.frame().spans.iter().any(|s| {
        s.line == row
            && s.left <= last
            && s.cells
                .get((last - s.left) as usize)
                .is_some_and(|c| c.flags().contains(CellFlags::WRAPLINE))
    })
}

#[test]
fn a_wrap_served_by_a_region_scroll_reaches_the_wire() {
    // #557's shape: the cursor stays on the same row index across the scroll, so nothing else
    // damages the row that just started wrapping.
    let mut t = Engine::new(4, 4);
    t.feed(b"\x1b[1;3r\x1b[3;1Habcd");
    t.reset_damage();

    t.feed(b"z");

    assert!(
        t.grid().is_row_wrapped(1),
        "fixture: the model says wrapped"
    );
    assert!(
        frame_says_wrapped(&t, 1),
        "and the wire must say the same, or a frame-mode consumer splits the line forever"
    );
}

#[test]
fn the_same_at_the_screens_last_row_without_a_region() {
    // The control that identifies this as pre-existing rather than a DECSTBM problem.
    let mut t = Engine::new(4, 4);
    t.feed(b"\x1b[4;1Habcd");
    t.reset_damage();

    t.feed(b"z");

    assert!(t.grid().is_row_wrapped(2), "fixture");
    assert!(frame_says_wrapped(&t, 2));
}

#[test]
fn a_wide_glyph_wrap_reaches_the_wire_too() {
    // The other set site (`vacate_for_wrap`) writes the vacated column and so damages it for its
    // own reason. Pinned so the two set paths cannot drift apart.
    let mut t = Engine::new(4, 4);
    t.feed(b"\x1b[1;3r\x1b[3;1Habc");
    t.reset_damage();

    t.feed("한".as_bytes());

    assert!(t.grid().is_row_wrapped(1), "fixture");
    assert!(frame_says_wrapped(&t, 1));
}

#[test]
fn an_ordinary_mid_screen_wrap_still_reaches_the_wire() {
    // This one was already covered — the cursor changes row, so `frame_damage` tops up the old
    // cursor cell. Kept as the side condition: the fix must not depend on that accident.
    let mut t = Engine::new(4, 4);
    t.feed(b"abcd");
    t.reset_damage();

    t.feed(b"z");

    assert!(t.grid().is_row_wrapped(0), "fixture");
    assert!(frame_says_wrapped(&t, 0));
}

#[test]
fn a_row_that_does_not_wrap_is_not_reported_as_wrapping() {
    // The negative: damaging the last column must not make an unwrapped row look wrapped.
    let mut t = Engine::new(4, 4);
    t.feed(b"ab");
    t.reset_damage();

    t.feed(b"c");

    assert!(!t.grid().is_row_wrapped(0), "fixture");
    assert!(!frame_says_wrapped(&t, 0));
}
