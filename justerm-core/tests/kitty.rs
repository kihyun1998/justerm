//! Kitty keyboard-protocol tests (#23): flag-stack negotiation + query +
//! enhanced key encoding, on top of #11's legacy baseline.
//!
//! Negotiation is driven feed→drain_replies (the query reuses #27's reply
//! channel); encoding is driven feed(enable)→encode_key.

use justerm_core::{Engine, Key, KeyAction, KeyEvent, Modifiers};

fn key_press(k: Key) -> KeyEvent {
    KeyEvent {
        key: k,
        ..Default::default()
    }
}

#[test]
fn kitty_query_reports_zero_when_unset() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?u"); // kitty query (CSI ? u)
    // No flags enabled yet → current flags 0.
    assert_eq!(t.drain_replies(), b"\x1b[?0u");
}

#[test]
fn push_sets_current_flags_reported_by_query() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>5u"); // push flags = 5 (disambiguate | alternate-keys)
    t.feed(b"\x1b[?u");
    assert_eq!(t.drain_replies(), b"\x1b[?5u");
}

#[test]
fn pop_restores_previous_flags() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>5u"); // push 5 (0 saved)
    t.feed(b"\x1b[>9u"); // push 9 (5 saved)
    t.feed(b"\x1b[<u"); // pop 1 → back to 5
    t.feed(b"\x1b[?u");
    assert_eq!(t.drain_replies(), b"\x1b[?5u");
}

#[test]
fn set_modes_modify_current_flags() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[=3;1u\x1b[?u"); // mode 1 replace → 3
    assert_eq!(t.drain_replies(), b"\x1b[?3u");
    t.feed(b"\x1b[=8;2u\x1b[?u"); // mode 2 or-in 8 → 11
    assert_eq!(t.drain_replies(), b"\x1b[?11u");
    t.feed(b"\x1b[=1;3u\x1b[?u"); // mode 3 and-not 1 → 10
    assert_eq!(t.drain_replies(), b"\x1b[?10u");
}

// ---- enhanced key encoding (consults the active flags) ---------------------

#[test]
fn disambiguate_encodes_shift_enter_as_csi_u() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u"); // enable disambiguate (bit 0)
    let ev = KeyEvent {
        key: Key::Enter,
        mods: Modifiers::SHIFT,
        ..Default::default()
    };
    // Legacy can't express Shift+Enter (both → \r); kitty: CSI 13 ; 2 u
    // (13 = Enter codepoint, modifier param 1+shift = 2).
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[13;2u");
}

#[test]
fn report_events_encodes_release_as_csi_u() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>2u"); // enable report-events (bit 1)
    let ev = KeyEvent {
        key: Key::Enter,
        mods: Modifiers::empty(),
        action: KeyAction::Release,
        ..Default::default()
    };
    // Legacy can't report a release; kitty: CSI 13 ; 1 : 3 u (event 3 = release).
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[13;1:3u");
}

#[test]
fn report_events_keeps_press_legacy() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>2u"); // report-events only (no disambiguate)
    let ev = KeyEvent {
        key: Key::Enter,
        mods: Modifiers::empty(),
        ..Default::default()
    };
    // A plain press is still legacy under report-events-only — only the events
    // legacy cannot express (release/repeat) take the CSI u form.
    assert_eq!(t.encode_key(ev).unwrap(), b"\r");
}

#[test]
fn kitty_super_modifier_uses_kitty_bit_value() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u"); // disambiguate
    let ev = KeyEvent {
        key: Key::Enter,
        mods: Modifiers::SUPER,
        ..Default::default()
    };
    // Super is kitty bit value 8 (legacy cannot express it) → param 1+8 = 9.
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[13;9u");
}

#[test]
fn disambiguate_escape_becomes_csi_u() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u"); // disambiguate
    // Escape introduces escape sequences, so it is ambiguous and disambiguates
    // even unmodified → CSI 27 u.
    assert_eq!(t.encode_key(key_press(Key::Escape)).unwrap(), b"\x1b[27u");
}

#[test]
fn disambiguate_keeps_enter_tab_backspace_legacy() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u");
    // The documented exceptions: legacy bytes even under disambiguate.
    assert_eq!(t.encode_key(key_press(Key::Enter)).unwrap(), b"\r");
    assert_eq!(t.encode_key(key_press(Key::Tab)).unwrap(), b"\t");
    assert_eq!(t.encode_key(key_press(Key::Backspace)).unwrap(), vec![0x7f]);
}

#[test]
fn functional_key_with_event_uses_legacy_form_plus_params() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>2u"); // report-events
    let ev = KeyEvent {
        key: Key::Up,
        mods: Modifiers::empty(),
        action: KeyAction::Release,
        ..Default::default()
    };
    // Functional keys keep the legacy terminator (A) but gain ;mods:event.
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[1;1:3A");
}

