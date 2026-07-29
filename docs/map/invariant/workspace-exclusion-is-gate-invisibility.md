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
| `fuzz` | carries its own `[workspace]`; nightly + libFuzzer toolchain |
| `justerm-facade` | frozen at `0.5.1` forever — must **not** be dragged by the version-lockstep that ties `justerm-core` and `justerm-wasm-decode` together |
| `justerm-renderer` | `web-sys` / `glow` are wasm32-only, so membership would break the host `cargo test --workspace` outright |

A future exclusion will have a fourth reason and the same consequence. That is what makes this an
invariant rather than a note in any one crate's territory.

## Territories it holds in

- [release](../territory/release.md) — the facade's
  exclusion is what keeps it frozen, and its exclusion is why nothing gates it either
- **renderer** *(no note yet)* — the crate this has actually bitten
- **infrastructure / CI** *(no note yet)* — where the compensating per-manifest gates live

Derivable half — the exclusion list is one grep, and it is the authority:

```sh
rg -A3 '^exclude' Cargo.toml
```

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

One recorded occurrence, and it ran for thirteen slices before anyone looked — which is the argument
for the node. A defect that announces itself is found once; this one is found only when someone
happens to run the right command by hand.

**A sibling shape, different mechanism, same symptom:** `justerm-wasm-decode/tests/web.rs` is
`#![cfg(target_arch = "wasm32")]`, so on the host it compiles to *nothing* and `cargo test
--workspace` passes over it. Not a workspace exclusion — a target gate — but it produces the same
"green, inspected nothing" outcome, and it is reached by
`cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown`.

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
