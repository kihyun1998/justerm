# Territory — release & published surface

## What it is

What leaves the repo and reaches a stranger: five artifacts across two registries, the tag that
pushes each, and everything a consumer reads before writing a line of code. **Publication is
one-way** — crates.io and npm are immutable, and the only thing that comes back is a yank.

That immutability is why this is a territory rather than a chore. Every other area can be fixed by a
commit; here a mistake is a permanent row in someone else's dependency graph.

## Governing decisions

- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) — the rename
  that produced the tombstone below
- [ADR-0008 — wasm-decode as a separate crate](../../adr/0008-wasm-decode-binding-separate-crate.md)
  and [ADR-0005](../../adr/0005-binary-reference-based-serialization.md) — together they require the
  **version lockstep**: one bump moves core and the wasm binding
- [ADR-0006 — supply-chain action pinning](../../adr/0006-supply-chain-action-pinning.md) — the
  publish workflows are the paths this protects
- `docs/agents/release.md` is the operational procedure and is **not** a decision record — read it
  for how, read the ADRs above for why

## Design model

- **Tag-driven and automatic.** Pushing a tag publishes; there is no confirmation step. Three tracks:

  | tag | publishes | to |
  |---|---|---|
  | `v*` | `justerm-core` **and** `justerm-wasm-decode` | crates.io **and** npm |
  | `renderer-v*` | `justerm-renderer` | npm |
  | `web-v*` | `justerm-web` | npm |

  **One tag, two registries** is the non-obvious one: `v*` fires `publish-crate.yml` *and*
  `publish-wasm.yml`. That is the lockstep made mechanical. A consumer reading only the crates.io
  version has half the picture.
- **`justerm-renderer` does not ship on `v*`.** It carries its own track and its own version, because
  it is workspace-excluded and not part of the lockstep. A `v*` release leaves it untouched.
- **The tombstone.** `justerm-facade/` publishes as the crate name **`justerm`**, version `0.5.1`,
  fourteen lines of `pub use justerm_core::*`. Published **manually, once, after** `justerm-core`
  0.6.0 was live (it depends on it, so it could not be built inside the rename PR). It has **no tag
  track and no gate**, and its own manifest says *"Do not re-publish; do not update."* It exists so
  `justerm = "0.5"` dependants keep compiling while learning the name changed.
- **The README is a behaviour surface, not packaging.** Registries snapshot it at publish time, so it
  is the first thing every new consumer reads and nothing about it is compiled. Two mechanised
  checks, deliberately at different moments: `justerm-wasm-decode/tests/readme_pins.rs` ties a
  constant the README *quotes* to the constant that owns it and fails on **every PR**;
  `.github/scripts/check-published-readme.mjs` rejects expiring claims ("under construction", "lands
  in #N") at **publish time only**, because an in-progress crate may honestly call itself a scaffold
  in the repo — snapshotting that sentence onto a registry is what makes it a lie.
- **Release notes are GitHub Releases; there is no `CHANGELOG.md`.** A published entry is never
  rewritten; a correction opens a new note.

## Code

- `.github/workflows/` — `publish-crate.yml`, `publish-wasm.yml`, `publish-renderer.yml`,
  `publish-web.yml`
- `.github/scripts/check-published-readme.mjs` — the expiring-claim gate
- `justerm-wasm-decode/tests/readme_pins.rs` — the README-constant pin
- `justerm-facade/` — `Cargo.toml`, `src/lib.rs`, `README.md` (the whole tombstone)
- Root `Cargo.toml` — `[workspace.package] version` is the lockstep's single source

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md` — how comparable projects stage a rename, or
carry a tombstone crate, has never been checked against a pinned tree. ADR-0010's reasoning stands on
naming convention rather than on a measured comparison.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the facade's exclusion is *why* it stays frozen, and equally why nothing gates it

## Blast radius

- [frame & wire](frame-and-wire.md) — a `WIRE_VERSION` bump ships on `v*` to two registries at once,
  and a consumer decoding a wrong layout gets garbage cells rather than an error. This is one of
  theflow's unconditional Step 5 triggers for that reason
- **renderer** *(no note yet)* — its own track means a `v*` release does **not** carry it;
  a change needing both must push two tags
- **a11y / web widget** *(no note yet)* — `justerm-web` consumes the *published* wasm decoder, not a
  workspace link, so a new binding is `undefined` at runtime until the wasm package is republished
- Every territory whose behaviour a README describes — the README is the surface, not the code

## Known holes / open

- **The tombstone has no gate by construction.** Nothing verifies it still compiles against a current
  `justerm-core`, because it pins `justerm-core = "0.6"` and is frozen. If that ever stops resolving,
  the failure appears to a stranger on crates.io, not here.
- **Two of the five artifacts have no note in this map** (`justerm-renderer`, `justerm-web`), so the
  blast list above points at absences rather than at territories.
- **No reference comparison** for any of the release model, per §Reference behaviour.
