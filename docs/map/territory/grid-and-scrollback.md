# Territory — grid & scrollback

## What it is

Where cells live, and what a cell is. The in-memory `Cell` (three packed `u32` words), the `Row` that
carries a line of them plus its column-keyed side maps and its soft-wrap bit, the `Grid` that holds
the visible screen as `Vec<Row>`, and the `VecDeque<Row>` of history beside it in `Term` — with the
rules for when a line leaves the screen and enters history, and when it leaves history altogether.
The storage the whole engine stands on, and the one whose performance was measured and misdiagnosed
before it was understood. The second screen (`alt_grid`) is stored here and governed by
[alt screen](alt-screen.md).

## Governing decisions

- [ADR-0009 — scroll eviction via a move+recycle value-handshake](../../adr/0009-o1-scroll-in-grid-row-ring.md)
  — the screen/history split as the right grain, and its amendment: the in-`Grid` ring it first
  adopted was built, measured as a regression and dropped; the row recycling stayed.
- [ADR-0025 — row-scoped and wide-pair-scoped state has one owner](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — why soft-wrap is a field of `Row` rather than a cell flag (D1); the wide-pair half belongs to
  [wide glyph](wide-glyph.md).
- [ADR-0005 — binary reference-based serialization](../../adr/0005-binary-reference-based-serialization.md)
  governs the *wire* cell record, not this one; the two share a field model (`docs/architecture.md`
  §Cell) and differ in where the overflow goes.

## Design model

From the source, `docs/architecture.md` §Cell / §Viewport / §Hidden VT state, and the ADRs above.

**The cell.**
- `Cell { content, fg, bg }` is three `u32` words, 12 bytes (`cell.rs` test `cell_is_12_bytes`). The
  layout mirrors xterm.js's `BufferLine` cell — the fg/bg words byte-identical to its `Attributes` +
  `FgFlags`/`BgFlags` (a comparison recorded in the `cell.rs` layout comment against
  `xtermjs/xterm.js@master` `Constants.ts`, unpinned). The content word keeps explicit layout markers
  (`WIDE`, `SPACER`, `WRAP`, the engine-internal `LEADING_SPACER` of #113) where xterm.js stores a
  2-bit width, and the underline style as a 3-bit field at bits 26..=28 (#829).
- The packing is **canonical**, so the derived `Eq` is a bitwise compare: `flag_words` normalises a
  bare `UNDERLINE` to `UnderlineStyle::Single`, and the fg word's underline bit is reserved and never
  written — the style is the single owner (#829).
- Colours are stored as references only (`Default` / `Indexed` / `Rgb`, `pack_color`); nothing here
  resolves one.
- `Cell::default` is `' '` (U+0020) with every other bit zero, built directly so row blanking is a
  copy. That is what makes U+0020 the padding code point
  ([only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md)).
- What does not fit lives on the `Row`, not the cell: a combining cluster, an OSC 8 URI and a non-default
  SGR 58 underline colour are column-keyed `BTreeMap`s, each read only when the cell's presence bit is
  set ([row-keyed side maps](../invariant/row-keyed-side-maps.md)). `ExtAttrs` carries the link +
  underline-colour family in one step for every path that moves a cell (#521); combining marks are
  content and are re-attached mark by mark instead. The link value is a shared `LinkUri` (`Arc<Box<str>>`)
  that dies with the last row holding it (#628); `Row::purge_side_maps` releases entries an in-place
  erase left behind — an optimisation under the gate, kept because since #628 a stale entry holds a
  heap allocation.
- Soft-wrap is `Row::wrapped`, set only by `Row::set_wrapped` and cleared through `Term::end_wrap`
  ([soft wrap](soft-wrap.md)); it reaches the wire as the last cell's `WRAPLINE` bit, derived at
  encode time (#538). `Row::blank_in_place` resets the cells *and* the row-scoped bit in one call, so
  no caller blanks half a row.

**Storage.**
- **History is not in `Grid`.** `Grid { cols, rows, lines: Vec<Row> }` holds the screen; `Term`
  holds `scrollback: VecDeque<Row>` and `scrollback_limit` beside it. The concatenated
  `[scrollback ++ grid]` coordinate every reader walks is an indexing convention, not a storage
  layout — which is why the alt-screen floor has to be reproduced at every walk site
  ([alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md)). ADR-0009 records why
  the split is the right grain: a random-write 2D screen and an append-only FIFO.
- **The row buffer is recycled; there is no ring.** `Grid::scroll_up_recycle` rotates the 24-byte row
  handles, swaps a caller-supplied blank into the bottom slot and returns the evicted top row by
  move; `Term` parks the row the cap pops in `recycled_row` and feeds it back as the next blank.
  The original flood profile (#41) blamed `rotate_left`; measured, it moves handles over a bounded
  screen height and was never the cost. The cost was `linefeed` copying ~2 KB and allocating every
  line, plus an alloc/free pair per line once history is at its cap — which a flood is throughout
  (ADR-0009 amendment). The in-`Grid` ring was reverted in `1fa3b14`.
- **Capacities are kept exact, because recycling carries them forever (#1038, 2026-10).** A recycled
  row and a reused buffer keep whatever capacity they had. Three were measured on #1038: reflow's
  history-sized buffer handed to the screen (a 24-row grid kept 16 384 slots, 1.7 MB) —
  `Grid::set_screen` now shrinks to `rows`; a reflowed or shrunk row kept its old cells (+2.1 MB on
  60 %-width lines; 768 KiB after a 65 535-column resize) — `Row::resize` now leaves the cell vector at
  exactly `cols`; and history rebuilt at exact length by reflow doubled to 20 000 on its next push —
  `Term::push_history` grows no further than `scrollback_limit + 1`. Pinned by
  `size_bounds.rs` — `a_full_scrollback_scrolls_without_allocating`.

**Accrual — when a line enters history.**
- Only when the scroll is at `scroll_bottom` with `scroll_top == 0` **and** the alt screen is
  inactive (`Term::linefeed_inner`). A top-anchored sub-region `[0..k]` still accrues (copying row 0,
  `Grid::row_owned`, and shifting the anchors of the fixed rows below it by +1); a region with
  `scroll_top > 0`, the alt screen and reverse index never do. Accrual (`scroll_top == 0`) and the
  recycle handshake (`scroll_bottom == rows-1`) are distinct predicates.
- **The explicit line-editing verbs never accrue.** `Term::scroll_region_lines` (SU/SD/IL/DL) drops a
  full-screen SU's top line rather than pushing it to history. `docs/architecture.md` §Hidden VT state
  records this as matching xterm.js (which carries a `FIXME` to accrue) and trailing xterm and
  alacritty, which rotate it into history — a deliberate divergence toward the weaker reference.
- Follow-bottom = stay: with `display_offset > 0` an accrual bumps the offset so the same lines stay in
  view ([viewport](viewport.md)).

**Leaving history from the front — one funnel (#936).** The cap evicts one line per linefeed;
`ED 3` (`Term::erase_history`) drops all of it in band; `Term::clear` drops it out of band together
with the rows above the cursor's logical line, which it first moves into history so one drop takes
both (their absolute lines do not change in the move, so no holder moves until the drop). All three
call `Term::lines_left_the_front(n)`, which advances `evicted_total` and repairs **five** holders:
selection, search highlights, markers, tracked points and the change watches. Until the adversarial
pass on #936 the cap kept its own copy of that call list, and `ED 3`'s first version named three
holders and missed search highlights. Each caller sets `display_offset` itself (the cap decrements
it, the other two zero it), and that is not optional: every viewport read computes
`scrollback.len() - display_offset`, and an offset that outlives its history underflows.

**Bounds on outside input.** The byte stream decides how many rows exist, and that count is bounded
by `scrollback_limit + rows`; row width by `MAX_COLUMNS`; a cluster's tail by `MAX_CLUSTER_TAIL`.
`scrollback_limit` itself is the consumer's constructor argument (`Engine::with_scrollback`, default
`DEFAULT_SCROLLBACK` = 10 000) with no ceiling, so memory is `limit × cols × 12 B` plus side maps by
the consumer's choice.

## Code

- `justerm-core/src/cell.rs` — `Cell`, `Cell::from_parts`, `Cell::reset`, `Cell::is_blank`,
  `CellFlags`, `UnderlineStyle`, `flag_words`, `pack_color`, `unpack_color`
- `justerm-core/src/grid.rs` — `Grid`, `Grid::new`, `Grid::set_screen`, `Grid::take_lines`,
  `Grid::scroll_up_recycle`, `Grid::scroll_up_region`, `Grid::scroll_down_region`, `Grid::row_owned`,
  `Grid::is_row_wrapped`; `Row`, `Row::resize`, `Row::clear`, `Row::blank_in_place`,
  `Row::purge_side_maps`, `Row::set_wrapped`, `Row::ext_attrs_at`, `Row::set_ext_attrs`,
  `Row::move_maps`; `ExtAttrs`, `LinkUri`
- `justerm-core/src/term.rs` — `Term::with_scrollback`, `Term::linefeed_inner` (accrual and the cap),
  `Term::push_history`, `Term::lines_left_the_front`, `Term::erase_history`, `Term::clear`,
  `Term::scroll_region_lines`, `Term::scrollback_len`, `DEFAULT_SCROLLBACK`
- `justerm-core/src/lib.rs` — `Engine::with_scrollback`, `Engine::scrollback_len`, `Engine::clear`
- `justerm-core/src/term/walk.rs` — the readers that treat the two stores as one buffer
- Tests: `justerm-core/tests/size_bounds.rs`, `justerm-core/tests/ed3.rs`, `justerm-core/tests/clear.rs`

## Reference behaviour

- [Dropping history — `ED 3` in band, and a Clear command out of band](../../agents/reference-facts.md#dropping-history--ed-3-in-band-and-a-clear-command-out-of-band-936-verified-2026-09-18)
  — `ED 3` follows xterm including on the alt screen; `Engine::clear` does nothing there (ghostty, the
  maintainer's call on #936).
- [OSC 8 hyperlinks — where the URI lives, and what frees it](../../agents/reference-facts.md#osc-8-hyperlinks--where-the-uri-lives-and-what-frees-it-628635-verified-2026-07-30)
  — the link side map's value and lifetime.
- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — `Cell::default` and blanking.
- Unpinned: the accrual condition is "verified against alacritty `region.start == 0`" and the SU
  divergence names xterm.js's `FIXME` (`docs/architecture.md` §Hidden VT state); the cell layout's
  xterm.js mirror is a source comment. ADR-0009's Context pins the row-ring comparison (alacritty
  `storage.rs`, xterm.js `CircularList.ts`).

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — this territory is its
  cause: one concatenated coordinate space over two logical buffers.
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — the *source* of the
  constraint: `Cell::default` packs `' '`, so a written space is indistinguishable from a blank.
- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — `scrollback_limit` survives `ESC c` by riding `Term::with_scrollback`'s argument in `full_reset`,
  not the copy-back list, so auditing that list undercounts.
- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the storage site: `Row`'s three maps and
  the presence bits in `Cell`.

## Blast radius

- [viewport](viewport.md) — the addressable height is `scrollback_len + rows`, and every viewport read
  assumes `display_offset <= scrollback.len()`, which eviction can break.
- [selection](selection.md) · [marker](marker.md) · [tracked points](tracked-points.md) ·
  [search](search.md) — they hold absolute line numbers, which a front drop and a top-anchored
  sub-region accrual both renumber; a new front-drop path that bypasses `lines_left_the_front` leaves
  them stale.
- [reflow](reflow.md) — it rebuilds screen and history as one stream, so it assumes the two stores
  are one logical sequence and that capacity is re-trimmed afterwards.
- [soft wrap](soft-wrap.md) — the wrap bit lives on the `Row`, and a wrap link can run from the last
  history row into the grid, so the artefact clear couples across the store seam.
- [damage](damage.md) — the full-screen accrual records its own scroll op outside `shift_region`, and
  a cap eviction while scrolled up moves the view and forces a full repaint.
- [vt-interpretation](vt-interpretation.md) — every vertical-motion verb reads the accrual condition.
- [wire format](wire-format.md) — it re-derives the cell's presence bits and `WRAPLINE` from groups and
  rows; a new cell bit or side map needs an encoding there.
- [alt screen](alt-screen.md) — the alt grid is a second `Grid` that never accrues, and `ED 3` on it
  drops the primary's history underneath.

## Known holes / open

- **The SU/SD/IL/DL non-accrual is a known divergence from the stronger references**, matching the
  one that itself carries a `FIXME`. It is recorded in `docs/architecture.md` and in no decision.
- **No pinned reference rows** for the accrual condition or the SU divergence, in the territory where
  "what counts as history" is a user-visible contract.
- **`scrollback_limit` is not readable back.** A consumer sees `scrollback_len` and cannot tell from the
  engine whether history is being evicted or has not grown; it knows only the limit it passed.
- **`Row` is re-exported publicly with no public method** (`grid.rs` doc-comment, #844): the open
  question recorded there is why it is exported at all.
- **No ceiling on `scrollback_limit`.** Memory scales with a consumer-chosen number; nothing in core
  refuses an absurd one.
