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

## Why it is cross-cutting

**One escape hatch, three unrelated facts.** Combining marks, OSC 8 hyperlinks and SGR 58 underline
colours have nothing to do with each other — they arrived years apart from different features — and
they share only the constraint that the cell word is full. The rules above therefore hold in three
places that never call each other, and a fourth fact hitting the same wall will reach for the same
hatch.

This is the same *shape* as [wide glyph & soft wrap](../territory/wide-glyph-and-soft-wrap.md)'s
problem (a truth whose scope is not the cell it is stored in) but not the same fact: there the row/pair
owns the meaning, here the row owns the *storage* while the meaning stays per-cell. ADR-0025 D1 governs
the first; nothing governs this one.

## Territories it holds in

- [cursor](../territory/cursor.md) — `Pen::underline_color` (SGR 58) is deliberately **not** packed
  into the printed cell; the print path stamps it into the row's ucolor map (#520)
- **hyperlinks** *(no note yet)* — OSC 8 link ids, moved out of the cell into the row's link map
  (#45/#46)
- **grapheme clusters** *(no note yet)* — multi-code-point cluster overflow, kept out so the cell stays
  fixed-width (#45/#46)
- [wide glyph & soft wrap](../territory/wide-glyph-and-soft-wrap.md) — adjacent, not identical: the
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

The first is silent and looks like a rendering bug; the second looks like data loss.

## Discovery history

Thinner than [alt-screen absolute-index floor](alt-screen-buffer-floor.md) — this is a *deliberate*
pattern that was extended, not a bug found three times — but it is recorded here because the extension
is exactly when the rules get re-derived rather than read.

| Event | Site | Issue |
|---|---|---|
| Pattern established | combining marks, hyperlinks | #43 epic → #45 / #46 |
| Pattern extended years later | SGR 58 underline colour | #520 |
| Rules met the pair rule | extended-attr rider on a wide lead | #521 (in the #552 roster) |

The tell that this note earns its place: #520 did not open a decision about *where* underline colour
should live. It reached for the hatch, correctly, from a code comment — which means the rules survive
only as long as the next author reads that comment.

## Where it will recur

The next per-cell attribute that does not fit the packed cell. Test: if a feature wants a new field on
`Cell` and the cell is full, it is subject to this invariant — and the first question is not "where do
I store it" but "which presence bit gates it, and which verbs clear that bit".
