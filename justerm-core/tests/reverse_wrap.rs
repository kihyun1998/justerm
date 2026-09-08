//! Reverse-wraparound tests (#80, #873, DEC private mode ?45). The mode applies to
//! BACKSPACE and to CURSOR-LEFT alike — one step serves both verbs, which is how xterm
//! and ghostty are built and what #873 decided against xterm.js's deliberate split. It
//! only undoes a SOFT wrap (the row was an autowrap continuation); a hard CR/LF newline
//! does not reverse-wrap, in this engine and in xterm (`LineTstWrapped`, `cursor.c:178`).

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

/// `CSI D` at column 0 walks back to the previous soft-wrapped row, exactly as BS does
/// (#873). One step serves both verbs, which is xterm's shape: `CursorBack` is reached
/// from `CASE_BS` (`charproc.c:3703`) and `CASE_CUB` (`:3933`) alike.
#[test]
fn cursor_left_reverse_wraps_to_the_previous_soft_wrapped_row() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?45h");
    t.feed(b"abcd"); // soft wrap; cursor (1,1)
    t.feed(b"\x1b[2;1H"); // cursor to (1,0) — the CUP also clears the park
    t.feed(b"\x1b[D"); // cursor-left at column 0 — reverse-wraps
    t.feed(b"Y");
    assert_eq!(
        t.grid().cell(0, 2).c(),
        'Y',
        "CUB walked onto the previous row"
    );
    assert_eq!(t.grid().cell(1, 0).c(), 'd', "and did not clamp in place");
}

/// The control for the walk: with `?45` off, `CSI D` at column 0 clamps.
#[test]
fn cursor_left_clamps_at_column_zero_by_default() {
    let mut t = Engine::new(3, 2);
    t.feed(b"abcd"); // soft wrap, but ?45 is off
    t.feed(b"\x1b[2;1H");
    t.feed(b"\x1b[D");
    t.feed(b"Y");
    assert_eq!(
        t.grid().cell(1, 0).c(),
        'Y',
        "default: CUB clamps at column 0"
    );
}

/// The walk is soft-wraps-only for `CSI D` as well, and this pins it **independently of
/// the shared step**: `reverse_wrap_does_not_cross_a_hard_newline` is the only other test
/// that observes the rule, and it drives `BS`. Found by mutating the walk's predicate to
/// `true` and watching exactly one test redden (#873).
#[test]
fn cursor_left_does_not_cross_a_hard_newline() {
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[?45h");
    t.feed(b"ab\x1b[2;1H"); // row 0 "ab" is not WRAPLINE; cursor to (1,0)
    t.feed(b"\x1b[D");
    t.feed(b"X");
    assert_eq!(
        t.grid().cell(1, 0).c(),
        'X',
        "CUB clamped: the previous row is not a soft wrap"
    );
}

/// The walk costs **one unit of the count**, not the whole sequence: xterm decrements
/// once per step inside the loop (`cursor.c:186`), so `CSI 2 D` at column 0 lands one
/// column short of the previous row's end.
#[test]
fn cursor_left_keeps_moving_after_it_walks() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?45h");
    t.feed(b"abcd");
    t.feed(b"\x1b[2;1H"); // (1,0)
    t.feed(b"\x1b[2D");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 1));
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

/// `CSI D` spends the park as the first unit of its move, exactly as BS does (#873).
///
/// **Decided by the maintainer on 2026-09-08 against a 2-2 reference split, and theirs to
/// reverse.** xterm and ghostty route both verbs through one function and so spend on
/// either — xterm's `CursorBack` is reached from `CASE_BS` (`charproc.c:3703`) and
/// `CASE_CUB` (`:3933`) alike, both outside any conditional compilation; ghostty's
/// `backspace` is `cursorLeft(1)` (`Terminal.zig:1696`) and the spend lives in
/// `cursorLeft` under a *"to match xterm"* comment (`:1774-1777`).
///
/// xterm.js is the one that separates them, and **on purpose rather than by accident of
/// its clamp order** — which is what the record here said until #873 read it properly.
/// Its `backspace` carries *"Our implementation deviates from xterm on purpose"* over
/// four bullets, of which *"any cursor movement sequence keeps working as expected"* is
/// this axis (`InputHandler.ts:810-818`), and `cursorBackward` is a bare
/// `_moveCursor(-n, 0)` (`:976-979`). alacritty implements no `?45` at all.
///
/// What broke the tie: `XTREVWRAP` is xterm-invented (`ctlseqs.txt:952`) with no DEC text
/// above it, ADR-0004 makes xterm the tie-breaker for this layer, and reach is ~0 through
/// terminfo — `?45` appears in neither xterm's `terminfo` nor its `termcap`, and its
/// `reverseWrap` resource defaults to `False` (`charproc.c:468`). Nothing arrives here
/// except an application that wrote the sequence against xterm's own definition of it.
///
/// The control is the whole test: an unparked cursor at the same coordinate must still
/// move its full count, or this has simply disabled `CSI D`.
#[test]
fn cursor_left_spends_a_park() {
    for (seq, want) in [(&b"\x1b[D"[..], 3usize), (&b"\x1b[3D"[..], 1)] {
        let mut t = Engine::new(4, 2);
        t.feed(b"\x1b[?45h");
        t.feed(b"abcd"); // fills row 0, parked at column 3
        assert!(t.cursor().pending_wrap, "precondition: parked");
        t.feed(seq);
        assert_eq!(
            (t.cursor().row, t.cursor().col),
            (0, want),
            "CUB {seq:?} spent the park as its first unit"
        );
        assert!(
            !t.cursor().pending_wrap,
            "and the park is spent, not still owed"
        );
    }

    // Control: same coordinate, no park. The full count still moves.
    for (seq, want) in [(&b"\x1b[D"[..], 2usize), (&b"\x1b[3D"[..], 0)] {
        let mut t = Engine::new(4, 2);
        t.feed(b"\x1b[?45h");
        t.feed(b"abc"); // cursor at column 3, unparked
        assert!(!t.cursor().pending_wrap, "precondition: not parked");
        t.feed(seq);
        assert_eq!(
            (t.cursor().row, t.cursor().col),
            (0, want),
            "an unparked CUB {seq:?} moves its full count"
        );
    }
}

/// With autowrap **off** a parked `CSI D` moves, the same 3-0 answer the backspace half
/// carries (#80) — pinned here so the shared step cannot acquire a different gate for
/// one of its two callers.
#[test]
fn a_parked_cursor_left_with_autowrap_off_moves() {
    let mut t = Engine::new(3, 2);
    t.feed(b"\x1b[?7l\x1b[?45h");
    t.feed(b"abc");
    assert!(t.cursor().pending_wrap, "precondition: parked under ?7l");
    t.feed(b"\x1b[D");
    assert_eq!(
        (t.cursor().row, t.cursor().col),
        (0, 1),
        "?7l: the park is spent by moving"
    );
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
