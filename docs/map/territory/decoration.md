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

### The packer's half (`frame.rs` `pack_instances`)

- **Decorations compose back-to-front around the highlight** (#120, #393): base < bottom
  decoration < highlight < top decoration — justerm-web's `composeCellColors` order. A decoration
  overrides the bg and/or fg with an **absolute** `0xRRGGBB` (the consumer owns its theme and resolves
  it before pushing), so it is used verbatim: no palette, inverse or bold→bright. A fg override sets
  `fg_overridden`, which the #230 re-dim keys off. bg and fg merge **independently** across every
  decoration covering the cell (#452, xterm's per-property last-wins), one accumulating pass per layer,
  so a bg-only and an fg-only decoration both apply. A bottom decoration that painted a bg (`deco_bg`,
  #444) counts as a real colour beneath the highlight: the selection's blend decision reads it, so the
  decoration is not erased. The top layer paints over the highlight, overriding the effective bg
  and/or fg (`composeCellColors` applies top last, after selection); it is *read* early, before the
  ink rules, because whether it takes the glyph decides what those rules are even for — a pure lookup,
  so reading it early moves nothing.
- **A tile glyph follows a bg-only top decoration** (#494), and this is a **deliberate divergence
  from xterm**, which is unanimous the other way (the #494 two-lens corrected an earlier, wrong reading
  — do not restore it). The cell paints solid in the decoration's colour instead of the glyph occluding
  the layer above it. It follows from #495's rule for the same classifier — a tile glyph is
  background-shaped ink, not text — and without it the cell is self-contradictory: `bg` is the
  decoration's while `fg` stays the selection's, so the layer *above* the selection loses the glyph area
  to it. Both of xterm's cell renderers let the glyph paint over a bg-only decoration: the webgl addon
  (`CellColorResolver.ts:178-187`, after the selection stage, `$hasBg` and `$hasFg` independent) and
  the DOM renderer (`DomRendererRowFactory.ts:357-408`). xterm's decoration *elements*
  (`css/xterm.css:194-201`, z-index 6 / 7 over the screen) can cover a glyph, but xterm never styles
  them — a consumer does, in `onRender`, and they are registered for every renderer
  (`CoreBrowserTerminal.ts:617`) — so they are a separate feature justerm has no equivalent of, not a
  second xterm answer. That is settled: ADR-0024 R1 states "colours + a mark, not an object" without a
  condition (#502, 2026-08-18). What xterm leaves undefined is only the interaction: `layer` is
  documented purely against the selection (`typings/xterm.d.ts:688-692`, whose `*` footnote has no
  text), never against glyphs.
  **The precedent is weaker than it looks.** xterm's tile re-tint under selection *blends* 50% from the
  cell's own fg (`CellColorResolver.ts:168-171`), leaving the tile distinguishable, and flattens to the
  band only for an inverse + Default-bg cell it calls *transparent* (`:139`). So "the tile participates
  in what is painted over it" is xterm's; "the tile is replaced by it" is justerm's own step — taken
  because a decoration bg *replaces* the background (`eff_bg = c`) where a selection washes over it.
  The same reason makes it flat rather than blended. xterm's DOM renderer resolves the contradiction a
  third way, rejected knowingly: `DomRendererRowFactory.ts:399-408` applies decorations before
  selection and paints the selection only `if (!isTop && isInSelection)`, so a top decoration suppresses
  the selection outright — dropping a highlight the user made, and splitting justerm's fg-channel model
  (which follows webgl deliberately, #430) across two features.
  Only a **bg-only** decoration means "this whole cell is background now": one that also sets `fg` keeps
  the art in the consumer's colour (the escape hatch), and a bottom decoration is untouched — "bottom"
  means *under* the glyph, so an opaque tile occluding it is correct (a transparent one lets it
  through, #453).
- **The decoration takes the glyph by dropping its slot, not by recolouring** (#508, fixed). It used
  to set `fg = bg`, which erased everything else the shader draws in the foreground — the underline and
  strikethrough and the visible half of a blink phase — none of which is the glyph. ADR-0019 rule 4 puts
  `I_line` and `I_cursor` on the TEXT side unconditionally, so the slot is blanked instead and the ink
  channel is left holding the cell's own ink for the line, stating the rule structurally rather than
  by a colour coincidence. `glyph_taken_by_decoration` carries it to the glyph field and stands every R1
  ink rule down — and the TEXT-class ones must *not* stand down, since the line is TEXT class; the attribute bits stay, since only the glyph was taken (unlike `ESC[8m`, which hides
  the whole cell). Blink is deliberately not restored: a dropped glyph has nothing to blink, which is
  rule 5 working. The #494 assignment sits in an `else` on purpose: written as its own guarded `if`
  (`exclude && top.fg.is_some()`) it was behaviourally dead — the branch above re-applied `top.fg`
  immediately, so no mutation of the guard could turn a test red (the two-lens caught this: dropping the
  guard left the suite green). As an `else` it is the only thing that keeps a both-channel decoration's
  art, and `a_top_decoration_setting_both_channels_keeps_the_tile_glyph` discriminates it.
  A dim tile survives either way, and the reason is arithmetic, not a flag: both dim paths resolve
  `dim_foreground(c, c)`, and `blend_over(c, c, DIM_BLEND_ALPHA)` adds `round(0)` per channel — exactly
  identity. (`fg_overridden` is not what protects it: a bottom fg-only decoration on the same cell
  already set it, so the `dim && fg_overridden` arm can run anyway — the two-lens caught that
  mis-stated reason.) Pinned by `a_dim_tile_following_a_top_decoration_is_not_dimmed_away_from_the_bg`.
- **The same visual concept routes differently by authorship, and that is the rule** (#494's AC,
  ADR-0019 rule 5). justerm's own active search match is an overlay *kind* (#427, #430), and a tile
  under it keeps the raw selection colour (pinned by
  `an_inverse_default_bg_tile_on_an_active_matched_selected_cell_uses_the_raw_selection_colour`); the
  same concept pushed as a bg-only top decoration goes solid. The two layers have the same shape —
  above the selection, a bg and no fg — so paint mode cannot tell them apart; authorship can. A
  decoration is the application declaring "this cell is now this colour", knowing what it covered;
  the active match is the user stepping through results, and erasing box-drawing and shading as they
  cycle is content loss — the tile is often the only thing drawing a table border or a progress bar.
  Do not "unify" the routes (#511, closed won't-do; the seam it named is real and accepted).
  The cost lands on a consumer porting xterm's decoration-based search: that addon marks the active
  match `layer: 'top'` with a background and every other match `'bottom'`
  (`addon-search/DecorationManager.ts: 134-144`), so a box-drawing or Powerline cell loses its glyph
  on the active hit and keeps it on the others — the glyph blinks as the user cycles — and the escape
  hatch is out of reach, since `ISearchDecorationOptions` (`addon-search.d.ts:46-76`) has no
  foreground field (#506, closed as not currently real for justerm itself).

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

- ~~**The governing record may still be a proposal while the model ships.**~~ **Closed 2026-08-18
  (#502)** — check ADR-0024's `Status:` line, which is where the answer lives and is why this bullet
  never went stale. Kept rather than deleted for the mechanism, which recurs: the record lagged its
  siblings by four weeks because R1 held an open question *inside the rule* (`until it is answered,
  "no object" is the model`), and a record cannot be adjudicated while one of its rules is
  conditional. That is the shape to recognise, not this instance.
- **No pinned reference comparison for the model itself.** #500 filled §Reference behaviour for the
  *ruler*, not for R1–R6, so the *declared divergence* (R4, `anchor` moving the colour span) is still
  argued only inside the record with nothing re-checking it. Worth knowing what #500 found while
  filling the neighbouring rows: xterm is the only reference with a marked ruler at all, so a
  divergence there is not a minority position — it is the only position.
- **The consumer half is spread across two crates and mapped by neither.** `justerm-web` and
  `justerm-renderer` have no territories yet, so this note names files in areas the map does not
  cover.
