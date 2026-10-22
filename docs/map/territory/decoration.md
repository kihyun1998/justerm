# Territory — decoration

## What it is

What a consumer *paints* at a [marker](marker.md): per-cell colour overrides on the grid and at most
one mark per covered line on the overview ruler. Consumer-side by identity — the engine anchors and
knows nothing about appearance, because appearance needs a theme and the engine is theme-agnostic. The
territory spans three places: `justerm-web` projects decorations to rects and ruler marks
(`DecorationRegistry`, kept anchored by the pulled `MarkerIndexCache`), the `Scrollbar` draws the ruler,
and `justerm-renderer` composes the rects into the cell (`decoration.rs`, `pack_instances`).

Only the demo wires it today; the one consumer, PenTerm, registers no decorations and builds no marker
index (penterm 7dadc3c95 `src/blocks/terminal/lib/nativeTerminalCache.ts` — `build`).

## Governing decisions

- [**ADR-0024 — decoration projection and precedence**](../../adr/0024-decoration-projection-and-precedence.md)
  — R1 through R6 and R3.1, the whole model. Its `Status:` line is authoritative and is not copied here
- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) — decoration colours
  enter the renderer's layer stack under this model; 0024 places itself on the axis ADR-0019 put out
  of its own scope
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why this is consumer-side at all, and why precedence follows the consumer's registration order
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — the absolute-line marker group was the violation it recorded against itself, paid off by v16 (#490)

## Design model

ADR-0024 is authoritative; the R-rules below are routing. **If they disagree, the ADR is right.**

- **R1 — a decoration is colours + a mark, not an object.** It projects to per-cell colour overrides
  and at most one ruler mark per covered line. Borders, outlines, opacity, classes and transitions have
  no expression by construction. Settled without a condition on acceptance (#502, 2026-08-18).
- **R2 — cell precedence is registration order, across markers.** `decorationsForFrame` walks
  decorations in registration order (#458) and the renderer resolves per property last-in-wire-order,
  so no anchor position can perturb it — stronger than xterm, whose per-line buckets re-append a
  decoration spanning an insert/delete point.
- **R3 — ruler order is position class first, registration order second**, so the ruler order is not
  the cell order. **R3.1** (#440): a second mark source — search matches — takes a fixed rank within
  its class after the decorations; `composeRulerMarks` is the one function that merges the two into the
  paint order `Scrollbar.setMarks` consumes.
- **R4 — `anchor` moves the colour span** (`'right'` measures `x` from the right edge). A declared
  divergence: xterm's colour path ignores `anchor` (only its DOM element honours it), so with no element
  ignoring it would leave a dead field (#459).
- **R5 — projection is per visible row, not per anchor visibility.** A decoration anchored above the
  viewport still projects its on-screen rows. This is why the frame once carried every marker's absolute
  line.
- **R6 — a projection that cannot be computed emits nothing.** A **non-finite** input yields no rect and
  no mark (the browser drops `top: NaN%` and stacks marks at the top edge); an **out-of-range** one is
  *clamped* — it has a value, it is merely off the track. The clause said "non-finite or out-of-range"
  until #500 amended it.

**Absolute anchors come from a pulled index; the frame carries only viewport rows** (#490, wire v16).
`DecorationRegistry.setMarkerIndex` takes a `MarkerLineSource` (one method, `lineOf`), which
`MarkerIndexCache` satisfies — the parameter is the capability, so a consumer that already tracks lines
need not adopt the cache. The cache pulls `Engine::marker_index` once over `MarkerPort` and keeps it
current from the frame's `evictedTotal`/`markerEpoch` basis plus marker create/dispose events.
- *Merged per marker; the absolute line wins* (#461). The cell projection takes the index's line where it
  has one — the only source that can place an anchor above the top — and falls back to the frame's
  `markerPositions` row otherwise; the ruler has the index alone. A lagging index does not compete:
  `lineOf` returns `undefined` while `adopted !== seen`, so a stale line is never served.
- *Unknown means do not project*, never line 0: a missing decoration self-corrects, one painted on a line
  it no longer owns does not.
- *Two checks keep the index honest*, both because create/dispose events are `O(1)` and do not move the
  epoch: the frame's `markerCount` is compared with the index's size every frame (a host that wired the
  pull and not the events drifts one frame, not forever); and a v16 frame with ruler decorations and no
  index warns once (`rulerMarksForFrame`), keyed on `markerCount` because "no marks this frame" is also
  true of a host with no live markers.
- *The outage is bounded by state, not by an edge* (#746). The trigger that ended an outage once asked
  whether the epoch had just *changed*, so a pull landing one generation behind the newest frame ended it
  nowhere and the index stayed unusable for good — reached by an ordinary drag-resize once the query
  round trip approaches the resize cadence (measured, RTT ≈ 100 ms against the 100 ms `FitController`
  debounce: 8 drags in 40). The retry now asks `adopted !== seen && !inFlight && !lastAttemptFailed`; a
  *refused* transport is deliberately not retried (the host's policy, not this class's).
- *The per-frame `O(M)` scan left with the group*: both reads are sized by decorations — the viewport
  group is row-filtered and `lineOf` is `O(1)` (#482, #490).
- *Clipped here because the wire cannot say "out of range"* (#457, #461, #462). Columns and rows cross as
  `u32` (`decorationWire`), so a negative column would wrap to ~4.29e9 and a non-finite row would loop
  forever; `decorationsForFrame` clips columns to the viewport, clamps the start row to the top, drops a
  non-finite anchor, and caps the span at the last viewport row.

**Outside input and its bounds.** Decorations are the consumer's own registrations and nothing bounds
their count; each projects at most `rows` rects per frame. Marker ids and lines come from core, bounded
by `MAX_MARKERS` (`u16::MAX`), which also bounds the index the cache holds. The cache's queue of marker
events during an in-flight pull (`pendingOps`) is bounded only by the events of one round trip.

### The packer's half (`frame.rs` `pack_instances`)

- **Decorations compose back-to-front around the highlight** (#120, #393): base < bottom decoration <
  highlight < top decoration. A decoration colour is an absolute `0xRRGGBB` the consumer resolved, used
  verbatim — no palette, inverse or bold→bright. bg and fg merge **independently** across every covering
  decoration (#452, xterm's per-property last-wins). A fg override sets `fg_overridden`, which the #230
  re-dim keys off. A bottom decoration's bg (`deco_bg`, #444) counts as a real colour under the highlight,
  so the selection blends over it rather than erasing it. The top layer is *read* before the ink rules,
  because whether it takes the glyph decides what those rules are for (`decoration_override_at`).
- **A span covers a wide pair whole** (#454): `decoration_override_at` takes the cell's `partner`, so a
  rect reaching either half covers both, in its own wire order.
- **A tile glyph follows a bg-only top decoration** (#494) — a **deliberate divergence from xterm**,
  which is unanimous the other way: both its cell renderers let the glyph paint over a bg-only decoration
  (webgl `CellColorResolver.ts:178-187`, DOM `DomRendererRowFactory.ts:357-408`). xterm's decoration
  *elements* can cover a glyph but are consumer-styled DOM, a feature justerm has no equivalent of (R1).
  It follows from #495's rule that a tile glyph is background-shaped ink; without it the cell is
  self-contradictory (bg the decoration's, fg the selection's). **The precedent is weaker than it
  looks**: xterm's tile re-tint under selection *blends* 50% from the cell's fg and flattens only for an
  inverse + Default-bg cell; "the tile is replaced" is justerm's own step, taken because a decoration bg
  *replaces* the background where a selection washes over it. xterm's DOM renderer's third answer — a top
  decoration suppressing the selection — is rejected knowingly (it drops a highlight the user made). Only
  a **bg-only** top decoration means "this whole cell is background": one that also sets `fg` keeps the
  art in the consumer's colour (the escape hatch), and a bottom decoration is untouched.
- **The glyph is taken by dropping its slot, not by recolouring** (#508). Setting `fg = bg` erased the
  underline, strikethrough and blink phase, none of which is the glyph; ADR-0019 rule 4 puts those on
  the TEXT side, so `glyph_taken_by_decoration` blanks the slot, keeps the attribute bits, and leaves the
  ink channel holding the cell's own ink for the line. #226's contrast exclusion does not reach such a
  tile. The #494 assignment sits in an `else` on purpose: as its own guarded `if` it was behaviourally
  dead, and `a_top_decoration_setting_both_channels_keeps_the_tile_glyph` discriminates it. A dim tile no
  longer needs the old arithmetic argument (`dim_foreground(c, c)` being identity): nothing is drawn that
  could seam, and the freed ink channel carries the *dim* line colour
  (`a_dim_tile_taken_by_a_top_decoration_leaves_its_dimmed_ink_for_the_line`).
- **The same visual concept routes differently by authorship, and that is the rule** (#494's AC,
  ADR-0019 rule 5). justerm's own active search match is an overlay *kind* (#427, #430), and a tile under
  it keeps the raw selection colour
  (`an_inverse_default_bg_tile_on_an_active_matched_selected_cell_uses_the_raw_selection_colour`); the
  same concept pushed as a bg-only top decoration goes solid. Paint mode cannot tell them apart;
  authorship can — a decoration declares "this cell is now this colour", while erasing box-drawing as a
  user cycles matches is content loss. Do not unify them (#511, closed won't-do). The cost lands on a
  consumer porting xterm's decoration-based search, which marks the active match `'top'` with a
  background (`addon-search/DecorationManager.ts:134-144`) and has no foreground field to escape with
  (`addon-search.d.ts:46-76`; #506, closed as not real for justerm itself).

## Code

- `justerm-web/src/decorations.ts` — `DecorationRegistry` (`register`, `setMarkerIndex`,
  `decorationsForFrame`, `rulerMarksForFrame`, `onMarkerDisposed`), `MarkerLineSource`,
  `DecorationOptions`, `searchRulerMarks`, `composeRulerMarks`, `decorationWire`
- `justerm-web/src/marker-index.ts` — `MarkerIndexCache` (`sync`, `lineOf`, `reset`), `MarkerPort`
- `justerm-web/src/markers.ts` — `readMarkers`, `MarkerKind`
- `justerm-web/src/scrollbar.ts` — `Scrollbar.setMarks`, the ruler's DOM
- `justerm-web/src/justerm-renderer.ts` — `setDecorationSource`, pushed per frame in `applyFrame`
- `justerm-renderer/src/decoration.rs` — `parse_decorations`, `DecorationRect`,
  `decoration_override_at`, `DecorationLayer`
- `justerm-renderer/src/frame.rs` — `pack_instances`; tests in `justerm-renderer/src/frame/tests.rs`
- `justerm-core/src/serialize.rs` — `MarkerPosition`, `marker_count`, `marker_epoch`, `evicted_total`;
  `justerm-core/src/lib.rs` — `Engine::marker_index`

## Reference behaviour

- [The overview ruler — who has one, how a mark is merged, and how big it is](../../agents/reference-facts.md#the-overview-ruler--who-has-one-how-a-mark-is-merged-and-how-big-it-is-500-verified-2026-08-10)
  — the merge key and threshold, class heights and their device/CSS split, and mark placement. The
  thinnest corpus in that file: alacritty has no scrollbar and ghostty delegates its to a native widget
  that cannot carry marks, so a *marked* ruler is xterm-only — a divergence there is not a minority
  position, it is the only one
- [Search on the overview ruler, and how a match position stays valid](../../agents/reference-facts.md#search-on-the-overview-ruler-and-how-a-match-position-stays-valid-440-verified-2026-08-13)
  — the second mark source R3.1 ranks
- [A span, a highlight and a caret over a wide pair](../../agents/reference-facts.md#a-span-a-highlight-and-a-caret-over-a-wide-pair-454-verified-2026-08-10)
  — why the pair rule is applied where the rect meets the cells

**Still none for R1–R6 themselves.** ADR-0024's *Named prior art* section is a comparison made once
inside a record, not a pinned row that survives an upstream move.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — a rect arrives
  in the consumer's coordinates, which cannot know where pairs are, so the rule is applied where the rect
  meets the cells (#454); ADR-0024 records why it is **not** one of R1–R6
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the underline-colour group's per-span count is still `u16` while its sibling per-span groups went
  `u32` in #621. Measured unreachable after #582 rather than fixed, which is a different state from
  bounded
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — `MarkerIndexCache.reset` is the host's to call, and no consumer builds the cache

## Blast radius

- [marker](marker.md) — every decoration joins a marker by id and assumes a disposed marker's id is
  reported; a marker that dies without an event leaves a decoration projected on a line it no longer owns
- [frame](frame.md) — the projection reads `markerPositions` as viewport rows and `markerCount` as the
  live total; a frame that changes what either counts breaks the drift check silently
- [viewport](viewport.md) — the ruler divides by `scrollbackLen + rows`, and the cell projection's
  `top` is `scrollbackLen − displayOffset`; a change to either scalar's meaning moves every mark
- [cell compositing](cell-compositing.md) — decoration colours enter the ADR-0019 layer stack there;
  a change to the highlight's place in that stack changes what a top or bottom decoration covers
- [search](search.md) — the ruler's second mark source since #440; R3's total order spans both
  projections, so a change to either one's emission order changes what the other appears under
- [wide glyph](wide-glyph.md) — the packer covers a pair through `partner`; a change to how pairs are
  identified changes which cells a one-half rect paints
- [fit](fit.md) — the index's outage was reached at the fit debounce cadence (#746); a faster resize
  path makes the round trip the binding constraint again

## Known holes / open

- **No consumer exercises it.** PenTerm registers no decoration and builds no `MarkerIndexCache`; the
  model has run only in the demo and its tests.
- **No pinned reference comparison for the model itself.** #500 filled the ruler rows, not R1–R6, so the
  declared divergence (R4) is argued only inside the record with nothing re-checking it.
- **A refused marker-index pull is never retried by this package** (#746). The index stays unusable until
  an epoch change or the host acts; how hard to retry someone else's transport is left to the host, and
  no host has said.
- **Gutter ruler marks are fixed at 6 px**, where upstream grows them toward 12 px below about 100
  total lines: `Scrollbar.setMarks` is not given the total line count, so a shallow-buffer ruler is
  thinner here (`scrollbar.ts — rulerMarkHeightPx` doc-comment; adaptive CSS is possible, the data is
  what is missing).

