# Territory — soft wrap

## What it is

A line that ran past the right margin and continues on the next row. The two rows are **one logical
line**, and that fact belongs to the *row* — it is what every text extractor joins across and what
every erase verb may or may not end.

## Governing decisions

- [**ADR-0025 — row and wide-pair cell state ownership**](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — **D1 and D2** are this territory's half (D3/D4 govern [wide glyph](wide-glyph.md))
- Live roster: **spine #552** (a GitHub issue — not a graph node)

## Design model

ADR-0025 is authoritative; this is routing. **If they disagree, the ADR is right.**

- **D1 — the wrap link is owned by the `Row`, not by a cell.** It used to ride
  `CellFlags::WRAPLINE` in the last cell, where every whole-cell write and clear destroyed it —
  ordinary typing in the last column silently split a logical line (#538). On the row, no cell
  operation can reach it.
- **It still crosses the wire as the last cell's `WRAPLINE` bit, derived at encode time**, so the
  frame stays a flat cell grid. That bit is **never the authoritative copy** and no live reader
  consults it.
- **Ask `Row::is_wrapped`, never the cell.** The polarity matters when borrowing from references:
  ghostty's `Row.wrap` is this exact flag ("wraps *into* the next"), while xterm.js's
  `BufferLine.isWrapped` is the opposite-polarity link ("continues the previous").
- **D2 — one property, one lifecycle, spelled out per verb.** One SET site-class, one CLEAR/REPAIR
  discipline, read sites gating uniformly. The alternative — a rule re-applied by hand at each new
  site — is **rejected on measured evidence**: it failed three times in this area (#521, #528, #538).
- **Which verbs end a wrap is a named per-verb table**, and it lives in `Term::end_wrap`'s doc
  comment. It is not derivable from the erased range, which is precisely why it has to be written
  down.
- **`pending_wrap` is the entry condition.** The wrap does not happen when the last column fills; it
  happens on the *next* print — see [cursor position](cursor-position.md).

## Code

- `justerm-core/src/grid.rs` — `Row::is_wrapped`, the row's `wrapped` field
- `justerm-core/src/cell.rs` — `WRAPLINE` (wire-only; the live flag is on the row)
- `justerm-core/src/term.rs` — `Term::end_wrap` (**the per-verb table is this function's doc
  comment**), `Term::begin_wrap`, `Term::shift_region`
- `justerm-core/src/term/walk.rs` — `prev_pos` / `next_pos`, the stepping that joins wrapped rows

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Soft wrap is a row property](../../agents/reference-facts.md#soft-wrap-is-a-row-property) — both
  references keep the flag off the cell, with opposite polarity; and the direct evidence for D2's
  table, that **which verbs end a wrap is a per-verb rule, not derivable from the erased range**
- [Row-shift verbs and the wrap link](../../agents/reference-facts.md#row-shift-verbs-and-the-wrap-link-540-verified-2026-07-25)

**Known divergence, deliberate:** `EL 2` does **not** end the wrap in either C xterm or ghostty, and
justerm ends it (#538) — because justerm *joins* logical lines for copy and search, so a
blanked-but-still-wrapped row would visibly merge two lines. A cost the references do not carry.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::end_wrap`'s
  previous-row join is one of the two sites that satisfy the floor by *argument* rather than by
  calling `abs_floor`, and therefore appear in no grep for it

## Blast radius

- [logical lines](logical-lines.md) · [selection](selection.md) · [search & active match](search.md)
  — all three join across wrapped rows; the join rule is this territory's and its consequences are
  theirs
- [wide glyph](wide-glyph.md) — a lead that does not fit at the margin causes the wrap and leaves the
  artefact these rules clear
- [cursor position](cursor-position.md) — `pending_wrap` is the entry condition
- [wire format](wire-format.md) — `WRAPLINE` is derived into a cell at encode time; changing the
  derivation is a version event

## Known holes / open

- **D2's per-verb table lives only in a code comment.** The ADR says a table exists; the table itself
  is authoritative and sits outside the record.
- **Two joiners implement the same rule** — `viewport_logical_lines` and `selection_text` — and
  nothing states they must agree or tests that they do.
