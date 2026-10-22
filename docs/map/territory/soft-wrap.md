# Territory — soft wrap

## What it is

A line that ran past the right margin and continues on the next row. The two rows are **one logical
line**, and that fact belongs to the *row* (`Row::is_wrapped`): it is what every text extractor joins
across, what reflow re-splits, and what each erase and shift verb may or may not end. This territory
owns the flag's lifecycle — where it is set, which verbs end it, how the seams of a region shift
repair it — and its wire projection.

## Governing decisions

- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — **D1 and D2** are this territory's half (D3/D4 govern [wide glyph](wide-glyph.md)), and the
  seam model of `shift_region` (two seams, both exemptions, the #557 discriminator) is the ADR's.
  **If this note and the ADR disagree, the ADR is right.** The per-verb table below is the ADR's
  "named per-verb table"; the ADR says one exists and names the `EL 2` divergence, this note holds the
  rows.
- Adjacent: spine #552 (closed) — the fifteen-issue cluster ADR-0025 was extracted from; read it for
  the archaeology, not current state.

## Design model

ADR-0025 is authoritative; this is routing plus what the source shows.

- **D1 — the wrap link is owned by the `Row`, not a cell.** It used to ride `CellFlags::WRAPLINE` in the
  last cell, where every whole-cell write and clear destroyed it — ordinary typing in the last column
  silently split a logical line (#538). On the row, no cell operation can reach it.
- **It still crosses the wire as the last cell's `WRAPLINE` bit, derived at encode time**, so the frame
  stays a flat cell grid. That bit is never the authoritative copy and no live reader consults it.
- **Ask `Row::is_wrapped`, never the cell.** The polarity matters when borrowing from references:
  ghostty's `Row.wrap` is this exact flag ("wraps *into* the next"); xterm.js's
  `BufferLine.isWrapped` is the opposite-polarity link ("continues the previous").
- **D2 — one property, one lifecycle, spelled out per verb.** One SET site-class (`Term::begin_wrap`,
  reached from the print path's deferred wrap and from `Term::vacate_for_wrap`), one END discipline
  (`Term::end_wrap`), read sites gating uniformly. The alternative — a rule re-applied by hand at each
  site — is rejected on measured evidence: it failed three times here (#521, #528, #538).
- **Which verbs end a wrap is a named per-verb table**, not derivable from the erased range, and both
  references spell it out call site by call site. The verbs that end it destroy content *from the
  cursor rightward*, so "this row continues past its last column" can no longer be asserted; erasing
  leftward or inserting blanks leaves the tail intact. Verified against `term.rs` at HEAD 7bb31f7
  (`erase_line`, `erase_chars`, `delete_chars`, `insert_chars`, `erase_display`, `step_back`):

  | verb | ends the wrap? | xterm | ghostty |
  |---|---|---|---|
  | `EL 0` (erase right) | **yes**, at any column | `ClearRight` → `LineClrWrapped` unconditionally (`util.c:1871`) | `cursorResetWrap()` in `eraseLine(.right)` |
  | `ECH` | **yes**, at any column | same `ClearRight` (`util.c:1961`) | `cursorResetWrap()` in `eraseChars` |
  | `DCH` | **yes**, before the shift (#534) | `screen.c` | `cursorResetWrap()` — *"Our row's soft-wrap is always reset"* |
  | `EL 2` | **yes** — deliberate divergence, below | `ClearLine` has no `LineClrWrapped` (`util.c:1905`) | no, with a comment naming xterm |
  | `EL 1` (erase left) | no (drops only the artefact marker) | `ClearLeft`, no clear | no |
  | `ED 0` / `ED 2` | yes, every erased row | — | — |
  | `ED 1` | rows above: yes; the cursor row only when the erase covers it whole (xterm.js's `ED 1` arm) | — | — |
  | `ICH` | no | no | no |
  | a reverse-wrap walk (`BS` / `CSI D` under `?45`) | **no** (#873) | `CursorBack` writes no wrap flag | only *reads* `prev_row.wrap` |

  The walk row was the wrong one until #873: it cleared the flag, copied from xterm.js's
  `line.isWrapped = false` (`InputHandler.ts:823`), which neither other reference does, and by writing
  the row directly it escaped this table and its damage obligation. Measured: two buffers with
  identical cells read as different logical lines depending on how the cursor arrived, a reflow kept
  them apart, and the clear's damage was `Partial([])` where `EL 0` through `end_wrap` reports one
  damaged line. Undoing the cursor's trip across the boundary does not undo the boundary.
- **`EL 2` ends the wrap — known divergence, deliberate (#538).** Neither C xterm nor ghostty ends it.
  justerm *joins* logical lines for copy and search, so a blanked-but-still-wrapped row would visibly
  merge two lines — a cost the references do not carry.
- **`begin_wrap` damages the cell the bit rides on, the mirror of `end_wrap` (#540, #557).** A
  `Partial` frame ships the bit only on a damaged last cell; without it a frame-mode consumer keeps the
  rows *split* forever, the dual of `end_wrap`'s "joined forever". It stayed invisible because a wrap
  normally moves the cursor and `frame_damage` tops up the old cursor cell; a scroll that serves the
  wrap keeps the cursor's row index, which is how #557 surfaced it. Damaging in the helper keeps it
  true for set sites added later.
- **A wrap is claimed only if a next row exists** (`Term::wrapline_advances`, #540) — see
  [cursor position](cursor-position.md); `pending_wrap` is the entry condition, and the wrap happens on
  the *next* print, not when the last column fills.
- **Every row-shifting verb goes through `Term::shift_region`** (IL/DL/SU/SD and the region paths of
  LF/RI; the full-screen primary linefeed is the one row-shift not routed there and owes no seam
  clear), which ends the wraps the shift falsified at the two seams: `top - 1`, and the row that lost
  its continuation (`bottom` going down, `bottom - 1` going up, never at the screen's bottom edge and
  never when the shift serves a wrap). `evicts_to_scrollback` exempts the top seam and `serves_wrap`
  the bottom one; both are caller facts. Derived, not ported — no reference implements the rule — and
  sound only without DECSLRM. The scroll op is recorded before any seam clear, because
  `record_scroll` rotates line damage with the content.
- **At `top == 0` the seam is the last scrollback row** on the primary (the readers walk
  `[scrollback ++ grid]` as one buffer), so `shift_region` clears that row's wrap and artefact marker
  there — **with no damage**, unlike `end_wrap`'s grid form: a scrollback row reaches the wire only
  while `display_offset > 0`, where `damage()` returns an empty `Partial` and any scroll that moves the
  viewport marks full damage. Valid as long as that frozen-viewport short-circuit holds. A linefeed is
  exempt because it pushes grid row 0 *into* scrollback, keeping the adjacency. Leaving the artefact
  marker out of that branch left #534's defect alive one row above the grid, reachable from every
  `scroll_region_lines` verb — a word selection one cell too wide, and a reflow that bakes the
  stranded marker mid-row. On the alt screen the branch is skipped (`!self.on_alt`): the floor is the
  screen top and nothing below it may be joined or cleared.

## Code

- `justerm-core/src/grid.rs` — `Row::is_wrapped`, `Row::set_wrapped`, the row's `wrapped` field,
  `Grid::is_row_wrapped`
- `justerm-core/src/cell.rs` — `CellFlags` (`WRAPLINE`, wire-only; the live flag is on the row)
- `justerm-core/src/term.rs` — `Term::begin_wrap`, `Term::end_wrap`, `Term::shift_region`,
  `Term::wrapline`, `Term::wrapline_advances`, `Term::erase_line`, `Term::erase_display`,
  `Term::erase_chars`, `Term::delete_chars`, `Term::step_back`, `Term::scroll_region_lines`
- `justerm-core/src/term/walk.rs` — `Term::prev_pos` / `Term::next_pos` (word-selection stepping that
  joins wrapped rows), `Term::walk_logical_line`, `Term::extract_lines` — the joiners that read the
  flag

## Reference behaviour

- [Soft wrap is a row property](../../agents/reference-facts.md#soft-wrap-is-a-row-property) — both
  references keep the flag off the cell, with opposite polarity; and the direct evidence that which
  verbs end a wrap is a per-verb rule, not derivable from the erased range
- [Row-shift verbs and the wrap link](../../agents/reference-facts.md#row-shift-verbs-and-the-wrap-link-540-verified-2026-07-25)
- [Damage / dirty tracking](../../agents/reference-facts.md#damage--dirty-tracking-536-verified-2026-07-28)
  — why a flag that rides no cell write still owes damage

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the write path reaches
  above grid row 0 into scrollback at two sites, `Term::shift_region`'s `top == 0` seam and
  `Term::void_wrap_artefact_above`, and holds the floor there with an explicit `!self.on_alt` guard
  rather than `abs_floor`, so neither appears in a grep for it

## Blast radius

- [logical lines](logical-lines.md) · [selection](selection.md) · [search](search.md) ·
  [accessibility](accessibility.md) — all join across wrapped rows; they assume the flag is true
  exactly when the next row continues this one
- [reflow](reflow.md) — reflow re-splits by the flag; a stale flag merges two logical lines
  permanently at the next resize
- [settled-cell paint](settled-cell-paint.md) — reports changed logical lines; a flag change that is
  not reported as a content change leaves a painted line describing the wrong text
- [wide glyph](wide-glyph.md) — a lead that does not fit at the margin causes the wrap and leaves the
  artefact; `end_wrap` clears the marker with the flag
- [cursor position](cursor-position.md) — `pending_wrap` is the entry condition, and the reverse-wrap
  walk reads the flag
- [damage](damage.md) — the flag rides no cell write, so both `begin_wrap` and `end_wrap` must damage
  the last cell themselves
- [wire format](wire-format.md) — `WRAPLINE` is derived into the last cell at encode time; changing
  the derivation is a version event

## Known holes / open

- **The joiners implement the same rule separately** — `walk_logical_line` (logical lines, paint),
  `extract_lines` (selection, accessible text, command text), `prev_pos`/`next_pos` (word selection)
  and search's haystack each read the flag themselves. No artifact states they must agree on a join,
  and no test compares them on one input (`tests/written_whitespace.rs` pins only the padding trim
  across them).
- **The `top == 0` seam's no-damage rule rests on the frozen-viewport short-circuit**; a change to
  how a scrolled-back viewport is damaged makes the scrollback-row clear invisible to a frame-mode
  consumer.
