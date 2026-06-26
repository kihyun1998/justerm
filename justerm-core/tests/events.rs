//! Consumer event-surface tests (#12): OSC/BEL → drained events.
//!
//! Drive the whole path — feed the OSC/BEL bytes an app emits, then drain the
//! queue — so both `osc_dispatch`/`execute` and the pull-based queue are
//! covered. Both OSC terminators are exercised: BEL (`0x07`) and ST (`ESC \`).

use justerm_core::{Engine, TermEvent};

#[test]
fn osc2_sets_title_bel_terminated() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;hello\x07");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("hello".into())]);
}

#[test]
fn osc0_sets_title_st_terminated() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]0;world\x1b\\");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("world".into())]);
}

#[test]
fn bel_rings_bell() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x07");
    assert_eq!(term.drain_events(), vec![TermEvent::Bell]);
}

#[test]
fn osc7_reports_cwd() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]7;file://host/home/ki\x07");
    assert_eq!(
        term.drain_events(),
        vec![TermEvent::Cwd("file://host/home/ki".into())]
    );
}

#[test]
fn drain_empties_the_queue() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x07");
    assert_eq!(term.drain_events(), vec![TermEvent::Bell]);
    // A second drain with no new output is empty — events are consumed, not
    // re-reported (the pull counterpart to an ack).
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn events_preserve_stream_order() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;t1\x07\x07\x1b]7;file://h/p\x07");
    assert_eq!(
        term.drain_events(),
        vec![
            TermEvent::Title("t1".into()),
            TermEvent::Bell,
            TermEvent::Cwd("file://h/p".into()),
        ]
    );
}

#[test]
fn osc8_hyperlink_emits_no_event() {
    // OSC 8 is per-cell state (slice #26), not an event surface concern — it
    // must not produce a TermEvent here.
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]8;;https://example.com\x07linked\x1b]8;;\x07");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn printing_does_not_emit_events() {
    let mut term = Engine::new(80, 24);
    term.feed(b"plain text\r\n");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}
