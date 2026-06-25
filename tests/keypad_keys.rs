//! Application-keypad key encoding (#83). With app-keypad mode (#74), the
//! numeric keypad keys send classic VT100/VT220 SS3 sequences; in numeric mode
//! they send the literal characters. The sequences are the DEC application
//! keypad (verified against xterm ctlseqs); the consumer sends a raw
//! `Key::Keypad(..)` identity (it owns NumLock / key-location resolution).

use justerm::{Engine, Key, KeyEvent, KeypadKey};

fn keypad(k: KeypadKey) -> KeyEvent {
    KeyEvent {
        key: Key::Keypad(k),
        ..KeyEvent::default()
    }
}

#[test]
fn keypad_digit_in_app_mode_sends_ss3() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?66h"); // application keypad on (#74 / DECNKM)
    // numpad 5 → SS3 u (ESC O u), the classic VT100 application keypad code.
    assert_eq!(
        t.encode_key(keypad(KeypadKey::Digit(5))),
        Some(b"\x1bOu".to_vec())
    );
}

#[test]
fn app_keypad_full_table() {
    let cases: [(KeypadKey, &[u8]); 12] = [
        (KeypadKey::Digit(0), b"\x1bOp"),
        (KeypadKey::Digit(9), b"\x1bOy"),
        (KeypadKey::Decimal, b"\x1bOn"),
        (KeypadKey::Enter, b"\x1bOM"),
        (KeypadKey::Add, b"\x1bOk"),
        (KeypadKey::Subtract, b"\x1bOm"),
        (KeypadKey::Multiply, b"\x1bOj"),
        (KeypadKey::Divide, b"\x1bOo"),
        (KeypadKey::Equal, b"\x1bOX"),
        (KeypadKey::Digit(1), b"\x1bOq"),
        (KeypadKey::Digit(4), b"\x1bOt"),
        (KeypadKey::Digit(8), b"\x1bOx"),
    ];
    for (k, seq) in cases {
        let mut t = Engine::new(80, 24);
        t.feed(b"\x1b="); // DECKPAM → application keypad
        assert_eq!(t.encode_key(keypad(k)), Some(seq.to_vec()), "{k:?}");
    }
}

#[test]
fn numeric_keypad_sends_literal_chars() {
    let t = Engine::new(80, 24); // default = numeric mode
    assert_eq!(
        t.encode_key(keypad(KeypadKey::Digit(5))),
        Some(b"5".to_vec())
    );
    assert_eq!(t.encode_key(keypad(KeypadKey::Enter)), Some(b"\r".to_vec()));
    assert_eq!(t.encode_key(keypad(KeypadKey::Add)), Some(b"+".to_vec()));
    assert_eq!(
        t.encode_key(keypad(KeypadKey::Decimal)),
        Some(b".".to_vec())
    );
}

#[test]
fn deckpam_deckpnm_toggle_keypad_encoding() {
    // ESC = (DECKPAM) and ESC > (DECKPNM) drive the same flag as ?66 (#74).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b="); // application
    assert_eq!(
        t.encode_key(keypad(KeypadKey::Digit(1))),
        Some(b"\x1bOq".to_vec())
    );
    t.feed(b"\x1b>"); // numeric
    assert_eq!(
        t.encode_key(keypad(KeypadKey::Digit(1))),
        Some(b"1".to_vec())
    );
}
