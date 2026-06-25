//! DECAWM autowrap toggle tests (#63, `?7` h/l). Behavior verified against
//! xterm.js InputHandler: wraparound default on; with it off, a glyph past the
//! right margin pins the cursor to the last column and overwrites in place
//! (`x = cols - 1`), and a wide glyph that does not fit is skipped.

use justerm::Engine;

#[test]
fn autowrap_off_overwrites_the_last_column() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?7l"); // DECAWM off
    t.feed(b"abcd"); // a,b,c fill the row; d overwrites the last column, no wrap
    assert_eq!(t.grid().cell(0, 0).c(), 'a');
    assert_eq!(t.grid().cell(0, 1).c(), 'b');
    assert_eq!(t.grid().cell(0, 2).c(), 'd'); // overwritten in place
    assert_eq!(t.grid().cell(1, 0).c(), ' '); // nothing wrapped to row 1
}

#[test]
fn autowrap_on_is_the_default_and_wraps() {
    let mut t = Engine::new(3, 2);
    t.feed(b"abcd"); // default on: d wraps to the next row
    assert_eq!(t.grid().cell(0, 2).c(), 'c');
    assert_eq!(t.grid().cell(1, 0).c(), 'd');
}

#[test]
fn re_enabling_autowrap_restores_wrapping() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?7l"); // off
    t.feed(b"\x1b[?7h"); // back on
    t.feed(b"abcd");
    assert_eq!(t.grid().cell(1, 0).c(), 'd'); // wraps again
}

#[test]
fn autowrap_off_skips_a_wide_glyph_that_does_not_fit() {
    // A width-2 glyph that cannot fit at the right margin is dropped, not wrapped
    // (xterm.js does `continue` when wraparound is off) — the last column and the
    // next row stay untouched.
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?7l");
    t.feed(b"ab"); // cursor at the last column
    t.feed("世".as_bytes()); // width-2, would overflow col 2 → skipped
    assert_eq!(t.grid().cell(0, 2).c(), ' '); // last column untouched
    assert_eq!(t.grid().cell(1, 0).c(), ' '); // nothing wrapped
}

#[test]
fn decrqm_reports_autowrap_state() {
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[?7$p"); // default on → set
    assert_eq!(t.drain_replies(), b"\x1b[?7;1$y");
    t.feed(b"\x1b[?7l\x1b[?7$p"); // off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?7;2$y");
}
