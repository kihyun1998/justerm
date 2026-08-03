# Territory — wide glyph

## What it is

A width-2 glyph (CJK, fullwidth, some emoji) occupying **two cells that must be treated as one
thing**: a lead carrying the character and a trailing `C_SPACER` standing for its second half. Every
verb that writes, moves, erases or frees a cell has to know the pair exists.

## Governing decisions

- [**ADR-0025 — row and wide-pair cell state ownership**](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — **D3 and D4** are this territory's half (D1/D2 govern [soft wrap](soft-wrap.md)), plus the #547
  and #529 amendments
- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) —
  the renderer-side geometry a width-2 cell is drawn into
- **spine #552** — the cluster ADR-0025 was extracted from, now closed. A GitHub issue, so not a
  graph node: read it for the fifteen-issue archaeology, not for current state

## Design model

ADR-0025 is authoritative; this is routing. **If they disagree, the ADR is right.**

- **A writer that lands on half a pair owes the other half — including a writer outside the engine.**
  The IME preedit pass (#249) writes viewport cells directly, so a run landing on an existing spacer
  would leave its lead drawing *"its left half only"* — legal when core's resize produces it, not
  something a preedit may create. The pass blanks the cell it orphans on either side. Same D1/D2
  obligation as an engine verb, reached from the consumer's side of the boundary.
- **D3 — a pair property is meaningful only at its defining position.** The leading-spacer marker
  means "wide-wrap artefact" *only* at the last column of a soft-wrapped row. A row-shift verb
  (ICH/DCH) that carries it inward has produced a marker describing nothing and must drop it (#528).
  **Position is part of the test, never the marker alone.**
- **D4 — both halves move together, set and clear** — within the verbs that **edit**. Any path that
  moves, synthesises or frees one cell carries the whole pair: the lead's extended-attr rider (#521),
  the trailing `C_SPACER`, and the reach-**back** repair of the previous row's leading spacer when a
  wrapped lead is overwritten.
- **D4's scope stops at reallocation** (#529). `Row::resize` narrows straight through a pair and
  leaves the lead without its spacer — **that is a legal buffer state**, not a violation. Repairing it
  would destroy text that currently survives.
- **D4's precondition is `MIN_COLUMNS = 2`** (#547). At one column both halves physically cannot fit,
  so while the engine accepted that width the rule had an unstated precondition — and the version of
  D4 read as universal lost data irreversibly.
- **Width is derived, not stored.** It reads out of `flags & WIDE_CHAR`; neither the in-memory cell
  nor the wire record spends a field on it.
- **Width is computed per character.** VS16 (`FE0F`) and keycap sequences therefore arrive as
  `wide = false` — string-level promotion is impossible here, and DECSET 2027 (#295) is the opt-in
  clustering that changes it.

## Code

- `justerm-core/src/cell.rs` — `WIDE_CHAR`, `C_SPACER`
- `justerm-core/src/grid.rs` — `Row::resize` (the boundary of D4's scope), `reflow`
- `justerm-core/src/term.rs` — `Term::write_glyph`, `Term::drop_artefact_if_erased`,
  `Term::free_cell`, `Term::vacate_for_wrap`
- `justerm-core/src/term.rs` — `MIN_COLUMNS` (defined there, re-exported from `lib.rs`), and its
  mirror `Term::print`'s `width.min(2)` with the matching `debug_assert` in `Term::write_glyph`
  (#595). The two are the pair model's preconditions from opposite sides: `MIN_COLUMNS` floors the
  screen so a pair has room, the clamp caps the glyph so a pair is enough

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Wide glyphs, spacers, and the wrap artefact](../../agents/reference-facts.md#wide-glyphs-spacers-and-the-wrap-artefact)
- [Relocating a cluster that grew to width 2](../../agents/reference-facts.md#relocating-a-cluster-that-grew-to-width-2-529-verified-2026-07-28)
- [Minimum screen size](../../agents/reference-facts.md#minimum-screen-size-547) — both references
  forbid one column for this exact reason; ghostty permits it and destroys the glyph
- [Maximum glyph width](../../agents/reference-facts.md#maximum-glyph-width-595) — the mirror of the
  one above, and the pair model's *other* precondition: 3 of 3 references cap a glyph at a pair, so a
  width of 3 from `unicode-width` never reaches the grid unmarked. justerm was the only one not
  capping it. Each reference's bound is weaker than its headline — the rows carry the qualifications
- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — with the trap beside it: ghostty has two `clearCells` and the first grep hit is how #530's body
  reached the wrong verdict

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — a wide lead's extended-attr rider is
  where the pair rule meets the presence-bit discipline (#521)

## Blast radius

- [soft wrap](soft-wrap.md) — a lead that does not fit at the right margin *causes* a wrap and leaves
  the artefact the wrap rules then have to clear
- [selection](selection.md) · [logical lines](logical-lines.md) · [search & active match](search.md)
  — every text extractor skips spacers and drops the artefact; a change to what a spacer means
  changes what all three return
- [pen](pen.md) — both halves are stamped from the same pen
- [wire format](wire-format.md) — width is derived at both ends rather than transmitted
- [emoji classification](emoji-classification.md) — how a spacer cell is drawn, and the classification that cannot
  use `wide` because of the per-character rule above

## Known holes / open

- **VS16 / keycap clusters** — per-character width makes string-level promotion impossible; #303 and
  #304 are the tail.
- **#562 — reflow cannot express a point one past the last cell.** Five designs built, measured and
  rejected; read that issue before touching relocation, the failures are the content.
