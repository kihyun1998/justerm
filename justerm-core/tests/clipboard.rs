//! #828 OSC 52 — an application asking to put text on the system clipboard, or
//! to read one back, relayed as `TermEvent`s for the consumer to honour or
//! refuse. The engine's half is mechanism only: recognise the sequence, resolve
//! the target, decode the base64 inbound and encode it outbound. It never
//! touches a clipboard, holds no clipboard state, and carries no allow/deny knob
//! — under ADR-0017 that policy is the consumer's, and dropping the event *is*
//! the refusal. Same `Query…` + `report_…` shape as OSC 4/10/11/12, which is why
//! `dynamic_color.rs` next door is this file's model.
//!
//! Every assertion is on what a consumer can see — a drained event, or the exact
//! reply bytes — and none reaches into the decoder. The reply *is* the contract,
//! so it is asserted byte for byte.

use justerm_core::{ClipboardTarget, Engine, TermEvent};

/// The bytes `tmux` 3.2a was measured emitting on the RHEL 9 VM under
/// `TERM=xterm-256color` with `set-clipboard on`, copying `HELLOJUSTERM` out of
/// copy-mode (#828, second comment). The only OSC 52 emission this project has
/// observed in the wild, and the reason the target rule below is what it is.
const TMUX_COPY: &[u8] = b"\x1b]52;;SEVMTE9KVVNURVJN\x07";

/// The same session's `tmux set-buffer -w` path, one payload over.
const TMUX_SET_BUFFER: &[u8] = b"\x1b]52;;SlVTVEVSTVBST0JF\x07";

/// **The first case, and it is a real capture rather than a synthesised
/// `52;c;…`.** The target field tmux sends is *empty*, and a rule that read an
/// unrecognised target as "ignore" would have shipped green against `52;c;…`
/// and still dropped every copy a real multiplexer makes.
///
/// The spec says an empty field means `s0` — the configurable primary/clipboard
/// selection plus cut-buffer 0 (`ctlseqs.txt:2161`) — and neither half is
/// representable here: `s` is resolved by a user resource, which is consumer
/// policy under ADR-0017, and cut buffers are not modelled. Of the two readings
/// this engine *can* mean, ghostty, alacritty and `vte` itself all say
/// clipboard, and so does the intent of the only emitter measured.
#[test]
fn a_store_with_an_empty_target_reaches_the_clipboard() {
    let mut t = Engine::new(80, 24);
    t.feed(TMUX_COPY);
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: "HELLOJUSTERM".into(),
        }]
    );

    let mut t = Engine::new(80, 24);
    t.feed(TMUX_SET_BUFFER);
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: "JUSTERMPROBE".into(),
        }]
    );
}

/// The query half of the same form. ghostty pins precisely this input
/// (`clipboard_operation.zig:64`), and a shell probe that asks with `52;;?`
/// would otherwise get silence.
#[test]
fn a_query_with_an_empty_target_is_a_clipboard_query() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;;?\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::QueryClipboard {
            target: ClipboardTarget::Clipboard,
        }]
    );
}

/// The explicit form, and the ST terminator — both are accepted inbound, since
/// `vte` normalises the two before the payload reaches us.
#[test]
fn an_explicit_clipboard_target_stores_under_either_terminator() {
    for bytes in [&b"\x1b]52;c;aGk=\x07"[..], &b"\x1b]52;c;aGk=\x1b\\"[..]] {
        let mut t = Engine::new(80, 24);
        t.feed(bytes);
        assert_eq!(
            t.drain_events(),
            vec![TermEvent::ClipboardStore {
                target: ClipboardTarget::Clipboard,
                text: "hi".into(),
            }],
            "{bytes:?}"
        );
    }
}

/// `p` and `s` are **different targets**, not one. The spec lists them
/// separately (`ctlseqs.txt:2157`) and xterm binds them to different atoms
/// (`misc.c:3327`); `s` is *"the configurable primary/clipboard selection"*,
/// which is a setting, so the engine relays the application's choice and the
/// consumer resolves it.
///
/// An earlier draft collapsed them the way alacritty does
/// (`alacritty_terminal/src/term/mod.rs:1713`). That is safe for alacritty
/// because it replies with the raw byte it was sent (`:1744`) and safe for
/// nobody who replies from the collapsed value — see the reply test below,
/// which is the assertion that actually catches it.
#[test]
fn p_and_s_are_different_targets() {
    for (field, expected) in [
        ("p", ClipboardTarget::Primary),
        ("s", ClipboardTarget::Selection),
    ] {
        let mut t = Engine::new(80, 24);
        t.feed(format!("\x1b]52;{field};aGk=\x07").as_bytes());
        assert_eq!(
            t.drain_events(),
            vec![TermEvent::ClipboardStore {
                target: expected,
                text: "hi".into(),
            }],
            "target field {field:?}"
        );
    }
}

