# Territory — published surface

## What it is

Everything a stranger reads before writing a line of code, and everything that is frozen the moment
it ships. Registries snapshot a README at publish time, so it is the front page every new consumer
sees — and **nothing about it is compiled.** No test imports it, no compiler sees it, no constant in
it is checked against the constant it names unless someone builds that check.

How a version gets there is [release](release.md).

## Governing decisions

**None.**

- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) decided the
  *names* on the registries, and produced the tombstone below — but nothing governs what the
  published prose must say or how it is kept true

## Design model

- **Immutability is the whole constraint.** crates.io and npm never rewrite a published artifact;
  only a yank comes back. Every other area is fixable by a commit — here a mistake is a permanent row
  in someone else's dependency graph.
- **Two mechanised checks, deliberately at different moments.**
  `justerm-wasm-decode/tests/readme_pins.rs` ties a constant the README *quotes* to the constant that
  owns it and fails on **every PR**. `.github/scripts/check-published-readme.mjs` rejects expiring
  claims — *"under construction"*, *"lands in #N"*, *"coming soon"* — at **publish time only**,
  because an in-progress crate may honestly call itself a scaffold in the repo. Snapshotting that
  sentence onto a registry is what makes it a lie.
- **The publish-time check fires after the tag is already pushed**, so a false positive costs a
  re-tag. That is why its phrase list is kept tight rather than thorough.
- **crates.io rewrites relative links**, resolving them against the crate's README subdirectory —
  so `[x](../CLAUDE.md)` in a crate README does reach the repo root. npm does **not**, and
  `justerm-web@0.7.0` shipped two broken links because of it (#473).
- **The family consumes its own published surfaces, so a mirror of one is the same uncompiled prose
  as a README — except a type can be gated and a paragraph cannot.** `justerm-web` depends on the
  *published* `justerm-wasm-decode` and `justerm-renderer` by version range, and declares each one's
  shape itself. It has a gate on exactly one of those two seams. `JustermRenderer.create` binds the
  real renderer class to its own `RendererBackend` by a **typed declaration, not a cast**, so a
  signature drift in the published renderer is a compile error here; it fired on its first real test
  (#645), naming an `apply_damage` call site that a hand-written list of sites did not contain. The
  decoder seam has no equivalent and, as declared, cannot: `DecodedFrame` types every column
  `ArrayLike<number>` *deliberately*, so that plain-object demo and test fixtures satisfy it — and
  that accepts a `Uint16Array` and a `Uint32Array` alike. #627 lived in exactly that gap for a
  release, the decoder having widened its cluster-index column to u32 while the adapter went on
  narrowing it back.
- **A version range decides when a consumed drift becomes reachable, which is not when it is
  introduced.** An npm 0.x caret is `>=0.N.0 <0.N+1.0`, so a widened column published by one family
  member is inert in another until that pin moves. The window a mismatch is dangerous in opens at
  the *bump*, not at the *tag* — which is where a gate on the paragraph above would fire, and why
  #633 sequences its steps rather than treating the tag as the deadline.
- **The tombstone is a published surface with no code behind it.** `justerm-facade` exists so that
  `justerm = "0.5"` dependants keep compiling *while learning the name changed* — its entire purpose
  is the message, and its fourteen lines of `pub use` are the delivery mechanism.

## Code

- `justerm-core/README.md` · `justerm-wasm-decode/README.md` · `justerm-renderer/README.md` ·
  `justerm-web/README.md` · `justerm-facade/README.md`
- `justerm-wasm-decode/tests/readme_pins.rs` — the constant pin (per-PR)
- `.github/scripts/check-published-readme.mjs` — the expiring-claim gate (publish-time)
- Public doc-comments in `justerm-core/src/lib.rs` — they ship verbatim as the docs.rs page
- `justerm-web/src/types.ts` — `DecodedFrame`, web's mirror of the published decoder's getters;
  width-agnostic by contract, so it gates a column's presence and never its width
- `justerm-web/src/justerm-renderer.ts` — `RendererBackend`, web's mirror of the published
  renderer, and the typed binding in `JustermRenderer.create` that is the only drift gate either
  mirror has
- `justerm-web/package.json` — the two version ranges that decide when a consumed drift is reachable

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the tombstone's README is published and reached by no gate at all, because the crate it belongs
  to is outside every `--workspace` command

## Blast radius

- [release](release.md) — publishing is what freezes this, so the two are one event seen from two
  sides
- **Every territory whose behaviour a README or doc-comment describes.** This surface is a *mirror*
  of the others, and a mirror drifts silently: `justerm-renderer/README.md` announced "the GPU
  pipeline lands in #260+" across six published versions, and `justerm-wasm-decode/README.md` told
  readers to assert `wireVersion() === 2` against a shipped 12
- [wire format](wire-format.md) — the constant most often quoted in published prose, and the one the
  per-PR pin exists for
- [frame](frame.md) · [frame adapter](frame-adapter.md) — the shapes web mirrors. A column added or
  retyped there reaches the widget only through the two declarations under `## Code`, and only once
  a version range moves

## Known holes / open

- **Zero governing records** for a surface whose defining property is that it cannot be corrected.
- **Doc-comments are a published surface with a narrower gate than READMEs.** The expiring-claim
  check reads READMEs only, which is how `Engine::resize` carried *"(Soft-wrap reflow lands in #7.)"*
  on docs.rs for six weeks after #7 closed.
- **Only one constant is pinned.** `readme_pins.rs` covers `wireVersion()`; any other number a README
  quotes is unchecked, and a README that starts quoting a new one gets no pin unless someone adds it.
- **One of web's two consumed seams is ungated, and the other one is the proof that gating works**
  (#646). Both instances found so far were found by a person, not by a check: a missing *field*
  (#129/#135, `mouseWantedEvents` reaching `types.ts` only at S16) and a disagreeing *width* (#627).
  Whether the fix is worth its coupling — deriving one mirror's declarations from another package's
  types — is undecided, and #646 records the ceiling: such a gate fires at the pin bump, so it makes
  the window's *end* automatic rather than catching the drift earlier.
