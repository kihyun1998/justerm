# Invariant — a pointer coordinate is bounded by the converter that produces it

## The fact

A pixel position becomes a grid coordinate by dividing by the cell. The division is **total** — it
answers for every pixel on the page, including the ones outside the grid — so the converter, and not
whatever consumes its answer, owes the bound on both axes and at both ends.

Two failures, not one, and they are asymmetric:

- **Overshoot** (`px` past the last column, `py` past the last row) yields a cell that is merely too
  large. Downstream code that clips a range tends to absorb it.
- **Undershoot** (`px`/`py` negative, because the pointer is above or left of the canvas origin)
  yields a **negative** cell. `Math.floor(-3 / 10)` is `-1`, not `0`. Nothing in the type system says
  a cell index is unsigned, and the value crosses a boundary — a wire field, an IPC parameter, a
  `usize` — where "negative" has no representation and becomes something enormous instead.

Out-of-grid input is **ordinary**, which is why this cannot be left to a caller's good behaviour:
a drag leaves the grid whenever the pointer does (`mousemove`/`mouseup` are window-scoped in every
wiring this repo ships), and `fit.ts` floors the grid from the container, so a box that is not an
exact multiple of the cell keeps a remainder strip that resolves one past the end.

## Why it is cross-cutting

The obligation belongs to a *shape* — "pixels in, cell out" — not to a feature. Every territory that
accepts a pointer grows its own converter, each written by whoever needed it, and nothing relates
them: they do not share a call graph, a type, or a test. A grep for the fix at one site
(`clampTo`, `min`, `clamp`) cannot find the site that never heard of it, which is the same reason
[the alt-screen absolute-index floor](alt-screen-buffer-floor.md) needed a note rather than a helper.

The engine is **not** the backstop that makes this unnecessary, and the reason is structural rather
than a matter of how much the engine currently bounds:

1. **Some pointer coordinates never reach the engine at all.** `SelectionController`'s alt-click
   cursor move leaves through the consumer's `onMoveCursor` callback, so no core guard — present or
   future — is on that path. This is the transitive reason, and it is the one that carries the rule.
