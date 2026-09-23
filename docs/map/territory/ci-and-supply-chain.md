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
- **The narrowest gates are the prose ones**, and each is narrow in a different direction: one
  resolves every relative link and `#anchor` across the docs, one checks a single map note as it is
  written, and the published-prose pair asks only whether a pointer is *resolvable from where it is
  printed* — never whether the prose is accurate, which no machine judges.
- **The link gate exists because a broken anchor is silent.** A missing *file* 404s on GitHub and
  in an editor; a missing `#anchor` falls back to the top of the target in both GitHub and Obsidian,
  so a link that landed on one verified row quietly points at a 200-line file. The anchors the map
  depends on are *known* volatile: `reference-facts.md` headings embed issue numbers and verification
  dates (`## Damage / dirty tracking (#536, verified 2026-07-28)`), so a routine re-verification
  re-slugs the heading and breaks the link without touching the linking file.
- **False positives are the failure mode the prose gates design against.** Fenced blocks and inline
  code spans are blanked before links are read — documentation about links quotes link-shaped text
  (`docs/agents/release.md` quotes `[x](../CLAUDE.md)` in a code span) — and replaced with spaces so
  line numbers survive. Lines are split on `/\r?\n/`: on a CRLF checkout a `\n` split leaves `\r` on
  every line, `.` does not match it and `$` does not match before it, so `^#{1,6}\s+(.*)$` found zero
  headings and the checker reported all 11 valid links of its first run as broken.
