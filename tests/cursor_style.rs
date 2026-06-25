//! Cursor-style reporting tests (#81): the frame carries cursor shape + blink so
//! the renderer can draw the caret, and ?12 (att610) toggles the blink axis.
//! Cursor style is renderer state that crosses the wire (like cursor_visible,
//! #38), not a getter. DECSCUSR (shape) is wired separately (#89).

use justerm::{CursorShape, Engine, decode, encode};

#[test]
fn mode_12_sets_cursor_blink_on_the_frame() {
    let mut t = Engine::new(80, 24);
    assert!(!t.frame().cursor_blink, "default: no blink");
    t.feed(b"\x1b[?12h");
    assert!(t.frame().cursor_blink, "?12h turns the caret blink on");
}

#[test]
fn cursor_shape_defaults_to_block_and_round_trips() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?12h");
    let frame = t.frame();
    assert_eq!(frame.cursor_shape, CursorShape::Block); // DECSCUSR (#89) sets shape
    // The new cursor-style fields survive the wire round-trip.
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded.cursor_shape, frame.cursor_shape);
    assert_eq!(decoded.cursor_blink, frame.cursor_blink);
}

#[test]
fn mode_12_reset_clears_blink_and_decrqm_reports_it() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?12$p"); // off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?12;2$y");
    t.feed(b"\x1b[?12h\x1b[?12$p"); // on → set
    assert_eq!(t.drain_replies(), b"\x1b[?12;1$y");
    t.feed(b"\x1b[?12l"); // off again
    assert!(!t.frame().cursor_blink);
}

#[test]
fn ris_resets_cursor_blink() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?12h"); // blink on
    t.feed(b"\x1bc"); // RIS
    assert!(!t.frame().cursor_blink);
}
