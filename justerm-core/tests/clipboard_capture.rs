//! #828, the real round-trip. `clipboard.rs` proves the rules the tests were
//! written from; this proves the one that matters against bytes a real `tmux`
//! actually emitted.
//!
//! `fixtures/tmux_clipboard.raw` is a `script(1)` recording made on the RHEL 9.2
//! VM (`TERM=xterm-256color`, `LC_ALL=C.UTF-8`, 24x80, `stty size` confirmed
//! inside the same invocation) of `tmux` 3.2a running `set-clipboard on` and
//! `set-buffer -w HELLOJUSTERM`. `fixtures/capture-clipboard.sh` is the recorder.
//!
//! The whole 1278-byte stream carries exactly one OSC 52:
//!
//! ```text
//! @1059  ESC ] 52 ; ; SEVMTE9KVVNURVJN  BEL
//! ```
//!
//! **The target field is empty, not `c`.** That is the fact no synthetic fixture
//! would have supplied, and the reason this recording exists: the spec reads an
//! empty field as `s0` (`ctlseqs.txt:2161`) and the issue's first draft read an
//! unrecognised target as "ignore", and *either* rule implemented literally drops
//! every copy a real multiplexer makes — green against a hand-written `52;c;…`
//! the whole way.
//!
//! **This stream is not byte-reproducible, unlike `cursor_color_nvim.raw`, and
//! the difference is worth stating rather than smoothing over.** Two recordings
//! made minutes apart differ in 12 bytes: eight are `script(1)`'s own timestamps,
//! and **four are a real reordering in tmux's output** — `ESC[?25l ESC[1;1H` in
//! one run against `ESC[1;1H ESC[?25l` in the other. Neither touches the OSC 52
//! or anything this file asserts, but "re-record and `sha256sum`" is not a check
//! this fixture can pass, so nobody should try it and conclude the transfer
//! broke.
//!
//! ## What these assertions can and cannot observe
//!
//! Stated because a capture that cannot fail reads as coverage while proving
//! nothing. Turning each part of #828 off, one at a time, and re-running **this
//! file**:
//!
//! | Mutation | Does this file redden? |
//! |---|---|
//! | empty target no longer defaults to the clipboard | **yes** — the store disappears |
//! | unknown target folded onto the clipboard instead of dropped | no |
//! | payload read as `params[2]` rather than `params[2..]` rejoined | no |
//! | the size bound removed | no |
//! | non-UTF-8 decoded lossily instead of refused | no |
//!
//! Four of the five are honest gaps in the *material*, not in the assertions:
//! one well-formed ASCII payload with an empty target cannot exercise a bad
//! target, a split payload, a huge payload or invalid UTF-8. Those axes are
//! covered by `clipboard.rs`, whose cases are synthetic precisely because a real
//! emitter never produces them. What this file uniquely observes is the first
//! row — and that is the row the whole slice turns on.

use justerm_core::{ClipboardTarget, Engine, TermEvent};

fn replay() -> Engine {
    let raw = include_bytes!("fixtures/tmux_clipboard.raw");
    let mut e = Engine::new(80, 24);
    e.feed(raw);
    e
}

/// The whole real session drains to exactly two events. The `Title("")` is
/// tmux's title-stack pop restoring a title it never set (#823) — a capture
/// recorded for one slice exercising another's path, which is the strongest
/// evidence either slice gets, so the vector is asserted whole rather than
/// filtered.
#[test]
fn a_real_tmux_copy_reaches_the_clipboard() {
    let mut e = replay();
    assert_eq!(
        e.drain_events(),
        vec![
            TermEvent::ClipboardStore {
                target: ClipboardTarget::Clipboard,
                text: "HELLOJUSTERM".into(),
            },
            TermEvent::Title(String::new()),
        ]
    );
}

/// The negative stated on its own, so a change that starts dropping the real
/// form fails on the sentence describing the defect rather than on a vector
/// comparison. Before this slice the stream produced **no** clipboard event at
/// all, which is exactly what the issue measured.
#[test]
fn the_real_form_is_not_read_as_an_unrecognised_target() {
    let mut e = replay();
    let events = e.drain_events();
    // Assert the window exists before asserting inside it: a filter over an
    // empty vector is vacuously satisfied, so a capture that stopped producing
    // events would pass this silently instead of reporting that it had.
    assert!(!events.is_empty(), "the capture produced no events at all");
    let stores: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            TermEvent::ClipboardStore { target, text } => Some((target, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        stores,
        vec![(&ClipboardTarget::Clipboard, "HELLOJUSTERM")],
        "the empty target field a real tmux sends must mean the clipboard"
    );
}

/// The engine answers nothing on its own, on real bytes: this stream asks no
/// clipboard question, and the reply channel stays empty until a consumer
/// chooses to say something. The store above did not put anything on it either,
/// which is the half a synthetic test states and a real one confirms.
#[test]
fn the_captured_session_puts_nothing_on_the_reply_channel() {
    let mut e = replay();
    let _ = e.drain_events();
    assert_eq!(e.drain_replies(), b"");
}
