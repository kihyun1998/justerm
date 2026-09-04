# ADR-0030: Which directory owns what — the tree rule, defended difference by difference

Status: **accepted** (2026-09-04). The rule and its divergence list were authored by `plat` on
2026-08-31 against seven maintainer-confirmed layout peers, and lived in the generated `thegraph`
build until that build was retired. **No other file in this repo restates them**, and neither is
re-derivable without running that comparison again — which is why they are a record rather than a
deletion.

**Scope.** This record says *where a file goes*. It does not say what any layer may depend on — that
is ADR-0017's, and every row below that cites a boundary cites it. It is also not a style guide: a
row exists only where this tree **differs from a named peer**, or where a split was suspected and
measured away.

## Context

In a repo whose identity *is* a boundary, the directory boundary is where the seam is **physically
expressed**. A file written to the wrong directory breaks the seam while producing **no error, no
failing test and no warning** — and everything the rule would have said arrives later as rework: the
imports, the module wiring, the history. Nothing else in the toolchain reaches this class of mistake.

Two prior inputs already constrained the tree, and the rule below is **sharper than either**, not in
conflict with them. Counted 2026-08-31: ADR-0010's crate-prefix rule holds 5/5 with one *recorded*
exception (`justerm-facade/` → package `justerm`, the tombstone); the shape ↔ `Term`-half split is
declared in the doc-comments at both ends and holds 3/3 (`search`, `selection`, `logical`); each
workspace exclusion carries its reason in its `Cargo.toml`.

**Four suspected splits were sorted by content rather than by filename**, because two directories
holding the same *kind* of file by name routinely hold two different *roles*. None survived as a
question:

| Suspected split | What the sort was on | Result |
|---|---|---|
| `justerm-core/src/<x>.rs` vs `src/term/<x>.rs` | the returned **shape** and the coordinate model it documents / the **`Term` half**, cell-aware and needing the whole buffer | 3 / 3, clean |
| core: inline `mod tests` vs files in `tests/` | module-internal unit / through the public API | **overlap 0** — no module is tested from both sides |
| renderer: inline everywhere vs **no** `tests/` | pure module / `webgl.rs` + `rasterizer.rs`, wasm32-only and 0-compiling on host, so their proof is `e2e/` | every module but those two |
| two script homes | CI's / the package's own | clean by owner |

Where the sort comes out clean, the measured rule **is** the rule.

## Decision

### D1 — The rule is a path list, because a rule stated as a layer cannot be matched against a diff

```
justerm-core/          the engine crate — VT parsing, grid, scrollback, cursor, selection,
                       serialize. No I/O, no drawing
justerm-wasm-decode/   the wasm binding crate and the npm artifact built from it
justerm-renderer/      the WebGL2 renderer crate (wasm32-only; carries its own [workspace])
justerm-web/           the browser widget TS package
justerm-facade/        the frozen `justerm` name tombstone. Directory name != package name — the
                       single recorded exception (ADR-0010)
fuzz/                  cargo-fuzz targets (own [workspace], package `justerm-fuzz`)
bench/<name>/          a cross-implementation comparison harness belonging to no crate
.github/scripts/       CI's scripts
teach/                 the learning-course workspace

<crate>/src/*.rs                one module per concern, flat
justerm-core/src/<x>.rs         the returned shape, and the coordinate model it documents
justerm-core/src/term/<x>.rs    the `Term` half — cell-aware logic needing the whole buffer
<crate>/src/<x>.rs + <x>/       a module root beside the directory it roots
<crate>/tests/*.rs              integration through the public API, one file per behaviour
<crate>/tests/fixtures/         recorded material — captured VT streams (*.raw), screen-dump
                                goldens (*.golden), and the capture-*.sh that record them.
                                .gitattributes pins each kind; it is material, not test source
<crate>/tests/*.proptest-regressions   proptest's committed failure corpus, written by the runner
inline #[cfg(test)] mod tests   module-internal units
<crate>/benches/*.rs            criterion
<crate>/examples/*.rs           cargo example binaries
justerm-wasm-decode/js/         hand-written JS shipped with the binding
justerm-wasm-decode/pkg*/       generated, git-ignored

<pkg>/src/**.ts                 shipped source ONLY — `tsconfig.json` includes exactly `src`
<pkg>/test/*.test.ts            vitest units
<pkg>/e2e/*.spec.ts             Playwright browser proofs
<pkg>/demo/                     the runnable browser harness pages
<pkg>/scripts/*.mjs             that package's own tooling

docs/architecture.md            the authoritative contract spec
docs/adr/NNNN-<kebab>.md        decision records
docs/map/{README.md,territory/,invariant/}   the territory graph
docs/agents/*.md                agent-read bindings and accumulated reference facts
docs/perf/*.md                  measurement write-ups (their harness lives in bench/<name>/)
.claude/agents/                 subagent definitions, where a session defines one

LICENSE-* · rust-toolchain.toml · .mcp.json · .github/dependabot.yml · <pkg>/.gitignore
                                toolchain and repo plumbing. No seam runs through it — claimed
                                anyway, because an unclaimed path is a finding, and a rule that
                                does not name plumbing reports a licence edit as a new top-level
                                area
```

