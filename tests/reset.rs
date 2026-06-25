//! RIS / DECSTR reset tests (#53). Scope verified against xterm.js InputHandler:
//! RIS (ESC c) is a full re-init; DECSTR (CSI ! p) is a soft subset that does
//! not destroy content or reset the mouse, and turns autowrap back ON.

use justerm::{Engine, Modifiers, MouseAction, MouseButton, MouseEvent, TermDamage};

fn left_press() -> MouseEvent {
    MouseEvent {
        button: Some(MouseButton::Left),
        action: MouseAction::Press,
        col: 0,
        row: 0,
        px: 0,
        py: 0,
        mods: Modifiers::empty(),
    }
}

#[test]
fn ris_recovers_stuck_mouse_tracking() {
    // The motivating symptom: an app enabled mouse tracking and never disabled
    // it; RIS (ESC c) brings the terminal back to power-on, so encode_mouse
    // reports nothing again.
    let mut t = Engine::new(10, 3);
    t.feed(b"\x1b[?1000h\x1b[?1006h"); // mouse tracking + SGR encoding on
    assert!(t.encode_mouse(left_press()).is_some()); // precondition: tracking on
    t.feed(b"\x1bc"); // RIS
    assert!(
        t.encode_mouse(left_press()).is_none(),
        "RIS must reset mouse tracking to Off",
    );
}

#[test]
fn ris_clears_screen_scrollback_and_homes_cursor() {
    let mut t = Engine::new(4, 2);
    t.feed(b"a\r\nb\r\nc\r\nd"); // build scrollback + screen content
    assert!(t.scrollback_len() > 0);
    t.feed(b"\x1bc"); // RIS
    assert_eq!(t.scrollback_len(), 0, "scrollback cleared");
    assert_eq!(t.grid().cell(0, 0).c(), ' ', "screen cleared");
    t.feed(b"X"); // cursor is home → lands at (0,0)
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
}

#[test]
fn ris_preserves_replies_queued_before_it() {
    // A DA reply queued earlier in the same feed must survive the reset — the
    // outbound queue is consumer-bound output, not terminal state (#53).
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[c\x1bc"); // DA1 query (queues a reply), then RIS
    assert_eq!(t.drain_replies(), b"\x1b[?62;22c");
}

#[test]
fn ris_signals_full_damage() {
    let mut t = Engine::new(4, 2);
    t.feed(b"abc");
    t.reset_damage(); // ack
    t.feed(b"\x1bc"); // RIS clears the screen → consumer must repaint
    assert!(matches!(t.damage(), TermDamage::Full));
}

#[test]
fn decstr_keeps_screen_and_does_not_reset_mouse() {
    // DECSTR is a soft reset: content stays, and (unlike RIS) it does NOT touch
    // the mouse — so a stuck mouse is only recovered by RIS, never DECSTR.
    let mut t = Engine::new(5, 2);
    t.feed(b"hi"); // screen content
    t.feed(b"\x1b[?1000h\x1b[?1006h"); // mouse on
    t.feed(b"\x1b[!p"); // DECSTR
    assert_eq!(
        t.grid().cell(0, 0).c(),
        'h',
        "DECSTR must not clear the screen"
    );
    assert_eq!(t.grid().cell(0, 1).c(), 'i');
    assert!(
        t.encode_mouse(left_press()).is_some(),
        "DECSTR must NOT reset mouse tracking (only RIS does)",
    );
}

#[test]
fn decstr_turns_autowrap_back_on() {
    // The xterm quirk: DECSTR resets autowrap to ON, not the VT100 "off". Source-
    // verified against xterm.js (CoreService default `wraparound: true`).
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[?7l"); // autowrap off
    t.feed(b"\x1b[!p"); // DECSTR
    t.feed(b"\x1b[?7$p"); // query DECAWM
    assert_eq!(t.drain_replies(), b"\x1b[?7;1$y"); // set (on)
}

#[test]
fn decstr_keeps_the_active_cursor_position() {
    // Only the saved (DECSC) cursor homes; the active cursor stays put.
    let mut t = Engine::new(10, 2);
    t.feed(b"hi"); // cursor at col 2
    t.feed(b"\x1b[!p"); // DECSTR
    t.feed(b"X"); // lands at col 2, not home
    assert_eq!(t.grid().cell(0, 2).c(), 'X');
}

#[test]
fn ris_returns_from_alt_screen_to_primary() {
    // The alt screen has no scrollback; if RIS left us on it, scrolling would
    // accrue none. After RIS we are back on the primary, which accrues.
    let mut t = Engine::new(4, 2);
    t.feed(b"\x1b[?1049h"); // enter alt screen
    t.feed(b"\x1bc"); // RIS
    t.feed(b"a\r\nb\r\nc\r\nd"); // scroll on the (post-reset) screen
    assert!(
        t.scrollback_len() > 0,
        "RIS must return to the primary screen"
    );
}

#[test]
fn decstr_resets_insert_mode_to_replace() {
    let mut t = Engine::new(5, 1);
    t.feed(b"abc\x1b[1;1H");
    t.feed(b"\x1b[4h"); // insert mode on
    t.feed(b"\x1b[!p"); // DECSTR → replace
    t.feed(b"X");
    assert_eq!(t.grid().cell(0, 1).c(), 'b'); // overwritten, not shifted
}
