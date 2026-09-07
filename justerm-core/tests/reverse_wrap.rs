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

/// With autowrap **off**, a parked backspace moves — the park is spent by moving, not by
/// standing still (#80).
///
/// This test asserted the opposite in a first version of this change, on a reading of
/// xterm's spend site that stopped one line too early. `cursor.c:153` gates on
/// `(rev || rev2) && screen->do_wrap`, and `rev` looks like the mode flag but is not:
/// `:123-127` define `WRAP_MASK (REVERSEWRAP | WRAPAROUND)` and
/// `rev = ((flags & WRAP_MASK) == WRAP_MASK)`, so it means *`?45` **and** `?7h`* and the
/// spend branch is dead under `?7l`. ghostty reaches the same answer earlier still —
/// `if (!self.modes.get(.wraparound)) break :wrap_mode .none;` (`Terminal.zig:1756`)
/// returns through the plain decrement at `:1766-1769`. xterm.js never reaches the state,
/// since its `?7l` print pins `x = cols - 1` (`InputHandler.ts:612`). 3-0.
///
/// Both ways of arriving at the park are covered, because they are different mechanisms:
/// armed under `?7l` (which only #869 made possible) and armed under `?7h` and then
/// carried into `?7l` (reachable long before it).
#[test]
fn a_parked_backspace_with_autowrap_off_moves() {
    // Armed with the mode already off.
    let mut fresh = Engine::new(3, 2);
    fresh.feed(b"\x1b[?7l\x1b[?45h");
    fresh.feed(b"abc");
    assert!(
        fresh.cursor().pending_wrap,
        "precondition: parked under ?7l"
    );
    fresh.feed(b"\x08");
    assert_eq!(
        (fresh.cursor().row, fresh.cursor().col),
        (0, 1),
        "?7l: the park is spent by moving"
    );

    // Armed while the mode was on, then carried across `?7l`.
    let mut carried = Engine::new(3, 2);
    carried.feed(b"\x1b[?45h");
    carried.feed(b"abc");
    carried.feed(b"\x1b[?7l");
    assert!(
        carried.cursor().pending_wrap,
        "precondition: park carried in"
    );
    carried.feed(b"\x08");
    assert_eq!(
        (carried.cursor().row, carried.cursor().col),
        (0, 1),
        "a park carried into ?7l is spent by moving too"
    );
}

/// `CSI D` does **not** spend the park, and moves a full column from it (#80).
///
/// The asymmetry with backspace is deliberate and reference-backed, but until now
/// nothing could see it: `cursor_left_does_not_reverse_wrap` drives to column 0 with
/// `CUP` first, which clears the park, so it pins only the row-walk half. Giving
/// `move_back` the same spend left the entire core suite green.
///
/// The split is 2-2 and this engine follows xterm.js by mechanism rather than by
/// preference: its `backspace` calls `_restrictCursor(cols)` (`InputHandler.ts:806`) so
/// the park survives into the decrement, while `cursorBackward` clamps to `cols - 1`
/// first (`:889-890`, `:919`) and therefore never spends. xterm and ghostty route both
/// verbs through one function and so spend on either — xterm's `CursorBack` is reached
/// from `CASE_BS` (`charproc.c:3703`) and `CASE_CUB` (`:3933`) alike — so at the parked
/// column 3 below, xterm's `CSI D` spends the park and stays at 3 where this engine moves to 2.
#[test]
fn cursor_left_does_not_spend_a_park() {
    for (seq, want) in [(&b"\x1b[D"[..], 2usize), (&b"\x1b[3D"[..], 0)] {
        let mut t = Engine::new(4, 2);
        t.feed(b"\x1b[?45h");
        t.feed(b"abcd"); // fills row 0, parked at column 3
        assert!(t.cursor().pending_wrap, "precondition: parked");
        t.feed(seq);
        assert_eq!(
            (t.cursor().row, t.cursor().col),
            (0, want),
            "CUB {seq:?} moves its full count from a parked cursor"
        );
    }
}

/// Spending the park over a wide glyph leaves the cursor on the pair's **spacer**, not
/// on its lead (#80).
///
/// This is the one shape where the fix is visible as a destroyed glyph: the next print
/// lands on the spacer and blanks the lead beside it. It is nonetheless the reference
/// answer — ghostty lands on the spacer identically, and xterm.js reaches the same cell
/// and then blanks `x - 1` because `getWidth(x - 1) === 2` (`InputHandler.ts:537-539`),
/// producing the same row. Pinned because "correct" and "harmless" are different claims
/// and only the first is being made.
#[test]
fn a_spent_park_lands_on_a_wide_pairs_spacer() {
    let mut t = Engine::new(4, 2);
    t.feed(b"\x1b[?45h");
    t.feed("ab\u{4e00}".as_bytes()); // lead at column 2, spacer at column 3, parked
    assert!(t.cursor().pending_wrap, "precondition: parked");
    t.feed(b"\x08");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 3), "on the spacer");
    t.feed(b"X");
    let row: String = (0..4).map(|c| t.grid().cell(0, c).c()).collect();
    assert_eq!(row, "ab X", "the print blanks the orphaned lead beside it");
}
