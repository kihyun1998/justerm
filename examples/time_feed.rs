//! Matched-methodology throughput timer for `Engine::feed`, mirroring
//! `bench/xterm-compare/bench-xterm.mjs` byte-for-byte: same input files, same
//! fresh-engine-per-sample, same WARMUP/SAMPLES, same median, same
//! MB = bytes / 1e6 formula. This is what fills the cross-impl comparison table
//! in docs/perf/xterm-comparison.md — criterion stays for the *intra-justerm*
//! fix-gate (its confidence intervals), but a cross-engine number must use the
//! *same* methodology on both sides or the table measures the harness, not the
//! engine.
//!
//! Usage: `cargo run --release --example time_feed -- <inputs-dir>`

use std::time::Instant;

use justerm::Engine;

const COLS: usize = 80;
const ROWS: usize = 24;
const SCROLLBACK: usize = 100; // == bench FLOOD_CAP and the xterm harness scrollback
const WARMUP: usize = 5;
const SAMPLES: usize = 15;
// Tile each stream to this size before timing, matching bench-xterm.mjs. The
// xterm side needs it (its callback floor swamps sub-12ms payloads); justerm
// doesn't, but the comparison must run the *same* bytes through both engines.
const TARGET_BYTES: usize = 32 * 1024 * 1024;
const INPUTS: [&str; 5] = ["ascii", "ansi", "cjk", "scrolling", "flood"];

fn tile_to_target(unit: &[u8]) -> Vec<u8> {
    let reps = (TARGET_BYTES.div_ceil(unit.len())).max(1);
    let mut out = Vec::with_capacity(unit.len() * reps);
    for _ in 0..reps {
        out.extend_from_slice(unit);
    }
    out
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let m = xs.len() / 2;
    if xs.len() % 2 == 1 {
        xs[m]
    } else {
        (xs[m - 1] + xs[m]) / 2.0
    }
}

// One timed feed into a fresh engine; returns seconds. Engine construction is
// outside the timed region (matches the xterm harness and criterion's
// iter_batched), so we time only feed().
fn time_once(input: &[u8]) -> f64 {
    let mut engine = Engine::with_scrollback(COLS, ROWS, SCROLLBACK);
    let t0 = Instant::now();
    engine.feed(std::hint::black_box(input));
    t0.elapsed().as_secs_f64()
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: time_feed <inputs-dir>");

    println!("{{");
    println!("  \"engine\": \"justerm\", \"cols\": {COLS}, \"rows\": {ROWS}, \"scrollback\": {SCROLLBACK}, \"warmup\": {WARMUP}, \"samples\": {SAMPLES},");
    println!("  \"results\": {{");
    for (idx, name) in INPUTS.iter().enumerate() {
        let unit = std::fs::read(format!("{dir}/{name}.bin"))
            .unwrap_or_else(|e| panic!("read {name}.bin: {e}"));
        let bytes = tile_to_target(&unit);
        let mb = bytes.len() as f64 / 1e6;
        for _ in 0..WARMUP {
            time_once(&bytes);
        }
        let times: Vec<f64> = (0..SAMPLES).map(|_| time_once(&bytes)).collect();
        let med = median(times);
        let mbps = mb / med;
        eprintln!(
            "justerm {name:<10} {mbps:>7.1} MB/s  (median {:.2} ms over {SAMPLES})",
            med * 1e3
        );
        let comma = if idx + 1 < INPUTS.len() { "," } else { "" };
        println!(
            "    \"{name}\": {{ \"bytes\": {}, \"median_s\": {med}, \"mbps\": {mbps} }}{comma}",
            bytes.len()
        );
    }
    println!("  }}");
    println!("}}");
}
