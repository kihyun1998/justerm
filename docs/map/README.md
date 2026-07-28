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
`scrollback.len()` on the alt screen"* — holds at four sites, was drawn nowhere, and was therefore
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
- **Source files are not nodes** — `term.rs` (4333 lines) is text under `## Code`. It is also
  shrinking under #584, which is moving the read surfaces into `term/` siblings, so treat any line
  count here as the date it was written rather than a fact to rely on.

## Nodes

Bare links only — deliberately no columns. A "which node is governed by what / how many territories /
how many rediscoveries" table restates what each note already says, nothing gates it, and it goes
stale within days: `#552` records exactly that, a hand-copied roster stale in five places three days
after it was written. The links themselves are cheap and a link checker catches them; the *columns*
are the roster.

- Territories — [cursor](territory/cursor.md) ·
  [damage & viewport](territory/damage-and-viewport.md) ·
  [logical lines](territory/logical-lines.md) ·
  [search & active match](territory/search.md) ·
  [selection](territory/selection.md) ·
  [wide glyph & soft wrap](territory/wide-glyph-and-soft-wrap.md)
- Cross-cutting invariants — [alt-screen absolute-index floor](invariant/alt-screen-buffer-floor.md) ·
  [row-keyed side maps](invariant/row-keyed-side-maps.md)

Ask the questions instead of copying the answers (run from `docs/map/`):

```sh
rg -l '^\*\*None\.\*\*' territory/   # territories with no governing decision — the holes
ls territory/ invariant/             # what exists; the folder is the roster
```

A territory with nothing governing it writes exactly `**None.**` under `## Governing decisions`, so
the first command stays honest without anyone maintaining a list.

**Known gap — the anchor links are not gated.** File links break loudly; a `#section-anchor` that
stops matching degrades *silently* to the top of the target file, and `reference-facts.md`'s headings
carry issue numbers and verification dates that will be edited. They were validated once, by
generating GitHub's slug for every heading in the target and comparing:

```sh
# slug: lowercase, drop anything not [a-z0-9 _-], spaces -> hyphens
sed -n 's/^#\{1,\} \(.*\)$/\1/p' FILE | tr '[:upper:]' '[:lower:]' \
  | sed 's/[^a-z0-9 _-]//g; s/ /-/g'
```

Nothing runs that on a change. Until something does, treat a reference link as verified only as of
the commit that added it.

## Current coverage

**Eight notes — core only.** Not the whole system. Territories with no note yet, referenced as
`(no note yet)` by the notes above:

VT interpretation (`term.rs`, 4333 lines = 49% of core) · grid & scrollback · input encoding
(`input.rs`, 801 lines) · frame & wire · hyperlinks · grapheme clusters · color references & palette ·
marker & decoration · a11y · renderer pipeline · cell geometry · infrastructure (CI / supply chain /
release)

Nothing outside `justerm-core` is mapped yet. Territories are **not** bounded by crate — decoration
spans core wire, renderer and web — so the renderer and web notes will attach to territories that
already exist here rather than forming a separate map.

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
