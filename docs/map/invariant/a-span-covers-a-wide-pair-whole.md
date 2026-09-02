# Cross-cutting invariant — a span covers a wide pair whole, or not at all

## The fact

A width-2 glyph occupies **two cells** — a lead and a trailing spacer — and every range this family
computes over columns is expressed as plain numbers that know nothing about that. A range whose edge
falls *between* the two halves is therefore representable, reachable, and wrong: it paints half a
glyph, or copies text the highlight does not show.

So: **any range that includes one half of a pair includes both.** It is a widening, never a
shrinking — both ends move outward — and it applies to every kind of range this family has, not
only the ones a user drags:

1. **A gesture range.** A press on the right half of a CJK glyph anchors on its spacer.
2. **A range the consumer authored.** A `Match` handed back to the engine, or a decoration rect in
   the consumer's own coordinates. The consumer has no obligation to know where pairs are.
3. **A bounded range.** An out-of-range column is clamped onto the row's *last* cell, which is a
   trailing spacer whenever the row ends in a wide glyph — so the bound itself can manufacture a
   half-covering span out of a coordinate that named no pair at all.
4. **The caret.** A block cursor is a one- or two-cell span like any other, and an application can
   put the cursor on a spacer with an ordinary `CUB` / `CHA`.

**Both cells must agree before they are treated as a pair.** A lead pairs only with a spacer to its
right and a spacer only with a lead to its left, and neither ever pairs across a row. This is not
defensive coding: a lead standing alone in the last column is a **legal buffer state**
(`Row::resize` narrows straight through a pair, and [ADR-0025] D4's scope stops at reallocation), so
"the next column is my spacer" is a predicate that is false in the buffer's own normal operation.

[ADR-0025]: ../../adr/0025-row-and-wide-pair-cell-state-ownership.md

## Why it is cross-cutting

**Five producers in two crates, and no two of them share a layer.** `Term::selection_range` and
`Term::match_spans` are core's; a decoration rect is the consumer's; the caret is nobody's — it comes
from the application's cursor position. There is no single seam they pass through *before* the cells
are known, which is why the rule lives twice on purpose: in `justerm-core`, so `selection_range` and
`selection_text` cannot disagree about what a selection contains, and in `justerm-renderer`, where
every span finally meets the flags.

**The fifth is not a range at all** (#791). `I_neighbour` — a glyph's ink landing in the cell above
or below it — is *withdrawn* where the two cells' backgrounds differ, and that withdrawal is decided
per cell. A background edge under one column of a pair therefore grants one half and withdraws the
other, and the pair's overflow is cut down the middle of the letter: this note's own symptom,
produced by a **gate** rather than by a range. The four producers above all answer "which cells does
this span cover"; a gate answers "may this cell's ink cross", and the invariant reaches it for the
same reason — a wide pair is one glyph, so any per-cell decision about it has to be reconciled
across both halves. Reconciled on the *receivers* in `frame.rs`'s packer, since the pair is in the
source row while the disagreement is in the receiving one.

That duplication is the reason this is an invariant rather than a note on one territory. The two
spellings must stay the same rule, and the failure mode is silent: each layer is locally correct
with a half-rule, and the disagreement only shows as a glyph split down the middle.

The mis-routing is on the record and is what the note exists to prevent repeating. Three artifacts
independently filed this as *consumer span policy* — the issue's own title, [ADR-0024]'s consequence
list, and a doc-comment in `justerm-renderer` `overlay.rs` — because a decoration rect was the
producer everyone had in mind. Two of the four producers are core's and never reach the consumer at
all.

[ADR-0024]: ../../adr/0024-decoration-projection-and-precedence.md

## Territories it holds in

- [wide glyph](../territory/wide-glyph.md) — the pair is this territory's subject; the rule is what
  every *reader* of one owes
- [selection](../territory/selection.md) — both observables, and the `Block` arm needs it **per row**
  because a rectangle meets a pair at a different column on each
- [search & active match](../territory/search.md) — `match_spans`, where a consumer-authored match
  and the clamp both reach it
- [decoration](../territory/decoration.md) — a rect in consumer coordinates, resolved at the paint
  site
- [cell compositing](../territory/cell-compositing.md) — the overlay and decoration lookups, and the
  caret's own span, all resolve through one helper here
- [cursor position](../territory/cursor-position.md) — the caret is a span, and the application can
  park it on a spacer
- [damage](../territory/damage.md) — **a sixth site, found working #826 and not fixed there.** The
  frame producer folds the cursor in per *cell*, expanding one column at each end (the old cell and
  the new). But the renderer's caret moves its origin **left** onto the lead when it stands on a
  spacer, so it lights a column the frame never named. Measured: with a wide glyph at cols 7–8 and
  the cursor walked onto col 8, the frame reports one span `left 8, right 14` and col 7 is outside
  it. Both prose statements of the fold — this map's cursor-position note and `architecture.md`'s
  *"damages the cell the cursor left + the cell it lands on"* — are written per cell and are wrong
  for a pair. **Validity condition:** no live artefact today, because `justerm-renderer` re-packs
  the whole viewport every frame and never consults damage for the caret. The gap is in the wire
  contract, for exactly the cell-invert consumer the fold exists to serve

## What a violation looks like

A CJK glyph split down the middle: one half in the selection colour, one half not. On the caret it
is a block cursor lighting the right half of a glyph — or, if the rule is written as "widen
rightward" instead of "cover the pair", a caret that stretches onto an innocent neighbour.

The quieter violation is the one with no visual tell: a **highlight and a copy that disagree**. A
range ending inside a pair paints two cells' worth of glyph while `selection_text` returns neither
half, because a spacer extracts as nothing. Nothing on screen says the clipboard is different.

## Discovery history

Filed as a `justerm-web` span-policy defect (#454, 2026-07-23) and stayed open for two and a half
weeks under a recorded direction of *"family, hold neutral — 1 of 3 references snaps, and 1 of 3 is
not a mandate"*. That tally was wrong in both directions: ghostty's rule was searched for in
`terminal/Selection.zig` and lives in its renderer, and alacritty's was summarised as absent from a
method that is literally on `Selection`. All three references refuse to paint half a glyph.

Three sibling issues had already met the same rule from other sides without the shape being named —
#529 (core producing an invalid pair), #535 (a word extent stopping mid-glyph), #678 (a clamp landing
on a trailing spacer). The caret's half of it (`cursor_span` widening on `WIDE_CHAR` only) had a test
**pinning the defect as expected**, whose recorded grounds were that no reference handled it.

## Where it will recur

- **A new span kind.** Anything that colours or bounds a column range — a fifth overlay group, a
  hover highlight, a bracket-match indicator — arrives without the rule unless it goes through the
  same helper.
- **A new bound.** Any clamp of an outside coordinate onto a real cell can land on a spacer, so a
  bound added for one axis silently creates this defect on the other ([ADR-0026] composes as *bound
  first, widen second*).
- **A second grid.** Per-viewport spans (#287) multiply the sites without changing the rule.
- **Anything that changes what a pair is.** Cluster-width promotion behind DECSET 2027 (#295) makes
  cells wide that per-character width says are narrow, and the agreement predicate is written against
  the flags, not against the codepoint.

[ADR-0026]: ../../adr/0026-outside-coordinates-are-bounded-once.md
