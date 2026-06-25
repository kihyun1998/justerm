//! Synchronized output tests (#73, DEC private mode ?2026). The engine only
//! *tracks and exposes* the flag (xterm.js core does the same); the consumer
//! owns the paint-hold and the spec-mandated timeout. Verified against xterm.js
//! setModePrivate (`decPrivateModes.synchronizedOutput`).

use justerm::Engine;

#[test]
fn synchronized_output_tracks_2026() {
    let mut t = Engine::new(80, 24);
    assert!(!t.synchronized_output(), "default off");
    t.feed(b"\x1b[?2026h");
    assert!(t.synchronized_output(), "?2026h opens the block");
    t.feed(b"\x1b[?2026l");
    assert!(!t.synchronized_output(), "?2026l closes the block");
}

#[test]
fn decrqm_reports_synchronized_output() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2026$p"); // off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?2026;2$y");
    t.feed(b"\x1b[?2026h\x1b[?2026$p"); // on → set
    assert_eq!(t.drain_replies(), b"\x1b[?2026;1$y");
}

#[test]
fn ris_resets_synchronized_output() {
    // A buggy app could leave the block open; RIS recovers (via the #53
    // reconstruct — the new field resets for free).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2026h"); // open block
    t.feed(b"\x1bc"); // RIS
    assert!(!t.synchronized_output(), "RIS clears the sync-output flag");
}
