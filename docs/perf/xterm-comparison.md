# justerm vs xterm.js — same-machine throughput comparison

> Same-machine, same-bytes, renderer-free comparison of justerm `feed()` against
> `@xterm/headless` `write()`. Built by the `/loop` diagnosis whose premise was
> "native, yet slower than xterm.js — why?". **Result: the premise is refuted.**

## Verdict (2026-06-24)

**justerm is faster than @xterm/headless on every input**, and the gap *widens*
under sustained load. There is no native-throughput gap to explain or fix; the
seed hypotheses H1–H5 ("why are we slower") are moot. Loop terminated.

The "native should be faster" intuition is both correct **and already realised** —
the slowness it was reacting to was never measured on equal footing (premise C: a
gut feeling, never a same-machine number). Once measured apples-to-apples, native
wins.

### Cross-reference: confirmed end-to-end in penterm (2026-06-24)

This bench is renderer-free `feed()` vs `write()`. The premise originated in
**penterm**, whose `perf-journey.md` recorded justerm `feed` as the "wall" and
xterm ~1.5× faster end-to-end. That turned out to be a **`tauri dev` (debug-Rust)
measurement artifact** — penterm's harness runs under `tauri dev`, which compiles
the `justerm-core` dependency in debug, while xterm is JIT-optimized JS regardless.
Re-run in release (`pnpm tauri dev --release`), penterm's *full* pipeline
(feed + encode + IPC + wasm decode + beamterm render) **beats xterm 2.6–4.8×**
(native ascii 73.5 / ansi 83.7 / cjk 103.4 MB/s vs xterm 24.3 / 31.6 / 21.4);
the backend `feed` alone dropped 646 → 98 ms (ascii, ~6.6×) between debug and
release — matching this repo's own ~7× `time_feed` debug-vs-release factor. So the
native-wins result here holds not just for isolated `feed` but for the real
consumer's end-to-end render path. (penterm `perf-journey.md` + issues 09/15 carry
the correction on their side.)

## Method

- **Same scope**: `@xterm/headless` is the real xterm.js core with no renderer —
  bytes → buffer state, exactly justerm's `feed()` contract. Neither side draws.
- **Same bytes**: the 5 streams are dumped from justerm's own `benches/inputs.rs`
  (`cargo run --release -p justerm-core --example dump_bench_inputs -- <dir>`) and fed verbatim to
  both engines — no re-authoring, so no workload confound.
- **Same config**: cols=80, rows=24, scrollback=100 (= bench `FLOOD_CAP`, so the
  flood stream drives the at-cap recycle path on both sides).
- **Same methodology**: fresh engine per sample (setup untimed), 5 warm-up + 15
  timed, **median**, `MB = bytes / 1e6`. justerm via `examples/time_feed.rs`,
  xterm via `bench/xterm-compare/bench-xterm.mjs` — deliberately matched so the
  cross-engine table measures the engines, not two different harnesses. (criterion
  stays reserved for the *intra-justerm* fix-gate, where its confidence intervals
  matter; it is **not** mixed into this cross-engine table.)
- **Payload size matters and is reported at two points (8 MiB, 32 MiB).** xterm's
  `WriteBuffer` defers its `write()` callback to an event-loop turn (~12–15 ms
  granularity), so a sub-12 ms payload measures the scheduler floor, not
  throughput. Streams are tiled by repetition past that floor. The two sizes also
  expose a real effect: justerm is size-independent; xterm degrades with volume.

## ① Comparison table (MB/s, median; higher = faster)

**8 MiB tiled payloads:**

| Input     | justerm | xterm | justerm advantage |
| --------- | ------- | ----- | ----------------- |
| ascii     | 143.0   | 112.1 | 1.28×             |
| ansi      | 239.9   | 105.9 | 2.27×             |
| cjk       | 237.1   | 138.2 | 1.72×             |
| scrolling | 132.9   | 92.5  | 1.44×             |
| flood     | 135.1   | 98.1  | 1.38×             |

**32 MiB tiled payloads:**

| Input     | justerm | xterm | justerm advantage |
| --------- | ------- | ----- | ----------------- |
| ascii     | 141.3   | 119.7 | 1.18×             |
| ansi      | 239.6   | 59.1  | 4.05×             |
| cjk       | 230.3   | 82.5  | 2.79×             |
| scrolling | 131.6   | 48.7  | 2.70×             |
| flood     | 126.8   | 55.7  | 2.28×             |

justerm's numbers barely move 8→32 MiB (flat = steady-state, no GC). xterm's drop
sharply on the allocation/newline-heavy streams (ansi/scrolling/flood) — JS GC and
per-line object churn under sustained volume, the cost native sidesteps. Only the
alloc-light ascii holds up on the xterm side (and even rises slightly as its small
callback floor amortises). **The 8 MiB numbers are the conservative ones for
justerm** (they flatter xterm); the true sustained gap is the 32 MiB column.

## ② Hypothesis queue — all moot

The seeds all assumed a deficit to find. There is none, so each is resolved
"moot — no gap" rather than tested. (They remain valid as *absolute* optimisation
ideas for justerm-vs-itself, should that ever become a goal — see below.)

