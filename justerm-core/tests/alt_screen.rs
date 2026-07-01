//! Legacy alt-screen mode tests (#72): ?47 / ?1047 (buffer switch, no cursor
//! save) and ?1048 (cursor save/restore, no switch). Verified against xterm.js
//! setModePrivate/resetModePrivate: ?1049 = ?1048 + ?47 combined; ?47 and ?1047
//! are treated identically (switch only).

use justerm_core::Engine;

#[test]
fn mode_47_switches_to_the_alt_buffer_and_back() {
    let mut t = Engine::new(5, 2);
    t.feed(b"P"); // 'P' on the primary
    t.feed(b"\x1b[?47h"); // switch to the (blank) alt buffer
    assert_eq!(t.grid().cell(0, 0).c(), ' ', "alt buffer is blank");
    t.feed(b"\x1b[?47l"); // back to primary
    assert_eq!(t.grid().cell(0, 0).c(), 'P', "primary content restored");
}

#[test]
fn mode_1047_switches_like_47() {
    let mut t = Engine::new(5, 2);
    t.feed(b"P");
    t.feed(b"\x1b[?1047h");
    assert_eq!(t.grid().cell(0, 0).c(), ' ');
    t.feed(b"\x1b[?1047l");
    assert_eq!(t.grid().cell(0, 0).c(), 'P');
}

#[test]
fn mode_47_does_not_save_or_restore_the_cursor() {
    // The key distinction from ?1049: ?47 switches the buffer but leaves the
    // cursor alone — on leave it stays where the alt screen put it.
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[1;5H"); // cursor at (0,4)
    t.feed(b"\x1b[?47h"); // switch to alt — cursor NOT saved
    t.feed(b"\x1b[2;1H"); // move to (1,0) on alt
    t.feed(b"\x1b[?47l"); // back to primary — cursor NOT restored
    t.feed(b"X"); // lands at (1,0), not the (0,4) a ?1049 would restore
    assert_eq!(t.grid().cell(1, 0).c(), 'X');
    assert_eq!(t.grid().cell(0, 4).c(), ' ');
}

#[test]
fn mode_1048_saves_and_restores_cursor_without_switching() {
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[1;5H"); // cursor at (0,4)
    t.feed(b"\x1b[?1048h"); // save cursor
    t.feed(b"\x1b[2;1HP"); // move to (1,0), write 'P' — same buffer (no switch)
    t.feed(b"\x1b[?1048l"); // restore cursor → (0,4)
    t.feed(b"X");
    assert_eq!(
        t.grid().cell(0, 4).c(),
        'X',
        "cursor restored to saved position"
    );
    assert_eq!(
        t.grid().cell(1, 0).c(),
        'P',
        "no buffer switch — content stays"
    );
}

#[test]
fn mode_1049_still_saves_and_restores_the_cursor() {
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[1;5H"); // cursor at (0,4)
    t.feed(b"\x1b[?1049h"); // save + switch
    t.feed(b"\x1b[2;1H"); // move on alt
    t.feed(b"\x1b[?1049l"); // switch back + restore → (0,4)
    t.feed(b"X");
    assert_eq!(t.grid().cell(0, 4).c(), 'X');
}

#[test]
fn decrqm_reports_alt_buffer_for_47_and_1047() {
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[?47$p"); // not on alt → reset
    assert_eq!(t.drain_replies(), b"\x1b[?47;2$y");
    t.feed(b"\x1b[?47h\x1b[?1047$p"); // on alt now; 1047 shares the state
    assert_eq!(t.drain_replies(), b"\x1b[?1047;1$y");
}

/// #149: the frame header exposes whether the alt screen is active, so a
/// frame-mode consumer (a11y announce policy, #119) can gate on it. All three
/// buffer-switch modes flip it — `?1049` and the older `?47`/`?1047` (full-screen
/// apps that repaint wholesale exactly like `?1049`, so they suppress announce
/// too). Matches xterm.js's single "is-alt" notion.
#[test]
fn frame_reports_alt_screen_state() {
    let mut t = Engine::new(5, 2);
    assert!(!t.frame().alt_screen, "primary screen at start");

    t.feed(b"\x1b[?1049h");
    assert!(t.frame().alt_screen, "alt after ?1049h");
    t.feed(b"\x1b[?1049l");
    assert!(!t.frame().alt_screen, "primary after ?1049l");

    t.feed(b"\x1b[?47h");
    assert!(t.frame().alt_screen, "alt after ?47h");
    t.feed(b"\x1b[?47l");
    assert!(!t.frame().alt_screen, "primary after ?47l");

    t.feed(b"\x1b[?1047h");
    assert!(t.frame().alt_screen, "alt after ?1047h");
    t.feed(b"\x1b[?1047l");
    assert!(!t.frame().alt_screen, "primary after ?1047l");
}

/// #149/#119: cursor-only `?1048` (save/restore) must NOT flip the alt-screen
/// flag — the single wiring mistake that would silently break the announce
/// policy (routing `?1048` to a buffer switch).
#[test]
fn frame_alt_screen_ignores_cursor_only_1048() {
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[?1048h"); // save cursor only, not a buffer switch
    assert!(!t.frame().alt_screen, "?1048h is cursor-only");
    t.feed(b"\x1b[?1048l");
    assert!(!t.frame().alt_screen, "?1048l is cursor-only");
}

/// #149: a full reset (RIS) while on the alt screen clears the flag, so the
/// a11y-facing field returns to primary.
#[test]
fn frame_alt_screen_cleared_by_ris() {
    let mut t = Engine::new(5, 2);
    t.feed(b"\x1b[?1049h");
    assert!(t.frame().alt_screen);
    t.feed(b"\x1bc"); // RIS
    assert!(!t.frame().alt_screen, "RIS returns to primary");
}
