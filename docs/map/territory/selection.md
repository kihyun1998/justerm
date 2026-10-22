# Territory — selection

## What it is

The user grabs a range spanning screen and scrollback, and that range becomes ① the **highlight** the
renderer paints and ② the **text** that reaches the clipboard. Engine-owned: core holds the anchors,
keeps them on their content while the buffer moves, and resolves them into viewport spans and copy
text; the web widget turns pointer gestures into anchor calls (`SelectionController` over a
`SelectionPort`) and reports a settled selection to the consumer, who owns clipboard transport.

## Governing decisions

- [ADR-0026 — a coordinate that arrives from outside is bounded once](../../adr/0026-outside-coordinates-are-bounded-once.md)
  governs **one axis only**: an out-of-range coordinate handed to `selection_begin` /
  `selection_extend`, where the bound goes (the producer, `Term::viewport_to_abs`), and that a reader
  may not bound one end of a pair. It says nothing about the model below.

Two more sit adjacent and govern neither the model:

- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  — the highlight rides the frame: *delivery* only.
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why selection lives in core ("needs the whole buffer") and why the word-separator set is injected:
  *routing* only.

So the anchor coordinate space, `Side`, the four types, the wrap join, the line-end rule and the
artefact drop are governed by **no record**. `docs/architecture.md` §Selection is the only prose.

## Design model

Read out of the code, and recorded on issues — which is itself this territory's status.

**The model.**
- **Anchors are absolute buffer coordinates** — `BufferPoint { line, col }`, `line` indexing
  `[scrollback ++ screen]` from the oldest line — with a `Side` (Left/Right) saying which edge of the
  cell the anchor sits on, which is what lets a drag include or exclude the cell under the pointer.
  Absolute because it is invariant under a top-anchored scroll: an evicted line grows
  `scrollback.len()` by exactly the screen shift.
