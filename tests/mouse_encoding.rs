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
