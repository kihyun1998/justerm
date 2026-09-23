# Territory — soft wrap

## What it is

A line that ran past the right margin and continues on the next row. The two rows are **one logical
line**, and that fact belongs to the *row* — it is what every text extractor joins across and what
every erase verb may or may not end.

## Governing decisions

- [**ADR-0025 — row and wide-pair cell state ownership**](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — **D1 and D2** are this territory's half (D3/D4 govern [wide glyph](wide-glyph.md))
- **spine #552** — the cluster ADR-0025 was extracted from, now closed. A GitHub issue, so not a
  graph node: read it for the fifteen-issue archaeology, not for current state

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
- **Which verbs end a wrap is a named per-verb table**, not derivable from the erased range, and
  both references spell it out call site by call site. The verbs that end it each destroy content
  *from the cursor rightward*, so "this row continues past its last column" can no longer be
  asserted; erasing leftward or inserting blanks leaves the tail intact.

  | verb | ends the wrap? | xterm | ghostty |
  |---|---|---|---|
  | `EL 0` (erase right) | **yes**, at any column | `ClearRight` → `LineClrWrapped` unconditionally (`util.c:1871`) | `cursorResetWrap()` in `eraseLine(.right)` |
  | `ECH` | **yes**, at any column | same `ClearRight` (`util.c:1961`) | `cursorResetWrap()` in `eraseChars` |
  | `DCH` | **yes** | `screen.c` | `cursorResetWrap()` — *"Our row's soft-wrap is always reset"* |
  | `EL 2` | **yes** — deliberate divergence, below | `ClearLine` has no `LineClrWrapped` (`util.c:1905`) | no, with a comment naming xterm |
  | `EL 1` (erase left) | no | `ClearLeft`, no clear | no |
  | `ICH` | no | no | no |
  | a reverse-wrap walk (`BS` / `CSI D` under `?45`) | **no** (#873) | `CursorBack` writes no wrap flag | only *reads* `prev_row.wrap` (`Terminal.zig:1842-1843`) |

  The walk row was the wrong one: it cleared the flag, copied from xterm.js's
  `line.isWrapped = false` (`InputHandler.ts:823`), which neither other reference does, and by
  writing the row directly it escaped this table and its damage obligation. Measured: two buffers
  with identical cells read as different logical lines depending on how the cursor arrived, a
  reflow kept them apart, and the clear's damage was `Partial([])` where `EL 0` through `end_wrap`
  reports `Partial([LineDamage { line: 0, left: 0, right: 2 }])`. Undoing the cursor's trip across
  the boundary does not undo the boundary.
- **`begin_wrap` damages the cell the bit rides on, the mirror of `end_wrap` (#540, #557).** A
  `Partial` frame ships the bit only on a damaged last cell; without it a frame-mode consumer keeps
  the rows *split* forever, the dual of `end_wrap`'s "joined forever". It stayed invisible because a
  wrap normally moves the cursor and `frame_damage` tops up the old cursor cell; a scroll that
  serves the wrap keeps the cursor's row index, which is how #557 surfaced it. Damaging in the helper
  rather than at each caller keeps it true for set sites added later.
- **`pending_wrap` is the entry condition.** The wrap does not happen when the last column fills; it
  happens on the *next* print — see [cursor position](cursor-position.md).
- **`shift_region`'s scrollback seam clears with no damage**, unlike `end_wrap`'s grid form: a
  scrollback row reaches the wire only while `display_offset > 0`, where `damage()` returns an empty
  `Partial` and any scroll that moves the viewport marks full damage. Valid as long as that
  frozen-viewport short-circuit holds. Leaving the artefact marker out of that branch left #534's
  defect alive one row above the grid, reachable from every `scroll_region_lines` verb — a word
  selection one cell too wide, and a reflow that bakes the stranded marker mid-row. The seam model,
  both exemptions and the #557 lesson (a stationary row below the region is necessary but not
  sufficient; *why* the shift happens is the discriminator) are
  [ADR-0025](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)'s.

## Code

- `justerm-core/src/grid.rs` — `Row::is_wrapped`, the row's `wrapped` field
- `justerm-core/src/cell.rs` — `WRAPLINE` (wire-only; the live flag is on the row)
- `justerm-core/src/term.rs` — `Term::end_wrap`, `Term::begin_wrap`, `Term::shift_region`
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

- **D2's per-verb table lives in this note, not in the ADR.** The ADR says a table exists; the table
  itself is authoritative and sits outside the record.
- **Two joiners implement the same rule** — `viewport_logical_lines` and `selection_text` — and
  nothing states they must agree or tests that they do.
