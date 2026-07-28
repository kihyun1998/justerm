# Territory — wide glyph & soft wrap

## What it is

A width-2 glyph (CJK, emoji) occupying two cells, and a line running past the right margin into the
next row (soft wrap). They are one territory because **they share the same storage problem**: the
scope of each truth is larger than a cell — soft wrap is a fact about the *row*, the spacer is a fact
about the *pair* — while storage is a per-cell word.

The opposite extreme from [selection](selection.md): here the **record is thick and the code is
scattered**.

## Governing decisions

- [**ADR-0025 — row and wide-pair cell state ownership**](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — D1–D4 plus two amendments. Authoritative for this territory
- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) —
  the renderer-side width geometry (a cell is the ink box of the font's `█`)
- Live roster: **spine #552** (not a graph node — a GitHub issue)

## Design model

ADR-0025's D1–D4 are authoritative. The summary here is for routing; **if they disagree, the ADR is
right.**

- **D1 — storage granularity may be the cell; semantic ownership is not.** A row property is owned by
  the `Row`; a pair property by the pair's designated cell. Where the wire format forces the bit into a
  cell (`WRAPLINE` rides the row's last cell so the frame stays a flat cell grid), that bit is
  **derived at encode time and is never the authoritative copy** — no live reader consults it (#538).
- **D2 — one property, one lifecycle, spelled out per verb.** One SET site-class, one CLEAR/REPAIR
  discipline, read sites gating **uniformly**. The alternative — a rule re-applied by hand at each new
  site — is **rejected on evidence**: it failed three times in this exact area (#521 extended attrs,
  #528 the wrap artefact, #538 the wrap flag). Which verbs owe a clear or repair is a **named per-verb
  table**, and that table lives in `Term::end_wrap`'s doc comment.
- **D3 — a pair property is meaningful only at its defining position.** The leading-spacer marker means
  "wide-wrap artefact" **only** at the last column of a soft-wrapped row. A row-shift verb (ICH/DCH)
  that carries it inward has produced a marker describing nothing, and must drop it (#528).
  **Position is part of the test, never the marker alone.**
- **D4 — both halves of a pair move together, set and clear** — but **only within the verbs that
  edit**. A structural reallocation (`Row::resize`) may cut through a pair, and **a lead with no
  spacer is a legal buffer state** (#529, 2026-07-28). D4's precondition is `MIN_COLUMNS = 2` (#547):
  at one column both halves physically cannot fit, so while the engine accepted that width the rule
  had an unstated precondition.

## Code

- `justerm-core/src/cell.rs` — the `WIDE_CHAR` / `C_SPACER` / `WRAPLINE` flags, `COMBINED_PRESENT`
- `justerm-core/src/grid.rs` — `Row::is_wrapped` ("ask this, not the last cell's `WRAPLINE`"),
  `reflow` (takes tracked `points`), `Row::resize` (the boundary of D4's scope)
- `justerm-core/src/term.rs` — `Term::end_wrap` (**the per-verb table lives in this function's doc
  comment**), `Term::shift_region`, `Term::drop_artefact_if_erased`, `Term::free_cell`
- `justerm-core/src/term.rs` — `MIN_COLUMNS` (defined there, re-exported from `lib.rs`)

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::end_wrap`'s
  previous-row join must not cross the boundary on alt

## Blast radius

- [selection](selection.md) — `selection_text` performs the wrap join and the artefact drop
- [logical lines](logical-lines.md) · [search & active match](search.md) — share the same wrap-joined
  walk and the same spacer-skipping rule
- [cursor](cursor.md) — `pending_wrap` is the entry condition for the wrap path
- **frame / wire** *(no note yet)* — `WRAPLINE` is derived into a cell at encode time (D1). Changing
  the derivation rule is a wire-version event
- **renderer** *(no note yet)* — how a spacer cell is drawn. VS16/ZWJ emoji arrive as `wide = false`
  because core uses per-char `UnicodeWidthChar`; DECSET 2027 (#295) is the opt-in clustering

## Known holes / open

- **#562 — reflow cannot express a point one past the last cell.** Five designs were built, measured
  and rejected. Read that issue before touching this area — **the failures are the content.**
- **D2's per-verb table lives only in a code comment** (`Term::end_wrap`). The ADR says only that a
  table exists. The table is authoritative but sits outside the record.
- **VS16 / keycap clusters** — width is computed per character, so string-level promotion is
  impossible. #303 / #304 are the tail.
