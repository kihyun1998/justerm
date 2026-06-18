//! Engine reply-channel tests (#27): app queries → bytes the consumer writes
//! back to the PTY, drained pull-style.
//!
//! Driven through the public API — feed the query an app emits, then drain the
//! reply. Reply bytes are the VT/DEC spec (DA1/DSR/DECRQM).

use justerm::Engine;

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
