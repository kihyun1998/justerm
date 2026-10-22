# Territory — wide glyph

## What it is

A width-2 glyph (CJK, fullwidth, some emoji) occupying **two cells that must be treated as one
thing**: a lead carrying the character and `WIDE_CHAR`, and a trailing cell carrying
`WIDE_CHAR_SPACER` (`C_SPACER`) for its second half — plus the *wide-wrap artefact*, the blank a wide
glyph leaves at the last column when it does not fit and wraps. Every verb that writes, moves, erases
or frees a cell has to know the pair exists, including one writer outside the engine (the renderer's
preedit pass).

## Governing decisions

- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — **D3 and D4** are this territory's half (D1/D2 govern [soft wrap](soft-wrap.md)), with the #547
  (`MIN_COLUMNS`), #595 (width cap) and #529 (D4's scope stops at reallocation) amendments. **If this
  note and the ADR disagree, the ADR is right.**
- [ADR-0028 — composition surfaces have one writer each](../../adr/0028-composition-surfaces-have-one-writer-each.md)
  D2 — why the renderer's preedit repair blanks only the glyph of the cell it orphans.
- Adjacent, not governing: [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md)
  decides the renderer-side geometry a width-2 cell is drawn into, nothing about the pair in the
  buffer. Spine #552 (closed) is the fifteen-issue cluster ADR-0025 was extracted from — read it for
  the archaeology, not for current state.

## Design model

ADR-0025 is authoritative; this is routing plus what the source shows.

