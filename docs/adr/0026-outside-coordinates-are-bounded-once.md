# ADR-0026: A coordinate that arrives from outside is bounded once, and no reader bounds half of a pair

Status: **proposed** (2026-07-31, #678). Promotes the model that accreted across #660, #671 and #678 —
three fixes to two projections, each decided as its own "(a) or (b)" and each producing the next.

**This is a derivation, not a product judgement**, and the distinction matters for how it is reversed:
every clause below follows from the shape of `justerm-core`'s public surface plus a named reference,
so a better derivation retires it. The one thing that *was* a judgement is recorded as such in D2's
grounds — the choice between clamping and hiding, where the references split 1–1.

## Context

`justerm-core` takes coordinates from callers it cannot see. Two public surfaces do it, and neither is
an accident:

- **A pointer position.** `selection_begin` / `selection_extend` document their input as *"what a mouse
  event carries"*. A pointer leaves the grid whenever a drag does, and `justerm-web`'s own fit floors
  the grid from the container, so a press can land in the strip below the last row. Out-of-grid input
  is **ordinary**, not a defect.
- **A search match.** `Match` has four public fields, and `set_active_search_match` is documented as
  taking one the consumer assembled *outside* the engine's own result set (the past-cap path, #436).
  Here the coordinate is not merely observed by the consumer — it is **authored** by it.

Both are consequences of ADR-0017's routing: the mechanism is core's, the policy is the consumer's, so
the consumer necessarily hands coordinates back across the boundary.

What made this a cluster rather than three bugs is that the engine's two projections —
`Term::selection_range` and `Term::match_spans` — each bounded *one* end of a coordinate pair and not
the other, and the missing half was invisible for as long as some other guard happened to cover it:

| | decided | where the bound went | what the next one found |
|---|---|---|---|
| **#660** | the anchor **row** | write site (`viewport_to_abs`) | the read site's own ordering was the outlier; `match_spans` already had it right |
| **#671** | the anchor **column** | write site, beside the row | `Side::Left` had no `+ 1` for the reader to clip, so the raw column reached an unbounded `left` and **deleted the anchor's row** |
| **#678** | a match's **column** | read site (`match_spans`) | the same asymmetry, still live — because search has no producer to clamp |

Three combinations of {axis, surface, bound-site} decided one at a time, each reinterpreting the last.
The tell that a rule was missing: #671's write-site clamp made `selection_range`'s half-bound
*unreachable* without removing it, so #678 met the identical expression in `match_spans` and had to
re-derive from scratch why the same fix did not apply in the same place.

## Decision

**D1 — An out-of-range coordinate from a public surface is bounded, never asserted.** It is ordinary
input, so a `debug_assert` would panic a consumer's debug build for a legal gesture. This is
`viewport_to_abs`'s existing reasoning, generalised: the assert belongs on engine-*internal* producers
(`damage_span`), where an out-of-range value really is a justerm bug.

**D2 — It is bounded once, at the point that first resolves it into engine coordinates.** Which point
that is follows from whether the engine owns a producer for it:

- the engine owns one (a viewport row/column becomes a `BufferPoint`) → bound **there**, and every
  reader downstream may then assume the range. That is #660 and #671, and it is why `resolve`'s five
  `Side`-dependent `+ 1`s stay unguarded.
- the engine owns none, because the coordinate *is* the consumer's (`Match`) → bound at the
  **projection** that reads it. That is #678.

The rule is not "prefer the write site"; it is *bound where the value first becomes the engine's
problem*. Deriving the site from producer-ownership is what makes both answers the same decision
rather than an inconsistency.

**D3 — A reader that bounds one end of a coordinate pair bounds both.** A half-bound is not half a
guard, it is a **worse failure**: the unbounded end survives into an emptiness test
(`right >= left`, `right_excl > left`) and turns an out-of-range value into a *silently dropped row*
instead of a visibly wrong one. Both defects in this cluster are that shape, and in both the bounded
end is what hid it.

**D4 — The bound is the grid, not the line's content.** `SelectionType::Line` already resolves its
exclusive end as `grid.cols()`, so the type works in grid coordinates throughout, and a short line must
not shrink a selection or a match that reaches past its text. In practice `abs_line(..).len()` *is*
the grid width — both row producers resize every row to `grid.cols()` — so this clause names the
invariant the existing `.min(len)` expressions silently rely on.

## Named prior art

Split, and the split is the point — it is why D2's *outcome* (clamp) is a judgement even though its
*site* is a derivation:

- **alacritty clamps.** `Point::grid_clamp` bounds the column unconditionally
  (`alacritty_terminal/src/index.rs:97`) and `Selection::to_range` runs it on **both** endpoints before
  any per-type arithmetic (`selection.rs:283`). It returns `None` on the line axis only.
- **xterm.js hides.** Its decoration renderer carries a commented arm for exactly this input —
  *"exceeded the container width, so hide"* (`src/browser/decorations/BufferDecorationRenderer.ts:83`)
  — and its public intake validates shape but not range
  (`src/browser/public/Terminal.ts:173`, throwing on negative/fractional/NaN).
- **ghostty cannot represent it** — a pin holds no out-of-range position (`src/terminal/Selection.zig:152`).

**What broke the tie is that justerm's prior behaviour was neither.** It dropped a start row and
painted the continuation rows — half of xterm's answer applied to one row of alacritty's shape. Given
two coherent answers and a third nobody holds, theflow's tie-breaker for API shape routes to this
repo's own precedent, and #660 had already set it to clamp.

Recorded with its cost, so the trade is not read as free: clamping paints wrong content *visibly*
where hiding paints nothing, and on a grid ending in a wide glyph the clamped column can be the pair's
trailing spacer — a bisected pair the dropped row could not produce (#454's class, `match_spans` now a
producer it does not name). Full comparison in
[reference-facts.md](../agents/reference-facts.md#search-who-may-hand-the-engine-a-match-and-what-happens-to-its-columns-678-verified-2026-07-31).

## Consequences

**The tests this record reproduces are its evidence.** `selection_column_bound.rs` (10) and
`match_span_column_bound.rs` (9) were each written before this file existed and each resolves against
D1–D4 without amendment, including their controls for what must *not* change —
`an_in_range_right_edge_anchor_still_starts_on_the_next_row` and `the_row_axis_is_unchanged` are D2 and
D3 holding at the boundary rather than exceptions to them.

**The one test it contradicts is its first finding, and it is adjudicated here rather than quietly
flipped.** `Term::selection_range` violates D3 in two places — its Linear arm bounds `right_excl` and
not `left`, and its Block arm bounds neither end against the line. Nothing reaches either today:
#671 clamps the producer, `resize` re-clamps reflowed points (#562), and the alt branch drops the
selection outright. It is fixed in the same change that proposes this record, on the grounds the
function's own comment already gives for the row axis — *"it makes the function total on its own
rather than by trusting a guard two files away"*.

**What this does not cover**, so the scope is not read wider than it was adjudicated:

- **Whether an intake should reject rather than clamp.** #663 weighed exactly that at the decode
  boundary; xterm does both (reject malformed, accept out-of-range). This record governs what happens
  *once a value is accepted*, not the admission policy.
  **#663 has since answered its own seam, and the answer does not transfer here** (2026-08-03):
  `decode` **rejects** a header declaring `cols < MIN_COLUMNS` or `rows == 0`. The two do not conflict
  and the reason is D2's own — *bound where the value first becomes the engine's problem*. A geometry
  is not a coordinate inside a grid; it is the grid, so there is no range to clamp it into and no
  reader downstream that could assume one. Read this bullet as still open for a **coordinate** at an
  intake, which is what it was about.
- **Wide-pair snapping** (#454). D4 says which coordinate space the bound lives in, not whether a
  bounded span should then expand onto a pair.
- **The row axis of `match_spans`**, which is total by a different mechanism (`if row >= rows { break }`
  before the read) and was measured non-defective (#678). D3 is about *column pairs*; the row filter is
  not a pair.
- **`justerm-web`'s converters.** The consumer's own obligation is a separate rule with a separate
  reason — see [pointer coordinates are bounded by their producer](../map/invariant/pointer-coordinates-are-bounded-by-their-producer.md),
  whose transitive argument (some coordinates never reach the engine at all) survives independently of
  anything here.

## Alternatives considered

**(A) Keep deciding it per issue.** What happened three times. Each decision was individually correct
and none of them generalised: #660's write-site clamp made #671's read-site half-bound unreachable
rather than wrong, so #678 met the same expression in a second file and re-derived the whole argument —
including, in its first draft, three claims about the references that were measured false. The cost is
not the deciding, it is that each decision reinterprets the previous one.

**(B) Bound everything at the read site, uniformly.** Simpler to state, and wrong for the pointer path:
`resolve` has five `Side`-dependent `+ 1`s, so one rule would become five clamps, and the anchor is
resolved once but read many times. D2 exists because the right site is derivable rather than uniform.

**(C) Make it unrepresentable, as ghostty does.** A `BufferPoint` that cannot hold an out-of-range
column would settle this by construction. Rejected on scope, not on merit — it is a change to every
coordinate in the crate, and the two projections are the only readers that have ever needed it. Worth
reopening if a third surface appears.
