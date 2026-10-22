# Territory — robustness testing

## What it is

The gates that feed `justerm-core` input nobody wrote by hand. Two entry points take bytes the
engine does not control — `decode` (a wire buffer back from the consumer's transport) and
`Engine::feed` (a VT stream from a PTY or SSH peer) — and the ordinary `tests/` suites only ever
hand them well-formed vectors. This territory is the two lanes ADR-0007 adds over that: proptest
properties that run on every PR, and a weekly coverage-guided fuzz job. It also holds the directed
size tests that walk to a bound on purpose, because a random lane reaches a capacity ceiling with
vanishing probability.

The bounds themselves belong to the territories that own each store; this note owns the gates that
prove them.

## Governing decisions

- [ADR-0007 — robustness testing: property tests + fuzzing](../../adr/0007-robustness-testing-property-and-fuzz.md)
  — the two lanes, threat-model-faithful generation, and the standing rule that a new parser of
  untrusted bytes gets its no-panic property in the same change
- [ADR-0005 — binary reference-based serialization](../../adr/0005-binary-reference-based-serialization.md)
  — the encode/decode contract the round-trip property asserts

Adjacent, not governing: [ADR-0026](../../adr/0026-outside-coordinates-are-bounded-once.md) bounds
outside *coordinates* at their producer; it says nothing about what the stream may allocate. The
size bounds tested here (#721, #1038) have no decision record — each is a constant with its reason
in its doc-comment.

## Design model

From ADR-0007 plus reading the tests, as marked.

- **Two lanes, non-redundant by measurement** (ADR-0007). The first fuzz run found a `decode` panic
  in ~0.2 s — a span with `left = 0, right = 65535` overflowed `right - left + 1` in `u16` (#33) —
  that hand vectors and proptest sampling had both missed. libFuzzer's `-timeout` also catches hangs,
  which proptest has no per-case timeout for.
- **Proptest is the stable, per-PR lane.** The properties are ordinary `#[test]`s in
  `tests/robustness.rs`, so `cargo test --workspace` in the `test` job runs them; no nightly needed.
  At HEAD there are five:
  - `decode_never_panics_on_arbitrary_input` and `decoded_frames_round_trip_through_encode`
    (2048 cases each, buffers up to 1024 bytes);
  - `feed_never_panics_on_arbitrary_input` (200×100, streams up to 2048 bytes);
  - `feed_resize_and_frame_never_panic` and `wide_glyphs_survive_interleaved_resizes`
    (1024 cases each), added for #536.
- **Threat-model-faithful generation** (ADR-0007). Geometry is bounded to a viewport's range
  because it comes from the caller, not the stream; only the stream or wire body is fully arbitrary.
  `wire_buf` prefixes half its cases with `JT` and the **current** `WIRE_VERSION`, so the generator
  reaches the body parser. It must track the constant: #159 found the pin rotted at `2` while the
  format was at v10, which silently rerouted the lane to the header-rejection path.
- **A feed-only lane never reaches `frame()` or `resize`, and that is where #536 lived.** Until #536
  both lanes stopped at `feed`, while the panic came from `frame()` and `resize` is what manufactures
  odd cell states (a pair cut through its middle). An off-by-one injected into `write_glyph`'s damage
  bound left the feed-only fuzz lane green (`fuzz/fuzz_targets/feed.rs` doc). Both lanes now
  interleave `resize` and `frame()`; the fuzz target takes up to 8 attacker-chosen cut points.
- **The lanes ack damage after each frame on purpose** (`robustness.rs` comments). `Term::resize`
  ends fully damaged and only `reset_damage` clears it, so an unacked engine returns `Full` for every
  later frame — and `Full` never slices by a recorded span, the code #536 came from. This buys reach,
  not a red test: two plausible mutations do not discriminate it.
- **Since #582 each engine frame is pushed through its own wire**, asserting `is_ok()`, not equality.
  `decode` rejects a part that does not fit the frame's declared geometry, which is only safe if
  `Term::frame` never emits one. Equality was tried first and refuted in one run: a frame holding
  `C_LEADING_SPACER` decodes but is not a fixed point, by design (`cell.rs`). The fixed-point
  property is pinned in `tests/span_bounds.rs`.
- **The round-trip property is blind to encode-side asymmetry by construction.** It starts from
  *decoded* bytes, so it proves a fixed point of the decoder's own output; the #531 defect satisfied
  it (`tests/wire_capacity.rs` header). Engine-produced frames are the only starting point that sees
  such a defect.
- **A random lane cannot reach a capacity ceiling, and it is not widened to try.** 200×100 cells and
  2048 bytes cannot reach a `u16::MAX` table count or a 65 536-character cluster; each of #621's four
  cases needed a directed fixture (`tests/wire_capacity.rs`). The comment on
  `feed_never_panics_on_arbitrary_input` says not to read a green there as coverage of one.
- **Size bounds are pinned twice: as behaviour, and as retained heap** (`tests/size_bounds.rs`,
  #1038). #721 had capped how *many* markers a stream allocates; #1038 found four stores with a count
  bound and no size bound (measured on `727865d`: 2.9 KB → 445 MiB through combining marks × REP,
  10 MiB → 236 MiB through the title stack, 0.74 MiB → 202 MiB through OSC 133 command text, and
  103 MiB of URIs kept after every linked cell was erased). The file installs a counting
  `#[global_allocator]`, serialises its tests on one mutex, and each heap test carries a positive
  control (a full scrollback must register) so a broken counter cannot pass as "bounded".
- **The fuzz lane is CI-only and non-blocking** (`fuzz.yml`, ADR-0007). Weekly (Sunday 04:17 UTC)
  plus manual dispatch, two matrix targets, `-max_total_time=300 -timeout=10`, on a floating nightly
  because libFuzzer needs it and the repo pins stable `1.96.0`. The corpus persists through
  `actions/cache` keyed per run with a prefix restore; a crash uploads `fuzz/artifacts/<target>/`.
  cargo-fuzz is not version-pinned, deliberately: nothing it builds is published, and the compiler
  under it floats anyway.
- **`vte` is fuzzed upstream** (ADR-0007), so the `feed` target exercises justerm's own state
  machine on top of it, not the tokenizer.
- **Geometry in the fuzz target includes the degenerate low end on purpose**: `cols`/`rows` map to
  `0..=199` / `0..=99`, and the engine clamps 0 and 1 (`cols` to `MIN_COLUMNS`, #547), so the fuzzer
  exercises the clamps rather than avoiding them.
- **A failure becomes a pinned case.** ADR-0007's standing rule: a fuzz crash is fixed with a
  regression test on the minimal input (#33). Proptest keeps its own seeds in
  `tests/robustness.proptest-regressions`, which it re-runs before novel cases; the one entry there
  (a 1×1 screen, a 4-byte stream, one resize) came in with #595's fix for a glyph wider than a pair
  (commit `0decd3c`).
- **What bounds this territory's own input.** Proptest inputs are bounded by each strategy (above).
  The fuzz input is bounded by libFuzzer's default `-max_len`, which `fuzz.yml` does not set, and the
  run by `-max_total_time`.

## Code

- `justerm-core/tests/robustness.rs` — `wire_buf`, `decode_never_panics_on_arbitrary_input`,
  `decoded_frames_round_trip_through_encode`, `feed_never_panics_on_arbitrary_input`,
  `feed_resize_and_frame_never_panic`, `wide_glyphs_survive_interleaved_resizes`
- `justerm-core/tests/robustness.proptest-regressions` — proptest's persisted failure seeds
- `fuzz/Cargo.toml` — the standalone `justerm-fuzz` crate (own `[workspace]`, `cargo-fuzz` metadata,
  two `[[bin]]` targets)
- `fuzz/fuzz_targets/feed.rs` — `Input` (`cols`, `rows`, `stream`, `resizes`) driving `Engine::new`,
  `feed`, `resize`, `frame`
- `fuzz/fuzz_targets/serialize.rs` — `decode` on raw bytes
- `.github/workflows/fuzz.yml` — the weekly matrix, corpus cache, crash upload
- `justerm-core/tests/size_bounds.rs` — `Counting` allocator, `retained`,
  `marks_times_rep_retain_a_bounded_amount`, `a_huge_title_and_its_stack_retain_a_bounded_amount`,
  `an_erased_hyperlink_keeps_no_copy_of_its_uri`, and the behaviour tests over `MAX_CLUSTER_TAIL`,
  `MAX_TITLE`, `MAX_COMMAND_TEXT_TOTAL`, `MAX_LINK_URI`, `MAX_LINK_ID`
- `justerm-core/tests/wire_capacity.rs` — the directed #621 capacity fixtures the random lanes cannot
  reach
- `justerm-core/tests/span_bounds.rs` — the fixed-point property on a frame with no leading spacer
- `justerm-core/Cargo.toml` — `proptest` as a dev-dependency

## Reference behaviour

- [Per-cell payload length](../../agents/reference-facts.md#per-cell-payload-length--three-of-four-leave-a-cluster-unbounded-the-binding-one-caps-it-and-a-uri-is-a-different-answer-621-verified-2026-07-29-xterm-row-added-867-2026-09-08)
  — how the references bound a cluster and a URI, the comparison behind the capacity fixtures and
  `MAX_CLUSTER_TAIL`

How comparable projects fuzz a terminal has never been compared; ADR-0007 cites only that `vte`
is fuzzed upstream.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — `fuzz` is outside the root workspace, so no per-PR command compiles its targets (see Known holes)

## Blast radius

- [wire format](wire-format.md) — the decode lanes assume the generator speaks the current
  `WIRE_VERSION`; a bump that a literal pin misses silently turns half the cases into header
  rejections
- [frame](frame.md) · [damage](damage.md) — the engine-frame lanes assume every frame `Term::frame`
  emits fits its own geometry; a change to what a frame may carry must keep `decode(encode(f))`
  accepting it
- [grapheme clusters](grapheme-clusters.md) · [hyperlinks](hyperlinks.md) ·
  [marker](marker.md) · [events & replies](events-and-replies.md) — the stores whose size bounds
  `size_bounds.rs` measures; a new store a stream can grow has no size test until one is written
- [wide glyph](wide-glyph.md) · [reflow](reflow.md) — `wide_glyphs_survive_interleaved_resizes`
  assumes resize is what cuts a pair; a new operation that can cut one is outside its chunk list
- [CI gates](ci-and-supply-chain.md) — the proptest lane exists only because `cargo test --workspace`
  reaches `justerm-core/tests`

## Known holes / open

- **The fuzz targets are compiled only by the weekly job.** No per-PR step builds `fuzz/`, so a core
  API change that breaks `feed.rs` or `serialize.rs` is found up to a week later, in a job nobody is
  watching for that (found 2026-10-22; `README.md` suggests a `cargo check --manifest-path` that no
  workflow runs).
- **A fuzz failure is non-blocking and has no owner.** The job uploads a reproducer and nothing
  files it; a red scheduled run is visible only to whoever opens the Actions tab.
- **No directed target for the newer stream parsers.** OSC 52, OSC 133 and the kitty keyboard
  stack parse stream bytes that only the generic `feed` target and the 2048-byte proptest stream
  reach; none has a directed target. ADR-0007 says the
  highest-risk parsers also get one, and nothing has been ranked since.
- **The `vte` OSC buffer keeps its high-water capacity for the engine's lifetime** (#1039, recorded
  as intended upstream). `a_huge_title_and_its_stack_retain_a_bounded_amount` measures above it
  (about 16 MiB for a 10 MiB payload) rather than bounding it, so a stream can still pin that much
  heap per engine.
- **The retained-heap tests are single-shot thresholds** (`< 100 MiB`, `< 17 MiB`), set well above
  the bounded figure; a regression that stays under the threshold passes.
