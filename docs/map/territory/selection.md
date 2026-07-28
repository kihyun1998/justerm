# Territory — selection

## What it is

The user grabs a range spanning screen and scrollback, and that range becomes ① the **highlight** the
renderer paints and ② the **text** that reaches the clipboard. Engine-owned — the consumer only does
pixel→cell conversion and clipboard transport.

## Governing decisions

**None.**

This blank is the actual state. Two decisions sit adjacent, and neither governs the selection *model*:

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
- **The coordinate moves in exactly three places** — scrollback cap eviction, in-screen region/RI
  scroll, and reflow. `Term` handles each explicitly (`Term::selection_evict_oldest`,
  `Term::selection_rotate_region`, `Term::selection_shift_below_margin`).
- **`Side` (Left/Right)** — which edge of a cell the anchor sits on. Lets a drag include or exclude the
  cell under the pointer; this is what makes mouse precision possible.
- **Four types** — Char (runs across lines) / Word / Line / Block (rectangular).
- **Two outputs** — `selection_range()` yields **per-viewport-row** `SelectionSpan { row, left, right }`
  and emits nothing for off-screen rows. `selection_text()` yields copy text, applying the wrap join,
  trailing-whitespace trim and scrollback traversal.
- **Split of labour** — `selection.rs` holds types only (75 lines). The cell-aware logic (text
  extraction, range clipping) lives in `term.rs`, where the cells are.

## Code

- `justerm-core/src/selection.rs` — `SelectionType`, `Side`, `SelectionSpan`, `BufferPoint`, `Anchor`,
  `Selection::ordered`
- `justerm-core/src/term.rs` — `Term::selection_begin` / `selection_extend` / `selection_clear` /
  `selection_range` / `selection_text`; the three coordinate fixups
  `selection_shift_below_margin` / `selection_evict_oldest` / `selection_rotate_region`
- `justerm-core/src/term/walk.rs` — the shared buffer-walk floor the selection reaches cells through:
  `Term::abs_line` / `abs_row`, `prev_pos` / `next_pos` (the logical-line step), `word_start` /
  `word_end`, `is_word_boundary`. Extracted from `term.rs` in #585
- Consumers: justerm-web does pixel→cell and clipboard; justerm-renderer paints the highlight

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::prev_pos` must
  not join down into the primary scrollback while on alt (#207)

## Blast radius

Check these after changing this territory:

- [wide glyph & soft wrap](wide-glyph-and-soft-wrap.md) — `selection_text` performs the wrap join and
  drops the wide-wrap artefact, so a change to the pair/wrap rules changes extraction output
- **frame / wire** *(no note yet)* — the highlight leaves as an overlay group, so ADR-0014's wire group
  is affected
- [search & active match](search.md) — the active search match is painted **on top of** selection in
  its own colour, so the precedence between the two overlays is a single decision touching both
  (#430 pins the active ∩ selected fg channel). Both also share the absolute coordinate space
- [damage & viewport](damage-and-viewport.md) — the highlight's visibility is gated by the same
  `display_offset` as everything else pushed into the engine
- **reflow** — one of the three places the coordinate moves. `grid.rs`'s `reflow` takes selection
  anchors as tracked `points`, and #562 (reflow cannot express a point one past the last cell) surfaced
  right here

## Known holes / open

- **Zero governing records.** The whole §Design model above is unrecorded. *"Why absolute
  coordinates"* and *"why exactly three places move the coordinate"* are the kind of thing that gets
  re-decided, and their grounds exist only in code comments.
- **Block selection over wide characters is unspecified** — no artifact states what happens when a
  rectangular range cuts a width-2 glyph in half.
- **Word-selection boundaries** — which character classes form a word is a *policy*, which under
  ADR-0017 may belong to the consumer, yet it lives in core. No record of that routing being
  considered.
