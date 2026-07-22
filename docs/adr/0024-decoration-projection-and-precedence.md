# ADR-0024: A decoration is colours plus a mark, and its projection rules follow from having no object

Status: **proposed** (2026-07-21). Records the consumer-side decoration model implemented across
#120/#198/#202/#457/#458/#459/#461/#463/#480/#498. Scoped to **span projection and precedence** — the
axis ADR-0019 explicitly put out of its own scope ("consumer policy under ADR-0017"). What a projected
rect then *composites to* is ADR-0019; ADR-0017 says only that this is the consumer's call, not what the
call should be.

## Context

A decoration in justerm-web is **a set of cell colour overrides plus an optional overview-ruler mark**.
There is no decoration object on screen — no element, no handle to style, nothing to hand back (#502).
That single fact generates most of what follows, and it is the one thing this ADR asserts rather than
derives.

xterm, by contrast, has **two** decoration paths, and they disagree with each other on nearly every
question below: a colour path (`DecorationService` → the renderers' per-cell resolve) and a DOM element
path (`BufferDecorationRenderer`, an absolutely-positioned div at z-index 6/7). Most of justerm's
"divergences from xterm" here are really *choices about which of xterm's two answers applies* to a model
that has only one path.

The decisions have accumulated one issue at a time — precedence (#458), ruler layering (#498), anchor
semantics (#459), above-viewport anchors (#461), clamping (#463), buffer-anchored ruler marks (#480) —
and live in `justerm-web/src/decorations.ts` doc-comments. The cluster is still open (#454 wide-char
snapping, #500 ruler mark fidelity, #502 the render hook), which is the same shape ADR-0019 was written
for one layer down.

## Decision

**R1 — A decoration is colours + a mark, not an object.** It projects to per-cell colour overrides and
at most one ruler mark per covered line. Anything a consumer would express by styling an element
(borders, outlines, per-decoration opacity, classes, transitions) has no expression, by construction.
#502 holds the open question of whether to add a declarative border; until it is answered, "no object" is
the model.

**R2 — Cell precedence is registration order, across markers.** Where two decorations set the same
property on the same cell, the later-registered one wins, whichever markers they anchor to. It is
deliberately *not* marker order: a per-marker grouping can only express order *within* a marker, and
using it across markers would leak **core's marker emission order into consumer policy** — the boundary
ADR-0017 draws. Raising a decoration means dispose + re-register; `register` alone mints a second one.

**R3 — Ruler order is the position class first, registration order second.** A `full`-width mark paints
above a gutter mark whatever the registration order; within a class, registration order. So **the ruler
order is not the cell order**, deliberately: on the track a thin `full` mark and a fat gutter mark
overlap physically, and class-last-wins is what keeps the thin one visible.

**R4 — `anchor` moves the colour span.** `anchor: 'right'` measures `x` in from the right edge and the
span extends leftward. This is a declared divergence (see below) and follows from R1: with no element to
position, ignoring `anchor` in the colour span would leave the option affecting *nothing* — a dead field.

**R5 — Projection is per visible row, not per anchor visibility.** A decoration whose anchor sits above
the viewport top still projects the rows of it that are on screen. This is what the frame's absolute
`markerLines` group is for, and it is why that group exists at all.

**R6 — A projection that cannot be computed emits nothing.** A non-finite or out-of-range input (a
`NaN` `scrollbackLen`, a marker line past the buffer, a ratio outside `[0,1]`) yields no rect and no
mark rather than an invalid one — `top: NaN%` is silently dropped by the browser and stacks marks at the
track default (#463).

## Named prior art — and what upstream actually *says*

Verified against real xterm source. The grading matters here more than usual, because **most of these
behaviours are only inferable from implementation**: citing them as xterm's *specification* would be
citing silence.

| behaviour | xterm's implementation | does upstream state it? |
|---|---|---|
| "last registered wins" on a cell | emergent, not guaranteed: `forEachDecorationAtCell` walks a per-line **insertion-order bucket** (`DecorationService.ts:100-112`, `:169-176`) while the *sorted* list (`SortedList` by `marker.line`, `:23-28`) feeds the ruler and the DOM instead; last-wins comes from the callback overwriting `$bg`/`$fg` (`CellColorResolver.ts:71-80`, `:178-187`) | **Yes, as a contract** — `typings/xterm.d.ts:673-685` — but only on `backgroundColor`/`foregroundColor`, and the bucket order that delivers it is undocumented and *perturbed*: an insert or delete re-adds span-crossing decorations at the bucket tail (`:270-273`, `:292-294`, `:328-330`), silently promoting them |
| `anchor` ignored by the colour path | `DecorationService.ts:106-108` computes `xmin = x`, `xmax = xmin + width` with no anchor term; `BufferDecorationRenderer._refreshXPosition` (`:122-132`) is the **only** consumer of the option in the repo | **No.** And the typings read the other way — `anchor` is "where the decoration will be anchored", `x` is "the x position offset **relative to the anchor**" (`xterm.d.ts:651-660`) |
| above-viewport anchors | the colour path keys the **absolute buffer line** (`WebglRenderer.ts:457-458`, `DomRenderer.ts:537`) and buckets every line the height covers (`DecorationService.ts:167-176`), so partial rendering happens — while the **element** is hidden all-or-nothing (`BufferDecorationRenderer.ts:91-98`) | only `// outside of viewport`, about the element. The two paths disagree and nothing reconciles them |
| ruler: `full` above gutter | two passes — every non-`full` zone, then every `full` zone (`OverviewRulerRenderer.ts:172-182`); intra-class order is **buffer-line** order (the `SortedList` feeding `ColorZoneStore`) | **No** — no comment, no test, no commit rationale |
| search marks are `position: 'center'` | `addon-search/src/DecorationManager.ts:140-143`, for active *and* non-active (only the colour differs); suppressed when the line already carries a mark | code only, unambiguous |

**Where justerm lands.** R2 keeps xterm's *stated* contract (last registered wins) and rejects its
*unstated* mechanism (a bucket that buffer edits reorder) — registration order here is stable because it
is the consumer's own input and nothing in the buffer can perturb it. R3 keeps xterm's class partition
and diverges on the intra-class tiebreak (registration, not buffer line) for the same reason. R4 is a
declared divergence from xterm's colour path and a match for its typings. R5 matches xterm's colour path
exactly — it is the *element* path that has the gap, and justerm has no element. `full` being the default
ruler position is xterm's too (`DecorationService.ts:376-378`).

## Consequences

- **R5 is why ADR-0020 has a violation.** Projecting rows of an above-viewport anchor needs every live
  marker's absolute line, which is the `markerLines` group — ADR-0020's one stated breach of its own
  `O(viewport)` rule. The two ADRs describe the same trade from opposite ends: this one wants the
  capability, that one records its cost, and #490 holds the fix. Neither should be read without the
  other.
- **The open cluster becomes lookups.** #454 (should a span snap to wide-char pairs?) is an R1/R2-level
  question about what a *span* is; #500 (ruler mark fidelity — zone merging, heights, centring) is R3's
  detail; #502 (a render hook) is a proposal to relax R1. Each is now "does the model answer this?"
  rather than a fresh pairwise decision.
- **R3's payoff is gated on mark geometry.** Class-last-wins only matters visibly once a `full` mark is
  thinner than a gutter mark, as upstream's are (`~2 device px` vs a `6-12 px` clamp,
  `OverviewRulerRenderer.ts:124-131`). Ours are a flat 2 px today, so #500 item 2 is the precondition for
  R3's benefit — the validity condition, recorded rather than assumed.
- **A consumer porting from xterm gets one surprise, and it is `anchor`.** Everything else here either
  matches xterm's colour path or is invisible; a right-anchored decoration's *background* moving is the
  single behavioural difference, and it is the one the typings predict.

## Alternatives considered

- **(A) Mirror xterm's colour path exactly, including ignoring `anchor`.** Rejected: with no element,
  `anchor` would affect nothing at all. Parity would be bought by making a public option dead.
- **(B) Make cell precedence follow marker/buffer order, as xterm's ruler does.** Rejected: it makes
  consumer-visible precedence depend on core's marker emission order, which is exactly the leak ADR-0017
  forbids — and it is unstable, since buffer edits reorder it upstream today.
- **(C) One order for both cell and ruler.** Rejected: the two surfaces have different geometry. On the
  grid, overlapping decorations paint on distinct cells; on the ruler, marks of different position
  classes physically overlap, so a class rule is needed that the grid does not want.
- **(D) Leave it in doc-comments (status quo).** Rejected on the same evidence as ADR-0019: nine issues,
  one axis, three still open, and the file's own comments already carry three declared divergences whose
  grounds nobody had checked against source until this ADR was written.

## Out of scope

- **What a rect composites to** — ADR-0019 (layers, channels, paint modes).
- **Whether the frame should carry `markerLines` at all** — ADR-0020 R3 and #490.
- **Ruler mark *appearance*** — merging, heights, centring (#500). R3 fixes the ordering, not the pixels.
