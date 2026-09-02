//! #836 — an OSC reply ends with the byte the query ended with.
//!
//! The engine used to answer every colour and clipboard query with `ST`, whatever
//! arrived, and it discarded `bell_terminated` at the parser boundary — so a
//! consumer could not have echoed even if it wanted to. That made it a *mechanism*
//! gap under ADR-0017 rather than a policy justerm declined: the choice is the
//! consumer's, but a consumer cannot exercise a choice over a fact it was never
//! handed.
//!
//! **The spec settles the direction.** `ctlseqs.txt:2021` — *"XTerm accepts either
//! BEL or ST for terminating OSC sequences, and when returning information, uses
//! the same terminator used in a query."* Under ADR-0004 that outranks every
//! implementation. Three of the four references agree with it (alacritty, ghostty,
//! xterm); xterm.js is the one that always sends ST, which is what justerm did.
//!
//! **Why the terminator rides the event rather than being remembered.** On the
//! colour path all three echoing references carry it *outward with the request* —
//! alacritty binds it into the reply closure it hands its consumer, ghostty puts it
//! on the parsed command, xterm threads it as a parameter. This engine has a reason
//! of its own: `drain_events` hands over a **batch**, so two queries can be
//! outstanding at once and answered in either order, which
//! `two_outstanding_queries_each_get_their_own_terminator_back` below is the case
//! for. See [`justerm_core::Terminator`] for the one reference that *does* store it,
//! and why that strengthens this rather than contradicting it.
//!
//! ## What these assertions can and cannot observe
//!
//! Stated because a test that cannot fail reads as coverage while proving nothing.
//! Every row was run, not reasoned; the first two were wrong in the first draft of
//! this table and an adversarial pass measured them.
//!
//! | Mutation | Does this file redden? |
//! |---|---|
//! | answer always `ST` (the behaviour being fixed) | **yes** — every `_bel_` case |
//! | answer always `BEL` | **yes** — every `_st_` case, which is why both halves are here |
//! | echo from a single stored scalar rather than from the event | **yes**, and *only* `two_outstanding_queries…` — measured at test granularity |
//! | drop the terminator from **one** of the five families | **yes** — that family's row |
//! | move `#[default]` from `St` to `Bel` | **yes** — `the_default_terminator_is_st`, and only it |
//!
//! What this file still cannot observe: whether a **sixth** reply family carries a
//! terminator at all. `FAMILIES` is a `const` with no link to `TermEvent`, which is
//! `#[non_exhaustive]`, so `answer`'s wildcard arm is mandatory in an integration
//! crate and a new variant can never break it. A draft of this file claimed the
//! table *was* such a guard; a real sixth family was added and all five tests stayed
//! green. There is no cheap guard here — the enumeration is held by
//! `justerm-core/src/term.rs`'s reply sites and by review.

use justerm_core::{Engine, TermEvent, Terminator};

/// Answer whichever query event this is, handing back the terminator it carried.
/// The wildcard arm is mandatory: `TermEvent` is `#[non_exhaustive]`.
fn answer(e: &mut Engine, ev: &TermEvent) {
    match *ev {
        TermEvent::QueryForeground { terminator } => {
            e.report_foreground("rgb:ff/ff/ff", terminator)
        }
        TermEvent::QueryBackground { terminator } => {
            e.report_background("rgb:1e/1e/2e", terminator)
        }
        TermEvent::QueryCursorColor { terminator } => {
            e.report_cursor_color("rgb:ff/00/00", terminator)
        }
        TermEvent::QueryPaletteColor { index, terminator } => {
            e.report_palette_color(index, "rgb:00/00/ff", terminator)
        }
        TermEvent::QueryClipboard { target, terminator } => {
            e.report_clipboard(target, "hi", terminator)
        }
        ref other => panic!("not a query event: {other:?}"),
    }
}

/// The five OSC replies this crate queues, as `(query, reply body)`. The reply
/// *terminator* is deliberately absent — it is what the tests vary.
///
/// Five, not the four #836 was written against: `OSC 52` landed in #828 after that
/// ticket, and #828's own doc-comment routed it here rather than deciding it.
const FAMILIES: &[(&str, &str)] = &[
    ("\x1b]10;?", "\x1b]10;rgb:ff/ff/ff"),
    ("\x1b]11;?", "\x1b]11;rgb:1e/1e/2e"),
    ("\x1b]12;?", "\x1b]12;rgb:ff/00/00"),
    ("\x1b]4;5;?", "\x1b]4;5;rgb:00/00/ff"),
    ("\x1b]52;c;?", "\x1b]52;c;aGk="),
];

/// A `BEL`-terminated query is answered `BEL`-terminated, for **all five** reply
/// paths — the headline case, and the one every real capture in this repo
/// exercises. Measured over all 19 `.raw` fixtures: every OSC in every one of them
/// is BEL-terminated, none ST.
#[test]
fn a_bel_query_is_answered_bel_in_every_family() {
    for (query, body) in FAMILIES {
        let mut e = Engine::new(80, 24);
        e.feed(format!("{query}\x07").as_bytes());
        let events = e.drain_events();
        assert_eq!(events.len(), 1, "one query expected for {query:?}");
        answer(&mut e, &events[0]);
        assert_eq!(
            e.drain_replies(),
            format!("{body}\x07").as_bytes(),
            "BEL query {query:?} must be answered BEL",
        );
    }
}