/// A query relays a query and **nothing else** — in particular not a store of
/// the literal text `?`, which is what a handler that decoded first would do.
#[test]
fn a_query_relays_no_store() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;?\x07");
    let events = t.drain_events();
    assert_eq!(
        events,
        vec![TermEvent::QueryClipboard {
            target: ClipboardTarget::Clipboard,
        }]
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, TermEvent::ClipboardStore { .. })),
        "a query must not also look like a store"
    );
}

/// **Nothing is on the reply channel until the consumer answers.** This is the
/// security property stated as an assertion: an engine that answered a query
/// itself would be leaking state it should not hold, and a consumer refuses a
/// clipboard *read* — independently of whether it honours *writes* — simply by
/// not calling `report_clipboard`.
#[test]
fn a_query_alone_queues_no_reply() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;?\x07");
    assert_eq!(
        t.drain_replies(),
        b"",
        "the engine has no clipboard to answer with"
    );
}

/// The reply *is* the contract, so it is asserted byte for byte: the consumer
/// hands over plain text and gets well-formed bytes back.
#[test]
fn the_report_method_encodes_a_well_formed_reply() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;?\x07");
    t.drain_events();
    t.report_clipboard(ClipboardTarget::Clipboard, "hi");
    assert_eq!(t.drain_replies(), b"\x1b]52;c;aGk=\x1b\\");
}

/// **The selector round-trips: the reply names the field the application
/// wrote.** This is the assertion that failed on the first draft, which
/// collapsed `p` and `s` and so answered `ESC ] 52 ; s ; ?` naming `p` — a
/// selector the application never sent, in the one field a client can pair a
/// reply on. Every reference echoes it: xterm the recognised list
/// (`misc.c:3384`), alacritty the raw byte (`…/term/mod.rs:1744`), ghostty its
/// three locations (`src/Surface.zig:5954`).
#[test]
fn the_reply_names_the_selector_the_application_wrote() {
    for (field, target) in [
        ("c", ClipboardTarget::Clipboard),
        ("p", ClipboardTarget::Primary),
        ("s", ClipboardTarget::Selection),
    ] {
        let mut t = Engine::new(80, 24);
        t.feed(format!("\x1b]52;{field};?\x07").as_bytes());
        let events = t.drain_events();
        let [TermEvent::QueryClipboard { target: relayed }] = events.as_slice() else {
            panic!("expected one clipboard query for {field:?}, got {events:?}");
        };
        assert_eq!(*relayed, target, "target field {field:?}");
        // The consumer answers with exactly what it was handed — no lookup, no
        // remembered byte — and the selector comes back out unchanged.
        t.report_clipboard(*relayed, "hi");
        assert_eq!(
            t.drain_replies(),
            format!("\x1b]52;{field};aGk=\x1b\\").as_bytes(),
            "reply for target field {field:?}"
        );
    }
}

/// A query with an empty target is answered naming `c`, which is the same thing
/// alacritty sends once `vte` has defaulted the field — the reply says what the
/// engine understood, and what it understood is the clipboard.
#[test]
fn an_empty_target_query_is_answered_naming_the_clipboard() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;;?\x07");
    let events = t.drain_events();
    let TermEvent::QueryClipboard { target } = events[0] else {
        panic!("expected a clipboard query, got {events:?}");
    };
    t.report_clipboard(target, "hi");
    assert_eq!(t.drain_replies(), b"\x1b]52;c;aGk=\x1b\\");
}

/// **The round trip that a hand-rolled base64 or an encoding assumption breaks
/// while every ASCII case stays green.** Text in, decoded on the way in,
/// re-encoded on the way out, and the payload that comes back is the one that
/// went in.
#[test]
fn non_ascii_text_survives_decode_and_re_encode() {
    const TEXT: &str = "héllo — 안녕 ✓";
    const PAYLOAD: &str = "aMOpbGxvIOKAlCDslYjrhZUg4pyT";

    let mut t = Engine::new(80, 24);
    t.feed(format!("\x1b]52;c;{PAYLOAD}\x07").as_bytes());
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: TEXT.into(),
        }]
    );

    t.report_clipboard(ClipboardTarget::Clipboard, TEXT);
    assert_eq!(
        t.drain_replies(),
        format!("\x1b]52;c;{PAYLOAD}\x1b\\").as_bytes()
    );
}

