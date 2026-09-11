//! XTMODKEYS / modifyOtherKeys (#890): the mode an application asks for with
//! `CSI > 4 ; Pv m`, and what a key encodes once it has.
//!
//! Driven the way `kitty.rs` drives its protocol — `feed(enable)` → `encode_key` —
//! because that is the only seam a consumer has: the mode is learned from the
//! *output* stream and spends itself on the *input* one.

use justerm_core::{Engine, Key, KeyAction, KeyEvent, Modifiers};

fn press(k: Key, mods: Modifiers) -> KeyEvent {
    KeyEvent {
        key: k,
        mods,
        action: KeyAction::Press,
        ..Default::default()
    }
}

fn enc(t: &Engine, k: Key, mods: Modifiers) -> Vec<u8> {
    t.encode_key(press(k, mods))
        .expect("every probe key encodes")
}

/// The collision the mode exists to break: `Ctrl+I` and `Tab` are both `0x09`
/// until the application asks for them to be told apart.
#[test]
fn ctrl_i_stops_being_tab_once_the_application_asks() {
    let mut t = Engine::new(80, 24);
    assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\t");

    t.feed(b"\x1b[>4;2m"); // XTMODKEYS, exactly what vim emits at startup

    // `CSI 27 ; <1+mods> ; <codepoint> ~` — xterm's own shape (`input.c:760-782`,
    // the `formatOtherKeys` = 0 arm). Ctrl alone is bitmask 4, so the parameter
    // is 5; `i` is 105.
    assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\x1b[27;5;105~");
    // Tab is untouched — it is not an "other" key, and the point of the mode is
    // that the two stop being the same bytes.
    assert_eq!(enc(&t, Key::Tab, Modifiers::empty()), b"\t");
}

/// vim clears the mode on exit, and the clear is the more common half: `CSI > 4 ; m`
/// outnumbers `CSI > 4 ; 2 m` in this repo's captures (4 against 3). A mode that
/// cannot be turned off would leave every later application reading bytes it never
/// asked for.
#[test]
fn an_omitted_value_turns_it_back_off() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;2m");
    assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\x1b[27;5;105~");

    t.feed(b"\x1b[>4;m"); // what vim emits at exit

    assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\t");
}

/// The other two collisions the mode is for.
#[test]
fn the_other_two_control_aliases_separate_as_well() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;2m");
    assert_eq!(enc(&t, Key::Char('['), Modifiers::CTRL), b"\x1b[27;5;91~");
    assert_eq!(enc(&t, Key::Char('m'), Modifiers::CTRL), b"\x1b[27;5;109~");
    // ...while the keys they used to be indistinguishable from keep their bytes.
    assert_eq!(enc(&t, Key::Escape, Modifiers::empty()), b"\x1b");
    assert_eq!(enc(&t, Key::Enter, Modifiers::empty()), b"\r");
}

/// The hazard the gate is written around: xterm's own predicate admits any codepoint
/// in `0x40..=0x7f`, which is every capital letter. xterm is safe because shift is
/// spent producing the character; justerm is handed both, so the predicate had to be
/// narrowed or ordinary typing would turn into escape sequences.
#[test]
fn ordinary_typing_is_untouched_while_the_mode_is_on() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;2m");
    assert_eq!(enc(&t, Key::Char('a'), Modifiers::empty()), b"a");
    assert_eq!(enc(&t, Key::Char('A'), Modifiers::SHIFT), b"A");
    assert_eq!(enc(&t, Key::Char('Z'), Modifiers::SHIFT), b"Z");
    assert_eq!(enc(&t, Key::Char('@'), Modifiers::SHIFT), b"@");
}

/// Level 2 is what vim asks for and what the collision needs: at 0 and 1 xterm strips
/// the Control modifier from a key already associated with control, sending it down
/// the C0 path. Level 3 asks for *more* than 2, so it is honoured rather than read as
/// off — only 2's behaviour is implemented, and an application that wanted more must
/// not silently get none.
#[test]
fn the_level_decides_and_only_two_or_more_enables() {
    for (seq, enabled) in [
        (&b"\x1b[>4;0m"[..], false),
        (b"\x1b[>4;1m", false),
        (b"\x1b[>4;2m", true),
        (b"\x1b[>4;3m", true),
    ] {
        let mut t = Engine::new(80, 24);
        t.feed(seq);
        let got = enc(&t, Key::Char('i'), Modifiers::CTRL);
        assert_eq!(
            got == b"\x1b[27;5;105~",
            enabled,
            "{:?} should {} enable it, got {:?}",
            String::from_utf8_lossy(seq),
            if enabled { "" } else { "not" },
            String::from_utf8_lossy(&got)
        );
    }
}

