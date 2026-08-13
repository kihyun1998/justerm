# Territory — decoration

## What it is

What a consumer *paints* at a [marker](marker.md): per-cell colour overrides on the grid and at most
one mark per line on the overview ruler. Entirely consumer-side — the engine anchors and knows
nothing about appearance, because appearance needs a theme and the engine is theme-agnostic by
identity.

The first territory in this map that lives **outside `justerm-core`**.

## Governing decisions

- [**ADR-0024 — decoration projection and precedence**](../../adr/0024-decoration-projection-and-precedence.md)
  — R1 through R6, the whole model. **Check its `Status:` line before relying on it** — that line
  is authoritative and is deliberately not copied here (`CLAUDE.md`: a status copied into another
  document has no gate and goes stale silently)
- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) — decoration colours
  enter the renderer's layer stack under this model; 0024 opens by placing itself on **the axis
  ADR-0019 explicitly put out of its own scope**
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why this is consumer-side at all

## Design model

- **Absolute anchors come from a *pulled* index; the frame carries only viewport rows (#490).**
  `DecorationRegistry.setMarkerIndex` takes a `MarkerLineSource` — one method, `lineOf(id)` — which
  `MarkerIndexCache` satisfies: the consumer asks the backend once (`MarkerPort`, sibling of
  `CommandNavPort`), then keeps the answer current from the frame's `evictedTotal`/`markerEpoch` basis
  plus the marker create/dispose events. The parameter is the capability rather than the class so a
  consumer that already tracks marker lines can feed the projection without adopting the cache.
  **The absolute-line group left the wire in v16**, so the cell projection merges the index (absolute,
  and the only thing that can express an anchor above the top) with the frame's `markerPositions`
  (viewport rows for on-screen markers), and the ruler projection has the index alone. Where both
  answer for one marker the **absolute line wins** — #461's rule, unchanged since it was the frame's
  group that supplied it: a derived viewport row must not mask an anchor the absolute line places
  above the top. A lagging index does not compete, because `lineOf` returns `undefined` while
  `adopted !== seen` rather than serving a stale line. The v15 migration ordering that lived here —
  frame group first, index as gap-filler — is gone with the group it protected.
  Two things keep the index honest, and both exist because the events are `O(1)` and therefore do
  **not** move the epoch: the frame's `markerCount` is compared against the index's size every frame,
  so a host that wired the pull and not the events drifts for one frame rather than forever; and a v16
  frame arriving with ruler decorations registered and **no** index warns once, because that
  configuration renders an empty overview ruler with no exception, no red test and no gate able to see
  it. Keyed on `markerCount`, which every v16 frame carries, rather than on "this frame produced no
  marks" — that is true of any frame with no live markers, and only the count distinguishes a wire
  that stopped shipping anchors from a host that simply has none.
  Two consequences a reader will otherwise re-derive: the per-frame `O(M)` stride scan over absolute
  lines is gone with the group (what remains is the viewport group, bounded by the rows on screen), and an
  **unknown** line means *do not project*, never line 0, because a decoration that is missing is
  self-correcting and one painted on a line it no longer owns is not. How long it stays missing was
  recorded as being set by the epoch's churn rather than by the round trip (#738: one frame for a
  single reflow, the whole workload where the epoch moves per line) — and **that was still too
  generous (#746)**. The trigger that ended the outage asked whether the epoch had just *changed*,
  so a pull landing one generation behind the newest frame ended it nowhere at all: the churn stopped
  and the index stayed unusable, permanently. Reached by an ordinary interactive drag-resize once the
  query round trip approaches the resize cadence — measured, at RTT ≈ 100 ms against our own 100 ms
  `FitController` debounce, 8 drags in 40. So "self-correcting" is a statement about *direction*
  only; the latency was bounded by the churn **after** the trigger learned to ask a state instead of
  an edge, and even now a *refused* transport is deliberately not retried (the host's policy, not
  this class's).

ADR-0024 is authoritative; this is routing. **If they disagree, the ADR is right.**

- **R1 — a decoration is colours + a mark, not an object.** It projects to per-cell colour overrides
  and at most one ruler mark per covered line. Borders, outlines, per-decoration opacity, classes,
  transitions have **no expression by construction** — the model is not a styling system that happens
  to be small.
- **R2 — cell precedence is registration order, across markers.** Where two decorations set the same
  property on the same cell, the later-registered wins, whichever marker each anchors to.
  Deliberately *not* marker order, which could only express ordering *within* a marker.
- **R3 — ruler order is position class first, registration order second.** A `full`-width mark paints
  above a gutter mark regardless of registration order. **So the ruler order is not the cell order**,
  on purpose.
- **R4 — `anchor` moves the colour span.** `anchor: 'right'` measures `x` from the right edge and
  extends leftward. A declared divergence from the references, and it follows from R1: with no
  element to position, ignoring `anchor` would leave the option affecting nothing — a dead field.
- **R5 — projection is per visible row, not per anchor visibility.** A decoration whose anchor sits
  above the viewport still projects the rows of it that are on screen. **This is why the frame's
  absolute-line group existed at all (it left in v16, #490).**
- **R6 — a projection that cannot be computed emits nothing.** A **non-finite** input yields no rect
  and no mark rather than an invalid one, because the browser silently drops `top: NaN%` and stacks
  marks at the top edge — a wrong answer that looks like a rendering choice. An **out-of-range** input
  is a different case and is *clamped*, not dropped: it has a value, it is merely off the track. This
  line said "non-finite or out-of-range" until #500 corrected it; the ADR carries the amendment and
  the reasoning.

## Code

- `justerm-web/src/` — the decoration registry and projection (the consumer half)
- `justerm-renderer/src/decoration.rs` — where the projected colours meet the layer stack
- `justerm-core/src/serialize.rs` — `MarkerPosition`, the only wire input it gets from the
  engine

## Reference behaviour

- [The overview ruler — who has one, how a mark is merged, and how big it is](../../agents/reference-facts.md#the-overview-ruler--who-has-one-how-a-mark-is-merged-and-how-big-it-is-500-verified-2026-08-10)
  — the merge key and its (density-adaptive, per-class) threshold, class-dependent heights and their
  device-px/CSS-px split, and where a mark is drawn relative to its line. **Read its first rows before
  the rest**: this is the thinnest corpus in that file — alacritty has no scrollbar at all, ghostty has
  one and deliberately delegates it to a native widget that cannot carry marks, so a *marked* ruler is
  xterm-only. The corollary is the useful part: on **who owns scroll geometry** ghostty's three scalars
  are our `ScrollPosition`, so the corpus is 2 of 3 with us there

**Still none for the decoration model itself** — the rows above cover the ruler, not R1–R6. ADR-0024
has a *"Named prior art — and what upstream actually says"* section, which is a comparison made once
inside a record rather than a pinned row that survives an upstream move.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — a rect arrives in the consumer's own coordinates, which cannot know where pairs are, so the rule
  is applied where the rect meets the cells (#454). ADR-0024 carries the amendment recording why it
  is **not** one of R1-R6
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the underline-colour group's count is still `u16` while its two sibling per-span groups went
  `u32` in #621. Measured unreachable after #582 rather than fixed, which is a different state from
  bounded

## Blast radius

- [marker](marker.md) — every decoration is anchored to one, joined by `MarkerId`; marker lifetime
  and disposal decide what the registry must reconcile
- [frame](frame.md) — consumes two overlay groups, and R5 is the reason the absolute one exists
- [viewport](viewport.md) — the ruler is buffer-relative, dividing by `scrollback_len + rows`
- [cell compositing](cell-compositing.md) — colour overrides enter the ADR-0019 layer stack there, and the
  precedence rules above decide what reaches it
- [search](search.md) — **the overview ruler has a second mark source since #440.** Search matches are
  projected to marks beside the decoration ones and joined by a single library function, so R3's total
  order now spans two territories: a change to either projection's emission order changes what the
  other one appears under

## Known holes / open

- **The governing record may still be a proposal while the model ships.** Read ADR-0024's
  `Status:` line rather than trusting this sentence — but if it is still `proposed`, the
  strongest statement available about this territory is a proposal, and that is worth knowing
  before treating R1–R6 as settled.
- **No pinned reference comparison for the model itself.** #500 filled §Reference behaviour for the
  *ruler*, not for R1–R6, so the *declared divergence* (R4, `anchor` moving the colour span) is still
  argued only inside the record with nothing re-checking it. Worth knowing what #500 found while
  filling the neighbouring rows: xterm is the only reference with a marked ruler at all, so a
  divergence there is not a minority position — it is the only position.
- **The consumer half is spread across two crates and mapped by neither.** `justerm-web` and
  `justerm-renderer` have no territories yet, so this note names files in areas the map does not
  cover.
