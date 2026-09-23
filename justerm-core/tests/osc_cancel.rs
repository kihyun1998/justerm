//! An OSC cancelled by `CAN` (`0x18`) or `SUB` (`0x1a`) has no effect (#970).
//!
//! `vte` ends the OSC string on either byte by dispatching it and *then* executing
//! the cancel, with `bell_terminated = false` exactly as for an `ST` ending. xterm
//! resets to ground without applying the sequence (`charproc.c:3612`, `:3640`).
//! Each case below pairs a cancelled stream with the same stream ended by `ST`, so a
//! test that passes because the arm does nothing at all cannot pass here.

use justerm_core::{Engine, TermEvent};

const CANCELS: [(&str, u8); 2] = [("CAN", 0x18), ("SUB", 0x1a)];

fn osc(body: &[u8], end: &[u8]) -> Vec<u8> {
    [b"\x1b]".as_slice(), body, end].concat()
}

/// The engine's observable reaction to one stream: events, replies, links, marks.
fn react(stream: &[u8]) -> (Vec<TermEvent>, Vec<u8>, Vec<String>, u32) {
    let mut e = Engine::new(80, 24);
    e.feed(stream);
    // Print after the OSC so an opened hyperlink would be stamped on a cell.
    e.feed(b"x");
    let frame = e.frame();
    (
        e.drain_events(),
        e.drain_replies(),
        frame.link_table,
        frame.marker_count,
    )
}

#[test]
fn every_arm_is_cancelled_and_every_arm_applies_under_st() {
    let bodies: [&[u8]; 7] = [
        b"2;title",                // window title
        b"7;file://h/p",           // cwd
        b"9;done",                 // notification
        b"52;c;aGk=",              // clipboard store
        b"11;?",                   // colour query
        b"8;;https://example.com", // hyperlink open
        b"133;A",                  // command mark
    ];
    for body in bodies {
        let applied = react(&osc(body, b"\x1b\\"));
        assert_ne!(
            applied,
            (vec![], vec![], vec![], 0),
            "{}: under ST the OSC must have an effect, or this case proves nothing",
            String::from_utf8_lossy(body)
        );
        for (label, cancel) in CANCELS {
            assert_eq!(
                react(&osc(body, &[cancel])),
                (vec![], vec![], vec![], 0),
                "{label} after `{}`: the OSC is cancelled",
                String::from_utf8_lossy(body)
            );
        }
    }
}

/// A BEL-ended OSC is complete before any later byte, so a `CAN` after it cancels
/// nothing.
#[test]
fn a_cancel_after_a_complete_osc_cancels_nothing() {
    for end in [b"\x07".as_slice(), b"\x1b\\"] {
        let mut e = Engine::new(80, 24);
        e.feed(&[osc(b"2;kept", end), vec![0x18]].concat());
        assert_eq!(e.drain_events(), vec![TermEvent::Title("kept".into())]);
    }
}

/// Every cancel byte in one feed is handled, not only the first: a stray cancel
/// and then an OSC the second cancel ends.
#[test]
fn a_second_cancel_in_the_same_feed_still_cancels() {
    for (label, cancel) in CANCELS {
        let mut e = Engine::new(80, 24);
        e.feed(&[vec![cancel], osc(b"2;x", &[cancel])].concat());
        assert_eq!(e.drain_events(), vec![], "{label}");
    }
}

/// A cancel byte outside any OSC cancels nothing, and an OSC after it in the same
/// feed applies.
#[test]
fn an_osc_after_a_stray_cancel_applies() {
    for (label, cancel) in CANCELS {
        let mut e = Engine::new(80, 24);
        e.feed(&[vec![cancel], osc(b"2;after", b"\x1b\\")].concat());
        assert_eq!(
            e.drain_events(),
            vec![TermEvent::Title("after".into())],
            "{label}"
        );
    }
}

/// An `ST` split across two `feed` calls still applies the OSC, and an OSC ended
/// by a bare `ESC` applies before the sequence that `ESC` opens.
#[test]
fn an_osc_applies_across_a_feed_boundary_and_in_stream_order() {
    let mut split = Engine::new(80, 24);
    split.feed(b"\x1b]2;split\x1b");
    assert_eq!(
        split.drain_events(),
        vec![TermEvent::Title("split".into())],
        "applied by the end of the feed that ended it"
    );
    split.feed(b"\\");
    assert_eq!(split.drain_events(), vec![]);

    let mut order = Engine::new(80, 24);
    order.feed(b"\x1b]2;first\x1b\\\x07\x1b]7;file://h/p\x1b[0m");
    assert_eq!(
        order.drain_events(),
        vec![
            TermEvent::Title("first".into()),
            TermEvent::Bell,
            TermEvent::Cwd("file://h/p".into()),
        ]
    );
}
