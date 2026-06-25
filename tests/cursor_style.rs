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

// #89 — DECSCUSR (CSI Ps SP q): shape + blink, into the #81 model. Verified
// against xterm.js setCursorStyle: 1/2 block, 3/4 underline, 5/6 bar; odd=blink.

#[test]
fn decscusr_5_sets_blinking_bar() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // CSI 5 SP q
    let f = t.frame();
    assert_eq!(f.cursor_shape, CursorShape::Bar);
    assert!(f.cursor_blink);
}

#[test]
fn decscusr_param_table() {
    let cases: [(&[u8], CursorShape, bool); 6] = [
        (b"\x1b[1 q", CursorShape::Block, true),
        (b"\x1b[2 q", CursorShape::Block, false),
        (b"\x1b[3 q", CursorShape::Underline, true),
        (b"\x1b[4 q", CursorShape::Underline, false),
        (b"\x1b[5 q", CursorShape::Bar, true),
        (b"\x1b[6 q", CursorShape::Bar, false),
    ];
    for (seq, shape, blink) in cases {
        let mut t = Engine::new(80, 24);
        t.feed(seq);
        let f = t.frame();
        assert_eq!(f.cursor_shape, shape, "seq {seq:?}");
        assert_eq!(f.cursor_blink, blink, "seq {seq:?}");
    }
}

#[test]
fn decscusr_0_resets_to_steady_block() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1b[0 q"); // explicit 0 → default (steady block)
    let f = t.frame();
    assert_eq!(f.cursor_shape, CursorShape::Block);
    assert!(!f.cursor_blink);
}

#[test]
fn decscusr_unknown_param_leaves_style_unchanged() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1b[9 q"); // unknown → unchanged
    let f = t.frame();
    assert_eq!(f.cursor_shape, CursorShape::Bar);
    assert!(f.cursor_blink);
}

#[test]
fn ris_resets_cursor_style() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1bc"); // RIS
    let f = t.frame();
    assert_eq!(f.cursor_shape, CursorShape::Block);
    assert!(!f.cursor_blink);
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