**Running it against the tree is what completed it.** Against all **499** tracked files the first
draft left **54** unclaimed, and every one was a *role the rule had simply not named* — recorded
capture material, proptest's corpus, the plumbing, the generated agents. **None was a new area.**
Prose does not execute, so it cannot demonstrate that it is lying; the list above is what survived
being matched.

### D2 — `<pkg>/src` is a gate, not a convention

`justerm-web/tsconfig.json` includes exactly `src`, and `tsconfig.test.json` includes `test`, `demo`
and `e2e`. That split is what makes `process` and `Buffer` a **compile error in shipped code**.
Co-locating a `*.test.ts` under `src/` breaks nothing visible and dissolves the gate — which is this
record's argument in one line, and the reason the rule is finer than a naming preference.

### D3 — Every difference from a named peer is classified, or it is visible as unclassified

A tree nobody can defend difference by difference is not a rule; it is the shape the repo happened to
grow. Each row below is a place this tree **chose against** a peer that was actually read.

| We do | The references do | Decided by |
|---|---|---|
| The seam is a **crate wall**, not a directory | xterm.js splits `src/common` from `src/browser`; ghostty keeps `src/terminal` | ADR-0017. The boundary has to hold *outside* this repo — penterm consumes `justerm-core` alone — and a directory boundary is not enforced across a publish |
| No `public/` directory per layer | xterm.js gives each of `common` / `browser` / `headless` a `public/` | `lib.rs`'s `pub use` plus rustdoc already carry the public surface; a directory would be a second copy of it |
| Directory name **equals** package name | wezterm's engine crate is `term/` and publishes as `wezterm-term` | ADR-0010. In a `-term` *family* the directory is the discoverable member. One exception is recorded: `justerm-facade/` → `justerm` |
| One topic file per VT behaviour in `justerm-core/tests/` | alacritty runs one `tests/ref.rs` over recorded fixtures | `CLAUDE.md`'s cumulative-conformance rule. One file per behaviour is how the long tail gets **named**; a ref harness records that a capture matched and never *which behaviour* it was |
| **No** co-located `*.test.ts` under `src/` | xterm.js co-locates units as `src/**/X.test.ts` | D2 above — **measured**, not preferred: co-locating dissolves the typecheck gate while breaking no test |
| The binding's JS lives **inside** the binding crate | beamterm keeps `js/` at the repository root | ADR-0008. The binding is a crate and its npm artifact is built from that crate, so the shims are that crate's surface |
| The renderer has **no** `tests/`; the pure modules test inline | alacritty mixes inline with `tests/`; wezterm uses `src/test/` | The pure modules are host-testable; `webgl.rs` and `rasterizer.rs` are wasm32-only and 0-compile on host, so a `tests/` directory could not build and `e2e/` is the proof |
| **Two** script homes | beamterm keeps one root `scripts/` | Split by owner: CI's (`.github/scripts/`) and the package's (`<pkg>/scripts/`). *Was three until 2026-09-04*, when the generated graph build was retired and `scripts/` left the tree with it |
| The widget is **one** package, not core plus addons | xterm.js publishes 13 `addons/*` | ADR-0017 already decided it: xterm's addons hold **in-process buffer access**, so that split buys them something a frame-mode consumer cannot have |

**Nothing was adopted**: no peer difference produced a move whose reason named justerm's boundary.

### D4 — One difference is **unclassified**, and it stays visible rather than resolved by majority

`docs/research/` holds prior art read from real source with `file:line` citations, and so does
`docs/agents/reference-facts.md`. No reference is involved — this is internal — and **nobody has
decided it**. The cost is concrete and worth stating so the row is not read as tidiness: an agent
starting a change is routed to `reference-facts.md` and to nothing else, so a survey filed in
`docs/research/` is **invisible to the exact path that exists to stop it starting from a blank tree**.

Resolving it is a decision this record deliberately does not make.

## Consequences

- A new **top-level area** is the trigger to re-open this record, not to quietly extend the list. The
  last one added nothing declared, which is D4.
- The list is matched by hand now. The generated matcher that produced the 499/54 measurement was
  retired with the rest of the graph build on 2026-09-04; the *measurement* survives here because it
  is what the rule rests on, and re-running it means writing the matcher again.
- ADR-0010's crate-prefix rule and ADR-0017's boundary keep their authority. Where a row cites one,
  this record is a **sharper statement** of it, and a change to the parent falsifies the row.
