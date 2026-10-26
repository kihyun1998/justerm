# Territory — pen (the SGR drawing state)

## What it is

The appearance stamped into every printed cell and every erased one: foreground, background,
attribute flags with the 3-bit underline style, and the underline colour. Modelled as a **template
cell** — `Pen::cell(c)` produces the `Cell` that gets written — so "what a glyph looks like" is one
value rather than parameters threaded through the print path. `SGR` (`Term::sgr`) is its only writer
besides the resets and DECRC. Nothing about it is positional: it lives inside `Cursor` because VT
couples them in `SGR` + print, not because they are one concept ([cursor](cursor.md)).

## Governing decisions

**None.**

- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) governs how the
  *renderer* resolves a finished cell into bg / fg / ink — downstream of everything here; it decides
  nothing about what the pen puts into the cell.
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  is why an underline colour on a cell that draws no underline is not stamped: inert per-cell payload
  stays off the wire. It governs the wire, and this note only cites it as a ground.
- [ADR-0025](../../adr/0025-row-and-wide-pair-cell-state-ownership.md) D4 decides that a synthesised
  spacer takes the *lead's* attributes rather than the pen's — a pair rule, owned by
  [wide glyph](wide-glyph.md).

## Design model

From reading the source; there is no record.

- **The pen is a template cell, a deliberate borrowing from Alacritty.** `Pen`'s doc-comment states
  the payoff: making erase fill cleared cells with the pen's `bg` *is* Background Color Erase.
