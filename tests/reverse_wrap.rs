//! Reverse-wraparound tests (#80, DEC private mode ?45). Verified against
//! xterm.js `backspace()`: reverse-wrap applies to BACKSPACE only (not cursor-
//! left), and only undoes a SOFT wrap (the row was an autowrap continuation) —
//! a hard CR/LF newline does not reverse-wrap.

use justerm::Engine;

#[test]
fn reverse_wrap_backspaces_to_the_previous_soft_wrapped_row() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?45h"); // reverse-wrap on
    t.feed(b"abcd"); // "abc" soft-wraps (WRAPLINE on row 0); 'd' at (1,0); cursor (1,1)
    t.feed(b"\x08"); // BS: (1,1) -> (1,0)
    t.feed(b"\x08"); // BS at col 0: reverse-wrap to (0,2)
    t.feed(b"X"); // overwrites the previous row's last cell
    assert_eq!(t.grid().cell(0, 2).c(), 'X');
}

#[test]
fn backspace_clamps_at_column_zero_by_default() {
    let mut t = Engine::new(3, 2);
    t.feed(b"abcd"); // soft wrap, but ?45 is off
    t.feed(b"\x08\x08"); // (1,1) -> (1,0) -> clamp
    t.feed(b"X");
    assert_eq!(
        t.grid().cell(1, 0).c(),
        'X',
        "default: BS clamps at column 0"
    );
}

#[test]
fn reverse_wrap_does_not_cross_a_hard_newline() {
    // Only soft wraps reverse — a hard CR/LF row is not WRAPLINE, so BS clamps.
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[?45h");
    t.feed(b"ab\r\nc"); // row 0 "ab" via hard CR/LF (not wrapped); cursor (1,1)
    t.feed(b"\x08\x08"); // (1,1) -> (1,0) -> clamp (prev row not WRAPLINE)
    t.feed(b"X");
    assert_eq!(t.grid().cell(1, 0).c(), 'X');
}

#[test]
fn cursor_left_does_not_reverse_wrap() {
    // Reverse-wrap is BS only; CSI D at column 0 still clamps.
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?45h");
    t.feed(b"abcd"); // soft wrap; cursor (1,1)
    t.feed(b"\x1b[2;1H"); // cursor to (1,0)
    t.feed(b"\x1b[D"); // cursor-left at column 0 — must NOT reverse-wrap
    t.feed(b"Y");
    assert_eq!(t.grid().cell(1, 0).c(), 'Y');
}

#[test]
fn reverse_wrap_at_home_has_no_effect() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?45h");
    t.feed(b"\x1b[1;1H"); // home (0,0)
    t.feed(b"\x08"); // BS at home — no previous line, clamp
    t.feed(b"Z");
    assert_eq!(t.grid().cell(0, 0).c(), 'Z');
}

#[test]
fn decrqm_and_ris_for_reverse_wrap() {
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[?45$p"); // off
    assert_eq!(t.drain_replies(), b"\x1b[?45;2$y");
    t.feed(b"\x1b[?45h\x1b[?45$p"); // on
    assert_eq!(t.drain_replies(), b"\x1b[?45;1$y");
    t.feed(b"\x1bc\x1b[?45$p"); // RIS resets, then query
    assert_eq!(t.drain_replies(), b"\x1b[?45;2$y");
}
