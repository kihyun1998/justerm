# Cross-cutting invariant — row-keyed side maps carry what the packed cell cannot

## The fact

The in-memory `Cell` is a fixed-width packed word with no room left. **Every per-cell fact that does
not fit lives in a column-keyed map on the `Row`, gated by a presence bit in the cell**, and each such
map obeys the same three rules:

1. **Read only through the gate.** Consult the map *iff* the cell's presence bit is set
   (`COMBINED_PRESENT` / `LINK_PRESENT` / `UCOLOR_PRESENT`). Never iterate the map as truth — a stale
   entry whose cell was overwritten is normal, and the bit is what makes it harmless.
2. **The row is the unit that moves.** The maps ride with the `Row` through scroll, scrollback entry
   and reflow, which is why the escape hatch is *row*-keyed rather than grid-keyed.
3. **A write that clears the cell owes the bit, not the map.** Clearing the presence bit is what
   retires the fact; purging the map is an optimisation, never the correctness step.
4. **Across the wire the bit is *reconstructed*, never transmitted.** The presence bits live in the
   packed colour words, which the wire's colour encoding strips, and the wire's flag field carries
   none — so `decode` re-derives each bit from whether its group carries an entry for that column.
   Where the group rides the per-cell record the cell decoder does it; where it is a *separate*
   group, that group's own loop owes the re-arm. A rider that adds a group and not a re-arm produces
   a frame whose value is present and whose gate is shut.

Rule 4 is the one a reader is least likely to derive, because rules 1–3 are all about the in-memory
row and read as complete. It arrived by defect: [wire format](../territory/wire-format.md) shipped the
underline-colour group with no re-arm, and nothing noticed for two releases.

## Why it is cross-cutting

**One escape hatch, three unrelated facts.** Combining marks, OSC 8 hyperlinks and SGR 58 underline
colours have nothing to do with each other — they arrived years apart from different features — and
they share only the constraint that the cell word is full. The rules above therefore hold in three
places that never call each other, and a fourth fact hitting the same wall will reach for the same
hatch.

This is the same *shape* as [wide glyph](../territory/wide-glyph.md)'s
problem (a truth whose scope is not the cell it is stored in) but not the same fact: there the row/pair
owns the meaning, here the row owns the *storage* while the meaning stays per-cell. ADR-0025 D1 governs
the first; nothing governs this one.

## Territories it holds in

- [pen](../territory/pen.md) — `Pen::underline_color` (SGR 58) is deliberately **not** packed
  into the printed cell; the print path stamps it into the row's ucolor map (#520)
- [hyperlinks](../territory/hyperlinks.md) — OSC 8 link ids, moved out of the cell into the row's link map
  (#45/#46)
- [grapheme clusters](../territory/grapheme-clusters.md) — multi-code-point cluster overflow, kept out so the cell stays
  fixed-width (#45/#46)
- [wide glyph](../territory/wide-glyph.md) — adjacent, not identical: the
  extended-attr rider a wide lead carries is one of these maps meeting the pair rule (#521)

Storage: `Row { cells, combining, links, ucolors, wrapped }` in `justerm-core/src/grid.rs`; the
combining and link maps share one implementation.

## What a violation looks like

Two symptoms, from opposite directions:

- **Gate bypassed** — a reader iterates the map instead of testing the presence bit, and resurrects an
  attribute on a cell that was overwritten. Visually: a stale underline colour or a phantom hyperlink
  on freshly typed text.
- **Map does not ride** — a row-moving verb rebuilds cells without carrying the maps, and combining
  marks / links / underline colours vanish on scroll or reflow while the glyphs survive.
- **Gate shut over a live value** — the rule-4 failure, and the only one of the three that is
  invisible on a live grid: the map holds the fact, the bit does not, so the gated read returns the
  default. It reaches a consumer only through the wire, which is why no grid test can see it.

The first is silent and looks like a rendering bug; the second looks like data loss; the third looks
like the feature was never used on that cell.

## Discovery history

Thinner than [alt-screen absolute-index floor](alt-screen-buffer-floor.md) — this began as a
*deliberate* pattern that was extended, not a bug found three times — but it is recorded here because
the extension is exactly when the rules get re-derived rather than read. That prediction then came
true against the note itself: rules 1–3 were written from the in-memory row and read as the whole
rule, so #531 rediscovered rule 4 rather than reading it.

| Event | Site | Issue |
|---|---|---|
| Pattern established | combining marks, hyperlinks | #43 epic → #45 / #46 |
| Pattern extended years later | SGR 58 underline colour | #520 |
| Rules met the pair rule | extended-attr rider on a wide lead | #521 (in the #552 roster) |
| Rule 4 found by defect | the underline-colour group crossed the wire with no re-arm | #531 |

The tell that this note earns its place: #520 did not open a decision about *where* underline colour
should live. It reached for the hatch, correctly, from a code comment — which means the rules survive
only as long as the next author reads that comment.

## Where it will recur

The next per-cell attribute that does not fit the packed cell. Test: if a feature wants a new field on
`Cell` and the cell is full, it is subject to this invariant — and the first question is not "where do
I store it" but "which presence bit gates it, which verbs clear that bit, and — if it crosses the
wire — which decode site re-arms it".
