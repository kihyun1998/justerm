//! What an application sends a terminal **that answers it** (#891).
//!
//! Every other capture in `fixtures/` was recorded by something that copies bytes and replies
//! to nothing, so a sequence an application sends only *after* a reply is excluded from the
//! corpus by construction — not under-sampled, absent. `vim_closed_loop.raw` was recorded
//! through `examples/reply_filter`, which is this engine plus a consumer policy, so it is the
//! first fixture here that can contain that class at all.
//!
//! These tests pin what the closed loop bought and what it did not. The pairing is the
//! evidence: the same sequence is counted here **and** across the open-loop captures, because
//! a count of ten means nothing without the zero beside it.

use justerm_core::{Engine, TermEvent};

const CLOSED_LOOP: &[u8] = include_bytes!("fixtures/vim_closed_loop.raw");

/// Every `.raw` recorded with no reply channel. Kept as a list rather than a glob so that a
/// capture added later is a deliberate entry here and not a silent change of the control.
const OPEN_LOOP: &[(&str, &[u8])] = &[
    ("vim_redraw", include_bytes!("fixtures/vim_redraw.raw")),
    (
        "vim_title_stack",
        include_bytes!("fixtures/vim_title_stack.raw"),
    ),
    (
        "alt_resize_vim.pre",
        include_bytes!("fixtures/alt_resize_vim.pre.raw"),
    ),
];

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|w| *w == needle).count()
}

/// The ten terminfo capabilities vim asks for once its DA2 question is answered, in the order
/// `#824` measured them. Held as names rather than as a count so a partial burst names which
/// half arrived.
fn xtgettcap_names(stream: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 < stream.len() {
        if &stream[i..i + 4] == b"\x1bP+q" {
            let start = i + 4;
            let mut j = start;
            while j < stream.len() && stream[j].is_ascii_hexdigit() {
                j += 1;
            }
            let hex = std::str::from_utf8(&stream[start..j]).unwrap_or("");
            let name: String = hex
                .as_bytes()
                .chunks(2)
                .filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                .map(|b| b as char)
                .collect();
            out.push(name);
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The whole issue in one assertion. The absence alone would not be evidence — a sequence can
/// be missing because nothing sends it — so the phenomenon is shown present on one side and
/// absent on the other, with the instrument the only thing that changed.
#[test]
fn the_ten_xtgettcap_questions_appear_only_once_the_loop_is_closed() {
    assert_eq!(
        xtgettcap_names(CLOSED_LOOP),
        vec!["Co", "ku", "kd", "kr", "kl", "#2", "#4", "%i", "*7", "k1"],
    );
    for (name, stream) in OPEN_LOOP {
        assert_eq!(
            count(stream, b"\x1bP+q"),
            0,
            "{name} is an open-loop capture and must not contain XTGETTCAP",
        );
        // The control: these captures *do* ask the question that gates the burst. They are
        // silent because nobody answered, not because vim never asked.
        assert!(
            count(stream, b"\x1b[>c") > 0,
            "{name} must still ask DA2, or it is not the control this test needs",
        );
    }
}

/// The reply that gated them, produced by replaying the capture through the engine. This is
/// what makes the fixture a recording of a conversation with *justerm* rather than with some
/// terminal: feeding it back produces the same answer it was recorded against.
#[test]
fn replaying_it_produces_the_da2_answer_that_gated_the_burst() {
    let mut engine = Engine::new(80, 24);
    engine.feed(CLOSED_LOOP);
    let replies = engine.drain_replies();
    assert!(
        count(&replies, b"\x1b[>1;1700;0c") == 1,
        "expected exactly one DA2 answer, got {:?}",
        String::from_utf8_lossy(&replies),
    );
}

/// The half a fixed reply table cannot do. vim asks twice from two different cells — it prints
/// a glyph and asks where the cursor ended up — so two identical answers would be a terminal
/// that drew nothing.
#[test]
fn the_two_cursor_reports_differ_because_the_position_does() {
    let mut engine = Engine::new(80, 24);
    engine.feed(CLOSED_LOOP);
    let replies = engine.drain_replies();
    let reports: Vec<&[u8]> = replies
        .split(|b| *b == 0x1b)
        .filter(|s| s.starts_with(b"[") && s.ends_with(b"R"))
        .collect();
    assert_eq!(reports.len(), 2, "vim asks DSR 6n twice in this capture");
    assert_ne!(
        reports[0], reports[1],
        "both cursor reports came back identical, which is the fixed-table failure",
    );
}

/// What the closed loop did **not** buy, kept as an assertion so it cannot quietly become
/// untrue. The ten questions are now in the corpus and this engine still answers none of them;
/// that gap is #47 tail material, and the day it is implemented this test is what says so.
#[test]
fn the_engine_still_answers_none_of_the_ten() {
    let mut engine = Engine::new(80, 24);
    engine.feed(CLOSED_LOOP);
    let replies = engine.drain_replies();
    assert_eq!(
        count(&replies, b"\x1bP"),
        0,
        "a DCS reply appeared — XTGETTCAP may now be answered; re-read this test",
    );
}

/// The colour queries are the consumer's, and the capture encodes the answer it was recorded
/// with. Replaying reproduces the *questions* as events; the answers are policy and are not
/// the engine's to reproduce (ADR-0017).
#[test]
fn the_colour_queries_arrive_as_events_for_a_consumer_to_answer() {
    let mut engine = Engine::new(80, 24);
    engine.feed(CLOSED_LOOP);
    let events = engine.drain_events();
    let queries = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                TermEvent::QueryForeground { .. } | TermEvent::QueryBackground { .. }
            )
        })
        .count();
    assert_eq!(queries, 2, "vim asks OSC 10 and OSC 11 once each here");
    assert_eq!(
        count(&engine.drain_replies(), b"\x1b]1"),
        0,
        "the engine must not answer a colour query itself",
    );
}