- **BCE is implemented** — erased and structurally blanked cells carry the pen's **background only**:
  `Term::clear_cells` (ED/EL/ECH), the gap `insert_chars` opens, the tail `delete_chars` vacates, the
  lines `scroll_region_lines` exposes, and the cell `Term::free_cell` frees when a pair is broken
  (xterm.js's `_eraseAttrData()`; the `free_cell` choice is the maintainer's, 2026-07-24). The
  artefact column a wide glyph vacates is the exception: it is written from the **whole** pen
  (`Term::vacate_for_wrap`, #528), as every reference does — see [wide glyph](wide-glyph.md).
- **`Pen::reset()` is SGR 0** and restores every field at once, including the underline colour and
  style. DECSTR resets the pen too (`soft_reset`), and `DECSC`/`DECRC` save and restore it with the
  position (`SavedCursor`). The alt-screen save (`?1048`/`?1049`) copies the whole `Cursor`, pen
  included.
- **Colours are references, never resolved.** `Color` is `Default | Indexed(u8) | Rgb(..)` and is
  deliberately exhaustive (#843) — the engine is theme-agnostic by identity, so the pen never holds a
  hex value.
- **`underline_color` (SGR 58/59) is in the pen but not in the cell.** The 12-byte cell is full (pinned
  by a `size_of::<Cell>() == 12` test in `cell.rs`), so the print path stamps a non-default value into
  the row's ucolor map — see [row-keyed side maps](../invariant/row-keyed-side-maps.md). `Default`
  means "follow the fg", a third state distinct from "unset" and "explicitly set to the fg colour".
- **The underline *style* is in the cell (#829).** `CellFlags::set_underline_style` is the single
  writer of the 3-bit style *and* of the derived `CellFlags::UNDERLINE`, so "is underlined" and
  "which style" cannot disagree; SGR 24 clears the style rather than only the flag. `SGR 4:n` maps
  0..5 and degrades an unknown sub-style to single (#829, #830). `SGR 21` is a double underline, not
  "cancel bold" — the spec (`ctlseqs.txt`) over `vte`'s reading (#830). Rows:
  [§ the underline style](../../agents/reference-facts.md#the-underline-style--where-it-is-stored-who-owns-is-underlined-and-how-it-reaches-the-drawing-layer-829-verified-2026-09-03),
  [§ `SGR 21`](../../agents/reference-facts.md#sgr-21--the-spec-and-three-implementations-say-double-the-fourth-says-cancel-bold-830-verified-2026-09-04).
- **`Term::pen_ext_attrs` is the one place a pen's extended attributes are built** — the open OSC 8
  link (#26, #46) and a non-default underline colour (#520), the colour gated on `UNDERLINE` because
  it is meaningless on a cell that draws none (xterm likewise does not persist it: `AttributeData
  isEmpty()` ignores it, `InputHandler.test.ts:2084`). Strikethrough alone does not arm it. Three
  sites take their extended attributes from the pen — the glyph and its wide spacer in
  `Term::write_glyph`, and the vacated wrap column in `Term::vacate_for_wrap` — so a new rider is added
  here once (#521, #528). **Five sites build a cell, not three**: `promote_cluster_to_wide` and
  `relocate_cluster_wide` synthesise a pair's spacer whose attributes are the *lead's* (ADR-0025 D4),
  read through `Row::ext_attrs_at` and the lead's `underline_style`. Counting them in is how a rider
  silently misses them — which is what #829's underline style did until a refuting pass measured it.
  The open link is ambient pen-like state (`current_link`) but not part of `Pen`, so SGR 0 does not
  close it.
- **The colon RGB form of `38`/`48`/`58` is count-based** (#520, `parse_extended_color`): a 5-param
  `38:2:r:g:b` reads RGB directly, while `38:2:cs:r:g:b` — or `38:2::r:g:b` with an empty colorspace,
  the form kitty and nvim emit — skips the colorspace slot. The short form is non-conformant to T.416
  / ISO-8613-6, but tolerating it is the ecosystem-dominant behaviour, verified against source
  (2026-07, #520): VTE (`src/sgr.hh`, `n > 4`), foot (`csi.c`, `sub.idx >= 5`) and alacritty
  (`ansi.rs`, `params.len() > 4`) count the sub-parameters; VTE calls it a "common misinterpretation
  of the standard" and supports it anyway. Only xterm.js is strict. ADR-0004's spec-faithfulness is
  about not *omitting* behaviour, not rejecting a widely emitted input.
- **The pen takes outside input only through SGR parameters**, whose count and value `vte` bounds (a
  parameter saturates at `u16::MAX`); colour components are cast to `u8`.

## Code

- `justerm-core/src/cursor.rs` — `Pen` (`fg`, `bg`, `flags`, `underline_color`), `Pen::reset`,
  `Pen::cell`
- `justerm-core/src/term.rs` — `Term::sgr`, `parse_extended_color`, `Term::pen_ext_attrs`,
  `Term::write_glyph` (pen → cell, plus the ucolor stamp), `Term::vacate_for_wrap`,
  `Term::clear_cells` / `Term::free_cell` (the BCE fill), `Term::soft_reset`, `SavedCursor`
- `justerm-core/src/color.rs` — `Color`
- `justerm-core/src/cell.rs` — `CellFlags`, `Cell::from_parts`, `UnderlineStyle`,
  `CellFlags::set_underline_style`
- `justerm-core/src/grid.rs` — `ExtAttrs::from_pen`, `Row::ext_attrs_at`

## Reference behaviour

- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — what the pen contributes to an erase fill in each reference. It also carries the trap that misled
  #530: ghostty has two `clearCells`, and the first grep hit is the wrong one.
- [The underline style](../../agents/reference-facts.md#the-underline-style--where-it-is-stored-who-owns-is-underlined-and-how-it-reaches-the-drawing-layer-829-verified-2026-09-03)
  — representation, ownership of "is underlined", the unknown sub-style (3–1).
- [`SGR 21`](../../agents/reference-facts.md#sgr-21--the-spec-and-three-implementations-say-double-the-fourth-says-cancel-bold-830-verified-2026-09-04)
  — the spec and three implementations say double.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — `underline_color` reaches the row's
  ucolor map rather than the cell, under the presence-bit discipline that governs all three maps

## Blast radius

- [wide glyph](wide-glyph.md) — a wide glyph's two cells are stamped from the same pen, and a
  synthesised spacer takes the lead's attributes, not the pen's; a new rider added to the pen alone
  misses the two synthesising sites (#521, #829)
- [colour policy](colour-policy.md) — resolves the references the pen stamps; it assumes `Default`
  in `underline_color` means "follow the fg"
- [published surface](published-surface.md) — `UnderlineStyle` leaves the repo by name (#831): the
  wasm binding mirrors the enum through an exhaustive `match`, so a variant added here is a compile
  error one crate over rather than an unnamed number on npm
- [wire format](wire-format.md) — the underline colour crosses as a sparse per-span group; a change to
  when it is stamped changes what that group carries
- [cursor position](cursor-position.md) — same struct and same saving verbs (DECSC, the alt save);
  a field added to `Pen` is saved and restored with the position whether or not that is wanted

## Known holes / open

- **Zero governing records.** The template-cell model is a real architectural choice with a named
  prior art, and it lives in a doc-comment.
- **`Default` as "follow the fg"** is a tri-state that no consumer-facing document explains.
