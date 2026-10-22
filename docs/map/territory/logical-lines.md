# Territory — logical lines

## What it is

Soft-wrap-joined text for the rows touching the viewport, plus a per-character map back to the
viewport cell each character came from (`Engine::viewport_logical_lines` → `LogicalLine`). It exists
because a **frame-mode consumer holds only the viewport** and physically cannot reassemble a line
that wraps — so the join is a *mechanism* that lives in core, while what the consumer does with the
text (URL regex, `new URL()` validation) stays policy. The forward run walk under it
(`walk_logical_line`) is shared with [settled-cell paint](settled-cell-paint.md)'s line reports.

One consumer of this shape: plain-text URL detection, which reaches a frame-mode widget one hovered
row at a time through the consumer-wired `LinkPort` (#934) — wired in PenTerm (penterm 7dadc3c95
`src-tauri/src/engine/links.rs` — `line_at`, #2101). The a11y screen-reader mirror does **not** consume
it: it reads rows from its own `CellMirror` and has never joined wraps.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) is
  the reason this is in core at all — but it decides *placement*, not the shape.

Nothing decides what a logical line contains (trimming, spacer skipping) or the off-screen context
contract.

## Design model

Status: no record; read from `logical.rs`, `term/logical.rs` and `term/walk.rs` at 7bb31f7, and from
#601/#685.

