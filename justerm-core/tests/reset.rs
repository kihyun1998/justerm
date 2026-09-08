//! RIS / DECSTR reset tests (#53). Scope verified against xterm.js InputHandler:
//! RIS (ESC c) is a full re-init; DECSTR (CSI ! p) is a soft subset that does
//! not destroy content or reset the mouse, and turns autowrap back ON.

use justerm_core::{
    Engine, Modifiers, MouseAction, MouseButton, MouseEvent, TermDamage, TermEvent,
};

/// Redefine every colour slot a consumer holds, and drain the announcements —
/// leaving the engine in the state #835 is about: the consumer's palette differs
/// from its theme and only the consumer knows it.
fn redefine_every_colour_slot(t: &mut Engine) {
    t.feed(b"\x1b]4;1;#ff0000\x07"); // an indexed entry
    t.feed(b"\x1b]10;#ff0000\x07"); // default foreground
    t.feed(b"\x1b]11;#ff0000\x07"); // default background
    t.feed(b"\x1b]12;#ff0000\x07"); // cursor
    assert_eq!(t.drain_events().len(), 4, "precondition: all four were set");
}

fn left_press() -> MouseEvent {
    MouseEvent {
        button: Some(MouseButton::Left),
        action: MouseAction::Press,
        col: 0,
        row: 0,
        px: 0,
        py: 0,
        mods: Modifiers::empty(),
    }
}

#[test]
fn ris_recovers_stuck_mouse_tracking() {
    // The motivating symptom: an app enabled mouse tracking and never disabled
    // it; RIS (ESC c) brings the terminal back to power-on, so encode_mouse
    // reports nothing again.
    let mut t = Engine::new(10, 3);
    t.feed(b"\x1b[?1000h\x1b[?1006h"); // mouse tracking + SGR encoding on
    assert!(t.encode_mouse(left_press()).is_some()); // precondition: tracking on
    t.feed(b"\x1bc"); // RIS
    assert!(
        t.encode_mouse(left_press()).is_none(),
        "RIS must reset mouse tracking to Off",
    );
}

#[test]
fn ris_clears_screen_scrollback_and_homes_cursor() {
    let mut t = Engine::new(4, 2);
    t.feed(b"a\r\nb\r\nc\r\nd"); // build scrollback + screen content
    assert!(t.scrollback_len() > 0);
    t.feed(b"\x1bc"); // RIS
    assert_eq!(t.scrollback_len(), 0, "scrollback cleared");
    assert_eq!(t.grid().cell(0, 0).c(), ' ', "screen cleared");
    t.feed(b"X"); // cursor is home → lands at (0,0)
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
}

