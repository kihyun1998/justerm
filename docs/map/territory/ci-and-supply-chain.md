# Territory — CI gates and action pinning

## What it is

What runs before a change lands, and the conventions that keep the workflows themselves trustworthy.
That is the per-PR gate (`test.yml`, seven jobs), the scripts under `.github/scripts/` that the gate
runs over the docs and the workflows, Dependabot, and the action-pinning convention every workflow
follows. The scheduled fuzz workflow is [robustness testing](robustness-testing.md)'s, and the four
publish workflows are [release](release.md)'s; this note holds only what they share with the gate
(pins, permissions, tool installs).

The defining property is **coverage, not correctness**. Each gate is honest about what it inspects,
and the defect that keeps coming back is a gate passing over something it was never given.

## Governing decisions

- [ADR-0006 — pin third-party actions to a commit SHA; least privilege](../../adr/0006-supply-chain-action-pinning.md)
  — every `uses:` is SHA-pinned and every workflow declares `permissions:`. Its 2026-10-12 amendment
  retired the just-shield scan that enforced it (commit `ecc34ff` deleted the `supply-chain`
  workflow), so the rule is now a convention that only review enforces
- [ADR-0034 — the family owns the PTY loop](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  D6 — why the `pty` job runs `justerm-pty`'s real-process tests on macOS and Windows

Adjacent, not governing: [ADR-0007](../../adr/0007-robustness-testing-property-and-fuzz.md) decides
the fuzz lane ([robustness testing](robustness-testing.md)), not the gate matrix. **Nothing governs
the gate matrix itself** — which checks exist, and what each is allowed not to see.
`docs/agents/theflow.md` § "Step 7 — gate matrix" is a retired process list, and `CLAUDE.md` names
the `run:` lines of `test.yml` as the authoritative gate list.

## Design model

Read from the source (workflow and script comments) unless a record is named.

- **Seven jobs in `test.yml`, split on purpose** (workflow comments): `test` (fmt, clippy, test,
  rustdoc, the doc and map gates, the pin and published-prose gates), `renderer` (the excluded
  renderer crate by manifest path), `renderer-proofs` (browser), `web` (typecheck, vitest, tsup),
  `web-e2e` (browser), `pty` (macOS + Windows, ADR-0034 D6) and `wasm` (the decoder on wasm32 plus
  the node smokes). Anything that downloads a browser or depends on runner fonts is kept apart from
  the deterministic cargo gate, because one flaky job beside a reliable one degrades both.
- **`--workspace` reaches three crates** — `justerm-core`, `justerm-wasm-decode` and `justerm-pty`
  (root `Cargo.toml` `members`). `fuzz`, `justerm-facade` and `justerm-renderer` are excluded, so each
  gate for them is named by `--manifest-path` or does not exist
  ([workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)).
  At HEAD the renderer has five named cargo gates plus the `.d.ts` gate in `renderer-proofs`, the
  facade has one (`cargo doc`, for the rustdoc pointer gate), and the fuzz crate has **none** per PR.
- **The published-prose gates run here but are argued in [published surface](published-surface.md).**
  Three run in this workflow (`check-published-pointers.mjs`, `check-published-rustdoc.mjs`, and
  `check-published-package.mjs` in `wasm` and `renderer-proofs`); the fourth,
  `check-published-readme.mjs`, runs only in the publish workflows. A second list of them here is
  what left `check-published-pointers.mjs` unnamed in this note before #953.
- **rustdoc is its own lint layer.** `cargo test` runs doctests and clippy does not carry rustdoc's
  lints, so `cargo doc` runs with `RUSTDOCFLAGS=-D warnings`. Before that step there were 15
  warnings across the family, 12 of them public docs linking private items (this note, before
  2026-10-22). The renderer repeats it by manifest path (#333 is the same blind spot with
  `cargo fmt --all`).
- **The map and doc gates are the narrowest, each in its own direction.**
  `check-map-links.mjs` resolves every relative link and `#anchor` under `docs/`, `CLAUDE.md`,
  `GLOSSARY.md` and `README.md`; `check-map-note.mjs` checks one note's shape; the published-prose
  gates ask only whether a pointer resolves from where it is printed. None judges whether prose is
  accurate.
- **The link gate exists because a broken anchor is silent.** A missing file 404s; a missing
  `#anchor` falls back to the top of the target on GitHub and in Obsidian. The anchors the map
  depends on are volatile: `reference-facts.md` headings carry issue numbers and verification dates,
  so a re-verification re-slugs the heading without touching the linking file.
- **The prose gates design against false positives.** Fenced blocks and inline code spans are
  blanked before links are read (documentation about links quotes link-shaped text), with spaces, so
  line numbers survive. Lines split on `/\r?\n/`: on a CRLF checkout a `\n` split leaves `\r` on
  every line and the heading regex matched nothing — its first run reported all 11 valid links
  broken (this note, before 2026-10-22).
- **The link gate enforces three map-integrity rules besides links** (`check-map-links.mjs`).
  *Reciprocity*: every `../territory/*.md` link in an invariant note must be answered by the
  territory's `## Cross-cutting invariants`; its first run found `wide-glyph-and-soft-wrap` missing
  `row-keyed-side-maps` while #557 changed a row-keyed side map. *No copied ADR status*: a status
  restated elsewhere has no gate (`CLAUDE.md` once called four accepted ADRs "proposed" for five
  days). *Code-comment citations*: a backticked `.md` path in a comment line under a known code root
  is checked like a link (#602).
- **The per-note check makes verifying cheap enough to do mid-write** (commit `87edad1`). It checks
  sections per note kind, symbols under `## Code`, and restated status; `**None.**` under `## Code`
  stands the symbol check down. CI runs it over every note, because the habit cannot reach someone
  editing a note later: two invariant notes sat without `## Where it will recur` until an unrelated
  change ran the script by hand.
- **"The tree" for the symbol check is what `git ls-files --cached --others --exclude-standard`
  lists** (#1016), so a note naming a file not yet added passes mid-write. The hand-written root list
  it replaced had no `e2e/`, `demo/` or `test/` tree, no `justerm-facade/src` and no
  `justerm-core/tests`.
- **A token shape the collector does not match is never checked, and passes like a resolved one.**
  Since #1016 it takes each identifier segment of `a`, `a()`, `a.b` or `a::b`; before, no camelCase
  name was checked and a stale `requestBottom` passed (fixed in #1015). Still unchecked: generics, an
  expression, a call with arguments, a `*` wildcard, `?.`, and file names with an extension outside
  `.rs .ts .mjs .yml .toml`.
- **Resolution is textual over the whole tree with comments stripped** (#1016), so a name in call or
  field form passes whether or not it is ours. Comments are out because a concept label is not a
  symbol: `COMBINED_PRESENT` and `LINK_PRESENT` resolved through `cell.rs` comments for bits the code
  names `C_COMBINED` and `BG_LINK`. The stripper is line-based and string-unaware, so `//` inside a
  string hides the rest of the line — a loud false red, never a silent pass. A symbol belonging to a
  reference is named under `## Reference behaviour`, never `## Code` (maintainer, #1016), but the
  gate enforces that only for names the tree never mentions: `bench/xterm-compare` drives xterm.js's
  API, so `Terminal` or `dispose` resolve whoever they belong to.
- **`check-tool-pins.mjs` checks that pins agree, never that they are current** (#616). Dependabot
  never edits a `run:` line (`git log -S "cargo install wasm-pack"`: three commits, all human), so
  the realistic failure is one workflow bumped alone. Whether the pin should move is a release-time
  decision in `docs/agents/release.md`, because a gate that fails whenever upstream publishes trains
  people to ignore it.
- **Why each `test.yml` step is shaped the way it is** — the facts its comments point here for:
  - *Toolchain*: `rust-toolchain.toml` pins `1.96.0`, which rustup honours on the first cargo call,
    so there is no install step and a new Rust release cannot turn CI red by itself.
  - *Renderer lockfile*: `--locked` sits on the job's first **bare** cargo command (#613), because
    `wasm-pack … -- --locked` resolves and rewrites the lock before cargo sees the flag.
  - *`renderer-proofs`*: Chromium only (xterm.js skips firefox/webkit on Linux for the same webgl2
    reason, `addon-webgl/test/WebglRenderer.test.ts`, per this note before 2026-10-22). The emoji font is
    installed twice over — the ubuntu image lists `fonts-noto-color-emoji` in `toolset-2404.json`,
    and `playwright install --with-deps` always adds its `tools` group (playwright-core
    `registry/index.ts`) — and the emoji pages still assert `colourEmojiFontPresent` so a font-less
    runner fails by name (#334). No WebGL flag is passed: Playwright injects
    `--enable-unsafe-swiftshader` (`chromium.ts`). The `.d.ts` gate builds ahead of the proofs so a
    flaky proof cannot hide a prose defect, from `--target web --dev` rather than the published
    `bundler` target — measured to differ only in wasm-pack's own init-function docs.
  - *pnpm is installed as `pnpm@10`*: pnpm 11 defaults `strictDepBuilds` to true and turns the skipped
    `esbuild` / `@swc/core` lifecycle scripts into `ERR_PNPM_IGNORED_BUILDS`.
  - *`web`* consumes the **published** `justerm-wasm-decode`, so it needs no Rust. Its tsup step
    guards the artifact path only: six probes that broke the public types all reddened
    `tsc --noEmit` first where they failed at all (#344).
  - *wasm-pack vs cargo-fuzz*: both are installed through cargo, not a third-party action, but only
    wasm-pack is pinned (#616) — `fuzz.yml` publishes nothing and runs a floating nightly
    (`fuzz.yml`'s own comment). While the pin equals the runner image's version, the install is a
    no-op.
- **Pinning and permissions, as they stand at HEAD** (read from the workflows): every `uses:` is a
  40-character SHA with a version comment; the only third-party action is `Swatinem/rust-cache`, in
  `test.yml` jobs only; every workflow declares `permissions: contents: read`, and only publish jobs
  raise `id-token: write`. Dependabot advances the SHA pins weekly (`github-actions` ecosystem) and
  watches two cargo roots, `/` and `/justerm-renderer` (#616).
- **Outside input** is a pull request's tree. Nothing bounds its size; each job is bounded only by
  GitHub's six-hour default, except `pty`, which sets `timeout-minutes: 20` because a child that
  never exits would otherwise hold the runner.

## Code

- `.github/workflows/test.yml` — jobs `test`, `renderer`, `renderer-proofs`, `web`, `web-e2e`, `pty`,
  `wasm`
- `.github/workflows/fuzz.yml` — owned by [robustness testing](robustness-testing.md)
- `.github/workflows/publish-crate.yml` · `publish-wasm.yml` · `publish-renderer.yml` ·
  `publish-web.yml` — owned by [release](release.md)
- `.github/scripts/check-map-links.mjs` — `slugify`, `anchorsOf`, the reciprocity, status and
  comment-citation passes
- `.github/scripts/check-map-note.mjs` — `TERRITORY_SECTIONS`, `INVARIANT_SECTIONS`, `stripComments`
- `.github/scripts/check-tool-pins.mjs` — `WASM_PACK_VERSION` agreement across workflows
- `.github/scripts/cite.mjs` — generates a `reference-facts.md` Site cell from a pinned tree (#610);
  not run by CI
- `.github/dependabot.yml` — the `github-actions` entry and the two cargo entries
- `rust-toolchain.toml` — the pinned toolchain
- Root `Cargo.toml` — `members` and `exclude`, which decide what `--workspace` means

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md` compares a gate matrix; which checks exist is
a property of this project's risk surface. How a comparable project structures its Playwright suite
is [browser proof harness](browser-proof-harness.md)'s comparison, not this one.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — a command whose scope silently leaves out what you meant to check reports success having
  inspected nothing
- [an awaited in-page promise needs an anchor](../invariant/an-awaited-in-page-promise-needs-an-anchor.md)
  — `web-e2e` and `renderer-proofs` can fail for a reason other than the one they report, which costs
  a gate its credibility rather than its coverage

## Blast radius

- [release](release.md) — the publish jobs share this note's pin and permission convention; a pin
  bumped in `test.yml` alone means CI builds the artifact with a different tool than the tag ships it
- [published surface](published-surface.md) — its gates run here; a step removed or reordered (the
  rustdoc gate reads `cargo doc` output) silently stops checking prose that ships
- [browser proof harness](browser-proof-harness.md) — two jobs are those suites; a job that reports
  machine speed as a defect spends the whole gate's credibility
- [robustness testing](robustness-testing.md) — `cargo test --workspace` is what runs the proptest
  lane; a gate that stops reaching `justerm-core/tests` drops it with no red
- [pty](pty.md) — the only territory whose tests depend on the runner's OS; dropping a matrix leg
  leaves that OS unproven with a green tick
- Every territory, indirectly: a gate that stops covering an area turns that area's guarantees into
  conventions

## Known holes / open

- **The gate matrix has no governing record.** Which checks exist, and what each deliberately does
  not see, is written only in workflow comments and this note.
- **Action pinning is a convention, not a gate** (ADR-0006 amendment). Nothing stops a `uses:` on a
  mutable tag, or a third-party action entering a job that holds `id-token: write`, except review.
- **The fuzz crate is compiled by nothing per PR.** No `test.yml` step names `fuzz/Cargo.toml`, so a
  `justerm-core` API change that breaks a fuzz target surfaces only at the next weekly `fuzz.yml`
  run (found 2026-10-22; `README.md` and the workspace-exclusion invariant still describe a
  `cargo check --manifest-path fuzz/Cargo.toml`, which no workflow runs).
- **Coverage is asserted, not measured.** Nothing reports which crates or files a gate visited; each
  fix so far has been to name the crate by hand after someone noticed.
- **The two doc-link gates overlap** — one batch, one per-note — and nothing says which is
  authoritative if they disagree.
- **The symbol check's unchecked shapes** (generics, expressions, calls with arguments, other file
  extensions) pass exactly like resolved names.
