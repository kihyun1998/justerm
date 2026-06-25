//! Color-scheme notification tests (#85, ?2031 + ?996/?997). justerm is theme-
//! agnostic — it never knows the scheme. It tracks the ?2031 flag, relays the
//! ?996 query as an event, and formats the ?997 reply the consumer asks for.
//! Verified against xterm.js (core tracks the flag + fires a query event).

use justerm::{Engine, TermEvent};

#[test]
fn mode_2031_tracked_and_decrqm_reports_it() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2031$p"); // off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?2031;2$y");
    t.feed(b"\x1b[?2031h\x1b[?2031$p"); // on → set
    assert_eq!(t.drain_replies(), b"\x1b[?2031;1$y");
}

#[test]
fn dsr_996_emits_a_color_scheme_query() {
    // The app asks "what is the scheme?"; the theme-agnostic engine relays it as
    // an event for the consumer (which knows the scheme) to answer.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?996n"); // CSI ? 996 n
    assert_eq!(t.drain_events(), vec![TermEvent::ColorSchemeQuery]);
}

#[test]
fn report_color_scheme_queues_the_997_reply() {
    let mut t = Engine::new(80, 24);
    t.report_color_scheme(true); // dark
    assert_eq!(t.drain_replies(), b"\x1b[?997;1n");
    t.report_color_scheme(false); // light
    assert_eq!(t.drain_replies(), b"\x1b[?997;2n");
}

#[test]
fn color_scheme_updates_getter_reflects_the_flag() {
    let mut t = Engine::new(80, 24);
    assert!(!t.color_scheme_updates());
    t.feed(b"\x1b[?2031h");
    assert!(t.color_scheme_updates());
}

#[test]
fn ris_resets_color_scheme_updates() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2031h");
    t.feed(b"\x1bc"); // RIS
    assert!(!t.color_scheme_updates());
}
