# Territory — CI & supply chain

## What it is

What actually runs before a change lands, and what protects the paths that publish. Seven workflows:
one gate (`test`), one adversarial job (`fuzz`), one supply-chain scan, and four publishers.

The territory's defining property is **coverage, not correctness** — every gate here is honest about
what it inspects, and the recurring defect is a gate that passes over something it was never given.

## Governing decisions

- [**ADR-0006 — supply-chain action pinning**](../../adr/0006-supply-chain-action-pinning.md) — every
  GitHub Action reference is SHA-pinned and scanned `--strict` by a first-party tool
- [**ADR-0007 — robustness testing: property and fuzz**](../../adr/0007-robustness-testing-property-and-fuzz.md)
  — why a fuzz job exists beside the unit gate

Nothing governs the gate matrix itself — which checks exist, and what each is allowed not to see.

## Design model

- **Seven workflows, and the jobs inside `test` are split on purpose.** The deterministic cargo gate
  is kept apart from anything that downloads a browser or depends on runner fonts, because *a gate
  people learn to ignore is not a gate* — one flaky job beside a reliable one degrades both.
- **Every excluded crate needs its gate named for it.** `--workspace` and `--all` reach two crates;
  the renderer, the fuzz crate and the frozen facade are each addressed by `--manifest-path` or not at
  all. See the invariant below — this is the single most repeated shape here.
- **Two README checks at two different moments.** A constant a README *quotes* is pinned by a host
  unit test and fails on **every PR**; a claim that *expires* is rejected at **publish time only**,
  because a scaffold may honestly call itself one in the repo and not on a registry.
- **rustdoc is its own lint layer.** `cargo test` runs doctests, not link resolution, and clippy does
  not carry rustdoc's lints — so public doc links had no mechanical check until one was added.
- **The doc-link gates are the newest members and the most narrowly scoped**: one resolves every
  relative link and `#anchor` across the docs, the other checks a single map note as it is written.
- **The supply-chain scan is first-party** (`just-shield`, a sibling repo, itself SHA-pinned), which
  makes the scanner a dependency of the same kind it exists to police.

## Code

- `.github/workflows/test.yml` — the gate: `test`, `renderer`, `renderer-proofs`, `web`, `web-e2e`,
  `wasm`
- `.github/workflows/fuzz.yml` · `supply-chain.yml`
- `.github/workflows/publish-crate.yml` · `publish-wasm.yml` · `publish-renderer.yml` ·
  `publish-web.yml`
- `.github/scripts/check-published-readme.mjs` · `check-map-links.mjs` · `check-map-note.mjs`
- `justerm-wasm-decode/tests/readme_pins.rs`
- `docs/agents/theflow.md` §"Step 7 — gate matrix" is the operational list, and is **not** a decision
  record

## Reference behaviour

**None** in `docs/agents/reference-facts.md`, and unlike every other territory the comparison set
does not obviously apply — a gate matrix is a property of this project's risk surface rather than of
terminal emulation.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the defining hazard: a command whose scope silently excludes what you meant to check reports
  success having inspected nothing

## Blast radius

- [release](release.md) — four of the seven workflows are publishers, and a tag is what fires them
- [published surface](published-surface.md) — two of the checks exist solely to keep published prose
  honest, at two different moments
- Every territory, indirectly: a gate that stops covering an area turns that area's other guarantees
  into conventions

## Known holes / open

- **The gate matrix has no governing record.** Which checks exist, and what each deliberately does
  not see, is documented operationally in a process file rather than decided anywhere.
- **The scanner is a supply-chain dependency of the supply-chain gate.** First-party and pinned, but
  the recursion is unaddressed in the record.
- **Coverage is asserted, not measured.** Nothing reports which crates a given gate actually visited,
  which is precisely the failure the invariant above describes — the fix each time has been to name
  the crate by hand after someone noticed.
- **The two doc-link gates overlap deliberately** — one batch, one per-note — and nothing states
  which is authoritative if they ever disagree.