/// The final carries four of xterm's resources and only one of them is routed. A
/// request aimed at modifyCursorKeys must not switch on modifyOtherKeys.
#[test]
fn a_request_aimed_at_another_resource_changes_nothing() {
    let mut t = Engine::new(80, 24);
    for seq in [&b"\x1b[>0;2m"[..], b"\x1b[>1;2m", b"\x1b[>2;2m", b"\x1b[>m"] {
        t.feed(seq);
        assert_eq!(
            enc(&t, Key::Char('i'), Modifiers::CTRL),
            b"\t",
            "{:?} must not enable modifyOtherKeys",
            String::from_utf8_lossy(seq)
        );
    }
}

/// Nothing changes for an application that never asked — the whole mode is opt-in,
/// and this is the assertion that would notice it leaking into the default path.
#[test]
fn the_default_encoding_is_untouched() {
    let t = Engine::new(80, 24);
    assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\t");
    assert_eq!(enc(&t, Key::Char('a'), Modifiers::CTRL), b"\x01");
    assert_eq!(enc(&t, Key::Char('a'), Modifiers::ALT), b"\x1ba");
}

/// kitty is checked first and stays first: an application that negotiated the newer
/// protocol gets it, even if an earlier one left modifyOtherKeys on.
#[test]
fn kitty_still_wins_when_both_are_on() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;2m");
    t.feed(b"\x1b[>1u"); // kitty: disambiguate
    assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\x1b[105;5u");
}

/// Both resets clear it, and DECSTR is xterm's answer rather than a convenience: the
/// line restoring its keyboard resources sits outside `ReallyReset`'s `full` gate.
#[test]
fn both_resets_clear_the_mode() {
    for reset in [&b"\x1bc"[..], b"\x1b[!p"] {
        let mut t = Engine::new(80, 24);
        t.feed(b"\x1b[>4;2m");
        assert_eq!(enc(&t, Key::Char('i'), Modifiers::CTRL), b"\x1b[27;5;105~");
        t.feed(reset);
        assert_eq!(
            enc(&t, Key::Char('i'), Modifiers::CTRL),
            b"\t",
            "{:?} must clear the mode",
            String::from_utf8_lossy(reset)
        );
    }
}

/// A modified character that is not a control alias also goes through, which is what
/// makes this a general mechanism rather than a three-key patch. Both values are
/// ghostty's own test expectations (`key_encode.zig`), so agreement is checkable
/// rather than asserted.
#[test]
fn a_modified_character_that_is_not_a_control_alias_goes_through_too() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>4;2m");
    assert_eq!(
        enc(&t, Key::Char('H'), Modifiers::CTRL | Modifiers::SHIFT),
        b"\x1b[27;6;72~"
    );
    assert_eq!(enc(&t, Key::Char('8'), Modifiers::ALT), b"\x1b[27;3;56~");
}

/// A real `vim` session, not a sequence typed into a test. The synthetic cases above
/// assert what the engine does with bytes *I* wrote; this one asserts that the bytes a
/// real editor emitted on a real PTY reach the same state — the gap those two leave is
/// where a mis-parsed parameter, or a form no literal in this file happens to use, would
/// hide.
///
/// **The capture contains the whole round trip, and asserting only its end state would
/// have asserted the wrong half.** `vim_redraw.raw` enables at byte 17 and clears twice
/// near the end (2943, 3035), because the recording runs until vim exits — so a test
/// that fed the file and checked the mode was *on* failed against a correct engine. What
/// the material actually proves is both transitions, which is what is pinned here.
#[test]
fn a_recorded_vim_session_drives_the_mode_on_and_back_off() {
    const VIM: &[u8] = include_bytes!("fixtures/vim_redraw.raw");
    // The material has to be there, or this test passes by describing nothing (#554).
    let on_at = VIM
        .windows(7)
        .position(|w| w == b"[>4;2m")
        .expect("the capture must contain the request this test is about");
    assert!(
        VIM[on_at + 7..].windows(6).any(|w| w == b"[>4;m"),
        "and the clear that follows it"
    );

    let mut t = Engine::new(80, 24);
    t.feed(&VIM[..on_at + 7]);
    assert_eq!(
        enc(&t, Key::Char('i'), Modifiers::CTRL),
        b"[27;5;105~",
        "vim's own startup request must engage the mode"
    );

    t.feed(&VIM[on_at + 7..]);
    assert_eq!(
        enc(&t, Key::Char('i'), Modifiers::CTRL),
        b"	",
        "and its exit must put the keyboard back"
    );
}
