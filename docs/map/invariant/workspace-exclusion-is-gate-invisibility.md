# Cross-cutting invariant — a crate outside the workspace is invisible to every `--workspace` gate

## The fact

**Three crates sit outside the root workspace, and no `--workspace` / `--all` command reaches any of
them.** Root `Cargo.toml`:

```toml
members = ["justerm-core", "justerm-wasm-decode"]
exclude = ["fuzz", "justerm-facade", "justerm-renderer"]
```

Every gate expressed as `cargo <cmd> --workspace` or `cargo fmt --all` therefore inspects **two**
crates. Reaching the other three requires naming them: `--manifest-path <crate>/Cargo.toml`.

The failure is silent in the worst way — the excluded crate is not skipped with a warning, it is
**not a thing the command knows about**, so the gate reports success having inspected nothing.

## Why it is cross-cutting

**Three crates, three unrelated reasons, one shared consequence.** Nothing about these decisions was
coordinated; they collide only at the tool's assumption that "the workspace" means "the code".

| Crate | Why it is excluded |
|---|---|
| `fuzz` | nightly + libFuzzer toolchain |
| `justerm-facade` | frozen at `0.5.1` forever — must **not** be dragged by the version-lockstep that ties `justerm-core` and `justerm-wasm-decode` together |
| `justerm-renderer` | `web-sys` / `glow` are wasm32-only, so membership would break the host `cargo test --workspace` outright |

A future exclusion will have a fourth reason and the same consequence. That is what makes this an
invariant rather than a note in any one crate's territory.

## Territories it holds in

- [release](../territory/release.md) — the facade's
  exclusion is what keeps it frozen, and its exclusion is why nothing gates it either
- the renderer crate — the one this has actually bitten. Its territories, all inside a crate no
  `--workspace` command reaches: [cell compositing](../territory/cell-compositing.md) ·
  [cell geometry](../territory/cell-geometry.md) ·
  [glyph atlas](../territory/glyph-atlas.md) ·
  [built-in block glyphs](../territory/builtin-block-glyphs.md)
- [CI & supply chain](../territory/ci-and-supply-chain.md) — where the compensating per-manifest gates live

Derivable half — **two greps, and the second is the one that holds from anywhere** (#608):

```sh
rg -A3 '^exclude' Cargo.toml                       # what THIS root disclaims
rg -l '^\[workspace\]' */Cargo.toml                # which crates declare their own root
```

The first was called "the authority" here until #608 measured otherwise. `exclude` is a list of
paths *relative to the file it sits in*, and cargo resolves a crate's workspace by walking
**upward** — so from a git worktree it climbs past the worktree's root into the main checkout's
manifest, compares the crate against paths that are not this one, and refuses to build it. Every
renderer gate died there, in the workflow `theflow.md` § Step 7 prescribes. `fuzz` and
`justerm-renderer` now each declare `[workspace]`, which is a fact about the directory stated *in*
that directory and therefore survives being reached from anywhere; `justerm-facade` still relies on
the list alone, deliberately (a tombstone nobody edits — see below).

**The two declarations are jointly load-bearing, and that is the safe direction.** A root that lists
a member which itself declares `[workspace]` does not silently win: `cargo metadata` reproduces
*"error: multiple workspace roots found in the same workspace"*. So deleting `justerm-renderer` from
`exclude` can no longer quietly re-include it — it breaks every root `--workspace` gate at once,
loudly, instead of dragging a wasm32-only crate into the host test run.

Non-derivable half — **which gates compensate, and which do not.** `test.yml` names
`justerm-renderer` explicitly in its own job (fmt, test, clippy, build, rustdoc — each by
`--manifest-path`). `fuzz` is reached only by `cargo check --manifest-path fuzz/Cargo.toml`.
`justerm-facade` is reached by **nothing**, deliberately: it is frozen, so there is no version of it
for a gate to protect. No tool can tell you which of those three states a given exclusion is in.

## What a violation looks like

**A green gate that inspected nothing.** Not a failure, not a warning — a passing command with an
empty work set. The reviewer sees a tick and the author sees exit 0.

The tell, when you suspect it: run the gate and check the *count*. `cargo fmt --all` visiting zero
files in a crate you just edited exits 0 exactly like a clean run does.

## Discovery history

| Occurrence | Site | Issue |
|---|---|---|
| 1st | `justerm-renderer` had **no gate at all** across slices #259–#340 — `cargo fmt --all` visited zero of its files and exited 0, and 13 slices of unformatted code accumulated behind that green tick | #333 |
| 2nd | `justerm-wasm-decode/tests/web.rs` held a **second copy** of the wire-version pin, hardcoded to `13`. A `WIRE_VERSION` bump to 14 left every local gate green — including the wasm-target build, which compiled the stale assertion without running it — and CI's `wasm` job was the only red, after the PR was already open | #621 |

Two occurrences, and they differ in the way that matters: the first was *no* gate, the second was a
gate that **ran and passed while answering a different question**. The first is discoverable by
reading the command; the second is not, because the command genuinely reaches the file. When
auditing this invariant, do not stop at "is there a gate" — ask **which of compile / run / assert it
actually performs**.

**A sibling shape, different mechanism, same symptom:** `justerm-wasm-decode/tests/web.rs` is
`#![cfg(target_arch = "wasm32")]`, so on the host it compiles to *nothing* and `cargo test
--workspace` passes over it. Not a workspace exclusion — a target gate — but it produces the same
"green, inspected nothing" outcome. Measured: `cargo test -p justerm-wasm-decode --test web` on the
host reports **`running 0 tests`**, and exits 0.

**And its compensating gate is only half a gate — this is the sharper trap of the two.**
`cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown` reaches the file, so it
is easy to read as coverage. It **compiles** the tests; it never **runs** them. An
`assert_eq!(wire_version(), 13)` against a shipped `14` compiles perfectly — assertions are runtime
constructs and the compiler only checks that comparing two `u8`s type-checks. The runtime assertions
in this file execute in **one place on earth**: CI's `wasm` job, through
`wasm-bindgen-test-runner` under node.

That is worse than the workspace case rather than merely different. An excluded crate has *no* gate
and the absence is discoverable by reading the command; this one has a gate that runs, passes, and
answers a question you did not ask. #621 hit it exactly that way — every local gate green, the wasm
job the only red, on a literal that had rotted.

**The repair generalises past this file: do not put a value in here that lives somewhere else.** The
fix was not to bump `13` to `14` (which rots again at the next bump) but to delete the literal —
`assert_eq!(wire_version(), justerm_core::WIRE_VERSION)` states the binding's actual contract
(*forward the constant*) and cannot go stale. The literal belongs in `justerm-core`'s own test, which
runs on host on every PR. **A duplicated expectation is only as fresh as the slowest place that
checks it** — so the copy that lives where nothing local can run it should assert a *relationship*,
never a *number*.

## Where it will recur

Two directions, and the second is the one that will actually happen:

1. **A new excluded crate.** Anything added to `exclude` starts with zero gates and must have each
   one named for it by hand.
2. **A new gate.** Every gate written as "run over the workspace" inherits this blind spot at birth,
   including gates that are not `cargo` at all — the doc-link checker
   (`.github/scripts/check-map-links.mjs`) takes a hand-written list of roots for exactly the same
   reason, and a new documentation directory is invisible to it until someone adds the path.

Test: if a command's scope is expressed as "everything", ask what its idea of *everything* is, and
compare it against `ls` rather than against the intent.
