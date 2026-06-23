//! Parse-throughput micro-bench (#9, #42).
//!
//! Feeds representative VT byte streams into `Engine::feed` and reports MB/s
//! per input. No hard threshold — this is a *trend record* (criterion saves a
//! baseline under `target/criterion` and prints the change vs the last run).
//!
//! ## The at-cap flood regime (#42)
//!
//! Flood throughput is **memory-bandwidth-bound** — every printed, erased, or
//! scroll-blanked cell is touched — and a real flood (`cat huge.log`) sits at the
//! scrollback *cap* the whole time, so eviction recycles a row every line. To
//! measure that regime, the engine is built with a **small scrollback cap**
//! (`FLOOD_CAP`): any input longer than the cap then churns recycling, the way a
//! real flood does, instead of just growing history. The `flood` input
//! (~5 MiB of short lines) is the one that stays in steady state long enough to
//! time the recycle path; the smaller inputs characterise the parse hot paths.
//!
//! `size_of::<Cell>()` is printed once at the start: it is the per-cell byte cost
//! this bench is bandwidth-bound on, and #43 (the deferred `Cell` pack) aims to
//! shrink it (24 -> ~12).
//!
//! Inputs, one per distinct hot code path:
//!
//! - `ascii`     — printable fast path (`write_glyph`, width 1) + line feeds.
//! - `ansi`      — escape state machine + pen mutation (a colour change/cell).
//! - `cjk`       — wide-glyph path (`write_glyph`, width 2) + pending-wrap/spacer.
//! - `scrolling` — short content lines, the line-feed scroll routine.
//! - `flood`     — ~5 MiB of short lines past the cap: the at-cap recycle path.
//!
//! Each input is generated deterministically (no files, no RNG), so the byte
//! stream — and thus the MB/s figure — is reproducible across runs.

use std::time::Duration;

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use justerm::{Cell, Engine};

#[path = "inputs.rs"]
mod inputs;
use inputs::{ansi_input, ascii_input, cjk_input, flood_input, scrolling_input};

const COLS: usize = 80;
const ROWS: usize = 24;
/// Small scrollback cap so any input longer than it drives the at-cap recycle
/// path (a real flood is always at cap). Kept in lockstep with the
/// `flood_saturates_the_scrollback_cap` test's `FLOOD_CAP`.
const FLOOD_CAP: usize = 100;

fn bench(c: &mut Criterion, name: &str, input: Vec<u8>) {
    let mut group = c.benchmark_group("throughput");
    // Reporting bytes/iter makes criterion print the result as MiB/s directly.
    group.throughput(Throughput::Bytes(input.len() as u64));
    // Longer warm-up + measurement tightens the interval enough to trust a
    // single-digit-% change (the noise that muddied #41's first measurements).
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    group.bench_function(name, |b| {
        // Fresh engine each iteration so we time only `feed` — `Engine` setup
        // (allocation) stays untimed. A *small-cap* engine so a long input churns
        // the eviction/recycle path instead of unbounded history growth.
        b.iter_batched(
            || Engine::with_scrollback(COLS, ROWS, FLOOD_CAP),
            |mut engine| engine.feed(black_box(&input)),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn throughput(c: &mut Criterion) {
    // Printed once so a `cargo bench` run records the per-cell byte cost the
    // measurement is bandwidth-bound on (#42; #43 shrinks it).
    eprintln!("size_of::<Cell>() = {} bytes", std::mem::size_of::<Cell>());
    bench(c, "ascii", ascii_input());
    bench(c, "ansi", ansi_input());
    bench(c, "cjk", cjk_input());
    bench(c, "scrolling", scrolling_input());
    bench(c, "flood", flood_input());
}

criterion_group!(benches, throughput);
criterion_main!(benches);
