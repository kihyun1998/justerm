# Territory — selection

## What it is

The user grabs a range spanning screen and scrollback, and that range becomes ① the **highlight** the
renderer paints and ② the **text** that reaches the clipboard. Engine-owned — the consumer only does
pixel→cell conversion and clipboard transport.

## Governing decisions

- [ADR-0026 — a coordinate that arrives from outside is bounded once](../../adr/0026-outside-coordinates-are-bounded-once.md)
  governs **one axis only**: what happens to an out-of-range coordinate handed in through
  `selection_begin` / `selection_extend`, where the bound goes (the producer, here), and that a reader
  may not bound one end of a pair. It says nothing about the selection *model* below — the blank that
  follows is still the state for everything else

The rest of this section is the actual state. Two more decisions sit adjacent, and neither governs the
selection *model* either:

- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  decides that the selection **highlight rides the frame** → *delivery* only, not the model
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  supplies the reason selection lives in core ("needs the whole buffer") → *routing* only

So the anchor coordinate space, `Side`, the four types, the wrap join and the artefact-drop rule are
governed by **no record at all**. The four lines under `docs/architecture.md` §Selection are the only
prose, and prose is not a decision record.

## Design model

With no record, everything below was **read out of the code** — which is itself this territory's
status.

- **Anchors are absolute buffer coordinates** — `BufferPoint { line, col }`, where `line` indexes
  `[scrollback ++ screen]` from the oldest line. Not viewport coordinates.
- **Why absolute**: it is invariant under a top-anchored scroll. A line evicted into scrollback grows
  `scrollback.len()` by exactly the screen shift, so existing content keeps its absolute index.
