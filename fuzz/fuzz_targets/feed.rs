#![no_main]
//! Fuzz the VT stream engine. Sibling of the `feed_never_panics` proptest in tests/robustness.rs.
//! `vte` (the escape-sequence tokenizer) is fuzzed upstream; this drives justerm's own state
//! machine (grid / scrollback / cursor / selection) atop it against adversarial sequences, where
//! a runaway repeat count or out-of-range cursor move would surface as a panic or a hang.

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;

/// `cols`/`rows` are bounded (u8, mapped to 0..=199 / 0..=99) because they come from the caller's
/// viewport size, not the stream; `stream` is the unbounded, attacker-controlled VT bytes. The
/// degenerate low end is generated deliberately rather than filtered out: a caller *can* pass 0 or
/// 1, and the engine clamps both (`cols` to `MIN_COLUMNS`, `rows` to 1, #547), so the fuzzer
/// exercises the clamps instead of avoiding them. The narrowest screen the state machine actually
/// sees is 2×1 — the width at which a wide glyph's pair only just fits.
///
/// `resizes` and the `frame()` calls close a measured gap (#536). Until then this target and its
/// proptest sibling both stopped at `feed`, so **neither had ever called `resize` or `frame`** —
/// while #536's panic was raised by `frame()`, and `resize` is what manufactures the odd cell
/// states (a pair truncated through its middle, ADR-0025 "D4's scope"). Measured on the gap: an
/// injected off-by-one in `write_glyph`'s damage bound left the feed-only lane **green**.
#[derive(Arbitrary, Debug)]
struct Input {
    cols: u8,
    rows: u8,
    stream: Vec<u8>,
    /// `(cols, rows, byte offset to feed up to)` — the resize points are attacker-chosen too, so
    /// the fuzzer can cut a wide pair in half mid-stream rather than only between whole inputs.
    resizes: Vec<(u8, u8, u16)>,
}

fuzz_target!(|input: Input| {
    let cols = usize::from(input.cols) % 200;
    let rows = usize::from(input.rows) % 100;
    let mut engine = justerm_core::Engine::new(cols, rows);

    let mut at = 0usize;
    for (c, r, cut) in input.resizes.iter().take(8) {
        let end = usize::from(*cut).min(input.stream.len()).max(at);
        engine.feed(&input.stream[at..end]);
        at = end;
        let _ = engine.frame();
        engine.resize(usize::from(*c) % 200, usize::from(*r) % 100);
        let _ = engine.frame();
    }
    engine.feed(&input.stream[at..]);
    let _ = engine.frame();
});
