# MAP — the justerm family dependency graph

The wiring diagram you open **before** starting work. Not a document index — a graph that answers two
questions:

1. **Horizontal** — if I touch this, **what else moves?**
2. **Vertical** — what design is this code derived from, and what **decision** is that design derived
   from?

No existing layer answers either. ADRs are indexed by *the day a decision was made*, Epics by *the
work*, spines by *the cluster that was re-litigated* — all **events**, and an event closes.
`architecture.md` covers exactly one seam (core ↔ consumer). None of them is indexed by **territory**.

## The failure this map exists to catch

One fact — *"a reader walking the single `[scrollback ++ grid]` buffer by absolute index must floor at
`scrollback.len()` on the alt screen"* — holds across several read surfaces, was drawn nowhere, and was therefore
discovered **three separate times** across months (#113 → #144 → #207). The `#552` roster is 15 issues
of the same disease: one rule rediscovered one VT verb at a time. In both cases the helper function and
the ADR arrived *after the fact*. Neither prevents a recurrence — the next author writing a *new* site
does not go looking for a helper they have never heard of.

## How to read it

- **Starting a change**: open the territories it touches, then follow `## Cross-cutting invariants`
  and `## Blast radius`. That is your checklist.
- **Asking "why is it like this?"**: follow `## Governing decisions` up to the ADR. **If it is empty,
  that is the answer** — nothing governs this.
- **Graph view**: open this repo as an Obsidian vault (no configuration needed). Links are plain
  markdown, so they work on GitHub too.

## Conventions

- **Never delete an empty section.** The blank under `## Governing decisions` is this map's output.
- **Territories are not exclusive.** One fact belonging to several is the normal case, not a defect.
- **A fact that holds across N territories is promoted to a cross-cutting note.** Buried inside one
  territory, it is invisible from the other N-1 — which is exactly how it gets rediscovered.
- **Never edit existing artifacts.** This map only links *out*. The reverse direction is free
  (Obsidian backlinks).
- **Reference facts are linked, never restated.** `docs/agents/reference-facts.md` holds what
  alacritty / ghostty / xterm.js actually do, and every row there is pinned to a `file:line` at a
  recorded SHA. A paraphrase here would read identically and carry no pin — the one place in this map
  where copying is destructive rather than merely redundant. Territory notes link to the **specific
  section anchor**, and an empty `## Reference behaviour` means the area has never been compared to a
  reference.
- **Symbols, never line numbers**, in `## Code`. A line number is an ungated copy of something the
  compiler owns — the same mistake as the prose API list `architecture.md` had to delete.
- Links are standard relative markdown — e.g. `[selection](territory/selection.md)`. `[[wikilinks]]`
  work in Obsidian but render as literal brackets on GitHub, and this repo is read in both places.
  (The example uses a real path on purpose: a fake one makes every link checker report a false
  positive.)

## What this map cannot answer

**Only `.md` files inside the repo can be graph nodes.** Therefore:

- **GitHub issues are not nodes** — spine `#552`, Epic `#287`, `#562`. They exist only as text inside a
  note. Some of the hottest material in this repo is invisible to the graph; this is mitigated, not
  solved.
- **Source files are not nodes** — `term.rs` (3646 lines) is text under `## Code`. It is also
  shrinking under #584, which is moving the read surfaces into `term/` siblings, so treat any line
  count here as the date it was written rather than a fact to rely on.

## Nodes

Bare links only — deliberately no columns. A "which node is governed by what / how many territories /
how many rediscoveries" table restates what each note already says, nothing gates it, and it goes
stale within days: `#552` records exactly that, a hand-copied roster stale in five places three days
after it was written. The links themselves are cheap and a link checker catches them; the *columns*
are the roster.

**One concept, one note.** Two notes are never merged because they feel related — what that costs is
the ability to *point at one of them*, and every edge into a merged note is then drawn thicker than
the truth. Where several concepts genuinely belong side by side, an **aggregate** note says why and
owns no detail of its own.

- Territories — [accessibility](territory/accessibility.md) ·
  [active match](territory/active-match.md) ·
  [built-in block glyphs](territory/builtin-block-glyphs.md) ·
  [caret drawing](territory/caret-drawing.md) ·
  [CI & supply chain](territory/ci-and-supply-chain.md) ·
  [caret report](territory/caret-report.md) ·
  [cell compositing](territory/cell-compositing.md) ·
  [cell geometry](territory/cell-geometry.md) ·
  [cursor position](territory/cursor-position.md) ·
  [damage](territory/damage.md) ·
  [decoration](territory/decoration.md) ·
  [events & replies](territory/events-and-replies.md) ·
  [emoji classification](territory/emoji-classification.md) ·
  [colour policy](territory/colour-policy.md) ·
  [frame](territory/frame.md) ·
  [fit](territory/fit.md) ·
  [frame adapter](territory/frame-adapter.md) ·
  [input encoding](territory/input-encoding.md) ·
  [GL context lifecycle](territory/gl-context-lifecycle.md) ·
  [glyph atlas](territory/glyph-atlas.md) ·
  [GPU upload](territory/gpu-upload.md) ·
  [grapheme clusters](territory/grapheme-clusters.md) ·
  [grid & scrollback](territory/grid-and-scrollback.md) ·
  [hyperlinks](territory/hyperlinks.md) ·
  [logical lines](territory/logical-lines.md) ·
  [marker](territory/marker.md) ·
  [pen](territory/pen.md) ·
  [published surface](territory/published-surface.md) ·
  [reflow](territory/reflow.md) ·
  [release](territory/release.md) ·
  [search](territory/search.md) ·
  [selection](territory/selection.md) ·
  [soft wrap](territory/soft-wrap.md) ·
  [viewport](territory/viewport.md) ·
  [wide glyph](territory/wide-glyph.md) ·
  [widget lifecycle](territory/widget-lifecycle.md) ·
  [wire format](territory/wire-format.md)
- Aggregates (relationship only, no detail) — [cursor](territory/cursor.md) ·
  [damage & viewport](territory/damage-and-viewport.md) ·
  [frame & wire](territory/frame-and-wire.md) ·
  [release & published surface](territory/release-and-published-surface.md) ·
  [wide glyph & soft wrap](territory/wide-glyph-and-soft-wrap.md)
- Cross-cutting invariants — [the cell size is derived state](invariant/cell-size-is-derived-state.md) ·
  [alt-screen absolute-index floor](invariant/alt-screen-buffer-floor.md) ·
  [row-keyed side maps](invariant/row-keyed-side-maps.md) ·
  [an IME composition is browser-owned state the engine never sees](invariant/composition-is-browser-owned-state.md) ·
  [workspace exclusion is gate invisibility](invariant/workspace-exclusion-is-gate-invisibility.md) ·
  [a decoded frame's columns are getters](invariant/decoded-columns-are-getters.md)

Ask the questions instead of copying the answers (run from `docs/map/`):

```sh
# no decision record governs this area
rg -lU '## Governing decisions\r?\n\r?\n\*\*None\.\*\*' territory/
# never compared against a reference implementation — a different hole
rg -lU '## Reference behaviour\r?\n\r?\n\*\*None\.\*\*' territory/
# recorded but not built — a decision waiting for code, the inverse of the two above
rg -lU '## Code\r?\n\r?\n\*\*None\.\*\*' territory/
ls territory/ invariant/             # what exists; the folder is the roster
```

An empty section writes exactly `**None.**`, so these stay honest without anyone maintaining a list —
but **the query has to name its section.** The first version of it grepped for the bare sentinel and
reported a territory with four governing ADRs as ungoverned, because the same sentinel also marks an
empty `## Reference behaviour`. A command in place of a stored answer is only better if it answers
the question it claims to; an unscoped one is a stored answer with extra steps and more confidence.

**The links are gated.** `.github/scripts/check-map-links.mjs` runs on every PR (the `test` job) and
resolves every relative markdown link under `docs/`, `CLAUDE.md`, `CONTEXT.md` and `README.md` —
**including `#anchors`**, whose failure mode is the reason the gate exists: a missing file is loud
(404), while a missing anchor degrades *silently* to the top of the target document, and
`reference-facts.md`'s headings embed issue numbers and verification dates that get edited.

```sh
node .github/scripts/check-map-links.mjs docs CLAUDE.md CONTEXT.md README.md
```

**Verify a note as you finish it, not the batch at the end.** `check-map-note.mjs` takes one file and
runs in a second — section set for its kind, every symbol under `## Code`, and nothing restating a
value another artifact owns:

```sh
node .github/scripts/check-map-note.mjs docs/map/territory/selection.md
```

The cadence is the point. Twenty-seven notes were written and verified once at the end, and all four
defects that pass found were **the same class**, spread across notes written hours apart — checking
after the third would have ended it there. Batching happens when checking is expensive, so the script
exists to make it cheap rather than to be remembered.

**It is also gated now, and the two are not the same job.** The `test` job runs it over every note on
every PR, because the cadence rule above only reaches the person *writing* a note — it cannot reach the
one who edits an existing one months later without knowing the script exists. That gap was not
hypothetical: two invariant notes sat incomplete (both missing `## Where it will recur`) and were found
only because an unrelated change happened to run the script by hand. Keep verifying as you write; the
gate is the backstop, not the workflow.

## Current coverage

**The scope is everything in this repository**, not `justerm-core`. That includes the crates a
`--workspace` command never visits and the artifacts that only exist on a registry — the frozen
`justerm-facade` tombstone is mapped for exactly that reason: zero commits, zero gates, and a
permanent published surface that breaks silently if anyone treats it as ordinary code.

Territories are **not** bounded by crate. Decoration spans core's wire, the renderer and the web
widget; cursor is split with the consumer by design. Renderer and web notes attach to territories
that already exist here rather than forming a second map.

What has no note yet. Prioritise by **how many notes point at it** — a dangling reference is a reader
hitting a dead end, and it is a better signal than commit frequency (which misses everything that
never changes; see the tombstone above). Ask:

```sh
rg -o '\*\*[a-zA-Z0-9 /&-]+\*\* \*\(no (territory )?note yet\)\*' . --no-filename \
  | sed 's/\*//g; s/ (no.*//' | sort | uniq -c | sort -rn
```

**The answer is not stored here — run the query.** It used to be, as a hand-written *"Currently:
renderer (7 references) · VT interpretation · grid & scrollback · input encoding · marker &
decoration · a11y · hyperlinks · grapheme clusters · colour references & palette · cell geometry ·
CI & supply chain"* list, and by 2026-07-30 **every area on it had a note** — so the paragraph told
each new reader that eleven mapped territories were unmapped. That is the same defect as the roster
columns two sections up, and it was the *more* expensive one: a stale roster is read as untidy, a
stale hole list is read as work to do.

Two things it took to notice, both worth keeping:

- **The query above was silently wrong.** Its character class omitted digits, so `a11y` — the label
  of the *only* remaining dangling reference — never matched, and it reported zero. Zero read as
  "clean", which is indistinguishable from "the pattern is narrow" (the same warning this file gives
  one section up, in that section's own tool).
- **That last dangling marker was itself stale.** `invariant/alt-screen-buffer-floor.md` still said
  *"a11y / whole-buffer text (no territory note yet)"* while
  [accessibility](territory/accessibility.md) already carried `Term::accessible_text` under its
  `## Code`. Because of the first defect nobody could see the second.

Line counts are deliberately not quoted here — and this paragraph used to quote two anyway, one of
which had already gone stale by #601. `term.rs` lost roughly a quarter of itself across #584's
slices; a number maintained by hand is the same defect as a roster maintained by hand, one size
smaller, and a sentence that says so while carrying one is the smallest version of it. Ask instead:

```sh
find justerm-core/src justerm-renderer/src justerm-web/src -name '*.rs' -o -name '*.ts' | xargs wc -l | sort -rn | head
```

### What the measurements found

- Of roughly **100 public promises** (`Engine` 52 + `Term` 49), **exactly one — `frame()` — has a
  governing decision.**
- `cursor` is *mentioned* in 19 of the 25 ADRs and is the **subject of none**. Of the eight territories
  written so far, **four have no governing record at all** (`rg -l '^\*\*None\.\*\*' territory/`).
- **Three** surfaces described behaviour that no longer exists — all now fixed, and the count is the
  point: the first sweep found one, and widening the phrasing found two more.
  `Engine::resize` promised soft-wrap reflow "lands in #7" for six weeks after #7 closed;
  `architecture.md` §Cadence called the viewport-vs-screen damage mapping an open question "tracked in
  #13", closed, and predicted a translation layer that was never built; §Hidden VT state said
  `scroll_region_lines` rotates *neither* marker nor selection anchors, which #162 closed — it rotates
  both. A sweep is judged by what it cannot see, not by its hit count.

## Language

English, deliberately, and this is a departure from `CLAUDE.md`'s default ("Korean except `CONTEXT.md`
and `docs/adr/`"). The map is read by an agent at the **start of every task**, which is the same
token-efficiency rationale that put the glossary in English. `CLAUDE.md`'s rule now names
`docs/map/` alongside them.