- **On one point the `Left` end orders first** (`Selection::ordered`, and the Block arm's column pick),
  because the boundary before a cell comes before the boundary after it, so the two edges of one cell
  bound it whichever end the drag began at (#1032). Until then a tie went to the anchor: a drag from a
  cell's right half to its left half selected nothing while the mirror selected the cell.
- **Four types — Char / Word / Line / Block — deliberately exhaustive on convergence** (#843):
  alacritty arrives at the same four independently (`Simple` / `Semantic` / `Lines` / `Block`,
  `alacritty_terminal/src/selection.rs:93`). Traffic could not decide it — the type is only ever a
  parameter to `Engine::selection_begin`.
- **Two outputs from one funnel.** `Term::resolve` turns the selection into `Resolved` bounds;
  `selection_range` projects them onto **per-viewport-row** `SelectionSpan`s (nothing for off-screen
  rows), and `selection_text` extracts copy text with the wrap join, the U+0020 trailing trim and
  scrollback traversal. Both widen onto whole wide pairs at `resolve` (`pair_start` / `pair_end`) so
  they cannot disagree (#454); a Block widens **per row**. Emptiness is decided from the endpoints
  *before* widening (`same line && from >= to`), as all three references do — until #914 a bare press
  between a lead and its spacer resolved to the glyph.
- **Split of labour.** `src/selection.rs` holds the types (88 lines); the cell-aware logic lives in
  `src/term/selection.rs` (moved out of `term.rs` in #587), over the walk floor in `term/walk.rs`. Two
  files are named `selection.rs`, so a bare `selection.rs:NN` citation is ambiguous.

**What moves the coordinate — five events, five handlers.** `src/selection.rs`'s module doc names
three kinds (cap eviction, in-screen region/RI scroll, reflow); the full set is:

| what moves it | handler |
|---|---|
| lines leaving the front — the cap, `ED 3`, `Engine::clear` (#936), and what a resize's reflow evicted (#1032) | `Term::selection_evict_oldest(n)`, reached through `Term::lines_left_the_front` and `Term::selection_reflowed`. An endpoint on a dropped line clamps to column 0 / Left of the new top line (a Block keeps its columns); a selection wholly inside them is cleared; `clear` then drops it outright. Until #935 the endpoint kept its column, so an indented top line lost cells per eviction — routine once select-all put the start on the oldest line (xterm.js `handleTrim` resets to `[0,0]`). Until #1032 a resize saturated an endpoint on an evicted line to line 0 with its old column, copying text it never covered |
| an in-screen region / RI scroll moving content | `Term::selection_rotate_region` — alacritty's `Selection::rotate`: an endpoint pushed past the region edge clamps to it, and only a true overtake clears. Unguarded on alt for the right reason: the content really moved (#660 corrected the comment that said otherwise) |
| a **top-anchored sub-region** scroll growing history while rows below the margin stay put (#449) | `Term::selection_shift_below_margin` — nothing on screen moved and nothing was evicted, yet every index below the margin rose by one |
| reflow re-splitting logical lines | `grid.rs` — `reflow` via the `points` slice, then `Term::selection_reflowed`: an endpoint that comes back one past a full row (`col == cols`, #562) clamps to `(cols - 1, Right)` — the boundary after the last cell, same rule for anchor and focus (#1032; keeping `Left` dropped or gained the row's last character) |
| any geometry change on the **alt** screen | **none — dropped** (`Term::resize`, #660), a policy not a capability limit: the alt pane tracks points through its own `reflow_pane`. A selection is two ordered endpoints, so a shrink that destroys one row and not the other has no "dispose" answer. *Any* change, not just a shrink: keeping it on a grow was refuted by a randomised sweep, because an alt resize reflows the primary pane underneath and `scrollback.len()` moves under the anchor. alacritty drops on a width change (`term/mod.rs:680-682`), xterm.js on a height change (`SelectionService.ts:156-160`). A same-size call is not a geometry change |

A screen swap also drops it (`Term::switch_to_alt`, `Term::switch_to_primary`), and RIS rebuilds `Term`.

**The line-end rule — opt-in, by moving the end** (#1031). `Term::set_selection_carries_line_end`
(off by default, survives RIS). When on, `resolve`'s Linear arm moves an end that covers a blank cell
past a finished row's text — or reaches the edge of a row the text fills — to `(next row, 0)`
(`end_past_text`); a single-row Char/Word run over padding alone resolves empty (`covers_padding_only`).
- *Maintainer's judgements* (on #1031): opt-in; "finished" is *not soft-wrapped and a row exists
  below* with no cursor-row guard, so a typed command on a prompt row carries its `\n`; Line follows
  the rule; the full-row exception stays. *Derivations*: the relocated coordinate (xterm's shape — both
  observables read it, so `extract_lines` and `selection_range` did not change), the padding-only
  guard, Block exempt.
- The padding-only guard is one row wide and is not xterm's start move: copying xterm's start move
  dropped an empty line a drag or triple-click *began* on. `from >= text_end`, not xterm's
  `col > LastTextCol + 1`: the first blank cell is already padding.
- The text's end is `Cell::is_blank` (`text_end`), never `select_all`'s `c != ' ' || is_combined`:
  a wide spacer packs `' '`, so that closure measures a row ending in a wide glyph one cell short
  (measured on #1031's mutation run).
- The wrap half of "finished" is observable only on a wrapped row ending in blanks; a test on a plain
  wrapped row passes with the check deleted. Spaces *written* over a wrapped row's tail keep the wrap.
- It is decided when read, not at the gesture: a full row parked on its deferred wrap carries until
  the next glyph wraps it, and text and highlight change together.

**Word selection.**
- `Term::set_word_separators` makes the set consumer policy (#545) and **forces `' '` into it**. A
  blank cell packs `' '`, so the space stops the walk at a row's written text *and* backstops the
  wide-pair rule — without it a double-click beside a wide separator starts on its spacer. That is
  ghostty's intake shape (`config/Config.zig`, *"Always include null as first boundary"*). The set is
  the only bound on the walk: one that omits the buffer's separators makes a double-click walk a
  whole soft-wrap run (measured 11.7 ms release, 801 920 chars). A length bound, if wanted, is a field
  beside this one — `word_start` / `word_end` are `pub(super)`.
- `DEFAULT_WORD_SEPARATORS` is alacritty's `SEMANTIC_ESCAPE_CHARS` verbatim plus U+3000
  (`alacritty_terminal/src/term/mod.rs:45` @ `852e971`): space, tab and punctuation that omits `.`,
  `/`, `-` so a path stays one word. A literal set, not `White_Space`, so the four glue spaces
  (U+00A0, U+2007, U+202F, U+205F) do not end a word — all three references agree. U+3000 is
  justerm's one divergence (the only East-Asian-Wide `White_Space` code point, measured over the
  whole range); it survives as a default a consumer can remove.
- The walkers break on the **neighbour** cell's class, never the start cell's, so word-selecting the
  space in `"ab cd"` returns both words; the references split, so it is a product choice.

**Select-all is two ordinary anchors, not a mode** (#935). `Term::select_all` needs no viewport
coordinate, so a frame-mode consumer need not scroll the view to anchor. It is a **snapshot** —
xterm.js keeps a recomputing flag, here the five handlers keep the two anchors on their content (a
derivation; a better one can overturn it). Blank edges are trimmed and an all-blank buffer selects
nothing (ghostty `Screen.selectAll`; the **maintainer's call** over xterm.js's whole-buffer extent);
"blank" is U+0020 with no combining mark. "All" is the active buffer, floored by `abs_floor` on alt.

**The web half** (`selection.ts`).
- `SelectionController` binds no listeners — since #902 `Terminal` feeds it through
  `TerminalOptions.selection` only the presses the application did not take
  ([input encoding](input-encoding.md)). It converts pixels to cells (`cellAndSide`), maps click
  count to type (`modeForClick`), auto-scrolls a drag past the edge (`dragScrollSpeed`, xterm's
  `_getMouseEventScrollAmount`, ticked by `Terminal`), and turns a quick Alt-click at the live prompt
  into a cursor move instead (`onMoveCursor`, xterm `altClickMovesCursor`).
- **Typing drops the selection, and the widget decides so** (#913): `Terminal.onUserInput` calls the
  optional `clear()` for any user input. It is **not** gated on the viewport (the snap beside it is);
  the "is there one?" guard is in the controller, which alone sees selection state; a live drag is
  skipped. Both references drop at the same moment.
- **Two selection-out signals, guarded differently** (#914). `onPrimarySelection` **commits** on
  release with the text; `onSelectionChange` **notifies** on each selection command with nothing.
  The commit is guarded on emptiness, never on motion — a motion guard made double- and triple-click
  unreportable; the emptiness test is `copySelection` skipping `null`/`""` plus core resolving a
  never-extended `begin` to a zero-width run (the `hasSelection` term beside it is a belt). **A web
  build carrying #914 must not ship against a core without #914's emptiness fix.** The change signal
  carries no text because `SelectionPort.text()` is a backend round-trip; its de-dup key is
  `type|anchor|focus`, never the pointer's cell (a cell key silenced every triple-click), and a
  `tick()` always reports. It can report a change that altered no text; the consumer is expected to
  debounce — PenTerm does, 150 ms (`penterm 7dadc3c95 src/shared/constants/timing.ts —
  COPY_ON_SELECT_DEBOUNCE_MS`).
- **`selectAll` goes through the controller**, which owns `hasSelection` and fires the change signal
  as xterm.js does; it does not feed primary (the references split). `SelectionPort.selectAll` is
  optional. A select-all during a live drag is extended by the drag's next move.
- `copySelection` normalises U+00A0 to a space (xterm parity), in the web layer because core never
  emits NBSP as padding.

**Bounds on outside input.** Viewport `(row, col)` from the pointer is clamped on both axes by
`Term::viewport_to_abs` (#660, #671, ADR-0026 D1), every stored anchor arrives through it, and the
web producer `cellAndSide` owes the bound too (#667). A selection is two anchors; text extraction is
bounded by the buffer (`scrollback_limit + rows` lines).

## Code

- `justerm-core/src/selection.rs` — `SelectionType`, `Side`, `SelectionSpan`, `BufferPoint`, `Anchor`,
  `Selection`, `Selection::ordered`
- `justerm-core/src/term/selection.rs` — `Term::selection_begin`, `Term::selection_extend`,
  `Term::selection_clear`, `Term::select_all`, `Term::selection_range`, `Term::selection_text`,
  `Term::accessible_text`; the fixups `Term::selection_shift_below_margin`,
  `Term::selection_evict_oldest`, `Term::selection_rotate_region`, `Term::selection_reflowed`; the
  private `Term::resolve` / `Resolved`, `covers_padding_only`, `end_past_text`, `finishes_row`,
  `text_end`, `pair_start`, `pair_end`
- `justerm-core/src/term/walk.rs` — `Term::abs_line`, `Term::abs_row`, `Term::abs_floor`,
  `Term::prev_pos`, `Term::next_pos`, `Term::word_start`, `Term::word_end`, `Term::is_word_boundary`,
  `Term::extract_lines`
- `justerm-core/src/term.rs` — `DEFAULT_WORD_SEPARATORS`, `Term::set_word_separators`,
  `Term::word_separators`, `Term::set_selection_carries_line_end`, `Term::full_reset` (carries both
  settings)
- `justerm-core/src/term/viewport.rs` — `Term::viewport_to_abs`
- `justerm-web/src/selection.ts` — `SelectionController` (`mouseDown`, `mouseMove`, `mouseUp`,
  `tick`, `clear`, `selectAll`, `reportChange`), `SelectionPort`, `copySelection`, `dragScrollSpeed`,
  `cellAndSide`, `modeForClick`
- `justerm-web/src/a11y-selection.ts` — `a11ySelectionToPort`
- Tests: `justerm-core/tests/selection.rs`, `select_all.rs`, `selection_column_bound.rs`,
  `selection_line_end.rs`, `wide_pair_selection.rs`, `alt_selection_resize.rs`,
  `region_edit_anchors.rs`

## Reference behaviour

- [Word selection started *on* a separator](../../agents/reference-facts.md#word-selection-started-on-a-separator--the-references-disagree-so-justerm-is-not-an-outlier)
  — the neighbour-class walk; a split reference makes it a product choice.
- [A selection past the end of a line's text, and its newline](../../agents/reference-facts.md#a-selection-past-the-end-of-a-lines-text-and-its-newline-1031-verified-2026-09-30)
  — 3–1 against carrying by default; xterm alone carries, by relocating the end.
- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
  — how an anchor crosses a re-split.
- [A selection when the screen changes under it](../../agents/reference-facts.md#a-selection-when-the-screen-changes-under-it-660-verified-2026-07-31)
  — three designs, one per reference: clear on width change and rotate otherwise (alacritty), clear
  on height change (xterm.js), a tracked pin (ghostty). The primary pane takes the first; the alt pane
  drops.
- [What the engine does with a column it was handed anyway](../../agents/reference-facts.md#what-the-engine-does-with-a-column-it-was-handed-anyway-671-verified-2026-07-31)
  — the references converge on "an out-of-range column is not observable" three ways; justerm clamps
  at the write site and needs no wrap-to-next-line arm.
- [Scroll-on-user-input](../../agents/reference-facts.md#scroll-on-user-input--both-references-snap-and-xtermjs-does-it-at-two-sites-913-verified-2026-09-16)
  — the selection drop on typing.
- [Two selection-out signals](../../agents/reference-facts.md#two-selection-out-signals-and-what-actually-guards-the-committing-one-914-verified-2026-09-16)
  — the commit/notify split.
- [A span, a highlight and a caret over a wide pair](../../agents/reference-facts.md#a-span-a-highlight-and-a-caret-over-a-wide-pair-454-verified-2026-08-10)
  — pair widening.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) —
  `CellGeometry.originX`/`originY` cannot be flagged by `geometryViolations` (a position may be `0`),
  so #819 made `getGeometry` answer `CellGeometry | undefined`; `mouseMove` and `tick` both reset
  `dragScrollAmount`, since an early return alone would latch the last auto-scroll speed. #680's
  `cellHeight > 0` guard passes for a renderer-derived cell, so the max-speed auto-scroll it fixed
  returned through the origin.
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — the **positional** answer: a selection is a region of the screen, so showing what is now under it
  after an in-place erase is the semantics (#750).
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md) — pointer-to-cell
  divides by a cell five setters can move, in a unit the renderer does not report (#578).
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::prev_pos` must not
  join down into primary history on alt (#207); `select_all` floors by `abs_floor`.
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the selection overlay group shares search's record shape and count prefix, so it inherits #621's
  `u32` widening.
- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — both halves: `word_separators` and `selection_carries_line_end` survive `ESC c` while the anchors
  die with the buffer they index.
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — two of its
  implementations: `extract_lines` for the linear arms and the block arm's own loop; #685 measured
  them disagreeing when only one was fixed.
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — both
  observables widen at `resolve` (#454), per row for a Block.
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — `cellAndSide` owes the bound on both axes; the engine's clamp is a backstop, and the alt-click
  cursor move leaves through the consumer's callback with no core guard at all (#667).

## Blast radius

- [soft wrap](soft-wrap.md) · [wide glyph](wide-glyph.md) — `selection_text` joins wrapped rows and
  skips the wide-wrap artefact, and the line-end rule reads the wrap bit; a change to the pair or wrap
  rules changes extraction output.
- [frame](frame.md) · [wire format](wire-format.md) — the highlight leaves as an overlay group whose
  count prefix it shares with search.
- [search](search.md) · [active match](active-match.md) — the active match paints over the selection
  in its own colour (#430 pins the active ∩ selected fg channel), and both live in the same absolute
  coordinate space.
- [viewport](viewport.md) — spans are projected through `scrollback.len() - display_offset`, and the
  drag auto-scroll requests scrolls that bypass the widget's offset mirror.
- [accessibility](accessibility.md) · [logical lines](logical-lines.md) — `accessible_text` lives in
  this module but its contract is a whole-buffer document; the edge was one-way until #587, which is
  how that move broke logical-lines' pin unnoticed. The AT selection bridge drives the same port.
- [reflow](reflow.md) — the anchors ride its `points` slice, and a `col == cols` answer is not a cell.
- [grid & scrollback](grid-and-scrollback.md) · [alt screen](alt-screen.md) — front drops, accrual and
  the screen swap are what move or drop the anchors.

## Known holes / open

- **Zero governing records** for the model; "why absolute coordinates" and "what moves the
  coordinate" exist only in code and here.
- **`src/selection.rs`'s module doc still names three moving events**; the table above has five.
- **`resolve`'s five `+ 1`s are unguarded, sound only while every stored anchor arrives through
  `Term::viewport_to_abs`.** The writers were enumerated when #671 landed: the fixups move lines or
  write in-range columns, `resize` re-clamps reflowed points and drops on alt, and `Term::resize` is
  the only writer of `grid.cols()`. A new writer of `self.selection` that builds an `Anchor` another
  way would put `resolve` back in reach, and nothing checks for one. The clamp is deliberately not
  `debug_assert`ed — pointer input past the grid is ordinary.
- **A resize forgets that an end was past its row's text, so under the line-end option the `\n`
  drops** (#1032, measured: `abc\r\nnext` at 10 columns, a drag to `(0, 8, Left)` copies `"abc\n"`;
  after `resize(20, _)` or `resize(5, _)` it copies `"abc"`). The join in `reflow` records the endpoint
  as `poff.min(line.len())`. A *start* past the text shows the same width dependence without the
  option. Kept out of #1032 by the maintainer's scope call: the collapse is in the join, not the clamp.
- **A Word endpoint past a row's text gains the row's last word across a narrowing resize** (#1032's
  lens): Word ignores `Side`, so the clamp moves the end onto `c`.
- **`reflow`'s trailing-blank branch clamps a column to `new_cols - 1` and keeps the side**, so
  #1032's rule does not reach it (an end at `(2, 8, Left)` highlights 0–3 of 5; a start at
  `(2, 8, Right)` copies `""` instead of `"\n"`). Reached only by an endpoint on a trailing blank line.
- **`SelectionController.hasSelection` lies three ways.** It is set at `begin`, so a bare click that
  selected nothing leaves it `true` (#914, every click); core drops the selection on a screen swap and
  the controller, which sees no frames, never hears (#902 sidesteps it only for the forced press); and
  `selectAll` sets it while core selects nothing on an all-blank buffer (#935). #913's `clear()` and
  #914's alt-click fix narrowed it; the screen-swap trigger needs a signal the controller lacks.
- **Neither selection-out signal hears about a selection the widget did not make**: core dropping it
  (screen swap, alt resize — xterm.js reports this from its buffer-activate listener), and an
  assistive-technology selection through `a11ySelectionToPort`, which drives the port directly
  (xterm.js routes it through `terminal.select`). Fixing the second changes a second published
  function (#914's completeness lens).
- **A written trailing ASCII space is dropped from copy text**, because a `Cell` cannot tell it from a
  blank; the references split 2–1, so it is a separate decision from #685's trim.
