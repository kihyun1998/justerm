# Territory — throughput bench

## What it is

How justerm measures its own speed. Two criterion benches in `justerm-core` (parse throughput of
`Engine::feed`, and query time of the soft-wrap-run walk), the deterministic input generators they
share, a set of plain-timer examples matched byte-for-byte to an out-of-tree `@xterm/headless`
harness, and the one written comparison those produced (`docs/perf/xterm-comparison.md`). Every
number here is a **trend record**: no bench has a threshold and none runs in CI. What CI does check
is that the inputs still drive the code paths they are named for.

## Governing decisions

**None.**

- `docs/agents/theflow.md`'s tie-breaker table has a **Performance claims** row — *"our measurement,
  on a release build"*; a number from a consumer's journey is a hypothesis until re-measured here.
  That table is a retired process document, not a decision record, so it informs and does not govern.
- [ADR-0017](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) decides where a bound on
  the wrap-run walk would go if one were taken (a window inside core whose value the consumer
  injects); it does not govern the bench.

## Design model

From the source (bench and example doc-comments) and `docs/perf/xterm-comparison.md`, as marked.

- **Criterion is the intra-justerm fix gate; the plain timers are the cross-engine instrument**
  (`time_feed.rs` doc). A cross-engine number must use the same methodology on both sides or the
  table measures the harness, so `time_feed.rs` mirrors `bench-xterm.mjs`: same input files, a fresh
  engine per sample, 5 warm-up + 15 timed, the median, `MB = bytes / 1e6`. Criterion's confidence
  intervals are never mixed into the cross-engine table.
- **One generator source for every consumer of the bytes** (`benches/inputs.rs`). The criterion
  harnesses, `tests/bench_inputs.rs` and `dump_bench_inputs.rs` all compile the same file through
  `#[path]`, so the xterm side is fed the byte-identical streams and never a re-authored workload.
  Generation is deterministic — no files, no RNG.
- **Five inputs, one per hot path** (`throughput.rs` doc): `ascii` (printable fast path, CRLF
  lines, because real PTY output arrives CRLF), `ansi` (an SGR before every glyph), `cjk` (width-2
  writes, pending wrap, spacers), `scrolling` (the line-feed scroll), and `flood` (~5 MiB of short
  lines).
