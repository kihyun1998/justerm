# Territory — logical lines

## What it is

Soft-wrap-joined text for the rows touching the viewport, plus a per-character map back to the
viewport cell each character came from. It exists because a **frame-mode consumer holds only the
viewport** and therefore physically cannot reassemble a line that wraps — so the join is a *mechanism*
that must live in core, while what the consumer does with the text (URL regex, `new URL()` validation,
screen-reader phrasing) stays policy.

Two consumers, one shape: OSC 8-less URL detection, and the a11y screen-reader mirror.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) is
  the reason this is in core at all, and this territory is one of its clearest applications — but it
  decides *placement*, not the shape

Nothing decides what a logical line contains (trimming, spacer skipping) or what the off-screen
context contract is.

## Design model

- **`LogicalLine { text, cells }`** — `text` is the wrap-joined string; `cells` maps each `char` of
  `text` back to a viewport `(row, col)`.
- **`text` matches xterm's `translateToString(true)`**: wrap-joined across soft-wrapped rows,
  wide-char spacers skipped, trailing blanks trimmed. Matching a named reference exactly is the whole
  point — a consumer's URL regex is tuned against that shape.
- **`row` is `i32`, and out-of-range is deliberate.** A row outside `0..rows` is off-screen wrapped
  context — a line that wraps in from above the top, or out past the bottom. It is *present* so that a
  URL spanning the viewport edge still matches; the consumer highlights only the in-range cells.
  **This is the field most likely to be mishandled**: a consumer that treats `cells` as viewport-safe
  will index out of bounds or highlight the wrong row.
- **The module holds only the returned shape.** The cell-aware assembly lives in `term.rs`, where the
  cells are — the same split as [selection](selection.md).

## Code

- `justerm-core/src/logical.rs` — `LogicalLine` (the shape only)
- `justerm-core/src/term.rs` — `Term::viewport_logical_lines`, `Term::viewport_line`
- `justerm-core/src/term/selection.rs` — `Term::accessible_text` (the whole buffer as one document,
  a different contract from this one). It sits in the selection module because it reuses that
  module's extraction path, not because it is a selection — moved there with it in #587
- `justerm-core/src/term/walk.rs` — the stepping and materialisation primitives the join is built
  from: `prev_pos` / `next_pos`, `extract_lines`, `append_cell`, `is_wrap_artefact`,
  `is_walk_transparent_spacer`. Extracted in #585

## Reference behaviour

**None.** `docs/agents/reference-facts.md` has no entry for logical-line assembly. This one matters
more than an empty section usually does, because the territory's central claim is a *reference
equivalence*: `text` is documented as matching xterm's `translateToString(true)` (wrap-joined,
spacers skipped, trailing blanks trimmed). A consumer's URL regex is tuned against that shape, so the
claim is load-bearing — and it is currently an unpinned paraphrase in a doc comment, never grepped
against the pinned tree.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the wrap-run walk must
  floor on alt. **This was site 1 of 3** (#113): the first place the fact was ever found

## Blast radius

- [search & active match](search.md) — the same wrap-joined walk. #144 fixed the floor bug in both at
  once, which is the empirical proof these two move together
- [wide glyph & soft wrap](wide-glyph-and-soft-wrap.md) — the join follows the row wrap flag and skips
  spacers, so both rules land here directly
- [selection](selection.md) — `selection_text` performs the same join for copy, by a different path.
  Two implementations of "join a wrapped line" exist; a change to the joining rule must reach both
- **a11y** *(no note yet)* — the screen-reader mirror consumes this shape; a change to trimming or
  off-screen context changes what is announced

## Known holes / open

- **Two joiners, one rule.** `viewport_logical_lines` and `selection_text` both join wrapped rows and
  both drop the wide-wrap artefact, independently. Nothing states they must agree, and nothing tests
  that they do.
- **The off-screen-context contract is undocumented outside the field's doc comment.** That a `row`
  may be negative or ≥ `rows` is a sharp edge on a public type with no record behind it.
- **`accessible_text` vs `viewport_logical_lines` overlap is unstated** — one is whole-buffer, one is
  viewport-plus-context, and no artifact says which a consumer should reach for.