/// An empty payload is a store of the empty string — which is how the sequence
/// *clears* a selection, and it needs no rule of its own: an empty payload is a
/// well-formed encoding of no bytes, so it arrives through the ordinary path.
/// Both the spec and xterm end that exchange with an empty selection
/// (`ctlseqs.txt:2174`, `misc.c:3410`); ghostty pins the same input under a test
/// named *"clear clipboard"* (`clipboard_operation.zig:93`).
#[test]
fn an_empty_payload_is_a_store_of_the_empty_string() {
    for bytes in [&b"\x1b]52;c;\x07"[..], &b"\x1b]52;;\x07"[..]] {
        let mut t = Engine::new(80, 24);
        t.feed(bytes);
        assert_eq!(
            t.drain_events(),
            vec![TermEvent::ClipboardStore {
                target: ClipboardTarget::Clipboard,
                text: String::new(),
            }],
            "{bytes:?}"
        );
    }
}

/// A missing payload *field* is not an empty payload, and the two are one
/// character apart. `OSC 52 ; c` names a target and asks for nothing; xterm's
/// whole handler sits inside `if (*buf == ';')` (`misc.c:3353`) so it does
/// nothing at all, and ghostty rejects the form outright
/// (`clipboard_operation.zig:20`).
#[test]
fn a_missing_payload_field_is_not_an_empty_payload() {
    for bytes in [
        &b"\x1b]52;c\x07"[..],
        &b"\x1b]52;\x07"[..],
        &b"\x1b]52\x07"[..],
    ] {
        let mut t = Engine::new(80, 24);
        t.feed(bytes);
        assert_eq!(t.drain_events(), vec![], "{bytes:?}");
    }
}

/// A payload that is not base64 produces **no event at all**, so a consumer is
/// never handed text the stream did not contain.
///
/// This diverges from the spec deliberately: `ctlseqs.txt:2174` ends a payload
/// that is *"neither a base64 string nor ?"* by clearing the selection. Clearing
/// is destructive, and inferring it from bytes the engine could not parse means
/// line noise wipes what the user copied by hand. alacritty drops it silently
/// too (`alacritty_terminal/src/term/mod.rs:1717`).
#[test]
fn a_malformed_payload_relays_nothing() {
    for payload in ["!!!!", "aGk==", "a", "Zm9v-Zm9v", "aG k=", "aG=k"] {
        let mut t = Engine::new(80, 24);
        t.feed(format!("\x1b]52;c;{payload}\x07").as_bytes());
        assert_eq!(t.drain_events(), vec![], "payload {payload:?}");
    }
}

/// Missing padding is **not** malformed, and this pins the one place this crate
/// is deliberately laxer than alacritty's `STANDARD` engine, which requires
/// canonical padding and would drop `aGk`. `aGk` and `aGk=` denote the same two
/// bytes, so accepting the first invents nothing; the asymmetry decides it,
/// since rejecting costs a silently dropped clipboard and accepting costs
/// nothing. The reach of unpadded emitters is unmeasured — this is here so that
/// tightening it later is a visible choice.
///
/// It is also the case that caught a disagreement between this file and
/// `src/base64.rs`: the first draft listed `aGk` as malformed while the module
/// documented it as accepted, and the test is what said which one was the code.
#[test]
fn a_payload_that_omits_its_padding_still_stores() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;aGk\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: "hi".into(),
        }]
    );
}

/// **The payload is `params[2..]` rejoined, and this is the case that proves
/// it.** `vte` splits the whole OSC body on `;` (#650, for OSC 8), so a payload
/// carrying one arrives in pieces — and `aGk=` is a *valid* encoding of `hi`, so
/// a handler reading `params[2]` alone would succeed, emit, and hand the consumer
/// a silently truncated clipboard. Rejoined, the stray `;` reaches the decoder,
/// which has no value for it.
#[test]
fn a_payload_split_by_a_semicolon_is_not_silently_truncated() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;aGk=;b29wcw==\x07");
    assert_eq!(t.drain_events(), vec![]);
}

/// A payload that decodes to bytes that are not UTF-8 relays nothing. Every text
/// surface this crate publishes is UTF-8, and a lossy conversion would hand the
/// consumer replacement characters the application never sent. `//4=` is the
/// well-formed base64 of `FF FE`.
#[test]
fn a_payload_that_is_not_utf8_relays_nothing() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;//4=\x07");
    assert_eq!(t.drain_events(), vec![]);
}

/// An unmodelled target is ignored rather than folded onto a neighbour — a
/// future or unsupported target must not be silently mapped onto the wrong one.
/// `q` is the secondary selection and `0`–`7` the cut buffers
/// (`ctlseqs.txt:2156`); ghostty folds all of them onto the clipboard
/// (`src/termio/stream_handler.zig:1009`) and this does not.
///
/// **A multi-character list is refused for a sharper reason.** The spec permits
/// `pc` — both targets — and this engine cannot express it. `vte` and alacritty
/// take the first byte, which honours one target and silently drops the other:
/// the same defect as a truncated payload, one axis over, and the issue's own
/// rule is that an ignored clipboard beats a partial one.
#[test]
fn an_unmodelled_or_multi_character_target_relays_nothing() {
    for field in ["q", "0", "7", "x", "pc", "cp", "sp0", "C", "P"] {
        let mut t = Engine::new(80, 24);
        t.feed(format!("\x1b]52;{field};aGk=\x07").as_bytes());
        assert_eq!(t.drain_events(), vec![], "target field {field:?}");
    }
}

