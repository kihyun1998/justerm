#![no_main]
//! Fuzz the VT stream engine. Sibling of the `feed_never_panics` proptest in tests/robustness.rs.
//! `vte` (the escape-sequence tokenizer) is fuzzed upstream; this drives justerm's own state
//! machine (grid / scrollback / cursor / selection) atop it against adversarial sequences, where
//! a runaway repeat count or out-of-range cursor move would surface as a panic or a hang.

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;

/// `cols`/`rows` are bounded (u8, mapped to 1..=200 / 1..=100) because they come from the caller's
/// viewport size, not the stream; `stream` is the unbounded, attacker-controlled VT bytes. A `cols`
/// of 1 is passed through deliberately rather than filtered: the engine clamps it to
/// `MIN_COLUMNS` (#547), so this also exercises the clamp, and the narrowest width the state
/// machine actually sees is 2 — the width at which a wide glyph's pair only just fits.
#[derive(Arbitrary, Debug)]
struct Input {
    cols: u8,
    rows: u8,
    stream: Vec<u8>,
}

fuzz_target!(|input: Input| {
    let cols = usize::from(input.cols) % 200 + 1;
    let rows = usize::from(input.rows) % 100 + 1;
    let mut engine = justerm_core::Engine::new(cols, rows);
    engine.feed(&input.stream);
});