#[test]
fn functional_key_unmodified_press_stays_legacy() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u"); // disambiguate
    assert_eq!(t.encode_key(key_press(Key::Up)).unwrap(), b"\x1b[A");
    assert_eq!(t.encode_key(key_press(Key::Delete)).unwrap(), b"\x1b[3~");
}

#[test]
fn functional_key_super_modifier_kitty_form() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u");
    let ev = KeyEvent {
        key: Key::Delete,
        mods: Modifiers::SUPER,
        ..Default::default()
    };
    // Delete (CSI 3 ~) + Super → CSI 3 ; 9 ~ (kitty modifier value, legacy can't).
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[3;9~");
}

#[test]
fn all_as_escape_encodes_char_as_csi_u() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>8u"); // all-as-escape (bit 3)
    let ev = KeyEvent {
        key: Key::Char('a'),
        mods: Modifiers::empty(),
        ..Default::default()
    };
    // 'a' = codepoint 97 → CSI 97 u (legacy would just send 'a').
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[97u");
}

#[test]
fn all_as_escape_covers_enter_tab_backspace() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>8u"); // all-as-escape — *every* key takes the CSI u form
    assert_eq!(t.encode_key(key_press(Key::Enter)).unwrap(), b"\x1b[13u");
    assert_eq!(t.encode_key(key_press(Key::Tab)).unwrap(), b"\x1b[9u");
    assert_eq!(
        t.encode_key(key_press(Key::Backspace)).unwrap(),
        b"\x1b[127u"
    );
}

#[test]
fn report_events_encodes_repeat() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>2u"); // report-events
    let ev = KeyEvent {
        key: Key::Enter,
        action: KeyAction::Repeat,
        ..Default::default()
    };
    // Repeat is event type 2 → CSI 13 ; 1 : 2 u.
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[13;1:2u");
}

#[test]
fn char_without_all_as_escape_stays_legacy() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u"); // disambiguate only — printable chars are unchanged
    assert_eq!(t.encode_key(key_press(Key::Char('a'))).unwrap(), b"a");
}

#[test]
fn alternate_keys_includes_shifted_codepoint() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>5u"); // disambiguate (1) + alternate-keys (4)
    let ev = KeyEvent {
        key: Key::Char('a'),
        mods: Modifiers::SHIFT,
        shifted_key: Some('A'),
        ..Default::default()
    };
    // CSI keycode : shifted ; mods u → CSI 97:65 ; 2 u.
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[97:65;2u");
}

#[test]
fn associated_text_appends_text_codepoint() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>24u"); // all-as-escape (8) + associated-text (16)
    let ev = KeyEvent {
        key: Key::Char('a'),
        text: Some('a'),
        ..Default::default()
    };
    // CSI keycode ; mods ; text u → CSI 97 ; 1 ; 97 u.
    assert_eq!(t.encode_key(ev).unwrap(), b"\x1b[97;1;97u");
}

// ---- dogfood: a real neovim session output stream (#23) --------------------

#[test]
fn neovim_kitty_session_consumed_to_stable_state() {
    // Frozen capture of a real neovim session (tests/fixtures/capture-kitty.sh):
    // it carries the kitty push/query/pop, plus DA1, alt-screen, mouse and a
    // real edit redraw. Feeding it must not panic and must leave coherent state.
    let raw = include_bytes!("fixtures/neovim_kitty.raw");
    let mut t = Engine::new(80, 24);
    t.feed(raw);

    // (1) The kitty stack nets to zero: neovim pushed `>1u` and popped `<u`, so a
    // fresh query now reports 0.
    t.feed(b"\x1b[?u");
    let replies = t.drain_replies();
    assert!(
        replies.windows(5).any(|w| w == b"\x1b[?0u"),
        "kitty flags should net to 0 after the session's push/pop"
    );

    // (2) neovim leaves the alt screen on exit, so the primary grid shows the
    // typescript header printed before alt-screen — a stable, non-degenerate cell.
    assert_eq!(t.grid().cell(0, 0).c(), 'S'); // "Script started on ..."
}

#[test]
fn neovim_kitty_session_answers_da1_query() {
    // The same stream contains neovim's DA1 query (CSI c) — the engine replies on
    // the #27 channel while parsing, proving the integration end to end.
    let raw = include_bytes!("fixtures/neovim_kitty.raw");
    let mut t = Engine::new(80, 24);
    t.feed(raw);
    let replies = t.drain_replies();
    assert!(
        replies.windows(9).any(|w| w == b"\x1b[?62;22c"),
        "DA1 query in the stream should have been answered"
    );
}