/// An oversized payload is dropped **whole**, never truncated: a prefix of a
/// clipboard is text the user pastes believing it is what they copied, so an
/// ignored copy — visible the moment they paste — is the better failure.
///
/// What the bound protects is the *decode* and the `String` on the event queue.
/// `vte` has already accumulated the payload by the time this runs, measured: a
/// 4 MB OSC 52 arrives at the dispatch complete. See `MAX_CLIPBOARD_BASE64`.
#[test]
fn an_oversized_payload_is_dropped_whole() {
    let mut t = Engine::new(80, 24);
    let mut bytes = Vec::from(&b"\x1b]52;c;"[..]);
    // Over the 16 MiB bound, and valid base64 to the last character, so the only
    // thing that can reject it is the bound itself.
    bytes.extend(std::iter::repeat_n(b'A', 16 * 1024 * 1024 + 4));
    bytes.push(0x07);
    t.feed(&bytes);
    assert_eq!(t.drain_events(), vec![]);

    // The control: the same shape just under the bound does relay, so the test
    // above is observing the bound and not some other refusal.
    let mut t = Engine::new(80, 24);
    let mut bytes = Vec::from(&b"\x1b]52;c;"[..]);
    bytes.extend(std::iter::repeat_n(b'A', 16 * 1024 * 1024));
    bytes.push(0x07);
    t.feed(&bytes);
    assert!(
        matches!(
            t.drain_events().as_slice(),
            [TermEvent::ClipboardStore { .. }]
        ),
        "a payload at the bound is still relayed"
    );
}

/// **A `RIS` between a store and its drain does not eat the store.** That is
/// this channel's clause of
/// [`ris-keeps-configuration-drops-coordinates`](../../docs/map/invariant/ris-keeps-configuration-drops-coordinates.md):
/// both queues survive `ESC c`, so a copy the application asked for before a
/// reset still reaches the consumer that drains after it.
///
/// The first version of this test asserted `drain_replies() == b""` after a
/// reset and called that proof the engine retains no clipboard. It was
/// **vacuous**: `report_clipboard` was never called, so the reply channel was
/// empty by construction and an engine that *did* hold a clipboard passed
/// identically. The retention claim is structural — there is no field — and a
/// test cannot observe the absence of one; what a test *can* observe is the
/// queue behaviour above, so that is what this now asserts.
#[test]
fn a_reset_between_a_store_and_its_drain_keeps_the_store() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;SEVMTE9KVVNURVJN\x07");
    t.feed(b"\x1bc"); // RIS, before anything drains
    let events = t.drain_events();
    assert!(
        events.contains(&TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: "HELLOJUSTERM".into(),
        }),
        "the queued store must survive the reset, got {events:?}"
    );
    // And the sequence still works afterwards — the reset cleared no handler
    // state, because there is none to clear.
    t.feed(b"\x1b]52;c;aGk=\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: "hi".into(),
        }]
    );
}

/// **The idiom every shell script uses, and it works because of one line in a
/// dependency.** `printf '\033]52;c;%s\a' "$(base64 <<<"$text")"` is how a
/// script reaches the clipboard, and GNU coreutils' `base64` wraps its output at
/// 76 columns — so the payload arrives with embedded newlines. `vte` deletes C0
/// bytes inside an OSC string (`vte-0.15.0/src/lib.rs:408`), so they never reach
/// the decoder, which would refuse them.
///
/// The decoder is strict *and* this works, which is only true while that line
/// exists. Pinned here so that a `vte` bump or a parser swap fails on a sentence
/// naming the reason rather than on a silently dropped copy. Note the contrast
/// asserted below: a **space** is not a C0 byte, does reach the decoder, and is
/// refused.
#[test]
fn a_line_wrapped_payload_arrives_contiguous() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;aMOpbGxvIOKAlCDs\nlYjrhZUg\r\n4pyT\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::ClipboardStore {
            target: ClipboardTarget::Clipboard,
            text: "héllo — 안녕 ✓".into(),
        }],
        "CR and LF inside the OSC string are dropped by the parser, not by us"
    );

    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]52;c;aGk =\x07");
    assert_eq!(t.drain_events(), vec![], "a space is not a C0 byte");
}
