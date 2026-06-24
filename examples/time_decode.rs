//! Native baseline for the per-frame wire decode, the floor the WASM
//! `decodeFrame` path (justerm-wasm) is taxed against. Reads a wire frame
//! produced by `gen_render_frame`, decodes it many times, reports ns/frame and
//! frames/sec. Decode is µs-scale, so each sample times a batch of DECODES_PER
//! decodes and divides — a single decode is below timer resolution.
//!
//! Usage: `cargo run --release --example time_decode -- <frame-file>`

use std::hint::black_box;
use std::time::Instant;

use justerm::decode;

const WARMUP: usize = 5;
const SAMPLES: usize = 15;
const DECODES_PER: usize = 2000;

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[xs.len() / 2]
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: time_decode <frame-file>");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));

    let batch = || {
        let t0 = Instant::now();
        for _ in 0..DECODES_PER {
            let _ = black_box(decode(black_box(&bytes)).expect("decode"));
        }
        t0.elapsed().as_secs_f64() / DECODES_PER as f64
    };

    for _ in 0..WARMUP {
        batch();
    }
    let per: Vec<f64> = (0..SAMPLES).map(|_| batch()).collect();
    let s = median(per);
    eprintln!(
        "justerm-native decode  {:.2} us/frame   {:.0} frames/s   ({} wire bytes)",
        s * 1e6,
        1.0 / s,
        bytes.len()
    );
    println!(
        "{{ \"engine\": \"justerm-native\", \"us_per_frame\": {}, \"frames_per_s\": {}, \"wire_bytes\": {} }}",
        s * 1e6,
        1.0 / s,
        bytes.len()
    );
}
