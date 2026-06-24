//! Extended mouse-encoding tests (#28): the remaining real encodings on top of
//! #11's Default (X10) + SGR (?1006) — urxvt (?1015), UTF-8 (?1005), and
//! SGR-pixels (?1016). Driven feed(mode)→encode_mouse, asserting exact bytes.

use justerm::{Engine, Modifiers, MouseAction, MouseButton, MouseEvent};

fn press(col: usize, row: usize) -> MouseEvent {
    MouseEvent {
        button: Some(MouseButton::Left),
        action: MouseAction::Press,
        col,
        row,
        px: 0,
        py: 0,
        mods: Modifiers::empty(),
    }
}

/// A wheel event at `(col, row)`, button held as a press (wheel has no release).
fn wheel(button: MouseButton, col: usize, row: usize) -> MouseEvent {
    MouseEvent {
        button: Some(button),
        action: MouseAction::Press,
        col,
        row,
        px: 0,
        py: 0,
        mods: Modifiers::empty(),
    }
}

// #50 — wheel-tilt (horizontal scroll) buttons. xterm buttons 6/7 ride the same
// 64-base wheel group as up/down (64/65): left = Cb base 66, right = 67. Prior
// art: Alacritty reports the wheel as 64/65/66/67.

#[test]
fn wheel_left_encodes_sgr_cb_66() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1006h"); // normal tracking + SGR
    // wheel-left at (5,10) 0-based → Cb base 66, 1-based coords 6;11, press 'M'.
    assert_eq!(
        t.encode_mouse(wheel(MouseButton::WheelLeft, 5, 10))
            .unwrap(),
        b"\x1b[<66;6;11M"
    );
}

#[test]
fn wheel_right_encodes_sgr_cb_67() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        t.encode_mouse(wheel(MouseButton::WheelRight, 5, 10))
            .unwrap(),
        b"\x1b[<67;6;11M"
    );
}

#[test]
fn wheel_tilt_rides_the_x10_default_framing() {
    // The 64-base group flows through the X10 framing too: Cb 66 + 32 = 98 ('b'),
    // coords +32. Different code path from SGR (byte form, not decimal).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h"); // default X10 encoding
    assert_eq!(
        t.encode_mouse(wheel(MouseButton::WheelLeft, 5, 10))
            .unwrap(),
        vec![0x1b, b'[', b'M', 98, 38, 43]
    );
}

#[test]
fn modifier_bits_compose_with_a_wheel_button() {
    // Ctrl (+16) rides the same Cb as the button: 66 + 16 = 82.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1006h");
    let mut e = wheel(MouseButton::WheelLeft, 5, 10);
    e.mods = Modifiers::CTRL;
    assert_eq!(t.encode_mouse(e).unwrap(), b"\x1b[<82;6;11M");
}

#[test]
fn wheel_release_emits_nothing() {
    // A wheel turn is a single press-like event; there is no release. A Release
    // action on a wheel button must not leak a stray report (in SGR it would have
    // emitted a bogus `m`, in X10 a release that loses button identity).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1006h");
    let mut e = wheel(MouseButton::WheelUp, 5, 10);
    e.action = MouseAction::Release;
    assert!(
        t.encode_mouse(e).is_none(),
        "wheel has no release; must not emit a report",
    );
}

#[test]
fn urxvt_encoding_is_decimal_csi_m() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1015h"); // normal tracking + urxvt encoding
    // Left press at (5,10) 0-based → cb 0+32=32, 1-based decimal coords 6;11,
    // always terminated by 'M' (urxvt has no separate release 'm').
    assert_eq!(t.encode_mouse(press(5, 10)).unwrap(), b"\x1b[32;6;11M");
}

#[test]
fn utf8_encoding_multibyte_past_127() {
    // UTF-8 mode exists for terminals wider than the X10 byte ceiling.
    let mut t = Engine::new(300, 24);
    t.feed(b"\x1b[?1000h\x1b[?1005h"); // normal tracking + UTF-8 encoding
    // col 200 → cx value 200+1+32 = 233 → UTF-8 é (0xC3 0xA9); cb 32 → 0x20;
    // cy 10+1+32 = 43 → '+'. Like Default's CSI M but each value UTF-8 encoded.
    assert_eq!(
        t.encode_mouse(press(200, 10)).unwrap(),
        b"\x1b[M\x20\xc3\xa9\x2b"
    );
}

#[test]
fn sgr_pixels_uses_consumer_pixel_coordinates() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1016h"); // normal tracking + SGR-pixels
    // SGR framing (CSI < Cb ; X ; Y M), but X/Y are the consumer's 0-based
    // pixel coords (+1 to 1-based on the wire) — cell col/row are ignored.
    let mut e = press(5, 10);
    e.px = 411;
    e.py = 214;
    assert_eq!(t.encode_mouse(e).unwrap(), b"\x1b[<0;412;215M");
}

#[test]
fn sgr_pixels_release_uses_lowercase_m() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1016h");
    let mut e = press(5, 10);
    e.action = MouseAction::Release;
    e.px = 10;
    e.py = 20;
    // SGR keeps the button identity on release; terminator 'm'.
    assert_eq!(t.encode_mouse(e).unwrap(), b"\x1b[<0;11;21m");
}

#[test]
fn disabling_an_encoding_returns_to_default() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1000h\x1b[?1015h"); // urxvt on
    t.feed(b"\x1b[?1015l"); // urxvt off → back to Default X10
    // Default X10: cb 0+32=' ', cx 6+32, cy 11+32.
    assert_eq!(
        t.encode_mouse(press(5, 10)).unwrap(),
        vec![0x1b, b'[', b'M', 32, 38, 43]
    );
}
