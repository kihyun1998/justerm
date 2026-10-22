# Invariant — a crate outside the workspace is invisible to every `--workspace` gate

## The fact

Three crates sit outside the root workspace, and no `--workspace` / `--all` command reaches any of them.
Root `Cargo.toml` at HEAD 7bb31f7:

```toml
members = ["justerm-core", "justerm-wasm-decode", "justerm-pty"]
exclude = ["fuzz", "justerm-facade", "justerm-renderer"]
```

So every gate written as `cargo <cmd> --workspace` or `cargo fmt --all` inspects the three member crates
(the old note quoted two; `justerm-pty` joined, ADR-0034). Reaching the others requires naming them:
`--manifest-path <crate>/Cargo.toml`. The failure is silent in the worst way: the excluded crate is not
skipped with a warning, it is not a thing the command knows about, so the gate reports success having
inspected nothing of it. (`justerm-web` is a pnpm package, not a cargo crate, and is outside this fact.)

A sibling shape with the same symptom: `justerm-wasm-decode/tests/web.rs` is
`#![cfg(target_arch = "wasm32")]`, so on the host it compiles to nothing — `cargo test -p
justerm-wasm-decode --test web` reports `running 0 tests` and exits 0 (measured). Its compensating
`wasm32` build compiles the assertions but never runs them; they execute in one place, CI's `wasm` job
(`wasm-pack test --node justerm-wasm-decode`).

## Why it is cross-cutting

Three crates, three unrelated reasons, one consequence; the decisions were not coordinated and collide
only at the tool's assumption that "the workspace" means "the code":

| Crate | Why it is excluded |
|---|---|
| `fuzz` | nightly + libFuzzer toolchain |
| `justerm-facade` | frozen at `0.5.1` — must not be dragged by the version lockstep of the member crates |
| `justerm-renderer` | `web-sys` / `glow` are wasm32-only, so membership would break the host `cargo test --workspace` |

Each excluded crate's gates are separate `run:` lines that do not share anything with the workspace
gates or with each other; a fourth exclusion will have a fourth reason and the same consequence.

Two declarations now hold the line, and they are jointly load-bearing in the safe direction (#608):
`exclude` is a list of paths relative to the root manifest, and cargo resolves a crate's workspace by
walking upward — so from a git worktree it climbed past the worktree into the main checkout's manifest
and refused to build the renderer. `fuzz/Cargo.toml` and `justerm-renderer/Cargo.toml` therefore each
declare `[workspace]`, a fact stated in the directory that survives being reached from anywhere;
`justerm-facade` relies on the list alone, deliberately (a tombstone nobody edits). A root listing a
member that itself declares `[workspace]` fails loudly (`cargo metadata`: "multiple workspace roots
found"), so deleting `justerm-renderer` from `exclude` breaks every root gate at once rather than
silently dragging a wasm32-only crate into the host run.

## Territories it holds in

Derivable half — two greps; the second holds from anywhere (#608):

```sh
rg -A3 '^exclude' Cargo.toml                       # what THIS root disclaims
rg -l '^\[workspace\]' */Cargo.toml                # which crates declare their own root
```

Non-derivable half — which gates compensate (read from `.github/workflows/test.yml` and `fuzz.yml` at
HEAD 7bb31f7):

- [CI & supply chain](../territory/ci-and-supply-chain.md) — the compensating per-manifest gates. The
  `renderer` job names `justerm-renderer` by `--manifest-path` for fmt, test, clippy, build and rustdoc.
- the renderer crate — where it bit. Its territories all sit in a crate no `--workspace` command
  reaches: [cell compositing](../territory/cell-compositing.md) ·
  [cell geometry](../territory/cell-geometry.md) · [glyph atlas](../territory/glyph-atlas.md) ·
  [built-in block glyphs](../territory/builtin-block-glyphs.md).
- [browser proof harness](../territory/browser-proof-harness.md) — `renderer-proofs` reaches the
  renderer without naming a manifest: `check-published-package.mjs justerm-renderer/pkg` (#951) reads
  the package `wasm-pack` produced, gating an artifact rather than a crate. Added here; this territory
  already linked back.
- [release](../territory/release.md) — the facade's exclusion keeps it frozen and is why nothing named
  gates it.
- [published surface](../territory/published-surface.md) — two gates grew onto the facade without
  anyone deciding to gate the tombstone, because both derive their work set from the tree:
  `check-published-pointers.mjs` (#949) walks manifests carrying a `description`, and
  `check-published-rustdoc.mjs` (#953) walks manifests without `publish = false` — the latter needing a
  `cargo doc --manifest-path justerm-facade/Cargo.toml` step beside it. A derived work set reaches an
  excluded crate; a named one never will. Added here; this territory already linked back.
- [robustness testing](../territory/robustness-testing.md) — `fuzz` is reached by no per-PR gate at
  all: only the weekly, non-blocking `fuzz.yml` (`cargo +nightly fuzz run`). (The old note said
  "`cargo check --manifest-path fuzz/Cargo.toml`"; no workflow runs that at HEAD.) Added here (new
  territory).
- [wasm decoder](../territory/wasm-decoder.md) — the target-gate sibling, `tests/web.rs`, whose runtime
  assertions run only in the `wasm` job. Added here (new territory).

## What a violation looks like

A green gate that inspected nothing: a passing command with an empty work set. Tell: run the gate and
check the count — `cargo fmt --all` visiting zero files in a crate you just edited exits 0 like a clean
run.

The sharper form is a gate that runs, passes, and answers a different question: the `wasm32` build of
`tests/web.rs` compiles an `assert_eq!(wire_version(), 13)` against a shipped `14` without complaint,
because assertions are runtime constructs.

## Discovery history

| Occurrence | Site | Issue |
|---|---|---|
| 1st | `justerm-renderer` had no gate across slices #259–#340: `cargo fmt --all` visited zero of its files and exited 0, and 13 slices of unformatted code accumulated | #333 |
| 2nd | `justerm-wasm-decode/tests/web.rs` held a second copy of the wire-version pin, hard-coded `13`; a bump to 14 left every local gate green including the wasm32 build, and CI's `wasm` job was the only red, after the PR was open | #621 |
| worktree | `exclude` resolved against the main checkout from a worktree; the renderer and fuzz gained their own `[workspace]` | #608 |

The two occurrences differ where it matters: the first was no gate (discoverable by reading the command);
the second was a gate that reached the file and checked something else. Audit compile / run / assert,
not "is there a gate".

The #621 repair generalises: do not put a value in a place that cannot run it. The literal was replaced
by a relationship — `wire_version_matches_the_engine` asserts `wire_version() ==
justerm_core::WIRE_VERSION` — and the number itself is pinned in `justerm-core`'s own host test. A
duplicated expectation is only as fresh as the slowest place that checks it.

## Where it will recur

1. **A new excluded crate** starts with zero gates; each must be named for it by hand.
2. **A new gate.** Every gate written as "run over the workspace" inherits this at birth, including
   non-cargo gates: `.github/scripts/check-map-links.mjs` takes a hand-written list of roots, so a new
   documentation directory is invisible to it until someone adds the path.
3. **A new member crate** joins every `--workspace` gate silently — the inverse case, and the reason
   any count of gated crates written in prose goes stale (as this note's did when `justerm-pty` joined).

Test: if a command's scope is "everything", ask what its idea of everything is, and compare it against
`ls`, not the intent.
