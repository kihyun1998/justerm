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
//! It also carries a `CSI > c` — tmux asking DA2 — which this recording was not
//! made for and which the engine ignored until #824. That is why the reply
//! assertion below is not an empty vector.
//!
//! **The target field is empty, not `c`.** That is the fact no synthetic fixture
//! would have supplied, and the reason this recording exists: the spec reads an
//! empty field as `s0` (`ctlseqs.txt:2161`) and the issue's first draft read an
//! unrecognised target as "ignore", and *either* rule implemented literally drops
//! every copy a real multiplexer makes — green against a hand-written `52;c;…`
//! the whole way.
//!
//! **This stream is not byte-reproducible, unlike `cursor_color_nvim.raw`, and
//! the difference is worth stating rather than smoothing over.** Three
//! independent sources of variation, none of which touches the OSC 52 or
//! anything this file asserts:
//!
//! 1. **A real reordering in tmux's output** — `ESC[?25l ESC[1;1H` in one run
//!    against `ESC[1;1H ESC[?25l` in another. Swapping two 6-byte sequences that
//!    share an `ESC[` prefix differs in **8** byte positions, not 4.
//! 2. **`script(1)`'s own timestamps** — 4 bytes between two runs in the same
//!    minute, more across a minute boundary.
//! 3. **tmux's status-bar clock**, three times in the stream, 2 bytes each.
//!
//! So the total depends on when you re-record: two runs seconds apart differ in
//! ~3 bytes, two a minute apart in ~12, two across the reordering and the clock
//! in ~21. An earlier version of this note said "12 bytes, eight timestamps and
//! four reordering" — the two figures were **transposed** and the clock was
//! never counted. Corrected here because the note's whole purpose is to stop a
//! reader chasing a defect that is not there, and a wrong breakdown does the
//! opposite. What stands unchanged: "re-record and `sha256sum`" is not a check
//! this fixture can pass.
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

use justerm_core::{ClipboardTarget, Engine, TermEvent, Terminator};

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

/// The `Pv` field this capture's DA2 reply carries (#824). A third, independent
/// copy of the arithmetic, for the reason `title_stack_capture.rs` gives beside
/// the second: what these files are for is disagreeing with the engine, and a
/// derivation imported from it — or from each other — could not.
fn da2_version() -> u32 {
    let v = env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap();
    let mut p = v.split('.').map(|c| c.parse::<u32>().unwrap_or(0));
    p.next().unwrap_or(0) * 10_000 + p.next().unwrap_or(0) * 100 + p.next().unwrap_or(0)
}

/// The engine puts **no clipboard reply** on the channel by itself: this stream
/// asks no clipboard question, and one is queued only when a consumer chooses to
/// answer.
///
/// **What it does answer is DA2, and that is worth having rather than filtering
/// out.** This test asserted an *empty* channel until #824 landed, at which
/// point it went red — not because either slice was wrong, but because the
/// capture was already carrying a `CSI > c` that the engine had not yet learned
/// to answer. A recording made for one slice exercising another's path is the
/// strongest evidence either slice gets (`cursor_color_capture.rs` gained two
/// `Title("")` from #823 the same way), so the whole vector is asserted rather
/// than the clipboard's share of it.
///
/// **The positive control is the other half of the test.** Before it, the
/// emptiness assertion stayed green with the entire `OSC 52` dispatch arm
/// deleted — nothing here ever made `drain_replies()` carry a clipboard reply,
/// so "the stream asked nothing" and "the channel is broken" were the same
/// observation. Measuring a nothing is worth exactly what the instrument's
/// demonstrated ability to see a something is worth.
#[test]
fn the_captured_session_asks_no_clipboard_question() {
    let mut e = replay();
    let _ = e.drain_events();
    assert_eq!(
        e.drain_replies(),
        format!("\x1b[>1;{};0c", da2_version()).as_bytes(),
        "the stream's own replies: a DA2 answer (#824), and no clipboard reply"
    );

    // Positive control: the channel that just carried no clipboard reply can.
    e.report_clipboard(ClipboardTarget::Clipboard, "hi", Terminator::Bel);
    assert_eq!(
        e.drain_replies(),
        b"\x1b]52;c;aGk=\x07",
        "the reply channel must be able to carry one, or the assertion above proves nothing"
    );
}
