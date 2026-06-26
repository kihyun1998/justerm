//! Application-keypad mode tests (#74, DECNKM ?66 / DECKPAM ESC = / DECKPNM
//! ESC >). The flag is tracked and DECRQM-reportable but NOT acted on in key
//! encoding — matching xterm.js, whose keyboard handler never reads
//! `applicationKeypad`. The observable is therefore the DECRQM reply.

use justerm_core::Engine;

#[test]
fn dec_private_66_sets_application_keypad() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?66$p"); // default off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?66;2$y");
    t.feed(b"\x1b[?66h\x1b[?66$p"); // ?66h → set
    assert_eq!(t.drain_replies(), b"\x1b[?66;1$y");
}

#[test]
fn deckpam_and_deckpnm_drive_the_same_flag() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b="); // DECKPAM (ESC =) → application keypad on
    t.feed(b"\x1b[?66$p");
    assert_eq!(t.drain_replies(), b"\x1b[?66;1$y"); // set
    t.feed(b"\x1b>"); // DECKPNM (ESC >) → numeric keypad (off)
    t.feed(b"\x1b[?66$p");
    assert_eq!(t.drain_replies(), b"\x1b[?66;2$y"); // reset
}

#[test]
fn ris_resets_application_keypad() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b="); // on
    t.feed(b"\x1bc"); // RIS
    t.feed(b"\x1b[?66$p");
    assert_eq!(t.drain_replies(), b"\x1b[?66;2$y"); // reset
}
