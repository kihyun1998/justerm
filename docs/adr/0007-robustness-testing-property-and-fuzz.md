# ADR-0007: Robustness testing for untrusted-input parsers — property tests + fuzzing

Status: accepted (2026-06-19)

## Context

Two entry points consume bytes the engine does not control:

- `decode` (ADR-0005) parses a wire buffer that arrives over the consumer's own transport (e.g. a Tauri Channel) — length-prefixed spans, a grapheme side-table, and a hyperlink table, every count attacker-influenced.
- `Engine::feed` consumes a VT stream originating from a PTY/SSH peer.

The existing `tests/` suites (serialize, vt_compliance, vttest, …) give good **correctness** coverage, but every input in them is one *we wrote by hand* — well-formed by construction. Two defect classes live outside that net: **malformed/adversarial input** (a byte pattern we never imagined that drives a panic, an arithmetic overflow, an out-of-bounds read, or an unbounded allocation) and **wide valid input** (a valid-but-unusual stream the hand vectors never hit).

This is not hypothetical. The first run of the fuzz lane below found a real panic in `decode` within ~0.2s: a span with `left = 0, right = 65535` computed its run length `right - left + 1` in `u16`, overflowing before the widening cast (#33). The hand vectors and the property test's random sampling had both missed it; coverage-guided fuzzing reached it immediately.

The toolchain constrains *how* we close this. `rust-toolchain.toml` pins **stable 1.96.0**; `cargo-fuzz`/libFuzzer needs nightly and is effectively unsupported on the maintainer's Windows host, so coverage-guided fuzzing is a **CI lane**, not a local loop.

## Decision

Untrusted-input parsers are verified beyond the hand vectors by two complementary lanes.

### 1. Property tests (`proptest`) — local, stable, always-on

`tests/robustness.rs`, run by plain `cargo test` on stable (so it ships in the CI test gate, ADR-0006 sibling). `proptest` is a dev-dependency. Three properties today:

- **No-panic** — an arbitrary wire buffer to `decode`, and an arbitrary VT stream to `Engine::feed`, must return / absorb, never panic, overflow, or read out of bounds.
- **Round-trip** — whatever `decode` accepts must survive re-encoding unchanged (`decode(buf) = Ok(f) ⇒ decode(encode(f)) == f`), the contract ADR-0005 promises. Driven from arbitrary bytes, so it needs no `Frame` generator.

**Threat-model-faithful generation:** a field from a fixed-size header is bounded to its real range (cols/rows from the viewport, not the stream); only the variable-length, attacker-controlled blob is fully arbitrary. A magic+version-biased generator drives ~half the cases past the header into the body parser.

### 2. Coverage-guided fuzzing (`cargo-fuzz` / libFuzzer) — CI lane only

A standalone `fuzz/` crate with one target per entry point (`serialize` → `decode`, `feed` → `Engine::feed`), run weekly on nightly Ubuntu (`fuzz.yml`). It catches what `proptest` structurally cannot:

- **Hangs** — `proptest` has no per-case timeout; libFuzzer's `-timeout` turns a non-terminating parse loop into a reportable crash.
- **Deeper paths** — coverage feedback mutates toward unexplored branches, as #33 demonstrated.

`vte` (the escape-sequence tokenizer) is fuzzed upstream; the `feed` target therefore exercises *justerm's own* state machine (grid / scrollback / cursor / selection) atop it, not the tokenizer.

### 3. Standing rule

A new parser of untrusted bytes gets its no-panic property (and round-trip where an encoder exists) in the same change; the highest-risk ones also get a fuzz target. A crash the fuzz lane finds is fixed with a regression test pinning the minimal input (as #33 was).

## Consequences

- The hand-vector blind spot is covered without weakening correctness testing — the two are complementary axes.
- The stable lane is free: the properties are ordinary `#[test]`s, already in the CI test gate, no nightly.
- The lanes are demonstrably non-redundant: #33 was invisible to the property test's sampling but trivial for the coverage-guided fuzzer — the concrete reason fuzzing is kept, not dropped as "we already have proptest".
- No runtime dependency added — `proptest` is dev-only and the `fuzz/` crate is an unpublished standalone workspace; the dependency boundary is untouched.

## Alternatives considered

- **Hand vectors only (status quo).** Rejected — proven insufficient the first time the fuzzer ran (#33).
- **Property tests only, no fuzzing.** Rejected — leaves hang-class defects and coverage-directed depth untested; #33 is exactly the depth `proptest` sampling missed.
- **`cargo-fuzz` as the only lane.** Rejected as primary — it cannot run on the maintainer's stable Windows host, so it would mean no local robustness signal. Kept as the CI lane for hangs and depth.