- **Four things move the coordinate, and they do not line up one-to-one with the fixups.**
  `selection.rs`'s module doc names three *kinds* — cap eviction, in-screen region/RI scroll, reflow —
  and that count is the one worth quoting, but the handlers are:

  | what moves it | handler |
  |---|---|
  | scrollback cap eviction | `Term::selection_evict_oldest` |
  | an in-screen region / RI scroll moving content | `Term::selection_rotate_region` |
  | a **top-anchored sub-region** scroll growing scrollback while rows below the margin stay put, so their absolute index rises (#449) | `Term::selection_shift_below_margin` |
  | reflow re-splitting logical lines | `grid.rs`'s `reflow`, via tracked points |
  | a **shrinking** resize on the **alt** screen | **none — the selection is dropped** (`Term::resize`, #660). Not for want of machinery: the alt branch tracks points through its own `reflow_pane` call and already rotates *markers* with the returned `evicted`. A selection is two ordered endpoints, so a shrink that destroys the row under one and not the other has no "dispose" answer, and reusing the marker policy would move its ends by different rules. A grow moves nothing and keeps it |

  The third is the one a three-item list hides: nothing on screen moved and no line was evicted, yet
  every absolute index below the margin changed — because `scrollback.len()` grew and the coordinate
  is measured from the oldest line. It is the counter-case to the invariant directly above.
- **`Side` (Left/Right)** — which edge of a cell the anchor sits on. Lets a drag include or exclude the
  cell under the pointer; this is what makes mouse precision possible.
- **Four types** — Char (runs across lines) / Word / Line / Block (rectangular).
- **Two outputs** — `selection_range()` yields **per-viewport-row** `SelectionSpan { row, left, right }`
  and emits nothing for off-screen rows. `selection_text()` yields copy text, applying the wrap join,
  trailing-whitespace trim and scrollback traversal.
- **Split of labour** — `src/selection.rs` holds types only (75 lines). The cell-aware logic (text
  extraction, range clipping) lives in `src/term/selection.rs`, where the cells are reachable —
  moved out of `term.rs` in #587.

## Code

- `justerm-core/src/selection.rs` — `SelectionType`, `Side`, `SelectionSpan`, `BufferPoint`, `Anchor`,
  `Selection::ordered`
- `justerm-core/src/term/selection.rs` — `Term::selection_begin` / `selection_extend` /
  `selection_clear` / `selection_range` / `selection_text` / `accessible_text`; the three coordinate
  fixups `selection_shift_below_margin` / `selection_evict_oldest` / `selection_rotate_region`; and
  the private `resolve` / `Resolved` that turn a selection into absolute bounds. Extracted from
  `term.rs` in #587. As with search, the crate now has **two** files named `selection.rs` — the
  types in `src/selection.rs` above, the mechanism here — so a bare `selection.rs:NN` citation is
  ambiguous
- `justerm-core/src/term/walk.rs` — the shared buffer-walk floor the selection reaches cells through:
  `Term::abs_line` / `abs_row`, `prev_pos` / `next_pos` (the logical-line step), `word_start` /
  `word_end`, `is_word_boundary`. Extracted from `term.rs` in #585. Since #545 `is_word_boundary`
  is a `Term` method reading injected policy, not a free function over a fixed set — the set itself
  lives in `term.rs` (`DEFAULT_WORD_SEPARATORS`, `set_word_separators`, and the `full_reset` line
  that carries it across RIS)
- Consumers: justerm-web does pixel→cell and clipboard; justerm-renderer paints the highlight

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row is pinned to a `file:line`
at a recorded SHA; a paraphrase drops the pin).

- [Word selection started *on* a separator](../../agents/reference-facts.md#word-selection-started-on-a-separator--the-references-disagree-so-justerm-is-not-an-outlier)
  — justerm's walkers break on the **neighbour** cell's class, never the start cell's own, so
  word-selecting the space in `"ab cd"` returns both words joined. That looks like a defect and is
  not: alacritty does the same and xterm.js does the opposite, so a **split reference makes this a
  product choice, not a correctness fix**. Recorded explicitly so it is not re-litigated
- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
  — how a reference carries an anchor across a re-split, which is what `reflow(points)` does with the
  selection
- [A selection when the screen changes under it](../../agents/reference-facts.md#a-selection-when-the-screen-changes-under-it-660-verified-2026-07-31)
- [What the engine does with a column it was handed anyway](../../agents/reference-facts.md#what-the-engine-does-with-a-column-it-was-handed-anyway-671-verified-2026-07-31)
  — the read-side half. The three references converge on *"an out-of-range column is not
  observable"* and reach it three different ways (clamp / a guaranteed producer plus a total reader /
  a type that cannot express it). justerm now clamps, at the write site, and the section records what
  that choice does **not** buy — no equivalent of alacritty's wrap-to-next-line arm, which it does not
  need (#671)
  — the three available designs, one per reference: clear on a width change and rotate otherwise
  (alacritty), clear on a height change (xterm.js), or make the anchor a tracked pin so it cannot go
  stale (ghostty). justerm's primary pane is the first; the alt pane had none, which is #660

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) —
  `CellGeometry.originX`/`originY` are the only two of its six fields with no stated precondition,
  because a position may legitimately be `0` or negative, so `geometryViolations` structurally cannot
  flag a box that has gone away. **Repaired in #819**: `getGeometry` answers `CellGeometry |
  undefined`, because the consumer took the measurement and is the only party that can tell absence
  from a legitimate `0`. This territory holds the site, and it is the one with **state** to unwind —
  `SelectionController.mouseMove` *and* `tick` both reset `dragScrollAmount`, since a refusal that
  only returned early would latch the last auto-scroll speed and the consumer's timer fires whether
  or not the pointer moves. Distinct from the product ambiguity #680 settled next door, which this
  note draws the boundary against — and #819 is what showed the two are reachable through the *same*
  symptom: #680's `cellHeight > 0` guard passes when the cell comes from the renderer, so the
  max-speed auto-scroll it fixed returns through the origin
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — this territory takes the **positional** answer, and it is the discriminator that keeps the note
  honest: a selection is a region of the screen, so showing what is now under the highlight after
  an in-place erase or overwrite is the semantics rather than staleness (#750)
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — the pointer-to-cell conversion here divides by a cell that five setters can move, in a unit the
  renderer does not report (#578)
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::prev_pos` must
  not join down into the primary scrollback while on alt (#207)
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the selection overlay group shares its record shape *and* its count prefix with search's, so it
  inherits the `u32` widening #621 made there. A selection cannot practically reach the ceiling the
  way a query can, which is why the fix arrived from the other territory
- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — this territory holds **both** halves: the injected `word_separators` must survive `ESC c` (#545)
  while the `selection` anchors beside it must die with the buffer they index
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — this territory
  holds **two** of the five implementations: `extract_lines` for the linear arms and the block arm's
  own loop. #685 measured them returning different text for the same cells when only one was fixed
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — this territory
  holds **both** observables of it: `selection_range` and `selection_text` widen at the same funnel
  (`resolve`) precisely so they cannot disagree (#454)
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — `cellAndSide` owes the bound on both axes, and the engine's own clamp (both axes since #671) is a
  backstop rather than a substitute: the alt-click cursor move leaves through the consumer's callback,
  so no core guard is on that path at all (#667)

## Blast radius

Check these after changing this territory:

- [wide glyph & soft wrap](wide-glyph-and-soft-wrap.md) — `selection_text` performs the wrap join and
  drops the wide-wrap artefact, so a change to the pair/wrap rules changes extraction output
- [frame & wire](frame-and-wire.md) — the highlight leaves as an overlay group, so ADR-0014's wire group
  is affected
- [search & active match](search.md) — the active search match is painted **on top of** selection in
  its own colour, so the precedence between the two overlays is a single decision touching both
  (#430 pins the active ∩ selected fg channel). Both also share the absolute coordinate space
- [damage & viewport](damage-and-viewport.md) — the highlight's visibility is gated by the same
  `display_offset` as everything else pushed into the engine
- [logical lines](logical-lines.md) — `accessible_text` is listed under **both** territories' `## Code`
  (it lives in this module, but its contract is a whole-buffer document), so a change to either side
  can invalidate the other's pin. The edge was one-way until #587, and that is exactly how the move
  broke logical-lines' pin without any sweep noticing
- [reflow](reflow.md) — the fourth row of the table above. `grid.rs`'s `reflow` takes selection
  anchors as tracked `points`, and #562 (reflow cannot express a point one past the last cell) surfaced
  right here

## Known holes / open

- **Zero governing records.** The whole §Design model above is unrecorded. *"Why absolute
  coordinates"* and *"what moves the coordinate"* are the kind of thing that gets
  re-decided, and their grounds exist only in code comments.
- ~~**Block selection over wide characters is unspecified**~~ — **closed by #454**: a rectangle
  widens onto whole pairs **per row** (a rectangle meets a pair at a different column on each), on
  both observables. The rule is
  [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md); the visible
  consequence is that a row where it fires is one column wider than the rectangle, which is what all
  three references do.
- ~~**Word-selection boundaries** — the set is hardcoded in core.~~ **Closed by #545**: the set is
  consumer policy now (`Term::set_word_separators`, default `DEFAULT_WORD_SEPARATORS`), so the
  routing conforms to ADR-0017. Two things it left behind, both narrower than the hole was:
  - ~~the **trailing trim** is still `char::is_whitespace()`-based~~ **closed by #685**: the trim
    removes `' '` only, at both of this territory's sites — `extract_lines` *and* the block arm's
    own per-row loop, which is a second implementation nothing had noticed. The rule is now
    [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md). What it did
    **not** close: a *written* trailing ASCII space is still dropped, because `Cell` cannot
    distinguish one from a blank — the references split 2–1 there against 3–0 on the property, so
    it is a separate decision, not a leftover;
  - `' '` is **forced into every injected set** at the setter, because a blank cell packs `' '` and
    the walk uses that both to stop at a row's padding and to backstop the wide-pair rule. That is
    ghostty's shape (it prepends its own blank codepoint at the config intake), and it is the one
    part of this policy the consumer does *not* own.
