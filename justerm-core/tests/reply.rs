//! Engine reply-channel tests (#27): app queries → bytes the consumer writes
//! back to the PTY, drained pull-style.
//!
//! Driven through the public API — feed the query an app emits, then drain the
//! reply. Reply bytes are the VT/DEC spec (DA1/DSR/DECRQM).

use justerm_core::Engine;

#[test]
fn da1_reports_device_attributes() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[c"); // primary DA (DA1) query
    // justerm advertises VT220 (62) + ANSI colour (22) — the levels it genuinely
    // implements; it does not claim Sixel/printer/etc. it does not do.
    assert_eq!(t.drain_replies(), b"\x1b[?62;22c");
}

#[test]
fn dsr_reports_cursor_position() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5;3H"); // CUP to row 5, col 3 (1-based)
    t.feed(b"\x1b[6n"); // DSR cursor-position query
    // Reply is 1-based row;col, matching the CUP coordinates.
    assert_eq!(t.drain_replies(), b"\x1b[5;3R");
}

#[test]
fn dsr_reports_operating_status_ok() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[5n"); // DSR operating-status query
    assert_eq!(t.drain_replies(), b"\x1b[0n"); // 0n = terminal OK
}

#[test]
fn decrqm_reports_mode_state() {
    let mut t = Engine::new(80, 24);
    // ?2004 (bracketed paste) starts reset → val 2.
    t.feed(b"\x1b[?2004$p");
    assert_eq!(t.drain_replies(), b"\x1b[?2004;2$y");
    // After enabling it, the report flips to set → val 1.
    t.feed(b"\x1b[?2004h\x1b[?2004$p");
    assert_eq!(t.drain_replies(), b"\x1b[?2004;1$y");
    // A mode the engine doesn't track → not recognized → val 0.
    t.feed(b"\x1b[?9999$p");
    assert_eq!(t.drain_replies(), b"\x1b[?9999;0$y");
}

#[test]
fn decrqm_reports_the_urxvt_encoding() {
    // #51: the non-SGR encodings must be DECRQM-reportable too. With urxvt
    // (?1015) active, querying it reports set — before the fix it answered
    // "not recognized" (val 0).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1015h\x1b[?1015$p"); // enable urxvt encoding, then query it
    assert_eq!(t.drain_replies(), b"\x1b[?1015;1$y"); // set
}

#[test]
fn decrqm_reports_the_utf8_and_sgr_pixels_encodings() {
    // The other two non-SGR encodings, each set when active (#51).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1005h\x1b[?1005$p"); // UTF-8 encoding
    assert_eq!(t.drain_replies(), b"\x1b[?1005;1$y");

    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1016h\x1b[?1016$p"); // SGR-pixels encoding
    assert_eq!(t.drain_replies(), b"\x1b[?1016;1$y");
}

#[test]
fn decrqm_encoding_is_single_state_and_defaults_reset() {
    // The coordinate encoding is one-active-at-a-time, so querying an encoding
    // that is NOT the active one reports reset — mirroring the protocol axis
    // (?1000 vs ?1002). And ?1006 is unaffected by this change (#51).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1005h"); // UTF-8 active
    t.feed(b"\x1b[?1015$p\x1b[?1016$p\x1b[?1006$p"); // the other encodings
    assert_eq!(
        t.drain_replies(),
        b"\x1b[?1015;2$y\x1b[?1016;2$y\x1b[?1006;2$y" // all reset
    );

    // ?1006 still reports set when SGR is the active encoding (unchanged).
    t.feed(b"\x1b[?1006h\x1b[?1006$p");
    assert_eq!(t.drain_replies(), b"\x1b[?1006;1$y");

    // Default (X10) encoding: every encoding mode reports reset.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?1006$p\x1b[?1005$p\x1b[?1015$p\x1b[?1016$p");
    assert_eq!(
        t.drain_replies(),
        b"\x1b[?1006;2$y\x1b[?1005;2$y\x1b[?1015;2$y\x1b[?1016;2$y"
    );
}

#[test]
fn drain_empties_the_reply_buffer() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[c");
    assert_eq!(t.drain_replies(), b"\x1b[?62;22c");
    // A second drain with no new query is empty — replies are consumed, not
    // re-sent (the consumer must not write them twice).
    assert_eq!(t.drain_replies(), Vec::<u8>::new());
}

#[test]
fn unhandled_query_produces_no_reply() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[7n"); // DSR with an unsupported param
    t.feed(b"\x1b[>c"); // secondary DA (DA2) — not in this slice
    t.feed(b"plain text\r\n"); // ordinary output
    assert_eq!(t.drain_replies(), Vec::<u8>::new());
}
