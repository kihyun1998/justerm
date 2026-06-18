//! Parse-throughput micro-bench (#9).
//!
//! Feeds representative VT byte streams into `Engine::feed` and reports MB/s
//! per input. No hard threshold — this is a *trend record* (criterion saves a
//! baseline under `target/criterion` and prints the change vs the last run).
//!
//! Four inputs, one per distinct hot code path in the engine. justerm has no
//! renderer, so the classic full-terminal taxonomy (alacritty/vtebench's
//! light/medium/dense cells, six scrolling-region variants, cursor motion,
//! alt-screen) collapses onto these four paths — measuring more would just
//! re-time the same functions:
//!
//! - `ascii`     — printable fast path (`write_glyph`, width 1) + line feeds.
//! - `ansi`      — escape state machine + pen mutation (`csi_dispatch` -> `sgr`,
//!   a colour change every cell; vtebench's `dense_cells`).
//! - `cjk`       — wide-glyph path (`write_glyph`, width 2) + pending-wrap and
//!   spacer-cell handling (vtebench's `unicode`).
//! - `scrolling` — the line-feed scroll routine, the engine's most frequent
//!   state mutation (vtebench's `scrolling*`, which are one path to us since we
//!   shift buffer rows rather than redraw a region).
//!
//! Each input is generated deterministically here — no external files, no RNG —
//! so the byte stream (and thus the MB/s figure) is reproducible across runs.

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use justerm::Engine;

#[path = "inputs.rs"]
mod inputs;
use inputs::{ansi_input, ascii_input, cjk_input, scrolling_input};

const COLS: usize = 80;
const ROWS: usize = 24;

fn bench(c: &mut Criterion, name: &str, input: Vec<u8>) {
    let mut group = c.benchmark_group("throughput");
    // Reporting bytes/iter makes criterion print the result as MiB/s directly.
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function(name, |b| {
        // Fresh engine each iteration so we time only parsing — `Engine::new`'s
        // allocation stays in setup (untimed), and reused-engine scrollback
        // growth can't pollute the measurement.
        b.iter_batched(
            || Engine::new(COLS, ROWS),
            |mut engine| engine.feed(black_box(&input)),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn throughput(c: &mut Criterion) {
    bench(c, "ascii", ascii_input());
    bench(c, "ansi", ansi_input());
    bench(c, "cjk", cjk_input());
    bench(c, "scrolling", scrolling_input());
}

criterion_group!(benches, throughput);
criterion_main!(benches);
