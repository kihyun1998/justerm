# Territory — multi-viewport

## What it is

One WebGL2 context drawing **N terminal grids** as viewports on one canvas, instead of one context
per terminal. A tabbed or split-pane host needs many grids on screen; browsers cap live WebGL
contexts, and each carries its own atlas and program. In the renderer this is the three-tier split
(global / per font configuration / per grid), the grid registry and the configuration registry, and
the draw loop. In the widget it is `TerminalSurface` — one canvas, one context, N attached
terminals — and `applyGrid`, the one site a terminal is placed on it.

## Governing decisions

- [ADR-0021 — one WebGL2 context draws N terminal grids as viewports](../../adr/0021-single-context-multi-viewport.md)
  — the direction, the `TerminalSurface` shape, the three-tier resource split (D2), the rule for
  assigning a resource to a tier (D1), the consumer's ownership of every device-px quantity (D3).
  The build plan was epic #287 (slices #769–#776).
- [ADR-0027 — liveness is answered by the source that owns it](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  governs what registration and the setters do while the context is lost
  ([GL context lifecycle](gl-context-lifecycle.md)).

## Design model

Status: ADR-0021 decides the shape; the rules below are from the source, its host tests and the
browser proofs. This note began as a record with no code — the inverse of every other note — until
#769 gave it its first code; every slice of #287 is built (tier split #769, grid registry #770, draw
loop #771, per-configuration atlas registry #772, per-grid contract #773, surface-wide context loss
#774, `TerminalSurface` #775, the consumer-side proof #776).

**Tiers and keys.**

- **A renderer holds no terminal until the consumer registers one** (#773). Until then it carried an
  implicit grid drawn over the whole buffer, so a surface with two terminals held three grids and the
  third painted under both. `GridRegistry::new` and `ConfigRegistry::new` start empty, every per-grid
  export names its grid, and the last rect with a producer other than the consumer's box is gone.
- **Resources sort into three tiers**: **global** (per context — program, shared quad buffer, uniform
  locations: `GlobalTier`), **per configuration** (atlas, rasteriser, glyph cache and the cell derived
  from them: `ConfigTier`), **per grid** (one terminal: `GridTier`, including its VAO and instance
  buffer). The record's contribution is the assignment rule, adjudicated in #768: a tier answers the
  *selector* question (whose state decides it — per grid whenever a consumer can set it per terminal)
  and the *resource* question (per configuration only when one instance serves two grids and
  rebuilding repays keying). An earlier version of this note named the tiers "per-context /
  per-surface / per-frame"; the middle axis is a font configuration, not a surface (#768). #769
  moved 309 field accesses into the tiers with **one** cross-tier compile error — evidence that the
  split matched the structure already there; the tier structs are the marking a comment could not be.
  Four operations genuinely span tiers — `apply_frame` and `repack_from_grid` reach
  `resolve_and_pack`, `set_letter_spacing` and `set_line_height` reach `adopt_spacing` — because
  packing and cell derivation need the configuration.
- **The middle tier is keyed by the seven consumer selectors and not by the DPR** (#772; four
  selectors then, six with the weights #928, seven with subpixel #961). `ConfigKey::new` takes family,
  size, both weights, letter spacing, line height and subpixel. One canvas has one
  `devicePixelRatio`, so a component every key shares separates nothing and would make every key wrong
  the moment it changed. A density change is the **rebuild-all** path (`rebuild_all_configs`), the
  one case where editing shared entries in place is right, and it is atomic across the registry.
  ADR-0021's prose said otherwise and ghostty hashes its density; the divergence is recorded in the
  ADR. The `f32` selectors are stored as bit patterns with `-0.0` normalised to `0.0`, or two equal
  floats would key two byte-identical atlases. A non-finite selector cannot arrive (`set_font_size`
  ignores it, `set_letter_spacing` maps it to `0.0`, `set_line_height` to `1`); one that did would key
  its own entry, not corrupt a shared one.
- **A shared entry is never mutated to serve one grid** — ghostty's shared grid is immutable for the
  same reason. A selector change joins a different entry; in-place mutation is for changes true of
  every entry (a DPR change, a restore). An entry is destroyed when its refcount reaches zero, as
  ghostty's `deref` does, rather than pooled: a closed terminal should not hold an atlas open.
- **"Surface" means the opposite in the references.** ADR-0021's `TerminalSurface` is one per app
  (canvas, context, atlas registry, frame loop); ghostty's `Surface` is one per terminal with its own
  renderer, atlas texture and render thread. A sentence borrowed with the word intact inverts. For a
  per-terminal thing here, say **grid**.

**The registries** (`registry.rs`, `config_registry.rs`, both pure and generic over the payload so
the GPU-free logic is host-tested).

- **Registered-not-drawn is a state carried by `Option<Viewport>` alone; the rect is not retained.**
  The consumer's adoption design keeps a hidden workspace's grid registered with its viewport cleared
  (penterm's `terminal-single-context-adoption` PRD, decision 3, cited at #770). Ghostty keeps a
  visible flag and retains the rect because its surface owns the OS window producing it; here the
  producer is a DOM box that reads zero while hidden, so a retained rect is a copy that can be wrong
  on the way back.
- **Grid ids are never reused and start at 1.** A `GridId` crosses the boundary as a bare number, so
  a stale handle must fail loudly (`RegistryError::UnknownGrid`), and `0` is what an unassigned JS
  number reads as. The draw loop addresses grids by slot, stable within a frame and never handed out.
  `ConfigId`s are never reused either, though they never cross the boundary.
- **Registration order is stable across removal** (`Vec::remove`, not `swap_remove`), so a pixel proof
  can tell "B drew over A" from "the order changed". `N` is a terminal count; the linear scan stays.
- **`Viewport::gl_rect` owns the y-flip** because the top-origin device-px space is this crate's own
  and a derivation belongs with its producer (`resize_surface` sets `canvas.width`/`height` to the
  grant, #337, so the consumer could compute it — a second site is a second site to keep true).

**The grid registry's exports** (`webgl/grids.rs`).

- **Registration.** `addGrid` registers a grid that is not drawn until `setViewport` places it — the
  consumer's order, since a widget has no rect until it is laid out. It costs one per-grid instance
  buffer **and the VAO that points at it** (`GridBuffers`): `vertex_attrib_pointer` captures the bound
  buffer, so a VAO follows its buffer and cannot be shared; a re-pointer per grid per frame would keep
  a per-grid fact in the global tier, which D2 rejects (#771, settling what #768 left open). **A grid
  names the font it is born into** — the seven selectors are optional trailing arguments (the
  `apply_frame` precedent) — so it joins a sibling's configuration and bakes nothing; six terminals in
  one font hold one atlas. Born at defaults and moved a line later, it would bake an atlas nobody
  asked for once per registration. Size and line height are floored exactly as their setters floor
  them; a failed buffer build releases the configuration reference it just took; a new grid is
  `(0, 0)` and `cols`/`rows` answer 0 rather than a sibling's size. Registering mid-loss: see
  [GL context lifecycle](gl-context-lifecycle.md) — it succeeds silently, because Chromium's
  `createBuffer()` returns a non-null object on a lost context (#770).
- **A configuration change is a move** (`select_config`, reached from `adopt_selectors` on a live
  context and from `restore`'s reconcile). Acquire the new entry, then release the old; re-selecting
  the current key returns before either, and a failed build leaves the grid where it was. The grid
  must re-pack, because its instances address the old entry's slots — ghostty forces a full rebuild on
  the same switch. **The instance count is dropped unconditionally**: `render` survives a failed
  re-pack rather than blanking, and without the drop that survival would draw the old entry's slot ids
  through the new atlas — a wrong glyph, the failure class this repo treats as sacred. For
  `apply_frame`, which keeps no columns to re-pack, the drop is the whole repair.
- **Placement.** `setViewport` stores the rect as measured; a rect with **no area is refused**, since
  it draws nothing and `isGridDrawn` answering `true` would lie. "No rect yet" is `clearViewport`, the
  answer for a box that is still `display: none`. Every grid is placeable, hideable and removable;
  since #773 every rect has one producer and no grid's lifetime belongs to anyone but the consumer.
- **Removal is session close, not hide.** `removeGrid` frees the grid's buffer and VAO and releases
  its configuration reference; closing one of six terminals in one font frees no font machinery.
- **The counters.** `gridCount` and `atlasCount` are registry *state*, not diagnostics — `atlasCount`
  makes sharing observable rather than asserted, as ghostty exposes the same number. `bakes` *is* a
  diagnostic read as a delta beside `packs`: a grid joining an existing configuration must move it by
  zero. It counts **committed** bakes, so a failed part-way rebuild leaves it unchanged and the delta
  stays deterministic. ADR-0021 D5 leaves where diagnostics live to whoever adds the second; `bakes`
  is that second.

**The selector funnel** (`adopt_selectors`, with `setDevicePixelRatio` beside it). All seven selector
setters — size, family, both weights, subpixel, and the two spacings through `adopt_spacing` — go
through one site, which owes three properties: *atomic* (a rasterise can fail; the selectors are put
back so grid and entry agree, #338/#359), *deferred while the context is dead* (the selectors advance
and `restore` re-selects from them, #269/#406; since #772 the cell does not move ahead of the atlas
either), and *it never edits the entry it is leaving*. **Neither the funnel nor a density change
touches the drawing buffer** (since #773): both used to end in `resize(cols, rows)`, but a surface's
size is the consumer's device-px measurement, and re-deriving it would convert through the
renderer's copy of the density, which lags. The consumer re-fits after every selector that moves the
cell — all but the two weights and subpixel.

**The draw loop** (`render`, `draw`, `draw_grid`).

- **Pack once, in `render`, per drawn grid a mutation dirtied** (#421): overlay + decorations +
  `apply_damage` mark the grid dirty three times and pack once. A pack error leaves the flag set and
  is **held**: the frame still draws, the error surfaces after.
- **A grid with no viewport is not packed either** (#771). Measured at #770 (120×40, release wasm,
  headless SwiftShader and a real NVIDIA/D3D11 browser within ~10 %): scatter ≈0.04 ms, pack + upload
  ≈0.33 ms, draw ≈0.003 ms — an ungated hidden grid ≈0.4 ms per frame, ten at 60 fps ≈4 ms of a
  16.7 ms budget. An identical frame and an all-changed frame cost the same, so the cost is wasm CPU,
  not upload. The dirty flag stays set while hidden, so the first render after placement packs once.
  ghostty gates the CPU rebuild as well as the paint, alacritty only the paint; #771 took ghostty's
  amount because the gate fell out of the loop for free.
- **The slot pin: one pin set spans the whole loop** (#772). `FramePins` is a renderer field because it
  models a scope with one owner — `render` clears it once for every grid it packs, `apply_frame` for
  its own pack. Without it, a second grid's pack repoints slots the first committed earlier in the
  frame, and the first is not re-diffed because its floats did not change: stably wrong. With it the
  second pack is refused (`FrameExceedsCapacity`, extended to the union).
- **The middle tier's hazard went LIVE at #771 — why it converges, and where it cannot.** There was
  always one shared glyph cache, but until #771 only the default grid could pack into it; a second
  grid packing made [glyph atlas](glyph-atlas.md)'s within-frame eviction a cross-grid event, one
  slice before #772's guarantee. A grid now also re-packs when its configuration's eviction count
  moved since it packed (`packed_at_evictions`) — the upload diff cannot see a slot a sibling
  repointed ([GPU upload](gpu-upload.md)). An eviction happens only once a region is full, which
  splits the cross-grid hazard (ADR-0021 named three candidate guarantees; #772 took two). If the
  grids' live glyph sets fit a region together, the eviction-count re-pack converges: the re-packed
  glyphs become most-recently-used and the next eviction takes a dead slot —
  `demo/per-config-atlas.html` drives 4 glyphs on one grid then 1953 on a sibling (the normal
  region's dynamic capacity, `NORMAL_CAPACITY` minus the ASCII prebake), and the first grid repairs
  itself; with the comparison off, ink over those cells goes 207 → 421. If they do not fit, the pin
  refuses rather than draws wrong. Two grids at 1200 live glyphs each, region 1953: before the pin,
  one drew 911 lit subpixels beside its sibling and 891 alone — stably wrong, every proof green;
  after, 891 every frame with the refusal reported, the alone-control in the same run the only thing
  that tells them apart.
- **A refused pack dirties every drawn grid on its configuration, so the refusal is a fixed point.**
  The pin covers only grids that pack this frame; without the dirtying, the refused grid's sibling
  goes clean, leaves the pins empty, and the refused grid succeeds by repointing its slots — measured,
  alternating 891 / 911 with the error on alternate frames. With it the grid registered first wins
  every frame. The extra packing is bounded by the overflow.
- **The full-buffer clear is to transparent.** Grids need not tile (every terminal is an overlay on
  one plane), so the area between rects belongs to the page; painting a terminal's colour there would
  decide a background nobody gave. three.js's multiple-views example does no full clear because its
  views tile — a silence, not a divergence.
- **Each grid draws under its own scissor**, because `clear` ignores the viewport: without it each
  grid's background clear would wipe the buffer. The projection is sized to the rect
  (`gl.viewport` already maps clip space onto it). Every uniform is set per draw, `u_cell_uv`
  included since #772, because two grids in two fonts draw through two atlases in one frame. The
  grid's own clear uses its `bg_alpha` (#298); a rect of `cols * cellWidth(grid)` leaves no uncovered
  area, and since #773 that arithmetic is the consumer's (#331).
- **Draw order and z-order are registration order, and a grid replaces rather than blends** where
  rects overlap, since each pass opens with a clear (#771). A translucent grid shows the page, never
  the grid under it. Tiling panes never overlap.

**The widget** (`justerm-web`).

- **`TerminalSurface` owns what one canvas has once** (#775): the grid registry, the one presenting
  loop (`requestRender`), the density watcher, the restore listener and the loss relay. It is
  constructible under vitest through `SurfaceDeps`. `SurfaceBackend` is the surface-scoped half of
  the renderer backend, and the split follows the renderer's own signatures: at HEAD its 14 members
  name no grid except `removeGrid`, and `RendererBackend`'s 30 per-grid members all take one (the
  #775 split measured 11 and 25). `JustermRenderer.create` composes a surface and claims sole tenancy;
  `attach` joins a host's surface and takes its rect from `setViewportRect`.
- **Registration hands back a `GridLease`, not an id** (#805). The renderer's ids are numbers because
  they cross the wasm boundary, safe only if a stale handle fails loudly; the web layer had been
  silently ignoring ids it no longer held in three methods. A lease makes the question
  unrepresentable, the idiom every other registration in the package uses.
- **`applyGrid` is the widget's one placement site** (`justerm-renderer.ts`). It is what
  `backend.resize(cols, rows)` was before renderer 0.15.0, assembled in the widget because a buffer
  shared by N grids in M configurations has no cell to be a multiple of. Seven re-placing paths reach
  it — `onReapply` (a density change or a restore), `setFontSize`, `setFontFamily`,
  `setLetterSpacing`, `setLineHeight`, `resize`, `setViewportRect` — plus `hide`/`show` through
  `setHidden`.
  - It asks for `cols * cell_width(grid)` device px, #331's exactness, and only as a sole tenant
    (`composedSurface`, #775); a shared tenant leaves the buffer to the host.
  - It reads the grant back and shrinks the grid to it (#339), so `terminalSize()` means the grid
    actually adopted; without it the cells past the edge are scissored away while `terminalSize()`
    still reports them. On a lost context `cssWidth()` is the committed request and nothing shrinks.
    **It protects a sole tenant only**: `cssWidth()` is the whole canvas while `cols` is one tenant's
    share — a 450 CSS-px pane at an 8 px cell fits 56 columns where this computes 112 (#775). A shared
    host reads the grant from `TerminalSurface.cssSize()`.
  - A hidden terminal is placed nowhere (#801), enforced here because all paths meet here;
    `resizeGrid` still runs, so coming back stays a placement. The rect is re-issued on every call.
  - It presents, because a placement change is a change on screen: on an idle two-terminal page,
    hiding a pane left its pixels and showing one left it unpainted until something unrelated
    presented. Found by reading the compositor, not the buffer — every probe presents before it reads,
    so a `readPixels` assertion is blind here. A sole tenant presents at once; a shared tenant
    coalesces, so N re-placements in one handler cost one present.
  - `reapplySurface` is a no-op before the first `resize`: a grid born `0`×`0` would floor to one cell.
  - The rect's extent is never a parameter: it is `cols * cell` by `rows * cell`, and a measured box
    would reintroduce the rounding #331 removed. A density change invalidates a rect whose box did not
    move (ADR-0021 D3); scaling it by the widget's copy of the density is the conversion the renderer
    refuses, so the host re-supplies it on `onDensityChange`, which a restore that adopts a density
    also fires.
  - `show` exists for the sole tenant, whose docs tell it never to call `setViewportRect`; without it
    `create` could enter the hidden state and leave only through a forbidden call. `setHidden`
    re-syncs the text-blink phase and the cursor on the true → false edge (they drift while hidden),
    keyed on the edge because `setViewportRect` passes through on every ancestor scroll. `isDrawn`
    asks the registry, because a never-sized grid is registered and not drawn.
- **Hidden-ness is state the widget consults, never a command it issues once** (#801). A `hide()`
  that merely called `clearViewport` would be undone by the first of the seven re-placing paths to
  fire, two of which carry no consumer call. It passed the browser proof until the proof's middle
  step changed from a density change — which went through the demo page's own handler, re-read the
  missing box and re-hid, agreeing with the wrong implementation — to a bare `resize`.
- **`viewportOrigin` answers `{x, y} | undefined`** (#801). Before, a `display: none` overlay's
  all-zero box was clamped to `{0, 0}` and, since the extent comes from `cols * cell`, a full-size
  grid was re-placed on the canvas corner over a sibling that kept reporting healthy, nothing thrown.
  The renderer's no-area guard is not on that path and cannot be. The union makes the caller decide —
  the same unrepresentable-rather-than-guarded move as `GridLease`.
- **A grid leaving the draw set of a living surface owes a present, by whichever door it leaves**
  (#939). The surface loop is the only presenter and the buffer keeps the last frame, so a grid that
  stops being drawn stayed on screen until a sibling rendered — forever if it was the last.
  `hide()` always presented; the lease's release asks for a coalesced `requestRender` after
  `removeGrid` since #939. An ended surface presents nothing: `dispose()` latches `disposed` before
  releasing and `requestRender` returns on it, so the last frame stays on the canvas the host still
  holds. #775's browser test never saw the defect because its probe presented before every read; the
  #939 test stops the timers and reads a composited screenshot with no present of its own.
- **PenTerm is a shared-surface host.** It opens one `TerminalSurface` for every Native pane, attaches
  each pane's renderer, places each grid with `observeViewportRect` and calls `hide()` when the
  overlay's box has no area, and on `onDensityChange` re-sizes the surface and re-places every pane
  (`penterm 7dadc3c95 src/blocks/terminal/lib/nativeSurface.ts — open`, `placeNativePane`,
  `attachNativeRenderer`).
- **Input bound.** Grid count is the consumer's and unbounded by the renderer; configurations are at
  most one per registered grid (each entry is released at refcount zero). Measured at #770 (dev
  build, headless Chromium, dpr 1): a populated grid costs ≤ ≈171 B/cell of wasm heap (an upper
  bound — the heap never shrinks and carries each frame's ~18 B/cell transient copy), so 80×24 ≈320
  KiB and 120×40 ≈800 KiB; an empty registered grid ≈3 KiB ±1 KiB, dominated by its 256-colour
  palette (64 registrations moved the heap by exactly three 64 KiB pages). Ten hidden 120×40
  terminals are ≈8 MB, the price of showing one being a placement rather than a rebuild. A rect is
  i32 device px; one past the buffer is clipped by the scissor, and one with no area is refused.

## Code

- `justerm-renderer/src/webgl.rs` — `JustermRenderer` (`global`, `configs`, `grids`, `pins`,
  `bake_count`), `GlobalTier`, `ConfigTier`, `GridTier` (`packed_at_evictions`, `needs_repack`),
  `GridBuffers`
- `justerm-renderer/src/registry.rs` — `GridRegistry`, `GridId`, `Viewport::gl_rect`,
  `RegistryError`
- `justerm-renderer/src/config_registry.rs` — `ConfigRegistry` (`find`, `retain`, `release`,
  `ids_wanted_by`), `ConfigKey`, `ConfigId`
- `justerm-renderer/src/webgl/grids.rs` — `JustermRenderer::add_grid`, `remove_grid`,
  `set_viewport`, `clear_viewport`, `grid_count`, `atlas_count`, `bakes`, `is_grid_drawn`,
  `key_of`, `acquire_config`, `release_config`, `select_config`
- `justerm-renderer/src/webgl/font.rs` — `JustermRenderer::adopt_selectors`, `adopt_spacing`,
  `set_device_pixel_ratio`, `rebuild_all_configs`
- `justerm-renderer/src/webgl/draw.rs` — `JustermRenderer::render`, `draw`, `draw_grid`
- `justerm-renderer/src/glyph_resolve.rs` — `FramePins`, `ResolveError`
- `justerm-web/src/terminal-surface.ts` — `TerminalSurface` (`addGrid`, `requestRender`,
  `present`, `cssSize`, `atlasCount`, `bakes`, `onDensityChange`, `dispose`), `SurfaceBackend`,
  `SurfaceDeps`, `GridLease`, `viewportOrigin`, `observeViewportRect`
- `justerm-web/src/justerm-renderer.ts` — `RendererBackend`, `applyGrid`, `reapplySurface`,
  `setViewportRect`, `hide`, `show`, `setHidden`, `isDrawn`

Browser proofs: `justerm-renderer/demo/grid-registry.html` (registry behaviour; where the resident
memory was measured), `multi-viewport.html` (three grids in three rects, the y-flip, a single-grid
control in the same run, #771), `per-config-atlas.html` (sharing and release as counts, a sibling's
rect byte-compared across another grid's font change, two configurations side by side, the eviction
and pin measurements, a restore with two drawn grids, #772), `per-grid-state.html` (differential:
set one grid, byte-compare the other's whole rect, #773), `context-loss-grids.html` (#774);
`justerm-web/demo/shared-surface.html` · `shared-surface.ts` · `e2e/shared-surface.spec.ts` (#776: two
terminals at two font sizes on one canvas, the GL viewport asserted where the DOM overlay is; the
transparent gutter is what separates two rects from one grid spanning both). Before #776 nothing
outside `justerm-web/src` called `TerminalSurface.open`, `attach`, `observeViewportRect` or
`onDensityChange`.

## Reference behaviour

- [Multi-viewport resource tiering](../../agents/reference-facts.md#multi-viewport-resource-tiering--how-the-one-reference-that-shares-font-machinery-splits-it-768-verified-2026-08-18)
  — ghostty's split and its immutable shared grid; the source of the tier rule and of the DPR-in-key
  divergence.
- [A terminal registry, and what "registered but not drawn" is made of](../../agents/reference-facts.md#a-terminal-registry-and-what-registered-but-not-drawn-is-made-of-770-verified-2026-08-19)
  — ghostty's `flags.visible` and retained rect; ghostty gates the CPU rebuild, alacritty only the
  paint.
- [Drawing N views on one canvas](../../agents/reference-facts.md#drawing-n-views-on-one-canvas--the-mechanism-reference-and-the-two-places-it-does-not-reach-771-verified-2026-08-19)
  — three.js's clear / viewport / scissor / per-rect clear; the y-flip and full clear are ours.
- [Sharing font machinery between terminals](../../agents/reference-facts.md#sharing-font-machinery-between-terminals--how-the-one-reference-that-does-it-refcounts-keys-and-invalidates-772-verified-2026-08-19)
  — refcount, key and full rebuild on a configuration switch.
- [Recovering a context loss when the resource is shared](../../agents/reference-facts.md#recovering-a-context-loss-when-the-resource-is-shared-between-terminals-774-verified-2026-08-20)
  — no reference loses one context across N terminals.

Not in the store, attributed: xterm.js holds hidden-ness as `_isPaused` consulted in `refreshRows`
(`src/browser/services/RenderService.ts:156-160`, #801). ADR-0021's named prior art also includes
WezTerm's glyph cache (unpinned, so unverifiable from here) and `virtual-webgl` as a counter-example
(multiplexing contexts underneath rather than sharing one).

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — found
  here (#801): a `display: none` overlay reports zeros, which re-placed a full-size grid on the canvas
  corner over a sibling. All four sites are repaired (#801, #810, #814, #819). The bound that comes
  with keying on the box (measured 2026-08-26 on `demo/shared-surface.html`, reading pane B's pixel):
  `display: none` clears it (`183,224,198` → `27,42,74`), but `content-visibility: hidden` leaves it
  byte-identical, a closed `<details>` ancestor too (full box, `checkVisibility()` false), and
  `visibility: hidden` (#801); `[hidden]`, a closed `<dialog>` and a detached node do zero the box.
  These are truthful measurements, not recurrences — which is why `hide()` must be called by hand. The
  predicate (`width <= 0 || height <= 0`) is the rule; a list would go stale.
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md) —
  the rule's second clause lives here: a surface is shared by construction, so a terminal handed one
  must not end it and a terminal that composed one must. One level down, a terminal releases its own
  grid and the surface releases every grid still registered when it ends. Promoted out of
  [widget lifecycle](widget-lifecycle.md) by #775. Measured then: with two terminals attached,
  disposing one left the other's rect byte-identical (ink 2232 → 2232) and the survivor still
  recovered from a real lose/restore with no re-feed.
- [a screen change no frame carries owes its own present](../invariant/an-unframed-screen-change-owes-a-present.md)
  — `applyGrid` presents on every placement (#801) and a released lease requests one (#939): a grid entering or leaving the draw set has no frame of its own

## Blast radius

- [glyph atlas](glyph-atlas.md) — one cache serves every grid on a configuration, so one grid's pack
  can repoint a slot a sibling still addresses; the eviction count and the render-scoped pin are what
  keep that from drawing a wrong glyph.
- [GPU upload](gpu-upload.md) — the baseline and the instance buffer are per grid; one shared buffer
  with N baselines would let one grid's upload silently invalidate another's (ADR-0021 listed
  `instance_vbo` as global until #768).
- [GL context lifecycle](gl-context-lifecycle.md) — one loss takes down every grid and every
  configuration at once, and `restore` predicts by key which configurations survive the reconcile.
- [cell geometry](cell-geometry.md) — the cell is a property of a configuration a grid selects into;
  the DPR stays global and is not part of the key (an earlier version of this edge said it was, a
  ghostty borrowing where a `Surface` is an OS window on its own monitor, #768).
- [widget lifecycle](widget-lifecycle.md) — the canvas, context, density watcher, restore listener
  and loss relay outlive any one terminal; a teardown that ends one of them ends it for every sibling.
- [published surface](published-surface.md) — every per-grid export takes a grid id and throws on an
  unknown one, and every device-px quantity is the consumer's; #773 broke the renderer's API in one
  release (0.15.0) to get there, and a further move of a member between `SurfaceBackend` and
  `RendererBackend` is a break on the web side.
- [fit](fit.md) — `applyGrid`'s read-back clamp protects only a sole tenant; a shared host's fit has
  to read `cssSize()` itself.

## Known holes / open

- **The per-grid selectors are loose fields that travel together** (#928, recorded by the
  maintainer rather than filed). The seven sit on `GridTier` and are copied field by field in
  `key_of`, the snapshot and rollback in `adopt_selectors`, and `GridTier::new`; #961's subpixel
  setting was one edit at each. Holding one `ConfigKey` would make those one clone each; ADR-0021 D1
  settles that the selectors are per grid, not how they are stored. The positional `addGrid`
  arguments would still move.
- **Where a per-grid grant check should live once a host really tiles is open** (#775): `applyGrid`'s
  clamp cannot fire for a pane smaller than the canvas.
- **CSS hides that do not zero the box leave a pane drawn** (`content-visibility: hidden`, a closed
  `<details>`, `visibility: hidden`); the host must call `hide()` (measured 2026-08-26, above).
- **Part of ADR-0021's prior art is unverifiable from here.** ghostty and three.js are pinned in
  `reference-facts.md`; WezTerm has no local tree, so if it has moved nothing would notice before
  someone builds against it — pin it for the question that needs it.
- **A note can re-enter the "record with no code" state.** The code-first passes could not see this
  territory while it was only ADR-0021, because there was nothing to read.