- **`LogicalLine { text, cells }`** — `text` is the wrap-joined string; `cells` maps each `char` of
  `text` to a viewport `(row, col)`. Wide-char spacers are skipped, combining marks map to their base's
  cell (#45), empty lines are dropped, and trailing `' '` is trimmed in lockstep so `text` stays 1:1
  with `cells`.
- **The trim removes U+0020 only (#685).** A blank cell packs `' '`, so `' '` is all the trim may
  remove; a printed trailing U+00A0 / U+3000 survives, a printed trailing ASCII space does not. It used
  to be the Unicode `White_Space` property.
- **`text` does *not* match xterm's `translateToString(true)`** — checked against the pinned tree in
  #601; only the spacer skip survives. The wrap join is absent there (`translateToString` spans one
  `BufferLine`; xterm pairs it with `Buffer.getWrappedRangeForLine`), and the trim differs: xterm keeps
  any *printed* trailing space, so the two still disagree on a printed ASCII space (no longer on a
  printed U+00A0 / U+3000, since #685).
- **`row` is `i32`, and out-of-range is deliberate.** A row outside `0..rows` is off-screen wrapped
  context — a line that wraps in from above the top or out past the bottom — present so a URL spanning
  the viewport edge still matches; the consumer highlights only in-range cells. **The field most likely
  to be mishandled**: a consumer treating `cells` as viewport-safe indexes out of bounds or highlights
  the wrong row.
- **The up-walk floors at `abs_floor`** (on the alt screen the scrollback is the primary's), and the
  forward walk (`walk_logical_line`) reaches past the viewport bottom.
- **The run walk is unbounded on purpose** — `O(viewport)` normally, `O(scrollback)` on a buffer whose
  whole scrollback is one soft-wrapped run: 7.3 ms against 17 µs for the same bytes as short lines
  (`benches/wrap_run.rs`). #206 closed it (2026-09-11) with reach measured at zero. **That zero is
  dated**: `LinkPort` makes a backend answer from `viewport_logical_lines` once per question the
  pointer's motion asks, and PenTerm now wires it — bounded in count by motion, not in cost. If a bound
  is ever wanted, two things are decided: it is a **sibling, not a parameter** (`max_run:
  Option<usize>` is breaking; the idiom is `search` / `search_with`, so a
  `viewport_logical_lines_with` is additive), and **the hard part is the trim**, which assumes only the
  run's last row carries padding — a mid-run cut must re-answer only-U+0020-can-be-padding at the cut.
- **The module holds only the returned shape.** The cell-aware assembly lives in
  `justerm-core/src/term/logical.rs`, moved out of `term.rs` in #601 — the last read surface to leave
  it under #584.
- **Outside input**: none directly; the output is bounded by the viewport plus the one run that
  touches each edge, and the run length is bounded only by scrollback (above).

## Code

- `justerm-core/src/logical.rs` — `LogicalLine` (the shape only)
- `justerm-core/src/term/logical.rs` — `Term::viewport_logical_lines`
- `justerm-core/src/term/walk.rs` — `walk_logical_line` (the forward run walk, shared with `term/paint.rs`), `abs_floor`, `abs_row`, `row_in`, `prev_pos` / `next_pos`, `extract_lines`, `append_cell`, `is_wrap_artefact`, `is_walk_transparent_spacer` (extracted in #585)
- `justerm-core/src/lib.rs` — `Engine::viewport_logical_lines`
- `justerm-core/tests/logical_lines.rs` · `written_whitespace.rs`
- `justerm-core/benches/wrap_run.rs`
- Not this surface, listed because a reader looks here: `term/selection.rs` — `Term::accessible_text`
  (the whole buffer as one document, a different contract; in the selection module because it reuses
  `extract_lines`, #587); `term/viewport.rs` — `Term::viewport_line` (one viewport row's cells; it
  open-codes the scrollback-vs-grid branch `line_in` owns, after its own viewport-to-absolute
  conversion).

## Reference behaviour

- [Logical-line assembly](../../agents/reference-facts.md#logical-line-assembly-601) — the
  territory's central claim was once a reference equivalence (`text` matches xterm's
  `translateToString(true)`) that had never been grepped; checked in #601 and partly wrong — the wrap
  join is absent there.
- [Trimming a line's end](../../agents/reference-facts.md#trimming-a-lines-end-685-verified-2026-08-03)
  — all three references trim only what was never written, by three mechanisms; the U+0020 rule.

## Cross-cutting invariants

- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — the read surface where a stale position-keyed assertion becomes a wrong *string* rather than a
  wrong colour, which is what made the marker case the severe one (#750).
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — the collapse defined here is *why* the document space and the absolute space move by different
  amounts under one eviction, so a rebase scalar correct for absolute lines is wrong for document ones.
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the up-walk must floor
  on alt. **This was site 1 of 3** (#113): the first place the fact was found.
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — the trailing trim
  here is its own implementation, and owes the `text`/`cells` 1:1 property through the trim.

## Blast radius

- [search](search.md) — the same wrap-joined walk written separately; #144 fixed the floor bug in
  both at once, the empirical proof that a join-rule change must reach both.
- [soft wrap](soft-wrap.md) · [wide glyph](wide-glyph.md) — the join follows the row wrap flag and
  skips spacers and the wide-wrap artefact, so a change to either rule lands here directly.
- [selection](selection.md) — `selection_text` and `accessible_text` join wrapped lines by
  `extract_lines`, a different path; the two joiners must agree.
- [settled-cell paint](settled-cell-paint.md) — shares `walk_logical_line`; a change to what the walk
  visits changes both the reported text and the paint guard's comparison.
- [hyperlinks](hyperlinks.md) — the one consumer; a change to trimming or the off-screen rows changes
  which URLs match.
- [marker](marker.md) — `CommandLine::line` counts logical lines of the `accessible_text` document;
  a change to how a run collapses moves it.

## Known holes / open

- **Several joiners, one rule.** `viewport_logical_lines`, `selection_text`/`accessible_text`
  (`extract_lines`), search's haystack and the block-selection arm each join wrapped rows and drop the
  wide-wrap artefact independently. `tests/written_whitespace.rs` asserts every joiner against the same
  *trim* (#685); the wrap join and the artefact drop are untested for agreement.
- **The off-screen-context contract is stated only in the field's doc comment.** That a `row` may be
  negative or ≥ `rows` is a sharp edge on a public type with no record behind it.
- **The unbounded run walk is reachable again** through a wired `LinkPort` (above): one hover over a
  buffer that is one long run costs `O(scrollback)` per question.
- **`accessible_text` vs `viewport_logical_lines` overlap is unstated** — one whole-buffer, one
  viewport-plus-context; no artifact says which a consumer should reach for. PenTerm uses both:
  `accessible_text` for its session log (penterm 7dadc3c95 `src-tauri/src/engine/native.rs` —
  `buffer_text`), `viewport_logical_lines` for link detection.
