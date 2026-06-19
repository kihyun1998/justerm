//! Input-encoding tests (#11): consumer events → bytes, gated by engine modes.
//!
//! Each test drives the *whole* path through the public API — feed the DEC mode
//! sequence the app would emit, then assert the bytes `encode_*` produces — so
//! both the mode tracking and the encoding are covered. Byte expectations are
//! the legacy xterm spec (ctlseqs / DEC), not guesses.

use justerm::{Engine, Key, KeyEvent, Modifiers, MouseAction, MouseButton, MouseEvent};

fn key(k: Key) -> KeyEvent {
    KeyEvent {
        key: k,
        mods: Modifiers::empty(),
    }
}

fn key_mod(k: Key, mods: Modifiers) -> KeyEvent {
    KeyEvent { key: k, mods }
}

// ---- characters & control codes ------------------------------------------

#[test]
fn plain_char_is_its_utf8() {
    let term = Engine::new(80, 24);
    assert_eq!(term.encode_key(key(Key::Char('a'))).unwrap(), b"a");
}

#[test]
fn ctrl_letter_folds_to_control_code() {
    let term = Engine::new(80, 24);
    // Ctrl+A = 0x01, Ctrl+C = 0x03.
    assert_eq!(
        term.encode_key(key_mod(Key::Char('a'), Modifiers::CTRL))
            .unwrap(),
        vec![0x01]
    );
    assert_eq!(
        term.encode_key(key_mod(Key::Char('c'), Modifiers::CTRL))
            .unwrap(),
        vec![0x03]
    );
}

#[test]
fn alt_char_is_escape_prefixed() {
    let term = Engine::new(80, 24);
    assert_eq!(
        term.encode_key(key_mod(Key::Char('x'), Modifiers::ALT))
            .unwrap(),
        b"\x1bx"
    );
}

#[test]
fn enter_tab_backspace_escape() {
    let term = Engine::new(80, 24);
    assert_eq!(term.encode_key(key(Key::Enter)).unwrap(), b"\r");
    assert_eq!(term.encode_key(key(Key::Tab)).unwrap(), b"\t");
    assert_eq!(term.encode_key(key(Key::Backspace)).unwrap(), vec![0x7f]);
    assert_eq!(term.encode_key(key(Key::Escape)).unwrap(), vec![0x1b]);
}

#[test]
fn shift_tab_is_back_tab() {
    let term = Engine::new(80, 24);
    assert_eq!(
        term.encode_key(key_mod(Key::Tab, Modifiers::SHIFT))
            .unwrap(),
        b"\x1b[Z"
    );
}

// ---- cursor keys & DECCKM (acceptance: app-cursor-key mode) ----------------

#[test]
fn cursor_keys_are_csi_in_normal_mode() {
    let term = Engine::new(80, 24);
    assert_eq!(term.encode_key(key(Key::Up)).unwrap(), b"\x1b[A");
    assert_eq!(term.encode_key(key(Key::Down)).unwrap(), b"\x1b[B");
    assert_eq!(term.encode_key(key(Key::Right)).unwrap(), b"\x1b[C");
    assert_eq!(term.encode_key(key(Key::Left)).unwrap(), b"\x1b[D");
}

#[test]
fn cursor_keys_are_ss3_under_app_mode() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1h"); // DECCKM set
    assert_eq!(term.encode_key(key(Key::Up)).unwrap(), b"\x1bOA");
    assert_eq!(term.encode_key(key(Key::Left)).unwrap(), b"\x1bOD");
    term.feed(b"\x1b[?1l"); // back to normal
    assert_eq!(term.encode_key(key(Key::Up)).unwrap(), b"\x1b[A");
}

#[test]
fn modified_cursor_key_uses_csi_even_in_app_mode() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1h");
    // Ctrl (bit 4) → param 5. The modified form is CSI regardless of DECCKM.
    assert_eq!(
        term.encode_key(key_mod(Key::Up, Modifiers::CTRL)).unwrap(),
        b"\x1b[1;5A"
    );
    // Shift (bit 1) → param 2.
    assert_eq!(
        term.encode_key(key_mod(Key::Right, Modifiers::SHIFT))
            .unwrap(),
        b"\x1b[1;2C"
    );
}

#[test]
fn home_end_follow_cursor_key_mode() {
    let mut term = Engine::new(80, 24);
    assert_eq!(term.encode_key(key(Key::Home)).unwrap(), b"\x1b[H");
    assert_eq!(term.encode_key(key(Key::End)).unwrap(), b"\x1b[F");
    term.feed(b"\x1b[?1h");
    assert_eq!(term.encode_key(key(Key::Home)).unwrap(), b"\x1bOH");
}

// ---- tilde keys & function keys -------------------------------------------

#[test]
fn navigation_keys_are_tilde_sequences() {
    let term = Engine::new(80, 24);
    assert_eq!(term.encode_key(key(Key::Insert)).unwrap(), b"\x1b[2~");
    assert_eq!(term.encode_key(key(Key::Delete)).unwrap(), b"\x1b[3~");
    assert_eq!(term.encode_key(key(Key::PageUp)).unwrap(), b"\x1b[5~");
    assert_eq!(term.encode_key(key(Key::PageDown)).unwrap(), b"\x1b[6~");
}