/// And an `ST`-terminated query is answered `ST`-terminated. Without this half the
/// suite would pass an implementation that answered BEL unconditionally — the
/// mirror-image bug of the one being fixed.
#[test]
fn an_st_query_is_answered_st_in_every_family() {
    for (query, body) in FAMILIES {
        let mut e = Engine::new(80, 24);
        e.feed(format!("{query}\x1b\\").as_bytes());
        let events = e.drain_events();
        assert_eq!(events.len(), 1, "one query expected for {query:?}");
        answer(&mut e, &events[0]);
        assert_eq!(
            e.drain_replies(),
            format!("{body}\x1b\\").as_bytes(),
            "ST query {query:?} must be answered ST",
        );
    }
}

/// **The case that decided the shape.** Two queries with *different* terminators
/// arrive in one `feed`, drain together, and are answered in the reverse order —
/// each still gets its own terminator back.
///
/// An engine that remembered one scalar instead of putting the fact on the event
/// would have to answer both the same way. Reversing the answer order is what makes
/// that fail: answering in arrival order would let a last-one-wins store pass by
/// accident. Measured — with `report_*` reading a stored scalar instead of its
/// argument, this is the **only** test in the file that reddens.
#[test]
fn two_outstanding_queries_each_get_their_own_terminator_back() {
    let mut e = Engine::new(80, 24);
    e.feed(b"\x1b]10;?\x07\x1b]11;?\x1b\\");

    let events = e.drain_events();
    assert_eq!(
        events,
        vec![
            TermEvent::QueryForeground {
                terminator: Terminator::Bel
            },
            TermEvent::QueryBackground {
                terminator: Terminator::St
            },
        ],
        "both queries outstanding at once, each carrying what it arrived with"
    );

    // Answered in the *opposite* order to arrival.
    answer(&mut e, &events[1]);
    answer(&mut e, &events[0]);

    assert_eq!(
        e.drain_replies(),
        b"\x1b]11;rgb:1e/1e/2e\x1b\\\x1b]10;rgb:ff/ff/ff\x07",
        "the ST question gets ST and the BEL question gets BEL, whatever the answer order"
    );
}

/// A stacked sequence queries more than one slot, and every event from it carries
/// the terminator of the sequence that produced them — there is one terminator per
/// *sequence*, not per slot.
#[test]
fn every_slot_of_one_stacked_query_carries_that_sequences_terminator() {
    let mut e = Engine::new(80, 24);
    e.feed(b"\x1b]10;?;?;?\x07");
    assert_eq!(
        e.drain_events(),
        vec![
            TermEvent::QueryForeground {
                terminator: Terminator::Bel
            },
            TermEvent::QueryBackground {
                terminator: Terminator::Bel
            },
            TermEvent::QueryCursorColor {
                terminator: Terminator::Bel
            },
        ]
    );
}

/// **An OSC does not have to end with a terminator at all**, and those streams
/// resolve to `St`.
///
/// `vte` ends a string on three byte classes, not two: `BEL`, `CAN`/`SUB` (`0x18` /
/// `0x1a`, the cancel pair), and a bare `ESC` beginning the next sequence. Only the
/// first is reported as bell-terminated, so the other two produce a query the engine
/// answers with `ST` — and the answer is right: xterm hardcodes `ST` on exactly this
/// shape (`charproc.c:8964`, `unparseputc(xw, '\\'); /* should be ST */`) and
/// ghostty's `Terminator.init` returns `.st` for a missing byte.
///
/// This test exists because the first draft of this file asserted the opposite in
/// prose — *"no byte stream can reach this default"* — and an adversarial pass
/// measured three that do. A default nothing can reach needs no test; one that three
/// byte classes reach needs this.
#[test]
fn a_query_ended_without_a_terminator_is_answered_st() {
    for (label, stream) in [
        ("CAN", b"\x1b]11;?\x18".as_slice()),
        ("SUB", b"\x1b]11;?\x1a".as_slice()),
        ("bare ESC", b"\x1b]11;?\x1bc".as_slice()),
    ] {
        let mut e = Engine::new(80, 24);
        e.feed(stream);
        let events = e.drain_events();
        assert_eq!(
            events,
            vec![TermEvent::QueryBackground {
                terminator: Terminator::St
            }],
            "{label}: the query is still relayed, carrying ST"
        );
        answer(&mut e, &events[0]);
        assert_eq!(
            e.drain_replies(),
            b"\x1b]11;rgb:1e/1e/2e\x1b\\",
            "{label}: answered ST"
        );
    }
}

/// The consumer may also name a terminator with no query in hand — answering
/// unprompted is still its call, and `ST` is the value it gets by default.
#[test]
fn the_default_terminator_is_st() {
    assert_eq!(Terminator::default(), Terminator::St);

    let mut e = Engine::new(80, 24);
    e.report_background("rgb:1e/1e/2e", Terminator::default());
    assert_eq!(e.drain_replies(), b"\x1b]11;rgb:1e/1e/2e\x1b\\");
}
