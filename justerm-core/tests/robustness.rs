//! Property-based robustness for the untrusted-input entry points.
//!
//! `decode` parses a wire buffer a consumer hands back over its own transport, and `Engine::feed`
//! consumes a VT stream originating from a PTY/SSH peer — both are attacker-influenced bytes. The
//! decoders must answer malformed input with a typed `DecodeError` (or absorb it), never panic,
//! overflow, or read out of bounds. These properties assert that contract across the whole input
//! space, not just the hand-written vectors in the other `tests/` files. The two-lane robustness
//! decision (these properties + the CI-only fuzz lane) is ADR-0007.

use justerm_core::{Engine, decode, encode};
use proptest::prelude::*;

/// A wire buffer that is half fully-arbitrary (exercising the magic/version rejection paths) and
/// half prefixed with the valid `JT` magic + version 2, so the generator actually reaches the
/// length-driven span/side-table/link-table body parser instead of bailing at the header.
fn wire_buf() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..=1024),
        proptest::collection::vec(any::<u8>(), 0..=1024).prop_map(|body| {
            let mut buf = vec![b'J', b'T', 2];
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
    fn feed_never_panics_on_arbitrary_input(
        cols in 1usize..=200,
        rows in 1usize..=100,
        stream in proptest::collection::vec(any::<u8>(), 0..=2048),
    ) {
        let mut engine = Engine::new(cols, rows);
        engine.feed(&stream);
    }
}
