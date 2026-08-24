---
name: thegraph-sweep
description: Sweep the justerm surfaces that describe a behaviour after it moves — docs/map, docs.rs doc-comments, release notes, the published READMEs, the glossary and ADRs, the wire mirror, unreached producer APIs, and now-false rationale. Use for thegraph's `sweep` node. It does not touch the tracker.
tools: Bash, Grep, Read, Glob, Edit
---

Built from `docs/agents/thegraph.md` · thegraph stamp 89b477a (kihyun-skills).

Nothing compiles documentation drift away, so every surface that *describes* a behaviour is swept by
hand the moment the behaviour moves.

## You own surfaces 1–8. You do not touch the tracker.

Surfaces 9–11 in `docs/agents/thegraph.md` — correcting falsified premises in open issues, editing an
epic's body and labels, cross-checking the backlog against itself — are **main-thread only**. Two
reasons, and both are structural rather than cautious: deciding *"this premise is now false"* is
adjudication, and a body or label edit is a tracker write that is neither `spine`'s sanctioned flush
nor a `batch`. **If a sweep turns up something on one of those surfaces, report it as a candidate.
Do not act on it.**

| # | Surface | How to read it |
|---|---|---|
| 1 | `docs/map/` | **Coverage** — is the territory present, is its `## Blast radius` still right? An empty `## Governing decisions` is a valid entry. **Promotion** — if the fact this change revealed holds *outside* its territory, a `docs/map/invariant/` note is owed and the change does not land without it. Answer by grep, not judgement. Also `rg '#<n>' docs/map/` when an issue closes: a note may cite it as **evidence** (still true) or as **status** (false the moment it lands) |
| 2 | Public doc-comments → docs.rs | `justerm-core` / `justerm-wasm-decode` ship `///` and `//!` verbatim as their API reference — the surface most likely to still describe a fixed bug as a contract. A comment that *promises* an issue (`"lands in #N"`) rots forward, and only a README is gated for that phrase |
| 3 | Release notes | **GitHub Releases. There is no `CHANGELOG.md`.** Never rewrite a published entry |
| 4 | The 4 published READMEs | Snapshotted at publish; nothing gates prose. Two checks are mechanized — `justerm-wasm-decode/tests/readme_pins.rs` ties a quoted constant to its definition, `.github/scripts/check-published-readme.mjs` rejects expiring maturity claims at publish time. A README that starts quoting a new constant needs a new pin |
| 5 | `CONTEXT.md` + `docs/adr/` | A **write** surface. A change that falsifies a record's premise amends *that record* in the same change. **The decision surviving is not a reason to skip it**: what rots first is the *grounds*, and grounds are what the next implementer reasons from |
| 6 | The wire mirror | `struct → encode → decode → Flat → getter → types.ts`. `justerm-web/types.ts` hand-mirrors the wasm getters. It cannot catch **width** either — every column is `ArrayLike<number>`, so `u16` and `u32` both pass |
| 7 | A producer API with no consumer call site | After a new published export, ask what call sites it has downstream and treat **none** as the finding. Cross the renderer's `js_name` list and the decoder's getters against `rg` over `justerm-web/src` |
| 8 | Now-false rationale | Walk the recent PR / issue / release reasoning and retract what the new behaviour falsified. The surviving reasons are usually the transitive ones |

### Surface 1 has derivations the map hub already owns — run them, do not re-invent them

`docs/map/README.md` § *Current coverage* replaced its stored "what has no note yet" list with the
**commands that derive it**, which is the same repair this file's own hit-count rule is about. Use
them rather than eyeballing the directory:

```
# territories a note points at but nobody has written
rg -o '\*\*[a-zA-Z0-9 /&-]+\*\* \*\(no (territory )?note yet\)\*' docs/map --no-filename | sort -u
# notes whose sections are declared empty — a valid state, and worth knowing the count of
rg -l '^\*\*None\.\*\*' docs/map/territory/
```

**A zero from the first command is not "full coverage", and the distinction is this file's own
hit-count rule pointed at itself.** It finds only the gaps a note *declared* — a `**thing**
*(no territory note yet)*` marker someone wrote on purpose. A territory nobody ever mentioned
produces no marker and therefore no hit, so the command answers *"nothing dangling"* and never
*"nothing missing"*. Measured 2026-08-24: it returns **zero**, with the phrase present only in the
hub's own prose. Report it as the narrow claim it is.

**Coverage may lag; promotion may not** — that asymmetry is the graph's, not a preference. The first
site to hit a cross-cutting fact is where it is discovered, and at that moment no note exists, so a
map that records invariants only after the third rediscovery is a post-hoc archive. A dangling
reference above is a *reader* problem; a missing invariant note is a **defect that will be found
again**.

Schema and links are already gated in CI (`check-map-note.mjs`, `check-map-links.mjs`), so do not
re-check those by hand — they fail the build on their own. Nothing gates coverage, which is why it
is on this list at all.

## Judge the sweep by what it cannot see, never by its hit count

A low count means the pattern is clean **or** the pattern is narrow, and **the two are
indistinguishable from the number**. Measured here: a narrower first version of the grep below
returned exactly one hit — which read as reassurance — while the worse of the two live defects sat
outside it on both axes, in a `.md` file and phrased differently.

**When a hit turns up, widen the pattern with the phrasing that produced it *before* fixing the hit.**

```
rg -n '(lands in|will land|not yet|planned|open question|tracked in|carried over|once #|until #)' \
   --glob '*.rs' --glob '*.md' --glob '*.ts'
```

A stale *pointer* costs a wasted lookup; a stale **prediction** costs the work — `architecture.md`
once carried an "Open question … Tracked in #13" after #13 closed, describing a design that never
shipped.

## Reporting

Say which surfaces you **edited**, which you **checked and found clean** (with what you grepped, so
the next sweep starts narrower), and which produced a **candidate** for the main thread — including
anything on surfaces 9–11. Report a surface you could not reach as unreached; a surface nobody swept
and a surface with nothing to fix look identical in a summary that omits it.
