//! Dump the throughput-bench input streams to files so an out-of-tree harness
//! (e.g. `bench/xterm-compare`, the @xterm/headless comparison) can feed the
//! *byte-identical* streams justerm's own bench measures. The generators in
//! `benches/inputs.rs` are the single source — the bytes are never re-authored
//! on the other side of the comparison, which would reintroduce the workload
//! confound the comparison exists to remove.
//!
//! Usage: `cargo run --example dump_bench_inputs -- <out-dir>`
//! Writes `<out-dir>/{ascii,ansi,cjk,scrolling,flood}.bin`.

#[path = "../benches/inputs.rs"]
mod inputs;
use inputs::{ansi_input, ascii_input, cjk_input, flood_input, scrolling_input};

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .expect("usage: dump_bench_inputs <out-dir>");

    let streams: [(&str, Vec<u8>); 5] = [
        ("ascii", ascii_input()),
        ("ansi", ansi_input()),
        ("cjk", cjk_input()),
        ("scrolling", scrolling_input()),
        ("flood", flood_input()),
    ];

    for (name, bytes) in &streams {
        let path = format!("{out_dir}/{name}.bin");
        std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {path}: {e}"));
        eprintln!("dump_bench_inputs: wrote {} ({} bytes)", path, bytes.len());
    }
}