| ID | Hypothesis | Status |
| -- | ---------- | ------ |
| H1 | No `[profile.release]` LTO / codegen-units=1 / target-cpu=native | moot — no gap (but a real *latent* win for justerm in absolute terms; see note) |
| H2 | No printable-run batching (per-char `print()` + `width()`) | moot — no gap |
| H3 | Per-byte vte dispatch + UTF-8 per byte | moot — no gap |
| H4 | Cell-write hot path (bounds/pending-wrap/`Row` Deref) | moot — no gap |
| H5 | `Row` BTreeMap recycle cost | moot — no gap |

**dry-counter: n/a** — terminated by premise refutation, not by exhaustion.

## Caveats / scope (what this does NOT measure)

1. **Native only.** This is justerm's native `feed()`. justerm's first consumer
   (penterm) uses it via **WASM** (`justerm-wasm-decode`) + the **beamterm** renderer. If
   slowness is perceived *there*, it lives in the WASM boundary or rendering — a
   different axis this bench does not touch. Measure that separately before
   concluding anything about penterm.
2. **Sync vs async.** justerm `feed()` is fully synchronous; xterm `write()`
   deliberately yields to the event loop (~12 ms) for UI responsiveness. That
   trade slightly taxes xterm's raw throughput here; it is a design choice, not a
   bug. The comparison is still fair as "bytes → buffer state", which is what both
   APIs are for.
3. **Comparable, not identical, work.** Both build renderer-free buffer state with
   wide/combining handling; neither is a bit-for-bit reimplementation of the
   other. The contract ("parse VT → screen state") is the same.

## WASM decode path (penterm's render-feeding cost)

A follow-up to "if justerm is fast natively, where does penterm slow down?".
Key architecture correction: **`justerm-wasm-decode` is a *decoder*, not the engine** — it
exposes `decodeFrame(bytes)`, not `feed()`. penterm's pipeline runs `feed()`
**native** in the Tauri backend (the fast path measured above), serialises a
compact wire frame, ships it over IPC, and the webview calls `decodeFrame` (WASM)
to unpack it into the structure-of-arrays beamterm renders. So the only WASM cost
in the pipeline is the per-frame decode; `feed()` is never WASM-taxed. beamterm's
GPU render and the IPC transport are *not* measurable in this repo.

Measured on a representative **Full 80×24 frame** (1920 cells, 34 724 wire bytes,
indexed colour every 5 cells), release wasm-pack build, batched median:

| Path | 1 cell (fixed/call) | 1920 cells (full repaint) | per-cell slope |
| ---- | ------------------- | ------------------------- | -------------- |
| native `decode()` (`examples/time_decode.rs`)        | 0.08 µs | 21.7 µs | 0.011 µs |
| WASM `decodeFrame` (`bench/xterm-compare/bench-decode.mjs`) | 3.89 µs | 605 µs | 0.31 µs |

- **Scales per-cell, not per-call**: the ~3.9 µs fixed boundary cost is negligible
  against 605 µs, so large frames don't amortise it. The work is `decode + flatten`
  inside wasm.
- **~28× per-cell vs native, but that overstates the pure wasm tax**: native
  `time_decode` does `decode()` only; wasm does `decode + flatten` (building the
  SoA). Part of the gap is flatten work native skips, not "wasm is slow". Forcing
  the renderer's array reads (`--touch`) barely moved it (621 vs 605 µs), so the
  typed-array copies out are not the cost.
- **Absolute verdict — not penterm's bottleneck at normal sizes**: 605 µs/frame is
  a ~1650 fps ceiling, ~3.6% of a 60 fps budget. Decode would only break 60 fps at
  ~50 000 cells (~600×80). And justerm ships *damage*, not full frames — steady-state
  typing decodes a few spans (µs), the 605 µs is the full-repaint worst case. If
  penterm feels slow and decode is sub-millisecond, the cause is more likely the
  **beamterm GPU render or the IPC transport** (both out of scope here).

Reproduce:
```
cargo run --release -p justerm-wasm-decode --example gen_smoke_frame  -- /tmp/smoke.bin   # 1-cell
cargo run --release -p justerm-core --example gen_render_frame -- /tmp/full.bin    # 80x24
cargo run --release -p justerm-core --example time_decode      -- /tmp/full.bin    # native
wasm-pack build justerm-wasm-decode --target nodejs --out-dir pkg-node
node bench/xterm-compare/bench-decode.mjs /tmp/full.bin [--touch]                  # wasm
```

## Latent absolute win (optional, not pursued by this loop)

H1 (build profile) is the one seed that could make justerm *even faster in
absolute terms* regardless of xterm — the root `Cargo.toml` has no
`[profile.release]`, so `cargo bench`/release builds run without LTO,
codegen-units=1, or target-cpu tuning. This loop did **not** pursue it: its
mission was the (refuted) xterm gap, and absolute optimisation with no gap forcing
it is a separate decision. Flagged here so it isn't lost.

## Reproduce

```
cargo run --release -p justerm-core --example dump_bench_inputs -- bench/xterm-compare/inputs
cargo run --release -p justerm-core --example time_feed         -- bench/xterm-compare/inputs   # justerm
cd bench/xterm-compare && npm install && node bench-xterm.mjs                  # xterm
```
Adjust `TARGET_BYTES` (both harnesses, kept equal) to change payload size.
