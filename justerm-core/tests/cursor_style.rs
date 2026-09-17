//! Cursor-style reporting tests (#81): the frame carries cursor shape + blink so
//! the renderer can draw the caret, and ?12 (att610) toggles the blink axis.
//! Cursor style is renderer state that crosses the wire (like cursor_visible,
//! #38), not a getter. DECSCUSR (shape) is wired separately (#89).

use justerm_core::{CursorShape, Engine, decode, encode};

#[test]
fn mode_12_sets_cursor_blink_on_the_frame() {
    let mut t = Engine::new(80, 24);
    assert!(!t.frame().cursor_blink, "default: no blink");
    t.feed(b"\x1b[?12h");
    assert!(t.frame().cursor_blink, "?12h turns the caret blink on");
}

#[test]
fn cursor_shape_is_unset_until_the_application_speaks_and_round_trips() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?12h");
    let frame = t.frame();
    assert_eq!(
        frame.cursor_shape, None,
        "no DECSCUSR yet: the consumer's default applies (#927)"
    );
    // The cursor-style fields survive the wire round-trip, unset included.
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded.cursor_shape, frame.cursor_shape);
    assert_eq!(decoded.cursor_blink, frame.cursor_blink);
    for shape in [CursorShape::Block, CursorShape::Underline, CursorShape::Bar] {
        let mut f = frame.clone();
        f.cursor_shape = Some(shape);
        assert_eq!(
            decode(&encode(&f)).expect("decode").cursor_shape,
            Some(shape)
        );
    }
}

// #89 — DECSCUSR (CSI Ps SP q): shape + blink, into the #81 model. Verified
// against xterm.js setCursorStyle: 1/2 block, 3/4 underline, 5/6 bar; odd=blink.

#[test]
fn decscusr_5_sets_blinking_bar() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // CSI 5 SP q
    let f = t.frame();
    assert_eq!(f.cursor_shape, Some(CursorShape::Bar));
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
        assert_eq!(f.cursor_shape, Some(shape), "seq {seq:?}");
        assert_eq!(f.cursor_blink, blink, "seq {seq:?}");
    }
}

#[test]
fn decscusr_0_clears_the_application_shape() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1b[0 q"); // explicit 0 → the application has not spoken (#927)
    let f = t.frame();
    assert_eq!(f.cursor_shape, None);
    assert!(!f.cursor_blink);
}

#[test]
fn decscusr_with_no_parameter_clears_the_application_shape_too() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[6 q"); // steady bar
    t.feed(b"\x1b[ q"); // CSI SP q
    assert_eq!(t.frame().cursor_shape, None);
}

/// The header's cursor-shape byte: magic 2, version, has_scroll, kind, cols 2, rows 2,
/// cursor_row 2, cursor_col 2, cursor_visible.
const SHAPE_BYTE: usize = 14;

#[test]
fn unset_rides_as_its_own_byte_and_a_hollow_block_id_is_rejected() {
    let t = Engine::new(80, 24);
    let mut bytes = encode(&t.frame());
    assert_eq!(bytes[SHAPE_BYTE], 0xFF, "unset is 0xFF on the wire (#927)");
    // 3 is the renderer's HollowBlock id and no core shape: a frame carrying it is malformed.
    bytes[SHAPE_BYTE] = 3;
    assert!(matches!(
        decode(&bytes),
        Err(justerm_core::DecodeError::BadTag)
    ));
}

#[test]
fn decscusr_2_is_an_explicit_block_not_unset() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[2 q");
    assert_eq!(t.frame().cursor_shape, Some(CursorShape::Block));
}

#[test]
fn decstr_clears_the_application_shape() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1b[!p"); // DECSTR
    assert_eq!(t.frame().cursor_shape, None);
}

#[test]
fn leaving_the_alt_screen_restores_the_shape_from_before_it() {
    // nvim under TERM=xterm-256color, measured (#927): `?1049h`, then `CSI 2 SP q` (terminfo `Se`)
    // inside the alternate screen, then `?1049l`.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1049h\x1b[2 q");
    assert_eq!(t.frame().cursor_shape, Some(CursorShape::Block));
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.frame().cursor_shape, None);

    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[6 q\x1b[?1049h\x1b[2 q\x1b[?1049l");
    assert_eq!(t.frame().cursor_shape, Some(CursorShape::Bar));
}

#[test]
fn decscusr_unknown_param_leaves_style_unchanged() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1b[9 q"); // unknown → unchanged
    let f = t.frame();
    assert_eq!(f.cursor_shape, Some(CursorShape::Bar));
    assert!(f.cursor_blink);
}

#[test]
fn ris_resets_cursor_style() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5 q"); // bar blink
    t.feed(b"\x1bc"); // RIS
    let f = t.frame();
    assert_eq!(f.cursor_shape, None);
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
