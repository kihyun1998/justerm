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
- **The tombstone is a published surface with no code behind it.** `justerm-facade` exists so that
  `justerm = "0.5"` dependants keep compiling *while learning the name changed* — its entire purpose
  is the message, and its fourteen lines of `pub use` are the delivery mechanism.

## Code

- `justerm-core/README.md` · `justerm-wasm-decode/README.md` · `justerm-renderer/README.md` ·
  `justerm-web/README.md` · `justerm-facade/README.md`
- `justerm-wasm-decode/tests/readme_pins.rs` — the constant pin (per-PR)
- `.github/scripts/check-published-readme.mjs` — the expiring-claim gate (publish-time)
- Public doc-comments in `justerm-core/src/lib.rs` — they ship verbatim as the docs.rs page

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

## Known holes / open

- **Zero governing records** for a surface whose defining property is that it cannot be corrected.
- **Doc-comments are a published surface with a narrower gate than READMEs.** The expiring-claim
  check reads READMEs only, which is how `Engine::resize` carried *"(Soft-wrap reflow lands in #7.)"*
  on docs.rs for six weeks after #7 closed.
- **Only one constant is pinned.** `readme_pins.rs` covers `wireVersion()`; any other number a README
  quotes is unchecked, and a README that starts quoting a new one gets no pin unless someone adds it.
