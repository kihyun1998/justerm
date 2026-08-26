//! #832, the real round-trip. `dynamic_color.rs` proves the rule the tests were
//! written from; this proves it against bytes a real `nvim` actually emitted.
//!
//! `fixtures/cursor_color_nvim.raw` is a `script(1)` recording made on the RHEL
//! 9.2 VM (`TERM=xterm-256color`, `LC_ALL=C.UTF-8`, 24x80, `stty size` confirmed
//! inside the same invocation) of `nvim` 0.8.0 with a `Cursor` highlight and
//! `guicursor` configured — the setup that makes nvim touch the cursor colour at
//! all. Re-recorded twice: the two runs differ in **three bytes**, all inside
//! `script(1)`'s own timestamp header, so the VT stream itself is deterministic.
//! `fixtures/capture-cursor-color.sh` is the recorder.
//!
//! The whole stream carries exactly four OSC sequences:
//!
//! ```text
//! @147  ESC ] 11 ; ?  BEL     background query
//! @194  ESC ] 12 ;    BEL     cursor colour, EMPTY SPEC
//! @328  ESC ] 12 ;    BEL     again
//! @398  ESC ] 112     BEL     reset the cursor colour
//! ```
//!
//! **Every OSC 12 a real editor emits is the degenerate empty-spec form**, even
//! with a cursor colour explicitly configured. That is the fact no synthetic
//! fixture would have supplied, and it is why the empty-spec rule is a
//! precondition for the cursor slot rather than a hardening of it.
//!
//! ## What these assertions can and cannot observe
//!
//! Stated because a capture that cannot fail reads as coverage while proving
//! nothing. Turning each half of #832 off, one at a time:
//!
//! | Mutation | Does this file redden? |
//! |---|---|
//! | drop the empty-spec guard | **yes** — two spurious `SetCursorColor("")` appear |
//! | do not dispatch `OSC 112` | **yes** — the reset disappears |
//! | remove the cursor slot from the stack | **no** |
//!
//! The third is an honest gap in the *material*, not in the assertions: this
//! stream never carries a non-empty cursor spec, so it cannot observe the slot.
//! That axis is covered by `dynamic_color.rs` and by xterm's `misc.c:3679` loop.

use justerm_core::{Engine, TermEvent};

fn replay() -> Engine {
    let raw = include_bytes!("fixtures/cursor_color_nvim.raw");
    let mut e = Engine::new(80, 24);
    e.feed(raw);
    e
}

/// The whole real session drains to exactly two events. The two `OSC 12`s are
/// **absent** — that is the assertion, not an omission: an empty spec addresses
/// its slot and changes nothing, so a real editor session disturbs a
/// user-configured cursor colour zero times before it finally resets it.
#[test]
fn a_real_nvim_session_relays_a_reset_and_no_empty_sets() {
    let mut e = replay();
    assert_eq!(
        e.drain_events(),
        vec![TermEvent::QueryBackground, TermEvent::ResetCursorColor]
    );
}

/// The negative stated on its own, so a future change that starts relaying empty
/// specs fails on the sentence that describes the defect rather than on a vector
/// comparison. Before #832 this stream produced no cursor events at all; the
/// failure mode it now guards against is the *opposite* one — relaying two.
#[test]
fn no_event_in_a_real_session_carries_an_empty_colour_spec() {
    let mut e = replay();
    let events = e.drain_events();
    // Assert the window exists before asserting behaviour inside it: a loop over
    // an empty vector is vacuously true, so a capture that stopped producing
    // events would pass this silently instead of reporting that it had.
    assert!(!events.is_empty(), "the capture produced no events at all");
    for ev in events {
        match ev {
            TermEvent::SetCursorColor(ref s)
            | TermEvent::SetForeground(ref s)
            | TermEvent::SetBackground(ref s) => {
                assert!(!s.is_empty(), "relayed an empty colour spec: {ev:?}")
            }
            _ => {}
        }
    }
}

/// The consumer answers the background query this same stream carries, and the
/// reply reaches `drain_replies` — the second channel, proven on real bytes
/// rather than on a hand-written sequence.
///
/// The stream answers itself first, which is worth pinning: a real `nvim`
/// startup asks two questions the engine replies to on its own — the kitty
/// keyboard protocol (`CSI ? u`) and primary device attributes — so a consumer's
/// colour reply is appended to traffic that is already queued, never the first
/// thing on the channel.
#[test]
fn the_captured_query_is_answerable_through_the_reply_channel() {
    let mut e = replay();
    let _ = e.drain_events();
    assert_eq!(
        e.drain_replies(),
        b"\x1b[?0u\x1b[?62;22c",
        "the stream's own replies, before the consumer says anything"
    );

    e.report_background("rgb:1e/1e/2e");
    assert_eq!(e.drain_replies(), b"\x1b]11;rgb:1e/1e/2e\x1b\\");
}
