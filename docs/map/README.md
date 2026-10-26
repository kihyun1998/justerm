# MAP — the justerm family dependency graph

<!-- grill-map build stamp: 8afe5e1 -->
<!-- grill-map synced at: 453396d (2026-10-26) -->

The wiring diagram you open **before** starting work. Not a document index — a graph that answers two
questions:

1. **Horizontal** — if I touch this, **what else moves?** Open the territories the change touches,
   then follow `## Cross-cutting invariants` and `## Blast radius`. That is your checklist; an edge
   with nothing to do is a correct outcome, not a wasted one.
2. **Vertical** — what design is this code derived from, and what **decision** is that design derived
   from? Follow `## Governing decisions` up to the ADR. **If it says `**None.**`, that is the answer**
   — nothing governs this area.

No other layer answers either. ADRs are indexed by *the day a decision was made*, epics by *the work*,
spines by *the cluster that was re-litigated* — all events, and an event closes. `architecture.md`
covers one seam (core ↔ consumer). None of them is indexed by **territory**.

## The failure this map exists to catch

One fact — *a reader walking the single `[scrollback ++ grid]` buffer by absolute index must floor at
`scrollback.len()` on the alt screen* — held across several read surfaces, was drawn nowhere, and was
discovered three separate times (#113 → #144 → #207). The spine `#552` rostered 15 issues of the same
shape: one rule rediscovered one VT verb at a time. In both cases the helper and the ADR arrived after
the fact, and neither prevents the next author writing a *new* site from missing a helper they have
never heard of. That fact is now
[an invariant note](invariant/alt-screen-buffer-floor.md) — the form this map gives such a fact.

## How to read it

- **Starting a change**: the territory notes it touches, then their invariant and blast-radius links.
- **Asking "why is it like this?"**: `## Governing decisions`, then `## Design model`, whose claims
  each carry their evidence (a symbol, a test, an ADR, a commit) or their source and date
  (`(#831, 2026-09-10)`, `(maintainer, 2026-10-22)`) — a stated reason reads differently from a
  checked one.
- **Asking "is this known?"**: `## Known holes / open`.
- **Graph view**: open the repo as an Obsidian vault; links are plain markdown, so they work on GitHub
  too.

## Conventions

- **Section headings are exact strings**, identical in every note of a kind (the checker enforces the
  set). Territory: What it is · Governing decisions · Design model · Code · Reference behaviour ·
  Cross-cutting invariants · Blast radius · Known holes / open. Invariant: The fact · Why it is
  cross-cutting · Territories it holds in · What a violation looks like · Discovery history · Where it
  will recur.
- **Never delete an empty section.** It holds exactly `**None.**`; the blank under
  `## Governing decisions` is this map's output.
- **Territories are not exclusive.** A fact belonging to several is the normal case.
- **A fact that holds in several territories is promoted** to an `invariant/` note once it can name at
  least two sites, by symbol, that do not call each other. Buried in one territory it is invisible from
  the rest — which is how it gets rediscovered.
- **One concept, one note.** Notes are never merged because they feel related; that costs the ability
  to point at one of them. Where concepts belong side by side, an **aggregate** note says why and owns
  no detail.
- **Link a fact, never restate it.** A record's status, a count, an issue's state are linked.
  `docs/agents/reference-facts.md` holds what alacritty / ghostty / xterm.js actually do, each row
  pinned to a `file:line` at a recorded SHA; notes link to the **section anchor** and never paraphrase
  it. An empty `## Reference behaviour` means the area was never compared.
- **Symbols, never line numbers**, under `## Code`.
- **Plain relative markdown links** (`[selection](territory/selection.md)`); `[[wikilinks]]` render as
  literal brackets on GitHub.
- **This map only links out.** It never edits the artifacts it describes.
- **English** — `CLAUDE.md`'s language rule names `docs/map/`, because an agent reads it at the start
  of every task.

## What this map cannot answer

Only `.md` files in the repo can be graph nodes, so:

- **GitHub issues are not nodes.** Spine `#552`, Epic `#287`, the open tail `#47` exist only as text
  inside a note.
- **Source files are not nodes.** `term.rs` is text under `## Code`. Line counts are not quoted here: a
  number maintained by hand is a roster one size smaller. Ask instead:

  ```sh
  find justerm-core/src justerm-renderer/src justerm-web/src justerm-pty/src -name '*.rs' -o -name '*.ts' | xargs wc -l | sort -rn | head
  ```

## The system in one paragraph

`justerm-core` folds a VT byte stream (parsed by `vte`) into one `[scrollback ++ grid]` cell buffer
with a cursor, a pen and DEC modes; keeps engine-owned state anchored to buffer content (selection,
markers and OSC 133 command marks, tracked points, search highlights and the active match, OSC 8
links, settled-cell paint); emits **frames** (viewport cells, damage, scroll op, caret, overlay
groups) in a versioned binary wire format; answers whole-buffer queries; encodes input; answers the
application and relays events to the consumer. A **Session** binds an engine to a frame sink;
`justerm-pty` runs a child in a PTY and drives one (or runs raw). `justerm-wasm-decode` is `decode`
compiled to wasm. `justerm-renderer` (WebGL2, wasm) packs decoded frames into one instanced draw per
grid. `justerm-web` is the browser widget that wires a frame source to the renderer and owns DOM input,
a11y, decorations, scrollbar and fit. It ships on four tag tracks, behind the gates in
`.github/workflows/test.yml`. The first consumer is PenTerm, a sibling checkout: its native backend
holds a `justerm_core::Session` (`src-tauri/src/engine/native.rs`), its local sessions run on
`justerm_pty::Pty` (`src-tauri/src/pty/manager.rs`), and its webview consumes `justerm-web` and
`justerm-wasm-decode` (penterm 7dadc3c95, 2026-10-22).

## Nodes

Bare links only — **no columns.** A "governed by / how many territories" table restates what each
note says, nothing gates it, and `#552` recorded exactly that: a hand-copied roster stale in five
places three days after it was written. The folder is the roster; the queries below answer the rest.

**Engine — write path:** [VT interpretation](territory/vt-interpretation.md) ·
[pen](territory/pen.md) · [cursor position](territory/cursor-position.md) ·
[grapheme clusters](territory/grapheme-clusters.md) · [wide glyph](territory/wide-glyph.md) ·
[soft wrap](territory/soft-wrap.md) · [alt screen](territory/alt-screen.md) ·
[grid & scrollback](territory/grid-and-scrollback.md) · [reflow](territory/reflow.md)

**Engine — state on content:** [selection](territory/selection.md) · [search](territory/search.md) ·
[active match](territory/active-match.md) · [marker](territory/marker.md) ·
[tracked points](territory/tracked-points.md) · [hyperlinks](territory/hyperlinks.md) ·
[logical lines](territory/logical-lines.md) · [settled-cell paint](territory/settled-cell-paint.md)

**Engine — output and edges:** [damage](territory/damage.md) · [viewport](territory/viewport.md) ·
[frame](territory/frame.md) · [wire format](territory/wire-format.md) ·
[caret report](territory/caret-report.md) · [events & replies](territory/events-and-replies.md) ·
[input encoding](territory/input-encoding.md) · [session](territory/session.md) ·
[PTY loop](territory/pty.md) · [WASM decoder](territory/wasm-decoder.md)

**Renderer:** [cell compositing](territory/cell-compositing.md) ·
[colour policy](territory/colour-policy.md) · [cell geometry](territory/cell-geometry.md) ·
[glyph atlas](territory/glyph-atlas.md) · [emoji classification](territory/emoji-classification.md) ·
[built-in block glyphs](territory/builtin-block-glyphs.md) · [GPU upload](territory/gpu-upload.md) ·
[frame adapter](territory/frame-adapter.md) · [GL context lifecycle](territory/gl-context-lifecycle.md) ·
[multi-viewport](territory/multi-viewport.md) · [caret drawing](territory/caret-drawing.md)

**Web widget:** [widget lifecycle](territory/widget-lifecycle.md) · [fit](territory/fit.md) ·
[accessibility](territory/accessibility.md) · [decoration](territory/decoration.md)

**Shipping and instruments:** [published surface](territory/published-surface.md) ·
[release](territory/release.md) · [CI](territory/ci-and-supply-chain.md) ·
[browser proof harness](territory/browser-proof-harness.md) ·
[capture corpus](territory/capture-corpus.md) ·
[robustness testing](territory/robustness-testing.md) ·
[throughput bench](territory/throughput-bench.md)

**Aggregates** (relationship only, no detail): [cursor](territory/cursor.md) ·
[damage & viewport](territory/damage-and-viewport.md) · [frame & wire](territory/frame-and-wire.md) ·
[release & published surface](territory/release-and-published-surface.md) ·
[wide glyph & soft wrap](territory/wide-glyph-and-soft-wrap.md)

**Cross-cutting invariants:**
[alt-screen absolute-index floor](invariant/alt-screen-buffer-floor.md) ·
[row-keyed side maps](invariant/row-keyed-side-maps.md) ·
[only U+0020 can be a row's padding](invariant/only-u0020-can-be-padding.md) ·
[a span covers a wide pair whole](invariant/a-span-covers-a-wide-pair-whole.md) ·
[the write path funnels motion and not destruction](invariant/no-funnel-for-destruction-in-place.md) ·
[RIS keeps configuration and drops coordinates](invariant/ris-keeps-configuration-drops-coordinates.md) ·
[a coordinate carries the instant it is true at](invariant/a-coordinate-carries-the-instant-it-is-true-at.md) ·
[a wire field is narrower than its value](invariant/wire-field-narrower-than-its-value.md) ·
[a decoded frame's columns are getters](invariant/decoded-columns-are-getters.md) ·
[a wasm `Err` payload is thrown verbatim](invariant/wasm-err-payload-is-thrown-verbatim.md) ·
[the cell size is derived state](invariant/cell-size-is-derived-state.md) ·
[an IME composition is browser-owned state](invariant/composition-is-browser-owned-state.md) ·
[a pointer coordinate is bounded by its producer](invariant/pointer-coordinates-are-bounded-by-their-producer.md) ·
[an absent box measures as zero](invariant/an-absent-box-measures-as-zero.md) ·
[a layer ends what it exclusively holds](invariant/a-layer-ends-what-it-exclusively-holds.md) ·
[an awaited in-page promise needs an anchor](invariant/an-awaited-in-page-promise-needs-an-anchor.md) ·
[workspace exclusion is gate invisibility](invariant/workspace-exclusion-is-gate-invisibility.md) ·
[an unframed screen change owes a present](invariant/an-unframed-screen-change-owes-a-present.md) ·
[a drained reply is the drainer's to write](invariant/a-drained-reply-is-the-drainers-to-write.md) ·
[a GPU mirror dies with its context](invariant/a-gpu-mirror-dies-with-its-context.md) ·
[renderer encodings are held by convention](invariant/renderer-encodings-are-held-by-convention.md) ·
[an atlas slot has one origin](invariant/an-atlas-slot-has-one-origin.md)

Ask the questions instead of storing the answers (run from `docs/map/`):

```sh
# no decision record governs this area
rg -lU '## Governing decisions\r?\n(\r?\n)?\*\*None\.\*\*' territory/
# never compared against a reference implementation — a different hole
rg -lU '## Reference behaviour\r?\n(\r?\n)?\*\*None\.\*\*' territory/
# recorded but not built
rg -lU '## Code\r?\n(\r?\n)?\*\*None\.\*\*' territory/
ls territory/ invariant/             # what exists; the folder is the roster
```

**A query has to name its section.** The first version grepped for the bare sentinel and reported a
territory with four governing ADRs as ungoverned, because the same sentinel marks an empty
`## Reference behaviour` (commit 0a0a44a, 2026-07-29). A command in place of a stored answer is only
better if it answers the question it claims to.

## The gates

Both run in the `test` job of `.github/workflows/test.yml` on every PR:

```sh
node .github/scripts/check-map-links.mjs docs CLAUDE.md GLOSSARY.md README.md   # every relative link and #anchor
node .github/scripts/check-map-note.mjs docs/map/territory/selection.md         # one note: sections, ## Code symbols, no restated status
```

The link gate exists for **anchors**: a missing file is a loud 404, a missing anchor degrades silently
to the top of the target, and `reference-facts.md`'s headings carry issue numbers and dates that get
edited. It also checks invariant ↔ territory reciprocity.

**Verify a note as you finish it, not the batch at the end.** Twenty-seven notes were once written and
checked once at the end, and all four defects found were the same class, spread across notes written
hours apart (commit 87edad1, 2026-07-29). The CI loop over every note is the backstop for the person
who edits a note months later without knowing the script exists — two invariant notes sat incomplete
until an unrelated change ran it by hand (commit b5de18b, 2026-07-31).

## Scope, and what an absent note means

**The scope is everything in this repository**, not only `justerm-core`: the crates a `--workspace`
command never visits, and the artifacts that exist only on a registry (the frozen `justerm-facade`
tombstone, held by [release](territory/release.md)). Territories are not bounded by crate — decoration
spans core's wire, the renderer and the widget.

**Out of scope:** `teach/` (a personal `/teach` course workspace, `teach/README.md`), `logo/` (brand
assets), and `docs/research/` (a prior-art essay, cited from notes as a source).

**A territory appears when something in the repo commits to it** — the first slice, a decision record,
an open issue with a design. Unbuilt work lives in the tracker until then; a note written earlier
describes something that does not exist, and its `## Code` would have nothing to name.

**An absent territory under code you are changing is a finding** for this change — you are the first
person standing there with the evidence. It does not block; carry it to the maintainer. **A
cross-cutting invariant the change reveals may not lag**: the change does not land until the
`invariant/` note does. Coverage is a report; promotion is a constraint.

A dangling reference — a note naming an area nobody wrote — is found by:

```sh
rg -o '\*\*[a-zA-Z0-9 /&-]+\*\* \*\(no (territory )?note yet\)\*' . --no-filename \
  | sed 's/\*//g; s/ (no.*//' | sort | uniq -c | sort -rn
```

It says *nothing dangling*, never *nothing missing*: an area nobody ever pointed at leaves no marker.
Its character class once omitted digits, so `a11y` never matched and the query reported a clean zero
(commit 67e086d, 2026-07-30).

## What the orientation found

- **The largest gaps were areas with a glossary name and no note**: the alt screen, the Session, tracked
  points, the WASM decoder as a crate, robustness testing and the throughput bench got notes on
  2026-10-22. Five existing notes were not linked from this hub until then.
- **Five facts held at several sites with no note**, each found by a territory reading on 2026-10-22
  and promoted: an unframed screen change owes a present, a drained reply is the drainer's to write, a
  GPU mirror dies with its context, renderer encodings are held by convention, an atlas slot has one
  origin.

## Once the map exists — how a change uses it

- **Read before the change, write after it.** The working discipline is the `thegraph` skill: its
  first step, `read-it`, reads the territory notes the change touches and their invariant and blast
  links; its `sweep` step brings the notes that name a touched symbol back in line once the change is
  otherwise complete. `sweep` leaves the `grill-map synced at` line alone — only `/grill-map` update
  advances it.
- **Promotion, at the first fix.** When a fix reveals a fact, ask *"does this hold at every site that
  shares <the assumption>?"* — phrased so grep can answer it. If yes, the fix lands with an invariant
  note.
