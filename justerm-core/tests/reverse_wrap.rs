//! Reverse-wraparound tests (#80, DEC private mode ?45). Verified against
//! xterm.js `backspace()`: reverse-wrap applies to BACKSPACE only (not cursor-
//! left), and only undoes a SOFT wrap (the row was an autowrap continuation) —
//! a hard CR/LF newline does not reverse-wrap.

use justerm_core::Engine;

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

/// A parked cursor spends the deferred wrap as the **first unit** of a backspace under
/// `?45h`, so the cursor does not move (#80).
///
/// The park means the cursor is logically one past the column it sits on, so the first
/// step back lands *on* that column — which is where it already is. Before this, the
/// flag was cleared and the column decremented anyway, so the logical `+1` was discarded
/// rather than spent and the parked and unparked states collapsed to the same landing.
///
/// Both engines that implement the reverse-wrap decrement do it this way, and gate it on
/// exactly this mode rather than on autowrap: xterm `cursor.c:154-157` —
/// `if ((rev || rev2) && screen->do_wrap) { --count; } else { --col; }` — and ghostty
/// `Terminal.zig:1773-1778`, under a comment saying it is *"to match xterm"*. xterm.js
/// reaches the same landing by letting `x == cols` stand in this branch.
///
/// The control is the whole test: an unparked cursor at the same coordinate must still
/// move, or the fix has simply disabled the backspace.
#[test]
fn reverse_wrap_backspace_spends_a_deferred_wrap_instead_of_moving() {
    // Parked: "abc" soft-wraps, "def" fills row 1, so the cursor sits at (1, 2) with the
    // wrap armed — logically at column 3.
    let mut parked = Engine::new(3, 2);
    parked.feed(b"\x1b[?45h");
    parked.feed(b"abcdef");
    assert!(parked.cursor().pending_wrap, "precondition: parked");
    parked.feed(b"\x08");
    assert_eq!(
        (parked.cursor().row, parked.cursor().col),
        (1, 2),
        "the backspace spent the park and did not move"
    );
    assert!(
        !parked.cursor().pending_wrap,
        "and the park is spent, not still owed"
    );

    // Control: same coordinate, no park. It must move.
    let mut control = Engine::new(3, 2);
    control.feed(b"\x1b[?45h");
    control.feed(b"abcde");
    assert!(!control.cursor().pending_wrap, "precondition: not parked");
    control.feed(b"\x08");
    assert_eq!(
        (control.cursor().row, control.cursor().col),
        (1, 1),
        "an unparked backspace still moves"
    );
}

/// The park is spent only under `?45`, which is the mode xterm gates it on — with reverse
/// wrap off a parked backspace moves like any other (#80).
#[test]
fn a_parked_backspace_without_reverse_wrap_still_moves() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?45l");
    t.feed(b"abcdef");
    assert!(t.cursor().pending_wrap, "precondition: parked");
    t.feed(b"\x08");
    assert_eq!((t.cursor().row, t.cursor().col), (1, 1));
}

/// The same rule with autowrap **off** (#869 opened this reach).
///
/// xterm's gate is `(rev || rev2) && do_wrap` — it does not consult `WRAPAROUND` — so a
/// park taken under `?7l` is spent the same way. Until #869 armed the deferred wrap
/// unconditionally there was no park here to spend, so this case could not arise; it is
/// the same defect in a second mode rather than a new one.
#[test]
fn reverse_wrap_backspace_spends_a_park_taken_with_autowrap_off() {
    let mut parked = Engine::new(3, 2);
    parked.feed(b"\x1b[?7l\x1b[?45h");
    parked.feed(b"abc"); // fills the row; the park is taken even with the mode off
    assert!(
        parked.cursor().pending_wrap,
        "precondition: parked under ?7l"
    );
    parked.feed(b"\x08");
    assert_eq!(
        (parked.cursor().row, parked.cursor().col),
        (0, 2),
        "spent, not discarded"
    );

    let mut control = Engine::new(3, 2);
    control.feed(b"\x1b[?7l\x1b[?45h");
    control.feed(b"ab");
    control.feed(b"\x08");
    assert_eq!((control.cursor().row, control.cursor().col), (0, 1));
}