#[test]
fn ris_preserves_replies_queued_before_it() {
    // A DA reply queued earlier in the same feed must survive the reset — the
    // outbound queue is consumer-bound output, not terminal state (#53).
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[c\x1bc"); // DA1 query (queues a reply), then RIS
    assert_eq!(t.drain_replies(), b"\x1b[?62;22c");
}

#[test]
fn ris_signals_full_damage() {
    let mut t = Engine::new(4, 2);
    t.feed(b"abc");
    t.reset_damage(); // ack
    t.feed(b"\x1bc"); // RIS clears the screen → consumer must repaint
    assert!(matches!(t.damage(), TermDamage::Full));
}

#[test]
fn decstr_keeps_screen_and_does_not_reset_mouse() {
    // DECSTR is a soft reset: content stays, and (unlike RIS) it does NOT touch
    // the mouse — so a stuck mouse is only recovered by RIS, never DECSTR.
    let mut t = Engine::new(5, 2);
    t.feed(b"hi"); // screen content
    t.feed(b"\x1b[?1000h\x1b[?1006h"); // mouse on
    t.feed(b"\x1b[!p"); // DECSTR
    assert_eq!(
        t.grid().cell(0, 0).c(),
        'h',
        "DECSTR must not clear the screen"
    );
    assert_eq!(t.grid().cell(0, 1).c(), 'i');
    assert!(
        t.encode_mouse(left_press()).is_some(),
        "DECSTR must NOT reset mouse tracking (only RIS does)",
    );
}

#[test]
fn decstr_turns_autowrap_back_on() {
    // The xterm quirk: DECSTR resets autowrap to ON, not the VT100 "off". Source-
    // verified against xterm.js (CoreService default `wraparound: true`).
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b[?7l"); // autowrap off
    t.feed(b"\x1b[!p"); // DECSTR
    t.feed(b"\x1b[?7$p"); // query DECAWM
    assert_eq!(t.drain_replies(), b"\x1b[?7;1$y"); // set (on)
}

#[test]
fn decstr_keeps_the_active_cursor_position() {
    // Only the saved (DECSC) cursor homes; the active cursor stays put.
    let mut t = Engine::new(10, 2);
    t.feed(b"hi"); // cursor at col 2
    t.feed(b"\x1b[!p"); // DECSTR
    t.feed(b"X"); // lands at col 2, not home
    assert_eq!(t.grid().cell(0, 2).c(), 'X');
}

#[test]
fn ris_returns_from_alt_screen_to_primary() {
    // The alt screen has no scrollback; if RIS left us on it, scrolling would
    // accrue none. After RIS we are back on the primary, which accrues.
    let mut t = Engine::new(4, 2);
    t.feed(b"\x1b[?1049h"); // enter alt screen
    t.feed(b"\x1bc"); // RIS
    t.feed(b"a\r\nb\r\nc\r\nd"); // scroll on the (post-reset) screen
    assert!(
        t.scrollback_len() > 0,
        "RIS must return to the primary screen"
    );
}

#[test]
fn decstr_resets_insert_mode_to_replace() {
    let mut t = Engine::new(5, 1);
    t.feed(b"abc\x1b[1;1H");
    t.feed(b"\x1b[4h"); // insert mode on
    t.feed(b"\x1b[!p"); // DECSTR → replace
    t.feed(b"X");
    assert_eq!(t.grid().cell(0, 1).c(), 'b'); // overwritten, not shifted
}

/// **RIS announces nothing about the palette, and the silence is the decision
/// (#835).** The engine is theme-agnostic, so the consumer's palette is the only
/// copy — which is exactly why an announcement would have to come from here if it
/// came at all. It does not: xterm is alone in resetting its own palette on a
/// reset (`charproc.c:14366`), ADR-0004's tie-breaker does not reach the question
/// (no DEC text governs a table DEC never defined), and terminfo settles the
/// reach — see the sibling test below.
///
/// This test pins an **absence**, so it is worth saying what makes it able to
/// fail: pushing any of the four `Reset*` events onto `full_reset`'s queue reddens
/// the assertion directly, and the `drain_events` in the helper is what stops the
/// four *set* events satisfying it vacuously.
#[test]
fn ris_announces_nothing_about_the_palette() {
    let mut t = Engine::new(10, 3);
    redefine_every_colour_slot(&mut t);
    t.feed(b"\x1bc"); // RIS
    assert_eq!(
        t.drain_events(),
        vec![],
        "RIS must not announce a palette or dynamic-colour reset (#835)",
    );
}

/// DECSTR likewise — and this is the half that surprises, because xterm resets
/// the palette on the *soft* reset too: its `if_OPT_ISO_COLORS` block sits above
/// the `if (full)` split, so `CSI ! p` takes it as well. The soft reset is
/// normally the weaker one, and "what each strength does **not** clear is the part
/// that matters" is this territory's stated model (#835).
#[test]
fn decstr_announces_nothing_about_the_palette() {
    let mut t = Engine::new(10, 3);
    redefine_every_colour_slot(&mut t);
    t.feed(b"\x1b[!p"); // DECSTR
    assert_eq!(
        t.drain_events(),
        vec![],
        "DECSTR must not announce a palette or dynamic-colour reset (#835)",
    );
}

/// **The reachable case, and why the two tests above cost a consumer nothing.**
/// These are the bytes `tput reset` actually emits under `TERM=xterm-256color`,
/// captured on the RHEL 9 VM rather than composed here — terminfo's `rs1` is
/// `\Ec\E]104\007`, i.e. RIS with an **explicit** palette reset appended, and
/// `rs2` supplies the DECSTR that follows. `linux` spells its own the same way
/// (`rs1=\Ec\E]R`).
///
/// Two things follow. The palette reset an application actually performs already
/// arrives as `OSC 104` and is already relayed, so the silence above has no
/// measured reach; and xterm's *own* terminfo entry appending `\E]104\a` after
/// `\Ec` is evidence from outside any implementation that `RIS` is not taken to
/// imply one.
///
/// **What this test does *not* prove**, stated because the first draft claimed it
/// did: the `OSC 104` here is announced *after* `full_reset` has returned, so the
/// wholesale rebuild's carrying of the event queue is never exercised — replacing
/// the carried queue with an empty `Vec` leaves this whole file green. The
/// guarantee is real and is pinned one file over, by
/// `clipboard.rs::a_reset_between_a_store_and_its_drain_keeps_the_store`, which is
/// the only test in the workspace that reddens for it.
///
/// The whole captured line is fed rather than the interesting prefix, and the
/// assertion is on **everything** it announces — which is how the second event
/// below got here rather than being predicted: `rs2`'s `\E[?3l` is DECCOLM, so a
/// consumer of the shipped reset string is told about a column-mode change too.
/// Trimming the input to the bytes under discussion would have hidden that and
/// cost the test its claim to be a capture.
#[test]
fn ris_then_osc104_is_the_reset_string_that_ships() {
    let mut t = Engine::new(10, 3);
    redefine_every_colour_slot(&mut t);
    t.feed(b"\x1bc\x1b]104\x07\x1b[!p\x1b[?3;4l\x1b[4l\x1b>\x1b[?69l");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::ResetPaletteColor(None),
            TermEvent::ColumnMode { cols: 80 },
        ],
        "the shipped reset string carries its own palette reset, and it must survive RIS",
    );
}
