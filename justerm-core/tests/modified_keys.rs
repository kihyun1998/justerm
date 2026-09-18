//! The modified-keys mask on the frame (#941): whether a modifier held on one of the four
//! keys whose bare form is a C0 control reaches the application, for the keyboard modes the
//! application has asked for.
//!
//! Driven the way `kitty.rs` and `modify_other_keys.rs` drive their modes — `feed(request)` →
//! `frame()` — and every bit is checked against `encode_key`, the thing it summarises.

use justerm_core::{Engine, Key, KeyAction, KeyEvent, ModifiedKeys, Modifiers, decode, encode};

fn enc(t: &Engine, k: Key, mods: Modifiers) -> Vec<u8> {
    t.encode_key(KeyEvent {
        key: k,
        mods,
        action: KeyAction::Press,
        ..Default::default()
    })
    .expect("every probe key encodes")
}

fn mask(t: &Engine) -> ModifiedKeys {
    t.frame().modified_keys
}

/// Every bit, against the encoder: set exactly when the modified press encodes differently
/// from the bare one.
fn assert_mask_agrees_with_encoder(t: &Engine) {
    let m = mask(t);
    for (bit, key, mods) in [
        (ModifiedKeys::SHIFT_ENTER, Key::Enter, Modifiers::SHIFT),
        (ModifiedKeys::SHIFT_TAB, Key::Tab, Modifiers::SHIFT),
        (
            ModifiedKeys::SHIFT_BACKSPACE,
            Key::Backspace,
            Modifiers::SHIFT,
        ),
        (ModifiedKeys::SHIFT_ESCAPE, Key::Escape, Modifiers::SHIFT),
        (ModifiedKeys::ALT_ENTER, Key::Enter, Modifiers::ALT),
        (ModifiedKeys::ALT_TAB, Key::Tab, Modifiers::ALT),
        (ModifiedKeys::ALT_BACKSPACE, Key::Backspace, Modifiers::ALT),
        (ModifiedKeys::ALT_ESCAPE, Key::Escape, Modifiers::ALT),
        (ModifiedKeys::CTRL_ENTER, Key::Enter, Modifiers::CTRL),
        (ModifiedKeys::CTRL_TAB, Key::Tab, Modifiers::CTRL),
        (
            ModifiedKeys::CTRL_BACKSPACE,
            Key::Backspace,
            Modifiers::CTRL,
        ),
        (ModifiedKeys::CTRL_ESCAPE, Key::Escape, Modifiers::CTRL),
    ] {
        let distinct = enc(t, key, mods) != enc(t, key, Modifiers::empty());
        assert_eq!(
            m.contains(bit),
            distinct,
            "{bit:?} disagrees with encode_key"
        );
    }
}

/// The case the issue is about: under the legacy encoding Shift+Enter is a plain CR.
#[test]
fn legacy_drops_shift_on_enter() {
    let t = Engine::new(80, 24);
    assert!(!mask(&t).contains(ModifiedKeys::SHIFT_ENTER));
    // Shift+Tab is back-tab (`CSI Z`) in legacy, so that bit is set with no mode asked for.
    assert!(mask(&t).contains(ModifiedKeys::SHIFT_TAB));
    assert_mask_agrees_with_encoder(&t);
}

#[test]
fn kitty_disambiguate_carries_shift_on_enter() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1u"); // push flags = disambiguate
    assert!(mask(&t).contains(ModifiedKeys::SHIFT_ENTER));
    assert_mask_agrees_with_encoder(&t);

    t.feed(b"\x1b[<u"); // pop back to legacy
    assert!(!mask(&t).contains(ModifiedKeys::SHIFT_ENTER));
    assert_mask_agrees_with_encoder(&t);
}

/// Non-zero kitty flags are not the answer: report-events alone leaves a modified Enter
/// press in its legacy form.
#[test]
fn kitty_flags_without_disambiguate_still_drop_shift_on_enter() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>2u"); // push flags = report event types only
    assert!(!mask(&t).contains(ModifiedKeys::SHIFT_ENTER));
    assert_mask_agrees_with_encoder(&t);
}

#[test]
fn kitty_all_as_escape_carries_every_bit() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[=9u"); // set disambiguate | all-as-escape
    assert_eq!(mask(&t), ModifiedKeys::all());
    assert_mask_agrees_with_encoder(&t);
}

#[test]
fn modify_other_keys_2_carries_shift_on_enter_and_decstr_clears_it() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;1m"); // level 1 is not level 2
    assert!(!mask(&t).contains(ModifiedKeys::SHIFT_ENTER));

    t.feed(b"\x1b[>4;2m");
    assert!(mask(&t).contains(ModifiedKeys::SHIFT_ENTER));
    // Ctrl+Backspace keeps its legacy byte under level 2.
    assert!(!mask(&t).contains(ModifiedKeys::CTRL_BACKSPACE));
    assert_mask_agrees_with_encoder(&t);

    t.feed(b"\x1b[!p"); // DECSTR
    assert!(!mask(&t).contains(ModifiedKeys::SHIFT_ENTER));
    assert_mask_agrees_with_encoder(&t);
}

#[test]
fn the_mask_survives_the_wire() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;2m");
    let frame = t.frame();
    let back = decode(&encode(&frame)).expect("round trip");
    assert_eq!(back.modified_keys, frame.modified_keys);
    assert!(back.modified_keys.contains(ModifiedKeys::SHIFT_ENTER));
}
