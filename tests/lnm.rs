//! LNM line-feed/newline mode tests (#71, SM/RM mode 20). Verified against
//! xterm.js: `convertEol` makes `lineFeed()` also set `x = 0` (a carriage
//! return) — it affects only the OUTPUT line feed, NOT the Enter key encoding
//! (xterm.js's Enter always sends CR).

use justerm::Engine;

#[test]
fn lnm_makes_line_feed_also_carriage_return() {
    let mut t = Engine::new(5, 2);
    t.feed(b"ab"); // cursor at col 2
    t.feed(b"\x1b[20h"); // LNM on
    t.feed(b"\n"); // bare LF → next row AND column 0
    t.feed(b"X"); // lands at (1, 0)
    assert_eq!(t.grid().cell(1, 0).c(), 'X');
}

#[test]
fn line_feed_does_not_carriage_return_by_default() {
    let mut t = Engine::new(5, 2);
    t.feed(b"ab\nX"); // no LNM: LF only moves down, column unchanged
    assert_eq!(t.grid().cell(1, 2).c(), 'X');
    assert_eq!(t.grid().cell(1, 0).c(), ' ');
}

#[test]
fn rm_disables_newline_mode() {
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[20h"); // on
    t.feed(b"\x1b[20l"); // off
    t.feed(b"ab\nX"); // LF no longer carriage-returns
    assert_eq!(t.grid().cell(1, 2).c(), 'X');
}

#[test]
fn ris_resets_newline_mode() {
    // The #53 reconstruct resets every field; LNM (a field added after #53) is
    // included for free.
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[20h"); // LNM on
    t.feed(b"\x1bc"); // RIS
    t.feed(b"ab\nX"); // LNM off again → LF no CR
    assert_eq!(t.grid().cell(1, 2).c(), 'X');
}
