# Invariant — row-keyed side maps carry what the packed cell cannot

## The fact

The in-memory `Cell` is a fixed-width packed word with no room left. Every per-cell fact that does not
fit lives in a column-keyed map on the `Row`, gated by a presence bit in the cell, and each map obeys
four rules (the old note said "three" and listed four):

1. **Read only through the gate.** Consult the map iff the cell's presence bit is set. The bits are
   private constants in `justerm-core/src/cell.rs` — `C_COMBINED` (content word), `BG_LINK` and
   `BG_UCOLOR` (bg word) — called `COMBINED_PRESENT` / `LINK_PRESENT` / `UCOLOR_PRESENT` in comments
   and in the old note (those names are not symbols at HEAD 7bb31f7). Never iterate the map as truth: a
   stale entry under an overwritten cell is normal, and the bit makes it harmless.
2. **The row is the unit that moves.** The maps ride with the `Row` through scroll, scrollback entry and
   reflow — why the hatch is row-keyed, not grid-keyed.
3. **A write that clears the cell owes the bit, not the map.** Clearing the bit retires the fact;
   purging the map is an optimisation.
4. **Across the wire the bit is reconstructed, never transmitted.** The wire's flag field carries only
   `CellFlags`, and no presence bit is a `CellFlags` bit: the combining gate is stripped by the
   codepoint mask, the link and underline-colour gates by the colour encoding. `serialize.rs — decode`
   re-derives each from whether its group carries an entry for that column (`C_COMBINED` and
   `LINK_PRESENT` from the combining and link groups since v14 / #621, `UCOLOR_PRESENT` from the
   underline-colour group, #531). Where the group rides the per-cell record the cell decoder re-arms;
   where it is a separate group, that group's loop owes it. The regime inverts on the wire: in memory
   the bit is authoritative and the map a gated cache; on the wire the map is authoritative and the bit
   derived.

Storage: `justerm-core/src/grid.rs — Row { cells, combining, links, ucolors, wrapped }`. Combining and
ucolor values are inline (per-cell, unique); the link map holds `LinkUri` (`Arc<Box<str>>`) because
cells share a URI (#628; the bytes behind a box since #1038). All three die with the row, which the
rules depend on.

## Why it is cross-cutting

One escape hatch, three unrelated facts. Combining marks, OSC 8 hyperlinks and SGR 58 underline colours
arrived years apart from different features and share only that the cell word is full. The three maps'
code paths — the combining attach, the link stamp, the ucolor stamp, and their three decode re-arms —
never call each other, so the rules hold in three places independently, and a fourth fact hitting the
same wall will reach for the same hatch.

Same shape as [wide glyph](../territory/wide-glyph.md)'s problem (a truth whose scope is not the cell
it is stored in) but not the same fact: there the row/pair owns the meaning (ADR-0025 D1); here the row
owns the storage while the meaning stays per-cell, and no record governs it.

## Territories it holds in

- [pen](../territory/pen.md) — `Pen::underline_color` (SGR 58) is deliberately not packed into the
  printed cell; the print path stamps it into the row's ucolor map (#520).
- [hyperlinks](../territory/hyperlinks.md) — OSC 8 URIs in the row's link map (#45 / #46), since #628 the
  value (`LinkUri`) rather than an index into a buffer-wide pool.
- [grapheme clusters](../territory/grapheme-clusters.md) — multi-code-point cluster overflow, kept out
  so the cell stays fixed-width (#45 / #46).
- [wide glyph](../territory/wide-glyph.md) — adjacent: the extended-attr rider a wide lead carries is
  one of these maps meeting the pair rule (#521).
- [wire format](../territory/wire-format.md) — rule 4 and the inverted regime; #531 is a site under the
  wire regime written by someone reading the memory regime.

## What a violation looks like

- **Gate bypassed** — a reader iterates the map instead of testing the bit and resurrects an attribute
  on an overwritten cell: a stale underline colour or phantom link on freshly typed text.
- **Map does not ride** — a row-moving verb rebuilds cells without the maps, and marks / links /
  underline colours vanish on scroll or reflow while the glyphs survive.
- **Gate shut over a live value** — rule 4: the map holds the fact, the bit does not, the gated read
  returns the default. Invisible on a live grid; it reaches a consumer only through the wire, so no grid
  test sees it. It looks like the feature was never used on that cell.

## Discovery history

A deliberate pattern that was extended rather than a bug found three times; recorded because extension
is when the rules get re-derived rather than read — and that came true against this note: rules 1–3 were
written from the in-memory row and read as the whole rule, so #531 rediscovered rule 4.

| Event | Site | Issue |
|---|---|---|
| Pattern established | combining marks, hyperlinks | #43 epic → #45 / #46 |
| Extended years later | SGR 58 underline colour | #520 |
| Met the pair rule | extended-attr rider on a wide lead | #521 (in the #552 roster) |
| Rule 4 found by defect | the underline-colour group crossed the wire with no re-arm for two releases | #531 |
| Link group moved onto the wire's per-column re-arm | v14 | #621 |
| The map became the owner | link value from pool index to an owned `Arc` | #628 |

#520 opened no decision about where underline colour should live; it reached for the hatch from a code
comment — the rules survive only as long as the next author reads it.

## Where it will recur

The next per-cell attribute that does not fit the packed cell. If a feature wants a new field on `Cell`
and the cell is full, ask first: which presence bit gates it, which verbs clear that bit, and — if it
crosses the wire — which decode site re-arms it.