2. ~~`Term::viewport_to_abs` bounds the row and not the column.~~ **Retracted (#671): it now bounds
   both.** Recorded rather than deleted, because the *shape* of the retraction is the useful part —
   this was cited as a second, independent measurement, and it was really a statement about how
   complete the engine's clamp happened to be on the day it was written. A reason that expires when
   the other layer improves was never load-bearing; reason 1 does not expire, because it is about a
   path the engine is not on. **The rule is unchanged.** Had it rested on this reason, closing #671
   would have retired an invariant that is still true.

[ADR-0026](../../adr/0026-outside-coordinates-are-bounded-once.md) now governs the *engine* side of the
same question — where a coordinate arriving from outside gets bounded, derived from whether the engine
owns a producer for it. It **explicitly leaves this rule alone**, on reason 1 above, and this note is
where it sends the reader for the consumer half. So the two are complements and neither subsumes the
other: read that record before changing where `justerm-core` bounds something, and this note before
changing what `justerm-web` hands it.

## Territories it holds in

- [selection](../territory/selection.md) — `cellAndSide` turns a press/drag into an anchor and a
  `Side`. The clamp must come **before** the side is computed: deriving the within-cell offset from
  the *clamped* column is what makes overshooting right land on the last column's `Right` and
  overshooting left on column 0's `Left`.
- [input encoding](../territory/input-encoding.md) — the mouse-reporting converter, whose bound
  exists because an unbounded value would wrap in core's `encode_mouse` (#266). It clamps `px`/`py`
  as well as `col`/`row`, because `?1016` SGR-pixel reporting sends the raw pixels too.
- [accessibility](../territory/accessibility.md) — the AT-selection bridge converts DOM text offsets
  rather than pixels, so the arithmetic differs, but the obligation is the same one and it is
  discharged (out-of-tree endpoints resolve to the tree's edges).
- [fit](../territory/fit.md) — the *source* of the remainder strip rather than a converter itself:
  flooring the grid from the container is what guarantees the out-of-range coordinate exists.

Not a member: `scrollbar.ts` computes a **ratio** of `rows`/`scrollbackLen`, never a cell from a
pixel. It clamps anyway, which is a coincidence of range arithmetic and not this invariant.

## What a violation looks like

Nothing throws, and the widget looks fine. A drag past the left edge selects from a negative column;
a press in the remainder strip anchors one row past the last. What the consumer sees depends entirely
on what it does with the number:

- a range-clipping reader absorbs it and returns an **empty selection** — a gesture that silently
  does nothing;
- an absolute-index walk reads the wrong region;
- a transport that reinterprets the sign turns `-1` into the maximum of its unsigned type, and
  `usize::MAX + 1` overflows (`justerm-core/src/term/selection.rs`, the `Side::Right` arms of
  `resolve`). On wasm32 `usize` is 32 bits, so `-1` maps exactly onto `usize::MAX`.

The tell while reading code is a bare `Math.floor(px / cellWidth)` with no bound on the same
expression — and, one level up, a converter whose *siblings in the same file or package* clamp while
it does not.

## Discovery history

Found twice, at sibling sites, months apart, by two different routes — which is the qualification for
being written down here rather than fixed a third time:

- **#266** (renderer S6 pixel→cell, then `justerm-web`'s `input.ts`) — reached through core's
  `encode_mouse`, where an out-of-range coordinate wraps to a huge `usize`. The fix landed with a
  comment naming that mechanism, so it read as a *mouse-reporting* fact.
- **#667** (`justerm-web`'s `selection.ts`) — the sibling converter beside it, which had never been
  given the same bound. Surfaced as a "deliberately not fixed" note while working #660, whose engine
  clamp for the **row** made the consumer-side gap look closed when only half of it was.
  #660's own doc comment names the unfixed sibling by file and line, which is the only reason the
  gap had a durable record at all.

The pattern in both: the second site was never edited by the change that fixed the first, and neither
fix mentions a shared rule.

## Where it will recur

- **Any new pointer affordance** — hover, link click, a context-menu target. ~~per-decoration
  hit-testing (#502)~~ — **retired 2026-08-18, not deleted, because the reasoning is the reusable
  part.** Hit-testing a decoration presumes a decoration *object* to hit; ADR-0024 R1 is now
  unconditional that there is none (#502 closed as won't-do, its alternative (F)). So this was never
  a queued affordance — it was an affordance that depended on a model question, and the question
  resolved the other way. The rest of the bullet is unaffected: hover and link click are live and
  need no object. The demo already carries an unbounded fourth converter
  (`demo/main.ts` `cellFromEvent`, feeding `LinkController`); it is inert because an out-of-range
  coordinate simply misses the link map, and it is listed here so the next reader does not have to
  re-derive that. **That inertness was conditional and the condition was never stated — #819.** It
  holds only while the page derives its cell from the measured BOX, which takes `cellHeight` to `0`
  and the quotient to `Infinity`. Derive the cell from the renderer, as this package's README
  recommends, and the same converter answers an **in-range** cell against a hidden pane, so the
  hover resolves a real link instead of missing. It is also the widest trigger of the four, needing
  no press at all — the listener runs on bare motion. `cellFromEvent` now answers
  `[number, number] | undefined`; the bound was never the missing part.
- **#287 multi-viewport** — one context serving N grids means N sets of bounds, and a pointer that
  is inside the canvas but outside *this* viewport becomes an ordinary case rather than an edge one.
- **`NaN`, which no *bound* catches — settled as a precondition instead (#672).** A bound cannot
  reach it: `clampTo` propagates `NaN` through both `Math.max` and `Math.min`, so the converter is
  answering a question its input never posed. `CellGeometry` now states what its six fields may be
  (finite; a positive finite cell; non-negative integer counts) and the converters *signal* a
  violation rather than refusing — xterm's `hasValidCharSize` → `undefined` guard is half of a repair
  loop whose other half (re-measure) this widget cannot have, since the geometry is the consumer's by
  ADR-0017. **One clause of that reading is retired (#819)**, and it is worth naming because it was
  the load-bearing objection to ever refusing here: the implied *"so a dropped gesture would not come
  back on its own"* is measured false — `getGeometry` is pulled per event, so a refused gesture
  resumes on the correct cell the moment the box returns. The rest stands, and its scope is the
  **cell**. `#819` refuses on a different axis — an *absent box*, where every field is in range and
  no precondition can fire — at the only site that can see it, the consumer that took the
  measurement. Both converters check, because both share `clampTo` and therefore shared the gap.
  **The rest of the geometry's readers were never in this note's scope, and #675 closed the scroll
  half of them.** Two of them consumed the same callback and handed the *consumer* a non-finite
  number: `WheelScroller` accumulated a non-finite delta into `wheelPartialScroll` and **latched**
  it (`Infinity % 1` is `NaN`, and only `reset()` cleared it), while `dragScrollSpeed` emitted `NaN`
  at exactly `py === 0`. Neither was a bound, so neither could be signalled away — they are fixed by
  making each producer total. The rule that generalises out of it is the sibling of this note's own,
  one layer out: **a producer owes its consumer a value the consumer's type can mean.** Where this
  note is about a coordinate that must be in range, that one is about a number that must be a
  number — and `Math.max`/`Math.min` no more produce it than they produce a bound.
  **#814 found a third producer of the same callback and made it total too**, and it is the one that
  shows why the rule has to be applied at the *inputs*. `dragToDisplayOffset` is exported, so it owes
  its own totality; measured across its four poisoned positions, three surfaced as `NaN` and
  `rows: Infinity` surfaced as **`0`** — `Math.round(60 - ∞)` is `-Infinity` and the clamp rescues it
  into a finite, plausible jump to the live edge. A finiteness test on the *result* therefore fixes
  three of four and reads as if it fixed all four, which `wheelScrollTarget` had already stated in
  prose (`terminal.ts`) and which a mutation now pins: moving the guard to the result reddens exactly
  the `rows: Infinity` assertion and nothing else. Note that this producer is **not** a member of the
  note's own rule — it computes a ratio, not a cell, exactly as the exclusion below says.
  **A fourth producer in the same file came with it, and finding it is the transferable part.**
  `scrollbarMetrics` divided after a *relational* `visible` check, so a non-finite position answered
  `visible: false` correctly and still emitted `NaN` ratios into `style.height` / `style.top` as
  `"NaN%"`. #463 had already fixed exactly this in `decorations.ts` and left the rule in its comment
  — *"`total <= 0` is a size comparison and NaN slips through every comparison; `Number.isFinite` is
  the check"* — but only on the **marks** half of `scrollbar.ts`'s style writes, not the **thumb**
  half. It was found by grepping the closed backlog **by artifact** after the change was written,
  not by reading the file, and it had first been graded *"no reachable consequence"* — which was true
  of the symptom and the wrong bar, since the sibling was filed on *"recorded rather than
  rediscovered"*.
  **#680 then closed the drag half, and how it closed is the part worth carrying.** A drag that
  outlives the canvas's box auto-scrolled at the maximum speed, because `SelectionController` builds
  its viewport height as `getRows() * geom.cellHeight` and a **product loses which factor was zero**.
  Guarding the product would have re-decided #667, whose reading of a 0-row viewport is load-bearing
  for `tick()`'s edge-row floor — its test caught exactly that attempt. The resolution was to notice
  that the factors carry *different contracts*: `cellHeight` has a documented precondition (#672),
  `getRows()` does not. So the guard sits at the caller that still holds them apart, and the
  published `dragScrollSpeed` keeps its signature and xterm's semantics. Generalised: **when a
  derived value is ambiguous, the fix is usually upstream of the multiplication, not a rule about
  the result.**
  One thing stays deliberately open, recorded where it belongs rather than here: the *staleness* of
  the wheel scroller's retained fraction across a cell change — #630's third instance, a different
  axis of the same field.
- **A side-from-raw-pixel refactor.** The clamp currently doubles as the overshoot rule for `Side`;
  computing the side from the unclamped pixel (alacritty's shape) would need alacritty's explicit
  `end_of_grid → Right` arm restored alongside it.

## Code

- `justerm-web/src/selection.ts` — `cellAndSide`, and `SelectionController.tick`, the one row
  producer that does not go through it
- `justerm-web/src/input.ts` — `clampTo` (shared by both converters since #667), `cellEvent`,
  `CellGeometry` (whose field docs carry the preconditions), `geometryViolations` / `checkGeometry`
  (the #672 signal, shared the same way the clamp is)
- `justerm-web/src/a11y-selection.ts` — `clamp`, the DOM-offset form of the same obligation
- `justerm-web/src/fit.ts` — `proposeDimensions`, which floors the grid and so creates the strip
- `justerm-core/src/term.rs` — `viewport_to_abs`, the engine-side backstop (both axes since #671)
  that is explicitly not a substitute

## Reference behaviour

In [reference-facts.md](../../agents/reference-facts.md#who-bounds-a-pointer-coordinate--the-producer-not-the-engine-667-verified-2026-07-31)
— **linked, never restated**. 3/3: all three references bound both axes at both ends in the
consumer-side converter, before anything reaches their engine, and xterm.js serves mouse reporting,
selection and linkification from a single converter. The two details that deliberately did *not*
transfer (xterm's `colCount + 1`, alacritty's separate `end_of_grid` arm) are recorded there.