- **The link gate also enforces three map-integrity rules.** *Reciprocity*: an invariant note names
  the territories it holds in and each must name it back under `## Cross-cutting invariants`, because
  the reading protocol walks that section as a checklist and Obsidian backlinks are neither on GitHub
  nor in that section (its first run found `wide-glyph-and-soft-wrap` missing `row-keyed-side-maps`
  while #557 changed `hyperlink`, a row-keyed side map). *No copied ADR status*: a status restated
  elsewhere has no gate — CLAUDE.md once called four accepted ADRs "proposed" for five days, and a
  territory note later re-introduced a parenthetical copy of one. *Code-comment citations*: a backticked
  `.md` path in a source comment is a link too; #602 replaced a hand-kept list in `term/walk.rs` with
  a pointer to an invariant note, and a pointer that goes stale on a rename is a lateral move, not a
  repair. That last scan is tight on purpose — comment lines, backticked, under a known top-level
  directory, ending `.md`.
- **The per-note check exists so verifying is cheap enough to do mid-write.** 27 notes were once
  written and verified only at the end, and every defect found was the same class, spread across
  notes written hours apart — checking after the third would have ended it. It checks sections per
  note kind (territory, invariant and aggregate have different schemas; applying the territory one to
  an invariant note reported three real notes broken), symbols under `## Code` (declarations *and*
  call/field, enum variant, macro, TOML key, Rust and TS keywords including `impl` for a foreign
  trait, a bare basename resolved anywhere in the source roots), and restated status. `**None.**`
  under `## Code` is a legal state — a design recorded and not built — and stands the symbol check down.
- **`check-tool-pins.mjs` checks that pins agree, never that they are current.** Dependabot never
  edits a `run:` line (`git log -S "cargo install wasm-pack"` returned three commits, all human), so
  the realistic failure is one workflow bumped alone and CI building the artifact with a different
  tool than it tests with. Whether the pin is current is a cost judgement — any version other than
  the runner image's makes cargo compile the tool from source — kept as a release-time trigger
  (`docs/agents/release.md`), because a gate that fails when upstream publishes trains people to
  ignore it.
- **Why each `test.yml` step is shaped the way it is** — the facts the workflow's comments point here:
  - *Toolchain*: taken from `rust-toolchain.toml`, which rustup honours on the first cargo call — no
    install step, and a new Rust release cannot turn CI red on its own.
  - *rustdoc*: before its step, 15 warnings across the family, 12 of them public docs linking
    **private** items — doc-comments are written with the source open and published from public
    items only, so each such link dies silently. The renderer repeats the step by manifest path
    (#333 is the same blind spot with `cargo fmt --all`); the facade gets one so docs.rs's page for
    the tombstone exists for the rustdoc pointer gate.
  - *Map-note schema in CI*: the per-note check was a run-it-yourself habit, which cannot catch a
    note edited later by someone who never ran it — two invariant notes
    (`cell-size-is-derived-state`, `composition-is-browser-owned-state`) sat missing `## Where it will
    recur` until an unrelated change happened to run the script.
  - *Renderer lockfile*: `--locked` sits on the job's first **bare** cargo command (#613), the only
    kind it works through — `wasm-pack … -- --locked` resolves and rewrites the lock before cargo sees
    the flag, so an assertion there passes while inspecting nothing.
  - *`renderer-proofs`*: Chromium only — xterm.js skips firefox/webkit on Linux because "webgl2 is
    often not supported in headless firefox on Linux" (`addon-webgl/test/WebglRenderer.test.ts`). The
    colour-emoji font is installed twice over — the ubuntu image lists `fonts-noto-color-emoji` in
    `toolsets/toolset-2404.json` `apt.common_packages`, and `playwright install --with-deps` always
    installs its `tools` group (`fonts-noto-color-emoji`, `fonts-liberation`, `fonts-wqy-zenhei`)
    whatever browser is named (playwright-core `registry/index.ts`, `targets.add('tools')`) — and the
    emoji pages still assert `colourEmojiFontPresent` so a font-less runner fails by name (#334). No
    WebGL flag is passed: Playwright injects `--enable-unsafe-swiftshader` (`chromium.ts`). The
    renderer's `.d.ts` gate is built ahead of the proofs so a flaky proof cannot hide a prose defect,
    from `--target web --dev` rather than the published `bundler` — measured to differ only in
    wasm-pack's own init-function docs.
  - *pnpm is pinned `@10`*: pnpm 11 defaults `strictDepBuilds` to true and turns the skipped
    lifecycle scripts (`esbuild`, `@swc/core`) from a warning into `ERR_PNPM_IGNORED_BUILDS`. Skipping
    them is harmless — esbuild resolves `@esbuild/linux-x64` from optionalDependencies at runtime.
  - *`web`*: consumes the **published** `justerm-wasm-decode`, so it needs no Rust. Its `tsup` build
    step guards only the artifact path: six probes (an emit-only TS4023, a `.d.ts` rollup of an
    `external` package's types, a bare type re-export, an `export *` collision, a missing entry) all
    reddened `tsc --noEmit` first where they failed at all (#344).
  - *wasm-pack vs cargo-fuzz*: both installed through cargo, not a third-party action, but only
    wasm-pack is version-pinned (#616) — `fuzz.yml` publishes nothing and runs a floating nightly, so
    pinning its tool would be theatre. While the pin matches the runner image, cargo installs nothing.
- **The supply-chain scan is first-party** (`just-shield`, a sibling repo, itself SHA-pinned), which
  makes the scanner a dependency of the same kind it exists to police.

## Code

- `.github/workflows/test.yml` — the gate: `test`, `renderer`, `renderer-proofs`, `web`, `web-e2e`,
  `wasm`
- `.github/workflows/fuzz.yml` · `supply-chain.yml`
- `.github/workflows/publish-crate.yml` · `publish-wasm.yml` · `publish-renderer.yml` ·
  `publish-web.yml`
- `.github/scripts/check-map-links.mjs` · `check-map-note.mjs` · `check-tool-pins.mjs`
- The **published-prose** gates — `check-published-readme.mjs` (publish time),
  `check-published-pointers.mjs` and `check-published-rustdoc.mjs` (both per PR) — are enumerated
  and argued in [published surface](published-surface.md), which owns that surface. This note holds
  only that they run in CI. A hand-copied second list is what left `check-published-pointers.mjs`
  unnamed here for the whole of its life before #953
- `justerm-wasm-decode/tests/readme_pins.rs`
- `.github/dependabot.yml` — what keeps the SHA pins current (ADR-0006 leans on it), and since #616
  the renderer's own dependency graph too. Note what it cannot reach: a version literal inside a
  `run:` line, which is why the `wasm-pack` pin is held by `check-tool-pins.mjs` instead
- `docs/agents/theflow.md` §"Step 7 — gate matrix" is the operational list, and is **not** a decision
  record

## Reference behaviour

**None** in `docs/agents/reference-facts.md`, and unlike every other territory the comparison set
does not obviously apply — a gate matrix is a property of this project's risk surface rather than of
terminal emulation.

That stayed true when a *harness* section was added to `reference-facts.md` (#733, #731), which is
worth saying because the two look adjacent: how a comparable project structures its Playwright suite
is a fact about [browser proof harness](browser-proof-harness.md), the territory two of the jobs
below run. **Which checks exist** still has no comparand.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the defining hazard: a command whose scope silently excludes what you meant to check reports
  success having inspected nothing
- [an awaited in-page promise needs an anchor](../invariant/an-awaited-in-page-promise-needs-an-anchor.md)
  — the same shape one layer in: `web-e2e` and `renderer-proofs` can fail for a reason that is not
  the one they report, which costs a gate its credibility rather than its coverage

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
