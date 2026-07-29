//! Property-based robustness for the untrusted-input entry points.
//!
//! `decode` parses a wire buffer a consumer hands back over its own transport, and `Engine::feed`
//! consumes a VT stream originating from a PTY/SSH peer — both are attacker-influenced bytes. The
//! decoders must answer malformed input with a typed `DecodeError` (or absorb it), never panic,
//! overflow, or read out of bounds. These properties assert that contract across the whole input
//! space, not just the hand-written vectors in the other `tests/` files. The two-lane robustness
//! decision (these properties + the CI-only fuzz lane) is ADR-0007.

use justerm_core::{Engine, WIRE_VERSION, decode, encode};
use proptest::prelude::*;

/// A wire buffer that is half fully-arbitrary (exercising the magic/version rejection paths) and
/// half prefixed with the valid `JT` magic + the *current* `WIRE_VERSION`, so the generator actually
/// reaches the length-driven span/side-table/link-table/marker body parser instead of bailing at the
/// header. The version MUST track `WIRE_VERSION` (not a literal) — a stale pin silently reroutes this
/// lane to the header-rejection path, leaving every post-bump wire feature unreached (#159 caught the
/// pin rotted at `2` while the format was at v10).
fn wire_buf() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..=1024),
        proptest::collection::vec(any::<u8>(), 0..=1024).prop_map(|body| {
            let mut buf = vec![b'J', b'T', WIRE_VERSION];
            buf.extend(body);
            buf
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// No-panic: an arbitrary wire buffer fed to `decode` must return `Ok`/`Err`, never panic.
    /// The whole buffer is attacker-controlled (magic, version, then length-prefixed spans, the
    /// grapheme side-table, and the hyperlink table), so it is fully arbitrary. Reaching the end
    /// without unwinding IS the assertion; proptest shrinks any failure to a minimal counterexample.
    #[test]
    fn decode_never_panics_on_arbitrary_input(buf in wire_buf()) {
        let _ = decode(&buf);
    }

    /// Round-trip stability: whatever `decode` accepts must survive re-encoding unchanged — the
    /// encode/decode contract ADR-0005 promises. Driven from arbitrary bytes (no `Frame` generator
    /// needed): if a buffer decodes, its `Frame` must re-encode to bytes that decode back to the
    /// same `Frame`. A failure here is a real encode/decode asymmetry, not a test artifact.
    #[test]
    fn decoded_frames_round_trip_through_encode(buf in wire_buf()) {
        if let Ok(frame) = decode(&buf) {
            prop_assert_eq!(decode(&encode(&frame)), Ok(frame));
        }
    }

    /// No-panic: an arbitrary VT byte stream fed to the engine must never panic. `cols`/`rows` are
    /// bounded because they come from the caller's viewport size, not the stream; the fed bytes are
    /// fully arbitrary. This exercises justerm's own state machine (grid/scrollback/cursor) atop the
    /// `vte` tokenizer against adversarial escape sequences.
    #[test]
    /// **These bounds are why no capacity defect in #621 could be found here, and that is
    /// deliberate rather than a gap to widen.** 200×100 is 20 000 cells and 2048 bytes of
    /// stream, so this generator cannot reach a `u16::MAX` table count, a 65 536-entry
    /// highlight group, or a 65 536-character cluster — every one of #621's four cases
    /// needed a directed fixture (`tests/wire_capacity.rs`) built at the threshold.
    /// Raising the bounds would not fix that: the volume needed is thousands of times
    /// larger, the run time scales with it, and a random stream reaches a specific
    /// overflow with vanishing probability. Property tests answer "does anything panic on
    /// junk"; a capacity ceiling is answered by walking to it on purpose. Each fix brings
    /// its own directed test — do not read a green here as coverage of one.
    fn feed_never_panics_on_arbitrary_input(
        cols in 1usize..=200,
        rows in 1usize..=100,
        stream in proptest::collection::vec(any::<u8>(), 0..=2048),
    ) {
        let mut engine = Engine::new(cols, rows);
        engine.feed(&stream);
    }
}

// ---- the verbs the lanes above never reach (#536) ---------------------------------------
//
// Both random lanes — this file's `feed_never_panics_on_arbitrary_input` and the CI fuzz target
// `fuzz/fuzz_targets/feed.rs` — stop at `feed`. Neither has ever called `resize` or `frame`, and
// that is exactly where this class of defect lives: **#536's panic was raised by `frame()`**, and
// `resize` is the operation that manufactures the odd cell states (a pair truncated through its
// middle, ADR-0025 "D4's scope"). The gap was measured, not assumed — a probe assertion on
// `damage_span`'s bound stayed silent across the whole 66-binary suite, so the fixed vectors do
// not reach it either.
//
// As in the lane above, **returning without unwinding IS the assertion**: `damage_span` carries a
// `debug_assert!` on its column bound, so any caller that computes an out-of-range span fails
// here, at the recording site, instead of downstream in `frame()` — which is the diagnosability
// #536 was filed for.
//
// **Both lanes `reset_damage()` after every `frame()`, and that is not hygiene — without it they
// stop exercising the path they exist for.** `Term::resize` ends in `mark_fully_damaged()`
// (`term.rs`), and only `reset_damage` clears the flag, so an unacked engine returns
// `FrameKind::Full` for every frame after its first resize — measured: `Partial`, then resize,
// then `Full`, `Full`, `Full`, and `Partial` again only after an ack. `Full` ships whole rows and
// never slices by a recorded span, which is exactly the code #536's panic came from.
//
// Justified by that measured coverage, **not by a mutation** — and the distinction is worth
// keeping, because two plausible mutations turn out not to discriminate it. A bad span recorded
// *inside* `resize` can never be sliced with or without the ack, since `reset_damage` resets every
// `LineBounds` before the next frame; and a bad span recorded by a *write* is already caught in the
// window before the first resize, which both lanes have. So the ack buys reach, not a red test.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Arbitrary bytes, with `resize` and `frame` interleaved at attacker-chosen cut points.
    ///
    /// `cut` is a **percentage** of the stream rather than an absolute offset: drawn absolutely
    /// from the same range as the stream length, roughly half of all cut points land at or past
    /// the end, which degenerates the lane into the feed-only one above.
    #[test]
    fn feed_resize_and_frame_never_panic(
        cols in 1usize..=40,
        rows in 1usize..=12,
        stream in proptest::collection::vec(any::<u8>(), 0..=2048),
        resizes in proptest::collection::vec((1usize..=40, 1usize..=12, 0usize..=100), 1..=6),
    ) {
        let mut engine = Engine::new(cols, rows);
        let mut at = 0usize;
        for (c, r, pct) in resizes {
            let end = (stream.len() * pct / 100).max(at);
            engine.feed(&stream[at..end]);
            at = end;
            let _ = engine.frame();
            engine.reset_damage();
            engine.resize(c, r);
            let _ = engine.frame();
            engine.reset_damage();
        }
        engine.feed(&stream[at..]);
        let _ = engine.frame();
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// The same three verbs over *this cluster's* material rather than arbitrary bytes. Random
    /// bytes almost never produce a wide glyph at a column boundary under mode 2027 on the alt
    /// screen inside a scroll region; these chunks do nothing else. Narrow widths are deliberate —
    /// `MIN_COLUMNS = 2` is the width at which a pair only just fits (#547).
    ///
    /// The resize varies **both** axes: a width-only lane never exercises the row-shrink half, and
    /// the arbitrary-byte lane above almost never holds wide material when it shrinks rows.
    #[test]
    fn wide_glyphs_survive_interleaved_resizes(
        cols in 2usize..=8,
        rows in 1usize..=4,
        chunks in proptest::collection::vec(
            prop_oneof![
                Just(&b"\xed\x95\x9c"[..]),                 // 한 — width 2
                Just(&b"\xe2\x96\xb6\xef\xb8\x8f"[..]),     // ▶ + VS16 — promotes to width 2
                Just(&b"\xf0\x9f\x87\xb0\xf0\x9f\x87\xb7"[..]), // regional-indicator pair
                Just(&b"a"[..]),
                Just(&b"\r\n"[..]),
                Just(&b"\x1b[?1049h"[..]),                  // alt screen: resizes, never reflows
                Just(&b"\x1b[?1049l"[..]),
                Just(&b"\x1b[?2027h"[..]),                  // grapheme clustering
                Just(&b"\x1b[?7l"[..]),                     // autowrap off
                Just(&b"\x1b[2;3r"[..]),                    // DECSTBM
                Just(&b"\x1b[1;1H"[..]),
                Just(&b"\x1b[K"[..]),
                Just(&b"\x1b[1@"[..]),                      // ICH
                Just(&b"\x1b[1P"[..]),                      // DCH
                Just(&b"\x1b[L"[..]),                       // IL
                Just(&b"\x1b[M"[..]),                       // DL
            ],
            1..=40,
        ),
        dims in proptest::collection::vec((2usize..=8, 1usize..=4), 2..=5),
    ) {
        let mut engine = Engine::new(cols, rows);
        let mut di = 0usize;
        for (i, chunk) in chunks.iter().enumerate() {
            engine.feed(chunk);
            if i % 5 == 4 {
                let _ = engine.frame();
                engine.reset_damage();
                let (c, r) = dims[di % dims.len()];
                engine.resize(c, r);
                di += 1;
                let _ = engine.frame();
                engine.reset_damage();
            }
        }
        let _ = engine.frame();
    }
}