- **Flood is measured at the scrollback cap** (#42). Flood throughput is memory-bandwidth-bound and
  a real flood sits at the cap, recycling a row every line, so the bench builds the engine with
  `FLOOD_CAP = 100`. `tests/bench_inputs.rs` keeps its own `FLOOD_CAP` in step and asserts the flood
  saturates it. The bench prints `size_of::<Cell>()` once because that is the per-cell byte cost it
  is bound on; it is 12 at HEAD, asserted in `cell.rs`'s tests.
- **Each input is pinned by behaviour, not by bytes** (`tests/bench_inputs.rs`): `ansi` must leave
  indexed colours, `cjk` must be all wide, `flood` must saturate the cap, `one_wrap_run_input` must
  be one buffer-spanning logical line. These run in `cargo test --workspace`, so an input that stops
  driving its path fails on a PR even though the bench itself never runs there.
- **`wrap_run` is the profiling #206 asked for** (`wrap_run.rs` doc). It times
  `viewport_logical_lines` and `search` on two buffers of the same content — one unbroken run, and
  the same characters as short lines. Measured in release: `viewport_logical_lines` 7.3 ms against
  17 µs (~430×), `search` ~1.0× (`docs/architecture.md`). #206 closed on 2026-09-11 on reach — no
  shipped consumer calls the three walk surfaces — not on the numbers; the bench re-measures if one
  does ([logical lines](logical-lines.md)).
- **Payload size changes the cross-engine answer** (`xterm-comparison.md`, 2026-06-24). xterm's
  `WriteBuffer` defers its `write()` callback to an event-loop turn (~12–15 ms), so streams are tiled
  to `TARGET_BYTES` (32 MiB in both harnesses at HEAD) before timing. `verify-sync.mjs` cross-checks
  the async `write()` against `writeSync()` and asserts the buffer was populated, to rule out a
  callback that fires without work.
- **The recorded verdict** (`xterm-comparison.md`, 2026-06-24): justerm's `feed()` is faster than
  `@xterm/headless` `write()` on every input, 1.18–4.05× at 32 MiB, and flat between 8 and 32 MiB
  while xterm degrades on allocation-heavy streams. The "slower than xterm" premise came from penterm
  measuring a `tauri dev` debug build; in release, penterm's whole pipeline beat xterm 2.6–4.8×
  (penterm `perf-journey.md`, 2026-06-24). These numbers predate the 12-byte `Cell` (#43) and the wire
  changes since.
- **Decode is measured on both sides of the wasm boundary** (`xterm-comparison.md`, 2026-06-24).
  `gen_render_frame.rs` writes a full 80×24 frame; `time_decode.rs` times native `decode`,
  `bench-decode.mjs` times `decodeFrame` through a `--target nodejs` build: 21.7 µs native against
  605 µs wasm for 1920 cells, the wasm side doing `decode + flatten`. Recorded as not penterm's
  bottleneck at normal sizes (~3.6% of a 60 fps budget for a full repaint).
- **What bounds the input.** Every input is generated in-tree at a fixed size (the largest is
  `flood`, ~5 MiB, tiled to 32 MiB for the cross-engine runs); nothing here reads outside input.

## Code

- `justerm-core/benches/throughput.rs` — `FLOOD_CAP`, `bench`, `throughput`
- `justerm-core/benches/wrap_run.rs` — `engine_with`, `wrap_run`
- `justerm-core/benches/inputs.rs` — `ascii_input`, `ansi_input`, `cjk_input`, `scrolling_input`,
  `flood_input`, `one_wrap_run_input`, `many_lines_input`, `CJK_GLYPHS`, `WRAP_COLS`, `WRAP_ROWS`
- `justerm-core/tests/bench_inputs.rs` — `flood_saturates_the_scrollback_cap`,
  `one_wrap_run_is_a_single_buffer_spanning_logical_line`,
  `every_input_is_non_empty_and_feeds_cleanly`
- `justerm-core/examples/time_feed.rs` — `tile_to_target`, `TARGET_BYTES`
- `justerm-core/examples/time_decode.rs` · `gen_render_frame.rs` · `dump_bench_inputs.rs`
- `bench/xterm-compare/bench-xterm.mjs` · `bench-decode.mjs` · `verify-sync.mjs` — Node harnesses,
  outside every workspace and every CI job
- `justerm-core/Cargo.toml` — the `criterion` dev-dependency and the two `[[bench]]` entries
- `docs/perf/xterm-comparison.md` — the dated comparison

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The comparison's own source is
`bench/xterm-compare/package.json`'s `@xterm/headless` range (`^5.5.0`), run on one machine on
2026-06-24; it compares throughput, not behaviour.

## Cross-cutting invariants

**None.**

## Blast radius

- [vt-interpretation](vt-interpretation.md) — its cancel-byte split was costed against these five
  inputs (~1 ms per 32 MB); a change to the inputs changes what that figure means
- [logical lines](logical-lines.md) · [search](search.md) — `wrap_run` is the measurement their
  unbounded-walk decision rests on; changing the walk without re-running it leaves #206's reopen
  condition unpriced
- [grid & scrollback](grid-and-scrollback.md) — the flood bench assumes eviction recycles a row per
  line at the cap; a scrollback change that stops recycling makes `flood` measure growth instead
- [wire format](wire-format.md) — `gen_render_frame.rs` builds a `Frame` by hand, so the decode
  timings describe whatever that fixture encodes, not what the engine emits now
- [CI gates](ci-and-supply-chain.md) — `clippy --all-targets` compiles the benches and examples and
  `cargo test --workspace` runs `bench_inputs.rs`; nothing runs a bench

## Known holes / open

- **No bench runs anywhere automatically.** A throughput regression is invisible until someone runs
  `cargo bench` by hand and reads criterion's change against a local baseline.
- **The cross-engine table is from 2026-06-24** — before the 12-byte `Cell` (#43) and the wire
  changes since — and nothing marks it stale.
- **The root `Cargo.toml` has no `[profile.release]`**, so release builds and benches run without
  LTO or `codegen-units = 1`. `xterm-comparison.md` flagged it as a latent absolute win and did not
  pursue it.
- **The wasm decode figure measures `decode + flatten` against native `decode` only**, so the
  ~28× per-cell ratio overstates the wasm tax by an amount never separated out.
