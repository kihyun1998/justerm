//! What an application sends a terminal **that answers it** (#891).
//!
//! Every other capture in `fixtures/` was recorded by something that copies bytes and replies
//! to nothing, so a sequence an application sends only *after* a reply is excluded from the
//! corpus by construction — not under-sampled, absent. `vim_closed_loop.raw` was recorded
//! through `examples/reply_filter`, which is this engine plus a consumer policy, so it is the
//! first fixture here that can contain that class at all.
//!
//! **What it was recorded against**, because the bytes are a function of it and not of vim
//! alone. RHEL 9.2, vim 8.2 (patches 1-2637), `TERM=xterm-256color`, 80x24, `vim -X -i NONE`,
//! no `COLORTERM`. The engine answered DA1 / DA2 / DSR / DECRQM / the kitty query itself; the
//! **consumer policy** in `examples/reply_filter` answered the six query families that reach a
//! consumer (ADR-0017) — foreground `rgb:c7c7/c7c7/c7c7`, background `rgb:0000/0000/0000` (so
//! vim reads the scheme as dark), palette queries the same flat spec, an OSC 52 clipboard read
//! refused in silence. Of those six only OSC 10 and OSC 11 are actually exercised by this
//! recording; the other four arms are present and unreached. A different policy is a different
//! recording: a white background flips vim's own `&background` to `light`. Re-record with
//! `fixtures/capture-closed-loop.sh`, which refuses a capture that does not reproduce three
//! times **and** refuses one that reproduces while holding none of the material.
//!
//! ## What these assertions can and cannot observe
//!
//! Stated because a capture that cannot fail reads as coverage while proving nothing.
//!
//! - **`the_ten_xtgettcap_questions…` constructs no `Engine`.** It is a guard on the *corpus*,
//!   not on the code: no change under `src/` can redden it. What it catches is a capture being
//!   added to `OPEN_LOOP` that is not in fact open-loop, and a `.raw` being edited.
//! - **Nothing here records what the harness replied.** The fixture is the pty's *output* only,
//!   so these tests observe *this engine's* answers during replay — which are a function of the
//!   recorded bytes. They cannot prove the original recording was made against a live engine
//!   rather than a fixed table. That claim rests on the capture script, not on this file.
//! - **The four `Engine` tests do observe code**: the DA2 reply and its version arithmetic,
//!   cursor tracking through DSR 6n, that no DCS reply is queued, and that a colour query
//!   routes to an event instead of being answered in the engine.

use justerm_core::{Engine, TermEvent};

const CLOSED_LOOP: &[u8] = include_bytes!("fixtures/vim_closed_loop.raw");

/// Every `.raw` that asks DA2 with no reply channel — the complete control, not a sample.
/// Kept as a list rather than a glob so a capture added later is a deliberate entry here and
/// not a silent change of the control.
///
/// `vim_redraw` is recorded `vim -u NONE -N` (`fixtures/capture-dogfood.sh`). `-u NONE` alone
/// would be a confound, because it implies 'compatible' and a compatible vim does not probe the
/// terminal at all — but `-N` puts that back, measured: with the loop closed that exact flag set
/// still yields all ten. Its zero is the open loop's doing. `tmux_clipboard` is here because it
/// is the only non-vim member, so the control is about terminals that answer rather than about
/// one application's defaults.
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
    (
        "tmux_clipboard",
        include_bytes!("fixtures/tmux_clipboard.raw"),
    ),
];

/// The `Pv` field this capture's DA2 reply carries (#824). Deliberately a third, independent
/// copy of the arithmetic rather than a shared helper — what these files are for is disagreeing
/// with the engine, and a derivation imported from it could not.
fn da2_version() -> u32 {
    let v = env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap();
    let mut p = v.split('.').map(|c| c.parse::<u32>().unwrap_or(0));
    p.next().unwrap_or(0) * 10_000 + p.next().unwrap_or(0) * 100 + p.next().unwrap_or(0)
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// The terminfo capabilities the stream asks for, decoded from hex, **deduplicated and sorted**.
/// Which set arrives is stable; the order and the multiplicity are not — `term.rs`'s DA2 block
/// measured one arm sending each capability once where another sent it twice — so pinning those
/// would be a stronger claim than this project's own measurement supports.
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
            let name: String = stream[start..j]
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
    out.sort();
    out.dedup();
    out
}

/// The whole issue in one assertion. The absence alone would not be evidence — a sequence can
/// be missing because nothing sends it — so the phenomenon is shown present on one side and
/// absent on the other, with the reply channel the only thing that changed.
#[test]
fn the_ten_xtgettcap_questions_appear_only_once_the_loop_is_closed() {
    assert_eq!(
        xtgettcap_names(CLOSED_LOOP),
        vec!["#2", "#4", "%i", "*7", "Co", "k1", "kd", "kl", "kr", "ku"],
    );
    for (name, stream) in OPEN_LOOP {
        assert_eq!(
            count(stream, b"\x1bP+q"),
            0,
            "{name} is an open-loop capture and must not contain XTGETTCAP",
        );
        // The control: these captures *do* ask the question that gates the burst. They are
        // silent because nobody answered, not because nobody asked.
        assert!(
            count(stream, b"\x1b[>c") > 0,
            "{name} must still ask DA2, or it is not the control this test needs",
        );
    }
}

/// The reply that gated them, produced by replaying the capture through the engine.
#[test]
fn replaying_it_produces_the_da2_answer_that_gated_the_burst() {
    let mut engine = Engine::new(80, 24);
    engine.feed(CLOSED_LOOP);
    let replies = engine.drain_replies();
    let expected = format!("\x1b[>1;{};0c", da2_version());
    assert_eq!(
        count(&replies, expected.as_bytes()),
        1,
        "expected exactly one DA2 answer {expected:?}, got {:?}",
        String::from_utf8_lossy(&replies),
    );
}

/// The half a fixed reply table could not have produced. vim prints U+25BD at row 2 column 1
/// and asks where the cursor ended up — `2;2R` says the glyph took **one** cell, which is the
/// ambiguous-width answer it is actually after. Then it throws an unknown DCS and an unknown
/// CSI at row 3 column 1 and asks again; `3;1R` says both were consumed as sequences rather
/// than printed as text. Two identical answers would be a terminal that drew nothing.
#[test]
fn the_two_cursor_reports_answer_vims_two_probes() {
    let mut engine = Engine::new(80, 24);
    engine.feed(CLOSED_LOOP);
    let replies = engine.drain_replies();
    let reports: Vec<Vec<u8>> = replies
        .split(|b| *b == 0x1b)
        .filter(|s| s.starts_with(b"[") && s.ends_with(b"R"))
        .map(|s| s.to_vec())
        .collect();
    assert_eq!(
        reports,
        vec![b"[2;2R".to_vec(), b"[3;1R".to_vec()],
        "the cursor reports are the probe answers, not just two different strings",
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

/// The colour queries are the consumer's. Replaying reproduces the *questions* as events; the
/// answers are policy and are not the engine's to reproduce (ADR-0017).
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
