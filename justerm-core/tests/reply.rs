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
    t.feed(b"\x1b[>q"); // XTVERSION — a `>`-prefixed final we deliberately do not answer
    t.feed(b"plain text\r\n"); // ordinary output
    assert_eq!(t.drain_replies(), Vec::<u8>::new());
}

// --- DA2, secondary device attributes (CSI > c) (#824) ---------------------
//
// `>` reaches `csi_dispatch` as an *intermediate*, and the dispatcher returns
// early on any intermediate it does not name — so DA2 was unreachable rather
// than unhandled, and vim could not identify the terminal. The route is opened
// for the `c` final only; every other `>`-prefixed final stays silent.

/// The version the reply must carry, derived here by arithmetic independent of
/// the engine's, so the two have to agree rather than share one expression.
fn expected_version() -> u32 {
    let v = env!("CARGO_PKG_VERSION");
    let v = v.split('-').next().unwrap(); // strip any pre-release suffix
    let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    major * 10_000 + minor * 100 + patch
}

#[test]
fn da2_reports_secondary_device_attributes() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>c"); // secondary DA (DA2) query — what vim sends
    // Pp = 1 (VT220), matching what DA1 already advertises (62 = level 2);
    // Pv = justerm's own crate version; Pc = 0, the ROM cartridge field the
    // spec fixes at zero.
    let want = format!("\x1b[>1;{};0c", expected_version());
    assert_eq!(t.drain_replies(), want.as_bytes());
}

#[test]
fn da2_answers_an_explicit_zero_first_parameter() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>0c"); // ctlseqs: "Ps = 0 or omitted -> request"
    let want = format!("\x1b[>1;{};0c", expected_version());
    assert_eq!(t.drain_replies(), want.as_bytes());
}

#[test]
fn da2_ignores_a_non_zero_first_parameter() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>1c"); // a qualifier we do not recognise
    t.feed(b"\x1b[>2c");
    assert_eq!(t.drain_replies(), Vec::<u8>::new());
}

#[test]
fn da2_version_field_tracks_the_crate_version() {
    // The engine must derive the number from its own crate version rather than
    // carry a literal, so a release cannot ship a report that disagrees with it.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[>c");
    let reply = String::from_utf8(t.drain_replies()).unwrap();
    let version = reply
        .trim_start_matches("\x1b[>1;")
        .trim_end_matches(";0c")
        .parse::<u32>()
        .expect("the version field is a single integer");
    assert_eq!(version, expected_version());
    // Padded base-100, so a higher semver always reports a higher number.
    assert!(version >= 1_00, "a released version reports at least 1_00");
}

#[test]
fn da1_still_replies_exactly_as_before() {
    // The dispatcher change opens one intermediate route; the unprefixed `c`
    // must be untouched by it.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[c");
    assert_eq!(t.drain_replies(), b"\x1b[?62;22c");
}

#[test]
fn an_unrelated_prefixed_sequence_stays_silent_and_leaves_the_screen_alone() {
    let mut t = Engine::new(80, 24);
    t.feed(b"ab");
    // XTVERSION and tertiary DA are deliberately unimplemented (#47), and both
    // reach the dispatcher through the same prefix byte DA2 now opens.
    t.feed(b"\x1b[>q");
    t.feed(b"\x1b[=c");
    t.feed(b"\x1b[>5n");
    assert_eq!(t.drain_replies(), Vec::<u8>::new());
    let grid = t.grid();
    let row: String = (0..grid.cols()).map(|c| grid.row(0)[c].c()).collect();
    assert_eq!(row.trim_end(), "ab");
}
