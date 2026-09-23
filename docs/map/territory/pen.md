# Territory — pen (the SGR drawing state)

## What it is

The appearance stamped into every printed cell: foreground, background, attribute flags and the
underline colour. Modelled as a **template cell** — `Pen::cell(c)` produces the `Cell` that gets
written — so "what a glyph looks like" is one value rather than a set of parameters threaded through
the print path.

Nothing about it is positional. It shares a struct with [cursor position](cursor-position.md) because
VT couples them in `SGR` + print, not because they are one concept.

## Governing decisions

**None.**

- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) governs how the
  *renderer* resolves a finished cell into bg / fg / ink — downstream of everything here, and it
  decides nothing about what the pen puts into the cell

## Design model

- **The pen is a template cell, and that is a deliberate borrowing.** Alacritty models it the same
  way, and the payoff is stated in the source: making erase (ED/EL) fill cleared cells with the pen's
  `bg` instead of `Default` **is** BCE (background colour erase), with no structural change. The
  model was chosen so a future feature is a one-line switch rather than a refactor.
- **`underline_color` (SGR 58) is in the pen but not in the cell.** The 12-byte cell is full, so the
  print path stamps a non-default value into the row's ucolor map instead — see
  [row-keyed side maps](../invariant/row-keyed-side-maps.md). `Default` means "follow the fg", which
  is a third state distinct from "unset" and "explicitly set to the fg colour".
- **`Pen::reset()` is SGR 0** and restores every field at once, including the underline colour.
- **Colours are references, never resolved.** `Default | Indexed(u8) | Rgb(..)` — the engine is
  theme-agnostic by identity, so the pen never holds a hex value.
- **`pen_ext_attrs` is the one place a pen's extended attributes are built** — the open OSC 8 link
  (#26, #46) and a non-default underline colour (SGR 58, #520), the colour gated on `UNDERLINE`
  because it is meaningless on a cell that draws none (xterm likewise does not persist it:
  `AttributeData isEmpty()` ignores it, `InputHandler.test.ts:2084`; and ADR-0020 keeps inert
  per-cell payload off the wire). Strikethrough alone does not arm it. Three sites take their
  extended attributes from the pen — the glyph, its wide spacer, the vacated wrap column — so a new
  rider is added here once (#521, #528). **Five sites build a cell, not three**: `promote_cluster_to_wide`
  and `relocate_cluster_wide` synthesise a pair's spacer, whose attributes are the *lead's*
  (ADR-0025 D4), so they read `Row::ext_attrs_at` and the lead's underline style directly. Counting
  them in is how a rider silently misses them — which is what #829's underline style did until a
  refuting pass measured it.

## Code

- `justerm-core/src/cursor.rs` — `Pen` (`fg`, `bg`, `flags`, `underline_color`), `Pen::reset`,
  `Pen::cell`
- `justerm-core/src/term.rs` — `Term::write_glyph` (pen → cell, plus the ucolor stamp), the SGR
  dispatch
- `justerm-core/src/color.rs` — `Color`
- `justerm-core/src/cell.rs` — `CellFlags`, `Cell::from_parts`, `UnderlineStyle` and
  `CellFlags::set_underline_style` — the pen carries a 3-bit underline **style** as well as a
  colour since #829, and that setter is the single writer of it *and* of the derived
  `CellFlags::UNDERLINE`

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row is pinned to a `file:line`
at a recorded SHA; a paraphrase drops the pin).

- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — what the pen contributes to an erase fill in each reference, i.e. exactly what turning BCE on
  would mean here. It also carries the trap that misled #530: ghostty has two `clearCells`, and the
  first grep hit is the wrong one

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — `underline_color` reaches the row's
  ucolor map rather than the cell, under the presence-bit discipline that governs all three maps

## Blast radius

- [wide glyph](wide-glyph.md) — a wide glyph's two cells are stamped from the same pen, and the
  lead's extended-attr rider is where the pen meets the pair rule (#521)
- [cursor position](cursor-position.md) — same struct, same writing verbs; no semantic coupling
- [colour policy](colour-policy.md) — consumes the finished cell under ADR-0019; the pen decides what it
  receives, not how it is resolved
- [published surface](published-surface.md) — `UnderlineStyle` leaves the repo by name (#831): the
  binding mirrors this enum through an exhaustive `match`, so a variant added to the pen's style is
  a compile error one crate over rather than an unnamed number on npm

## Known holes / open

- **Zero governing records.** The template-cell model is a real architectural choice with a named
  prior art and a stated future payoff (BCE), and it lives in a doc comment.
- **BCE is not implemented**, and the note that it *would be* a one-line change is itself unverified
  — nothing tests that claim, so its cost could have grown since it was written.
- **`Default` as "follow the fg"** is a tri-state that no consumer-facing document explains.
