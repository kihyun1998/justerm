//! #823, the real round trip. `events.rs` proves the rules the implementation
//! was written from; this proves them against bytes a real `vim` emitted.
//!
//! `fixtures/vim_title_stack.raw` is a `script(1)` recording made on the RHEL 9
//! VM (`TERM=xterm-256color`, `LC_ALL=C.UTF-8`, 24x80, `stty size` confirmed
//! inside the same invocation) of `vim` editing a two-line file and quitting.
//! `fixtures/capture-title-stack.sh` is the recorder, and it carries the one
//! condition that is easy to get wrong: RHEL 9's `xterm-256color` has no
//! `tsl`/`fsl`, so vim comes up with `title` **off** and never sets a title,
//! while still emitting the stack operations. The recording forces the option
//! on, which is the shape a consumer with a title-capable `TERM` sees.
//!
//! The whole 2560-byte stream carries these sequences, and nothing else that
//! reaches either outbound channel:
//!
//! ```text
//! @81    CSI 22;0;0t              push both
//! @139   CSI 22;2t                push the window title
//! @146   CSI 22;1t                push the icon name
//! @217   CSI 6n                   DSR — cursor position
//! @246   CSI 6n
//! @277   OSC 10 ; ?               query the foreground
//! @284   OSC 11 ; ?               query the background
//! @2157  OSC 2 ; PROBE            the title vim sets while it runs
//! @2371  OSC 2 ; Thanks for flying Vim
//! @2397  CSI 23;2t                pop the window title
//! @2404  CSI 23;1t                pop the icon name
//! @2411  CSI 22;2t                push again on the way out
//! @2418  CSI 22;1t
//! @2425  CSI 23;2t                pop
//! @2432  CSI 23;1t
//! @2485  CSI 23;0;0t              pop both
//! ```
//!
//! The four non-title sequences are not noise to be filtered out of the
//! assertions — they are what makes the assertions *exact*. A test that matched
//! only the title events would keep passing if this slice started emitting on a
//! channel it has no business touching.
//!
//! Two facts about that stream are worth stating because no synthetic fixture
//! would have supplied them. First, **every push happens before vim sets its
//! own title**, so what a pop restores is the *empty* string — which is exactly
//! how the consumer is told to go back to its own default, and why restoring an
//! empty title must not be mistaken for "nothing to do". Second, vim uses the
//! **axis-limited** forms rather than only the `;0;0` one, which is what makes
//! two independent stacks a requirement rather than a design preference.
//!
//! ## What this proves that a unit test cannot
//!
//! Before #823 this stream produced exactly **two** events — the two titles vim
//! set — and nothing for any of the six pops, so the consumer's window kept the
//! name "Thanks for flying Vim" for the rest of the session. That is the defect
//! in the issue, on the bytes that cause it.
//!
//! ## What these assertions can and cannot observe
//!
//! Stated because a capture that cannot fail reads as coverage while proving
//! nothing. Turning each part of #823 off, one at a time:
//!
//! | Mutation | Does this file redden? |
//! |---|---|
//! | ignore the axis parameter (alacritty's behaviour) | **yes** — five restores appear instead of three |
//! | let a non-zero third parameter suppress the operation | no |
//! | drop the newest rather than the oldest at the depth bound | no |
//! | change the depth bound | no |
//!
//! All three negatives are gaps in the **material**, not in the assertions, and
//! the second is the one worth naming because it looks like it should be
//! covered: every third parameter in this stream is `0`, so a rule keyed on a
//! *non-zero* one is unreachable here no matter how the assertions are written.
//! The stream also never nests more than two deep on either axis, so it cannot
//! observe a bound of ten or what happens at it. Both axes are covered by
//! `events.rs`, which is exactly the division of labour a capture is for: it
//! supplies the shapes nobody would have invented, not the edges nobody emits.

use justerm_core::{Engine, TermEvent};

const CAPTURE: &[u8] = include_bytes!("fixtures/vim_title_stack.raw");

/// The `Pv` field this capture's DA2 reply carries (#824). Deliberately a
/// second, independent copy of the arithmetic in `reply.rs` rather than a
/// shared helper: what both files are for is disagreeing with the engine, and a
/// derivation they imported from it — or from each other — could not.
fn da2_version() -> u32 {
    let v = env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap();
    let mut p = v.split('.').map(|c| c.parse::<u32>().unwrap_or(0));
    p.next().unwrap_or(0) * 10_000 + p.next().unwrap_or(0) * 100 + p.next().unwrap_or(0)
}

#[test]
fn a_real_vim_session_restores_the_title_it_pushed() {
    let mut term = Engine::new(80, 24);
    term.feed(CAPTURE);

    // The whole vector, in order: vim's two colour queries, the two titles it
    // set, then three restores — one per window-axis pop. The icon-axis pops
    // are silent because the engine has no icon-name event.
    assert_eq!(
        term.drain_events(),
        vec![
            TermEvent::QueryForeground,
            TermEvent::QueryBackground,
            TermEvent::Title("PROBE".into()),
            TermEvent::Title("Thanks for flying Vim".into()),
            TermEvent::Title(String::new()),
            TermEvent::Title(String::new()),
            TermEvent::Title(String::new()),
        ]
    );
}

#[test]
fn the_title_stack_adds_nothing_to_the_reply_channel() {
    // XTWINOPS 22/23 are state operations, not queries: nothing may go back to
    // the application. What this stream *does* reply with is the two cursor
    // position reports its two `CSI 6n` asked for, so pinning the exact bytes
    // is what catches a regression that started answering the rest of the
    // `CSI t` family — `CSI 18 t` replies, and 22/23 must not.
    //
    // The third report arrived with #824 and is the reason this expectation
    // moved: the capture carries one `CSI > c` at offset 273, so this recording
    // of a real vim asks for the *secondary* device attributes and never for
    // the primary — it contains no `CSI c` at all. Until #824 the engine
    // answered nothing, which is the asymmetry that issue is about. The 22/23
    // claim this test exists for is unchanged: every byte below is accounted
    // for by a query the stream actually made.
    let mut term = Engine::new(80, 24);
    term.feed(CAPTURE);
    let _ = term.drain_events();
    let want = format!("\x1b[2;2R\x1b[3;1R\x1b[>1;{};0c", da2_version());
    assert_eq!(term.drain_replies(), want.as_bytes());
}
