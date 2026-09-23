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
  something a preedit may create. The pass blanks the **glyph** of the cell it orphans on either side
  — codepoint, cluster override and the two `WIDE` bits — and nothing else, because the debt is owed
  to the *pair* and the cell itself is still the application's (#715: for one release it blanked the
  whole cell and took the application's background with it). Same D1/D2 obligation as an engine verb,
  reached from the consumer's side of the boundary.
- **The engine and the renderer repair that same event differently, deliberately.** `Term::free_cell`
  leaves the pen's background and defaults everything else; the preedit pass leaves the cell whole and
  removes only the glyph. Both answers carry their grounds — core's in `free_cell`'s own doc-comment
  (#530: a destroyed glyph's hyperlink, the pen's DECSCA; the renderer has neither), the renderer's in
  [ADR-0028](../../adr/0028-composition-surfaces-have-one-writer-each.md) D2, which also names the
  third option that was rejected and what it cost. Read both before "fixing" either into the other.
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
- **The wide-wrap artefact column is written, not flagged** (#528): a blank from the current pen,
  as every reference does — xterm.js `setCellFromCodepoint(col, 0, 1, curAttr)`
  (`InputHandler.ts:609-611`), ghostty `printCell(0, .spacer_head)` (`Terminal.zig:1410-1412`),
  alacritty `write_at_cursor(' ')` under a `LEADING_WIDE_CHAR_SPACER` template (`mod.rs:1108-1113`).
  Flagging in place left the previous occupant's glyph, link and underline colour alive in a cell
  every text reader skips, so a renderer drew a character that could not be copied, searched or
  announced. The marker is alacritty's (ghostty's `.spacer_head`), not xterm's: xterm.js writes a
  bare null and re-infers the artefact at reflow time, so a lost marker there degrades to an empty
  cell trimming drops — justerm writes `' '`, so the marker is the only thing keeping the column
  out of extracted text. Blanking it is an overwrite like any other, so it owes the no-orphan repair
  when the column was a spacer.
- **A width past 2 is coerced to a pair** (#595). `unicode-width` returns 3 for at least U+17D8
  KHMER SIGN BEYYAL, which is not wrong but unrepresentable here: left uncoerced it fell through
  every `width == 2` branch while still driving the advance, so the glyph landed as one narrow cell
  followed by columns no flag distinguished from blanks. All three references bound it — ghostty
  says why (`unicode/props.zig:11-13`, *"3-em dash becomes a 2-em dash"*). The clamp is at the
  intake (`place_grapheme`), the invariant asserted where it is relied on (`write_glyph`).
- **A cluster relocated to the next row owes the destination's no-orphan repair** (#303, #529, D4):
  its spacer lands on `(nr, 1)` and would half-destroy a wide glyph standing there. xterm.js and
  ghostty repair it structurally, writing a pair as two cell writes with the repair in the write;
  (ghostty through `cursorRight(1); printCell(0, .spacer_tail)`, whose `.wide` arm clears the
  neighbouring tail at `Terminal.zig:1489-1499`; alacritty never relocates, a width-0 scalar returning
  through `push_zerowidth`, but its one-repair-per-write at `term/mod.rs:994-1008` is the mechanism);
  justerm writes both halves in one step, so each wide-writing path restates it (`write_glyph`,
  `promote_cluster_to_wide`, `relocate_cluster_wide`). ghostty's reach-back to the previous row's
  `.spacer_head` at this site is *not* copied — that is the marker the relocation just set (#534's
  rule: a repair keyed on a state predicate must not fire mid-construction). `write_glyph`'s other
  two obligations are N/A here: the left-orphan repair asks `col > 0` and the lead lands at 0, and
  `void_wrap_artefact_above(nr)` would clear the record `vacate_for_wrap` just set, in both the
  advance and the scroll case — self-clobbering, not merely redundant (measured after a repairing
  relocation: `is_row_wrapped(0)` and `(0, cols-1).is_leading_spacer()` both hold). **`2 < cols` is a
  live bound**: the print paths cannot leave a lead in the last column, but `Row::resize` can — the
  alt screen resizes without reflowing (#567), so truncating a row through a pair strands its lead,
  and at `cols == 2` the relocation would read `(nr, 2)`, an out-of-bounds panic reachable by
  shrinking a window over a CJK glyph. Pinned by
  `min_columns.rs::a_relocation_beside_a_truncated_wide_lead_does_not_index_past_the_row`.
- **The artefact marker goes with the wrap, in `end_wrap`** (ADR-0025 D3): its claim is "this blank
  was vacated *because this row continues*", so a row that stops continuing cannot hold one. Coupling
  the two there makes every wrap-ending path one rule; ghostty couples them the same way in
  `Screen.cursorResetWrap` (`terminal/Screen.zig:1524`, spacer-head clear at `:1539-1545`), which
  early-returns on an unwrapped row where justerm clears unconditionally. The clear is redundant for
  callers that erase the column anyway; it matters for the row-shift seams and `delete_chars`. The
  leftward erases (`EL 1`, `ED 1`) are the mirror — they keep the wrap but can blank the column — so
  `drop_artefact_if_erased` drops only the marker. The one wrap-ending path that does not reach
  `end_wrap` is `shift_region`'s `top == 0` seam, whose row is in scrollback; it couples the two
  clears inline.
- **`void_wrap_artefact_above` reaches into scrollback at grid row 0** on the primary screen: the
  readers walk `[scrollback ++ grid]`, so the row above grid row 0 is the last scrollback row.
  alacritty reaches the same row (`topmost_line()` is `Line(-history_size)`, `grid/mod.rs:504`);
  ghostty stops at the viewport (`cursor.y > 0`). No damage is owed: the marker is a `content` bit
  outside `CONTENT_MARKER_MASK`, so it never crosses the wire, and its `damage_span` is defensive.
  **Asking after the mutation instead is not equivalent** — it answers "is some wide lead at
  column 0", which a `DCH` pulling the next wide glyph left also satisfies, and which a two-step
  placement (VS16 promotion under mode 2027, or IRM's insert-then-write) satisfies only at the end;
  both were measured disagreeing with the rule. The erase and intra-row-shift sites are ported from
  ghostty's `Screen.splitCellBoundary`; only justerm's `ICH` site has no counterpart there.

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

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — the obligation every *reader* of a pair owes, as opposed to D3/D4's rules for the verbs that
  write one (#454)
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