- **D3 — a pair property is meaningful only at its defining position.** The leading-spacer marker
  means "wide-wrap artefact" *only* at the last column of a soft-wrapped row. A row-shift verb that
  carries it inward has produced a marker describing nothing and must drop it (#528). Position is part
  of the test, never the marker alone.
- **D4 — both halves move together, set and clear**, within the verbs that **edit**: any path that
  moves, synthesises or frees one cell carries the whole pair — the lead's extended-attr rider (#521),
  the trailing spacer, and the reach-**back** repair of the previous row's artefact when a wrapped lead
  is overwritten. The no-orphan repair is `Term::free_cell`, called at every overwrite, erase and
  row-shift seam (`write_glyph`, `clear_cells`, `insert_chars`, `delete_chars`, `vacate_for_wrap`,
  `promote_cluster_to_wide`, `relocate_cluster_wide`, `demote_cluster_to_narrow`).
- **D4's scope stops at reallocation** (#529). `Row::resize` narrows straight through a pair and leaves
  the lead without its spacer — a **legal buffer state**, not a violation; repairing it would destroy
  text that survives. The alt screen reaches it on every narrowing, because it resizes without
  reflowing (#567).
- **D4's two preconditions are mirrors (#547, #595).** `MIN_COLUMNS = 2` floors the screen so a pair
  has room — at one column both halves cannot fit, and D4 read as universal lost data irreversibly.
  `Term::place_grapheme` caps a glyph at a pair (`width.min(2)`) and `Term::write_glyph` asserts it
  (`debug_assert!`): `unicode-width` returns 3 for at least U+17D8 KHMER SIGN BEYYAL, which left
  uncoerced fell through every `width == 2` branch while still driving the advance. All three
  references bound it — ghostty says why (`unicode/props.zig:11-13`, *"3-em dash becomes a 2-em
  dash"*).
- **Width is derived, not stored**: it reads out of `WIDE_CHAR`; neither the cell nor the wire record
  spends a field on it. **Width is computed per character** while mode 2027 is off, so VS16 (`FE0F`)
  and keycap sequences arrive as `wide = false`; under 2027 a join can promote a cluster to a pair
  ([grapheme clusters](grapheme-clusters.md)).
- **The wide-wrap artefact column is written, not flagged** (#528, `Term::vacate_for_wrap`): a blank
  from the current pen, marked as a leading spacer, as every reference does — xterm.js
  `setCellFromCodepoint(col, 0, 1, curAttr)`, ghostty `printCell(0, .spacer_head)`, alacritty
  `write_at_cursor(' ')` under a `LEADING_WIDE_CHAR_SPACER` template. Flagging in place left the
  previous occupant's glyph, link and underline colour alive in a cell every text reader skips, so a
  renderer drew a character that could not be copied, searched or announced. The marker is
  alacritty's (ghostty's `.spacer_head`); xterm.js writes a bare null and re-infers the artefact at
  reflow, so a lost marker there degrades to an empty cell trimming drops — justerm writes `' '`, so
  the marker is the only thing keeping the column out of extracted text. Blanking it is an overwrite,
  so it owes the no-orphan repair when the column was a spacer.
- **The artefact goes with the wrap, in `Term::end_wrap`** (D3): its claim is "this blank was vacated
  *because this row continues*", so a row that stops continuing cannot hold one. ghostty couples them
  the same way in `Screen.cursorResetWrap` (`terminal/Screen.zig:1524`), which early-returns on an
  unwrapped row where justerm clears unconditionally. The leftward erases (`EL 1`, `ED 1`) keep the
  wrap but can blank the column, so `Term::drop_artefact_if_erased` drops only the marker. The one
  wrap-ending path that does not reach `end_wrap` is `shift_region`'s `top == 0` seam, whose row is in
  scrollback; it couples the two clears inline.
- **`Term::void_wrap_artefact_above` voids the row above's record before a pair at columns 0..=1 is
  destroyed or moved (#534).** Call sites ask `Term::wrapped_pair_at_row_start` first, *before* the
  mutation; the record survives only an in-place same-width overwrite (ghostty's `if (cell.wide !=
  wide)` escape; alacritty, with none, is the outlier). **Asking after the mutation is not
  equivalent** — it answers "is some wide lead at column 0", which a `DCH` pulling the next wide glyph
  left also satisfies, and which a two-step placement (VS16 promotion, or IRM's insert-then-write)
  satisfies only at the end; both were measured disagreeing. The erase and intra-row-shift sites are
  ported from ghostty's `Screen.splitCellBoundary`; justerm's `ICH` site has no counterpart there.
- **At grid row 0 on the primary, "the row above" is the last scrollback row**, because the readers
  walk `[scrollback ++ grid]` as one buffer; alacritty reaches the same row (`topmost_line()` is
  `Line(-history_size)`), ghostty stops at the viewport (`cursor.y > 0`). On the alt screen the floor
  is the screen top and nothing above it may be touched, so the reach-back is guarded by `on_alt`
  (the [alt floor](../invariant/alt-screen-buffer-floor.md) lists it under soft wrap). No damage is
  owed for the scrollback reach: the marker is a content bit outside `CONTENT_MARKER_MASK`, so it never
  crosses the wire.
- **ICH needs no repair to its own row's marker**: a right shift always pushes the last column off the
  edge (pinned by `ich_discards_the_marker_off_the_edge`). DCH is the opposite: it carries the marker
  inward, so it ends the wrap **before** the shift (#534), or the marker is stranded mid-row and
  swallows the blank between two runs in copy and search.
- **A cluster relocated to the next row owes the destination's no-orphan repair** (#303, #529, D4):
  its spacer lands on `(nr, 1)` and would half-destroy a wide glyph standing there. xterm.js and
  ghostty repair it structurally (a pair is two cell writes with the repair in the write; ghostty's
  `.wide` arm clears the neighbouring tail); alacritty never relocates. justerm writes both halves in
  one step, so each wide-writing path restates it (`write_glyph`, `promote_cluster_to_wide`,
  `relocate_cluster_wide`). ghostty's reach-back to the previous row's `.spacer_head` at this site is
  *not* copied — that is the marker the relocation just set (#534's rule: a repair keyed on a state
  predicate must not fire mid-construction). `void_wrap_artefact_above(nr)` would clear the record
  `vacate_for_wrap` just set, in both the advance and the scroll case. **`2 < cols` is a live bound**:
  the print paths cannot leave a lead in the last column but `Row::resize` can on the alt screen, and
  at `cols == 2` the relocation would read `(nr, 2)` — an out-of-bounds panic from shrinking a window
  over a CJK glyph. Pinned by `a_relocation_beside_a_truncated_wide_lead_does_not_index_past_the_row`.
  #562's five measured-and-rejected designs for a point one past the last cell are the content to read
  before touching relocation.
- **A writer outside the engine owes the same debt.** The renderer's IME preedit pass (#249) writes
  viewport cells directly, so a run landing on one half of an existing pair would leave the other half
  drawing alone — legal when core's resize produces it, not something a preedit may create. The pass
  blanks the **glyph** of the cell it orphans (codepoint, cluster override and both `WIDE` bits) and
  nothing else, because the debt is owed to the pair and the cell is still the application's
  (`WriteKind::Repair`; #715 — for one release it blanked the whole cell and took the background with
  it).
- **The engine and the renderer repair that event differently, deliberately.** `Term::free_cell`
  leaves the pen's background and defaults everything else (the maintainer's call, 2026-07-24; its
  doc-comment carries the grounds — a destroyed glyph's hyperlink, and DECSCA, which is not
  implemented); the preedit pass keeps the cell and removes only the glyph (ADR-0028 D2, which also
  names the rejected third option). Read both before "fixing" either into the other.

## Code

- `justerm-core/src/cell.rs` — `CellFlags` (`WIDE_CHAR`, `WIDE_CHAR_SPACER`), `C_SPACER`,
  `CONTENT_MARKER_MASK`, `Cell::is_leading_spacer` / `Cell::set_leading_spacer` /
  `Cell::clear_leading_spacer`
- `justerm-core/src/grid.rs` — `Row::resize` (the boundary of D4's scope), `reflow`
- `justerm-core/src/term.rs` — `MIN_COLUMNS`; `Term::place_grapheme` (the width cap),
  `Term::write_glyph`, `Term::free_cell`, `Term::vacate_for_wrap`, `Term::wrapline_advances`,
  `Term::end_wrap`, `Term::drop_artefact_if_erased`, `Term::void_wrap_artefact_above`,
  `Term::wrapped_pair_at_row_start`, `Term::clear_cells`, `Term::insert_chars`, `Term::delete_chars`,
  `Term::promote_cluster_to_wide`, `Term::relocate_cluster_wide`, `Term::demote_cluster_to_narrow`
- `justerm-renderer/src/preedit.rs` — `WriteKind`, `writes`, `patch` (the out-of-engine writer)

## Reference behaviour

- [Wide glyphs, spacers, and the wrap artefact](../../agents/reference-facts.md#wide-glyphs-spacers-and-the-wrap-artefact)
  — with [the marker's clear discipline](../../agents/reference-facts.md#the-markers-clear-discipline-534-verified-2026-07-27)
  (#534)
- [Relocating a cluster that grew to width 2](../../agents/reference-facts.md#relocating-a-cluster-that-grew-to-width-2-529-verified-2026-07-28)
- [Minimum screen size](../../agents/reference-facts.md#minimum-screen-size-547) — both references
  forbid one column for this reason; ghostty permits it and destroys the glyph
- [Maximum glyph width](../../agents/reference-facts.md#maximum-glyph-width-595) — 3 of 3 references
  cap a glyph at a pair; justerm was the only one not capping. Each reference's bound is weaker than
  its headline — the rows carry the qualifications
- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — with the trap beside it: ghostty has two `clearCells`, and the first grep hit is how #530's body
  reached the wrong verdict

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — the obligation
  every *reader* of a pair owes, as opposed to D3/D4's rules for the verbs that write one (#454)
- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — a wide lead's extended-attr rider is
  where the pair rule meets the presence-bit discipline (#521)

## Blast radius

- [soft wrap](soft-wrap.md) — a lead that does not fit at the right margin *causes* a wrap and leaves
  the artefact; the wrap rules assume the artefact exists only at the last column of a wrapped row
- [selection](selection.md) · [logical lines](logical-lines.md) · [search](search.md) ·
  [accessibility](accessibility.md) — every text extractor skips spacers and drops the artefact; a
  change to what a spacer or the marker means changes what all of them return
- [grapheme clusters](grapheme-clusters.md) — under mode 2027 a join creates, relocates and destroys
  pairs; the join path assumes these repair sites are complete
- [pen](pen.md) — both halves are stamped from the same pen, and a synthesised spacer from the lead
- [reflow](reflow.md) — reflow assumes a pair and its artefact are well-formed on the primary; the alt
  screen's truncated lead is the one legal exception it must tolerate
- [wire format](wire-format.md) — width is derived at both ends rather than transmitted; the leading
  spacer never crosses
- [emoji classification](emoji-classification.md) — how a spacer cell is drawn, and the classification
  that cannot use `wide` because of the per-character rule
- [cell compositing](cell-compositing.md) — the renderer's preedit pass is the out-of-engine writer of
  a pair; it assumes the only lead-without-spacer core hands it is the one a resize truncated

## Known holes / open

- **Per-character width with mode 2027 off is the default and stays wrong for VS16 / keycap
  sequences** by design; the opt-in is the only remedy (#295).
- **D4's site list is restated per path, not funnelled.** `write_glyph`, `promote_cluster_to_wide`
  and `relocate_cluster_wide` each restate the destination repair; a fourth wide-writing path owes it
  and nothing enforces that.
