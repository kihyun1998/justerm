//! IRM insert/replace mode tests (#64, the non-private SM/RM mode 4). Behavior
//! verified against xterm.js InputHandler: in insert mode, print shifts the row
//! right by the glyph width (insertCells) before writing, discarding cells off
//! the right edge and clearing an orphaned wide half at the last column.

use justerm::Engine;

#[test]
fn insert_mode_shifts_the_tail_right() {
    let mut t = Engine::new(5, 1);
    t.feed(b"abc"); // "abc  "
    t.feed(b"\x1b[1;1H"); // cursor home
    t.feed(b"\x1b[4h"); // IRM on
    t.feed(b"X"); // insert at col 0 → "Xabc "
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
    assert_eq!(t.grid().cell(0, 1).c(), 'a');
    assert_eq!(t.grid().cell(0, 2).c(), 'b');
    assert_eq!(t.grid().cell(0, 3).c(), 'c');
}

#[test]
fn replace_is_the_default() {
    let mut t = Engine::new(5, 1);
    t.feed(b"abc\x1b[1;1HX"); // no IRM → X overwrites col 0
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
    assert_eq!(t.grid().cell(0, 1).c(), 'b'); // 'a' overwritten, not shifted
}

#[test]
fn reset_mode_returns_to_replace() {
    let mut t = Engine::new(5, 1);
    t.feed(b"abc\x1b[1;1H");
    t.feed(b"\x1b[4h"); // insert on
    t.feed(b"\x1b[4l"); // back to replace
    t.feed(b"X");
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
    assert_eq!(t.grid().cell(0, 1).c(), 'b'); // overwritten, not shifted
}

#[test]
fn insert_discards_cells_past_the_right_margin() {
    let mut t = Engine::new(3, 1);
    t.feed(b"abc"); // row full
    t.feed(b"\x1b[1;1H\x1b[4h");
    t.feed(b"X"); // insert at col 0 → "Xab", 'c' falls off
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
    assert_eq!(t.grid().cell(0, 1).c(), 'a');
    assert_eq!(t.grid().cell(0, 2).c(), 'b');
}

#[test]
fn insert_mode_shifts_a_wide_glyph_coherently() {
    // A width-2 insert opens a 2-column gap; the wide lead + spacer land cleanly
    // and the tail shifts by 2 — no orphaned half (insert_chars repairs seams).
    let mut t = Engine::new(6, 1);
    t.feed(b"abcd"); // cols 0..3
    t.feed(b"\x1b[1;1H\x1b[4h");
    t.feed("世".as_bytes()); // width-2 insert at col 0
    assert_eq!(t.grid().cell(0, 0).c(), '世');
    assert_eq!(t.grid().cell(0, 2).c(), 'a'); // tail shifted right by 2
    assert_eq!(t.grid().cell(0, 3).c(), 'b');
    assert_eq!(t.grid().cell(0, 4).c(), 'c');
    assert_eq!(t.grid().cell(0, 5).c(), 'd');
}
