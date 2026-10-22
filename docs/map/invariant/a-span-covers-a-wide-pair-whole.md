# Invariant — a span covers a wide pair whole, or not at all

## The fact

A width-2 glyph occupies two cells, a lead and a trailing spacer, and every range this family computes
over columns is plain numbers that know nothing about that. A range whose edge falls between the halves
is representable, reachable and wrong. So **any range that includes one half of a pair includes both** —
a widening, never a shrinking — for every kind of range:

1. **A gesture range** — a press on the right half of a CJK glyph anchors on its spacer.
2. **A consumer-authored range** — a `Match` handed back, or a decoration rect in the consumer's
   coordinates; the consumer need not know where pairs are.
3. **A bounded range** — an out-of-range column clamped onto the row's last cell, a spacer whenever the
   row ends in a wide glyph, so the bound itself manufactures a half-covering span (#678).
4. **The caret** — a one- or two-cell span the application can park on a spacer with `CUB` / `CHA`.

**A range that includes no cell is not widened** (#914). Emptiness is decided from the endpoints before
any pair handling — linear `same line && from >= to`, rectangle `to <= from` — as all three references
order it. Widening a bare click conjured a highlight, and once a settled selection fed the X11 primary
buffer, a click that overwrote it. The line half is load-bearing: a multi-line run whose end column is
left of its start is not empty.

**Both cells must agree before they are a pair.** A lead pairs only with a spacer to its right, a
spacer only with a lead to its left, never across a row. A lead alone in the last column is a legal
buffer state (`Row::resize` narrows straight through a pair, and
[ADR-0025](../../adr/0025-row-and-wide-pair-cell-state-ownership.md) D4's scope stops at
reallocation), so "the next column is my spacer" is false in normal operation.

## Why it is cross-cutting

The producers sit in different layers and share no seam before the cells are known:
`Term::selection_range` and `Term::match_spans` are core's, a decoration rect is the consumer's, the
caret comes from the application's cursor. So the rule is spelled twice on purpose, and the two
spellings do not call each other (re-verified at HEAD 7bb31f7):

- core — `justerm-core/src/term/selection.rs — Term::pair_start` / `Term::pair_end`, used by selection
  and by `term/search.rs — Term::match_spans`, so `selection_range` and `selection_text` agree on what a
  selection contains.
- renderer — `justerm-renderer/src/pair.rs — partner_at`, the paint-site rule every span passes
  through: `overlay.rs`, `decoration.rs`, `cursor.rs — cursor_cells_at`, and `frame.rs`'s packer.

Each layer is locally correct with a half-rule; the disagreement shows only as a glyph split down the
middle. A per-cell **gate** reaches it too (#791): `I_neighbour` (ink crossing into the cell above or
below) is withdrawn per cell where backgrounds differ, so a background edge under one column of a pair
would cut the overflow down the middle of the letter. It is reconciled on the receivers in
`justerm-renderer/src/frame.rs`'s packer, because the pair is in the source row while the disagreement
is in the receiving one.

The mis-routing is on the record: three artifacts filed this as consumer span policy — #454's title,
[ADR-0024](../../adr/0024-decoration-projection-and-precedence.md)'s consequence list, and a
doc-comment in `justerm-renderer` `overlay.rs` — because a decoration rect was the producer everyone
had in mind. Two of the producers are core's and never reach the consumer.

## Territories it holds in

- [wide glyph](../territory/wide-glyph.md) — the pair is this territory's subject; the rule is what
  every reader of one owes.
- [selection](../territory/selection.md) — both observables (`selection_range`, `selection_text`); the
  `Block` arm needs it per row, since a rectangle meets a pair at a different column on each.
- [search](../territory/search.md) — `Term::match_spans`, where a consumer-authored match and the clamp
  both reach it (`pair_start` / `pair_end`).
- [decoration](../territory/decoration.md) — a rect in consumer coordinates, resolved at the paint site
  (`decoration.rs`, via `partner_at`).
- [settled-cell paint](../territory/settled-cell-paint.md) — `term/paint.rs — Term::paint_logical_line`
  (#967) writes colour into a span's cells and takes the spacer with its lead only when the next cell
  is a spacer. Its first version tested the lead alone and indexed past the row on an alt re-fit that had
  cut the pair — the legal state above, met by a writer.
- [cell compositing](../territory/cell-compositing.md) — overlay and decoration lookups, the caret's
  span, and the `I_neighbour` gate all resolve through `partner_at`.
- [cursor position](../territory/cursor-position.md) — the application can park the cursor on a
  spacer, so the position core reports can be half a pair.
- [caret drawing](../territory/caret-drawing.md) — `justerm-renderer/src/cursor.rs — cursor_cells_at`
  widens the caret to the whole pair through `pair::partner_at`, moving it back onto the lead.
- [damage](../territory/damage.md) — a site found working #826 and not fixed:
  `justerm-core/src/term.rs — Term::frame_damage` folds the cursor in per cell (old and new cell), while
  the renderer's caret moves left onto the lead when it stands on a spacer, lighting a column the frame
  never named. Measured: a wide glyph at cols 7–8, cursor walked onto col 8, frame reports one span
  `left 8, right 14`, col 7 outside it. The prose statements of the fold (the cursor-position note and
  `architecture.md`'s "damages the cell the cursor left + the cell it lands on") are per cell and wrong
  for a pair. Validity condition: no live artefact while `justerm-renderer` re-packs the whole viewport
  every frame and never consults damage for the caret; the gap is in the wire contract, for the
  cell-invert consumer the fold exists to serve.

## What a violation looks like

A CJK glyph split down the middle, one half in the selection colour. On the caret, a block cursor
lighting half a glyph — or, if the rule is written "widen rightward" instead of "cover the pair", a caret
stretching onto an innocent neighbour.

The quiet one has no visual tell: a highlight and a copy that disagree. A range ending inside a pair
paints two cells while `selection_text` returns neither half, because a spacer extracts as nothing.

## Discovery history

- **#454** (filed 2026-07-23) — filed as a `justerm-web` span-policy defect and held open two and a
  half weeks under "family, hold neutral — 1 of 3 references snaps". The tally was wrong both ways:
  ghostty's rule was searched for in `terminal/Selection.zig` and lives in its renderer; alacritty's was
  summarised as absent from a method that is on `Selection`. All three refuse to paint half a glyph
  ([reference facts](../../agents/reference-facts.md#a-span-a-highlight-and-a-caret-over-a-wide-pair-454-verified-2026-08-10)).
- Three siblings had met the rule without naming it: #529 (core producing an invalid pair), #535 (a
  word extent stopping mid-glyph), #678 (a clamp landing on a spacer). The caret's half (`cursor_span`
  widening on `WIDE_CHAR` only) had a test pinning the defect as expected, on the grounds that no
  reference handled it.
- **#791** — the gate form, `I_neighbour` reconciled across a pair.
- **#826** — the damage-fold site above.
- **#914** — the empty-range exception.
- **#967** — the paint writer.

## Where it will recur

- **A new span kind** — a fifth overlay group, a hover highlight, a bracket-match indicator. If it
  colours or bounds a column range and does not go through `partner_at` (renderer) or `pair_start` /
  `pair_end` (core), it lacks the rule.
- **A new per-cell gate** on a glyph's ink or colour: a pair is one glyph, so a per-cell decision must
  be reconciled across both halves.
- **A new bound.** A clamp of an outside coordinate onto a real cell can land on a spacer;
  [ADR-0026](../../adr/0026-outside-coordinates-are-bounded-once.md) composes as bound first, widen
  second.
- **A second grid.** Per-viewport spans (#287) multiply sites without changing the rule.
- **Anything that changes what a pair is.** Cluster-width promotion behind DECSET 2027 (#295) makes
  cells wide that per-character width calls narrow; the agreement predicate reads the flags, not the
  codepoint.
