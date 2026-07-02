//! Query-time bench for the unbounded soft-wrap-run walk (#206).
//!
//! `search` and `viewport_logical_lines` assemble a `WRAPLINE` run into one
//! logical line with **no per-run length cap** (docs/architecture.md, "the
//! soft-wrap run walk is intentionally unbounded"). #206 defers a cap until
//! *profiling* shows the pathological single-multi-KB-line case matters — this
//! bench IS that profiling. It times both calls on two buffers of identical
//! content size:
//!
//! - `one_run`    — one unbroken newline-free line: the whole buffer is ONE run.
//! - `many_lines` — the same chars, CRLF every row: many short logical lines.
//!
//! The discriminating question the numbers answer:
//!
//! - `viewport_logical_lines` is `O(viewport)` on `many_lines` (it joins only the
//!   ~24 visible rows) but `O(scrollback)` on `one_run` (it walks up to the run's
//!   start and forward to its end, though only 24 rows are visible). The
//!   `one_run / many_lines` ratio quantifies that blow-up — the case a cap fixes.
//! - `search` scans the whole buffer either way, so its ratio ~= 1 shows the run
//!   is *not* what drives search cost (the giant run is one big allocation vs
//!   many small ones, same total work).
//!
//! Like `throughput`, this is a **trend record** with no hard threshold: it
//! surfaces whether the O(scrollback) single-run walk is expensive in absolute
//! terms, so #206 can be decided (cap vs keep-documented) on data, not a guess.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use justerm_core::Engine;

#[path = "inputs.rs"]
mod inputs;
use inputs::{WRAP_COLS, WRAP_ROWS, many_lines_input, one_wrap_run_input};

const ROWS: usize = 24;

/// An engine wide enough that a full row is `WRAP_COLS` chars and deep enough to
/// retain the whole run (scrollback cap = `WRAP_ROWS`, so nothing is evicted),
/// fed with `input`. Both shapes get the same geometry so only their line
/// structure differs.
fn engine_with(input: &[u8]) -> Engine {
    let mut e = Engine::with_scrollback(WRAP_COLS, ROWS, WRAP_ROWS);
    e.feed(input);
    e
}

fn wrap_run(c: &mut Criterion) {
    // Build both buffers once (untimed) — the calls under test take `&self`, so
    // each iteration re-runs the query against the same fixed buffer.
    let one = engine_with(&one_wrap_run_input());
    let many = engine_with(&many_lines_input());

    let mut group = c.benchmark_group("wrap_run");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    // A needle absent from the pattern (no punctuation in it): a non-matching
    // search isolates the walk + haystack-assembly cost #206 is about, with no
    // match-collection noise.
    group.bench_function("search/one_run", |b| {
        b.iter(|| black_box(one.search(black_box("~~~"))))
    });
    group.bench_function("search/many_lines", |b| {
        b.iter(|| black_box(many.search(black_box("~~~"))))
    });

    group.bench_function("viewport_logical_lines/one_run", |b| {
        b.iter(|| black_box(one.viewport_logical_lines()))
    });
    group.bench_function("viewport_logical_lines/many_lines", |b| {
        b.iter(|| black_box(many.viewport_logical_lines()))
    });

    group.finish();
}

criterion_group!(benches, wrap_run);
criterion_main!(benches);
