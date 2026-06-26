//! win32-input-mode flag tracking (#86, ?9001). justerm tracks the flag for
//! protocol completeness + DECRQM, but the raw win32 key-record encoding
//! (CSI Vk;Sc;Uc;Kd;Cs;Rc _) is a non-goal — it is raw passthrough with no
//! semantic conversion, so it belongs to the ConPTY consumer, not the engine.
//! encode_key and the Key/KeyEvent model are deliberately unchanged.

use justerm_core::{Engine, Key, KeyEvent};

#[test]
fn mode_9001_tracked_and_decrqm_reports_it() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?9001$p"); // off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?9001;2$y");
    t.feed(b"\x1b[?9001h\x1b[?9001$p"); // on → set
    assert_eq!(t.drain_replies(), b"\x1b[?9001;1$y");
}

#[test]
fn win32_input_mode_getter_reflects_the_flag() {
    let mut t = Engine::new(80, 24);
    assert!(!t.win32_input_mode());
    t.feed(b"\x1b[?9001h");
    assert!(t.win32_input_mode());
}

#[test]
fn ris_resets_win32_input_mode() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?9001h");
    t.feed(b"\x1bc"); // RIS
    assert!(!t.win32_input_mode());
}

#[test]
fn win32_input_mode_does_not_hijack_encode_key() {
    // The raw-record encoding is a non-goal: even with ?9001 on, encode_key still
    // produces the normal semantic sequence (Up → CSI A). The ConPTY consumer,
    // not the engine, would emit any win32 records.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?9001h");
    let up = KeyEvent {
        key: Key::Up,
        ..KeyEvent::default()
    };
    assert_eq!(t.encode_key(up), Some(b"\x1b[A".to_vec()));
}