#[test]
fn modified_tilde_key_carries_param() {
    let term = Engine::new(80, 24);
    // Delete + Ctrl → CSI 3;5~.
    assert_eq!(
        term.encode_key(key_mod(Key::Delete, Modifiers::CTRL))
            .unwrap(),
        b"\x1b[3;5~"
    );
}

#[test]
fn function_keys_split_ss3_and_tilde() {
    let term = Engine::new(80, 24);
    assert_eq!(term.encode_key(key(Key::F(1))).unwrap(), b"\x1bOP");
    assert_eq!(term.encode_key(key(Key::F(4))).unwrap(), b"\x1bOS");
    assert_eq!(term.encode_key(key(Key::F(5))).unwrap(), b"\x1b[15~");
    assert_eq!(term.encode_key(key(Key::F(10))).unwrap(), b"\x1b[21~");
    assert_eq!(term.encode_key(key(Key::F(12))).unwrap(), b"\x1b[24~");
}

#[test]
fn modified_function_key() {
    let term = Engine::new(80, 24);
    // F1 + Shift → CSI 1;2P.
    assert_eq!(
        term.encode_key(key_mod(Key::F(1), Modifiers::SHIFT))
            .unwrap(),
        b"\x1b[1;2P"
    );
}

// ---- mouse (acceptance: events honor active reporting mode) ----------------

fn mouse(button: Option<MouseButton>, action: MouseAction, col: usize, row: usize) -> MouseEvent {
    MouseEvent {
        button,
        action,
        col,
        row,
        px: 0,
        py: 0,
        mods: Modifiers::empty(),
    }
}

#[test]
fn mouse_off_encodes_nothing() {
    let term = Engine::new(80, 24);
    assert_eq!(
        term.encode_mouse(mouse(Some(MouseButton::Left), MouseAction::Press, 5, 10)),
        None
    );
}

#[test]
fn mouse_press_release_sgr() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1000h\x1b[?1006h"); // normal tracking + SGR
    // col/row are 0-based; SGR is 1-based. Left press at (5,10) → cb 0.
    assert_eq!(
        term.encode_mouse(mouse(Some(MouseButton::Left), MouseAction::Press, 5, 10))
            .unwrap(),
        b"\x1b[<0;6;11M"
    );
    assert_eq!(
        term.encode_mouse(mouse(Some(MouseButton::Left), MouseAction::Release, 5, 10))
            .unwrap(),
        b"\x1b[<0;6;11m"
    );
}

#[test]
fn mouse_motion_gating_by_mode() {
    let bare_move = mouse(None, MouseAction::Motion, 5, 10);
    let drag = mouse(Some(MouseButton::Left), MouseAction::Motion, 5, 10);

    // ?1000: no motion at all.
    let mut t1000 = Engine::new(80, 24);
    t1000.feed(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(t1000.encode_mouse(bare_move), None);
    assert_eq!(t1000.encode_mouse(drag), None);

    // ?1002: drag reported, bare move not.
    let mut t1002 = Engine::new(80, 24);
    t1002.feed(b"\x1b[?1002h\x1b[?1006h");
    assert_eq!(t1002.encode_mouse(bare_move), None);
    assert_eq!(
        t1002.encode_mouse(drag).unwrap(),
        b"\x1b[<32;6;11M" // motion flag +32
    );

    // ?1003: bare move reported too.
    let mut t1003 = Engine::new(80, 24);
    t1003.feed(b"\x1b[?1003h\x1b[?1006h");
    assert_eq!(
        t1003.encode_mouse(bare_move).unwrap(),
        b"\x1b[<35;6;11M" // no-button (3) + motion (32)
    );
}

#[test]
fn mouse_wheel_and_modifiers_sgr() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        term.encode_mouse(mouse(Some(MouseButton::WheelUp), MouseAction::Press, 0, 0))
            .unwrap(),
        b"\x1b[<64;1;1M"
    );
    // Ctrl+left press → cb 0 + 16.
    let ctrl_left = MouseEvent {
        button: Some(MouseButton::Left),
        action: MouseAction::Press,
        col: 0,
        row: 0,
        px: 0,
        py: 0,
        mods: Modifiers::CTRL,
    };
    assert_eq!(term.encode_mouse(ctrl_left).unwrap(), b"\x1b[<16;1;1M");
}

#[test]
fn mouse_default_encoding_x10() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1000h"); // tracking on, no ?1006 → default X10
    // Left press at (0,0): cb 0+32, cx 1+32, cy 1+32.
    assert_eq!(
        term.encode_mouse(mouse(Some(MouseButton::Left), MouseAction::Press, 0, 0))
            .unwrap(),
        vec![0x1b, b'[', b'M', 32, 33, 33]
    );
}

// ---- paste (acceptance: wrapped when bracketed-paste on) -------------------

#[test]
fn paste_raw_then_bracketed() {
    let mut term = Engine::new(80, 24);
    assert_eq!(term.encode_paste("hi"), b"hi");
    term.feed(b"\x1b[?2004h");
    assert_eq!(term.encode_paste("hi"), b"\x1b[200~hi\x1b[201~");
}

// ---- focus reporting ------------------------------------------------------

#[test]
fn focus_reporting_gated_by_1004() {
    let mut term = Engine::new(80, 24);
    assert_eq!(term.encode_focus(true), None);
    term.feed(b"\x1b[?1004h");
    assert_eq!(term.encode_focus(true).unwrap(), b"\x1b[I");
    assert_eq!(term.encode_focus(false).unwrap(), b"\x1b[O");
}
