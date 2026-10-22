# Territory — GL context lifecycle

## What it is

Surviving the browser taking the GPU away. A WebGL context can be lost at any moment — GPU reset,
tab backgrounded, driver eviction — and every GL object it owned dies with it. The browser fires
`webglcontextlost` and *may* later fire `webglcontextrestored`. This territory is the state machine
that decides what the renderer does in between, the rebuild that runs after, the deferral every
GPU-touching entry point applies while the context cannot be used, and the consumer half: the
loss notification, the restore deadline, and what the widget re-derives once the context is back.

## Governing decisions

- [ADR-0027 — a liveness question is answered by the source that owns the answer](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  — when GPU work may be attempted and which predicate each site asks. Its conformance map resolves
  every entry point in this territory and derives the ones still wrong. Promoted from spine #689
  after the rule produced a site — #695 — that nobody had reported. Before it (until 2026-08-03) the
  territory had no record; the anchor was a cheap hypothesis opened at the second rhyming issue,
  not the archaeology that produced other records at cluster sizes of 20 and 9 (#689).
- [ADR-0017 — mechanism in core, policy in the consumer](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  — the restore grace period is consumer policy; the renderer times it.
- [ADR-0021 — one WebGL2 context draws N terminal grids](../../adr/0021-single-context-multi-viewport.md)
  — D3 makes every device-px quantity (surface size, viewport rects) the host's, which is why a
  restore that adopts a new density must tell a shared host
  ([multi-viewport](multi-viewport.md)).
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — owning a GL context is
  this crate's premise; it decides nothing about loss.

## Design model

Status: ADR-0027 records the predicate rule; everything else here is from the source, its host
tests and the browser proofs under `justerm-renderer/demo/context-loss*.html`.

**The state machine.** "May later fire" is the whole difficulty: restoration is not promised, so the
machine cannot be "wait for the event" — it needs a deadline and a way to say recovery is overdue
rather than pending. `ContextState` is pure and host-tested; the event closures and GL recreation
live in the wasm32-only `webgl/context.rs`, which is the split the packer, the upload planner and
the frame adapter use. It holds `is_lost`, `pending_rebuild`, `restore_overdue` and `loss_epoch`;
`action` returns `Skip` / `Rebuild` / `Draw` per frame and `must_defer` answers for setters.

- **The consumer is told, not guessed at.** `isContextLost`, `isRestoreOverdue`, `setOnContextLoss`
  and `setContextRestoreTimeoutMs` are the four exports. All four have a consumer since #579, now on
  `TerminalSurface` (one context per canvas means one loss and one deadline, #775). The one thing
  that took adapting: `setOnContextLoss` takes a `Function`, offers no unset, and clears its slot
  only in `Drop` — which runs at `free()`, a call the widget never makes. So the surface registers
  `ContextLossRelay.notify` once and swaps the consumer's handler behind it. A push channel whose
  only teardown is `Drop` pushes that teardown onto whoever holds it — worth knowing before adding a
  fifth export of that shape. PenTerm registers no loss handler and no timeout; it relies on the
  automatic restore (`penterm 7dadc3c95 src/blocks/terminal/lib/nativeSurface.ts — open`).
- **The listeners hold only the shared state** (#269). The two closures capture the `Rc`'d
  `ContextState` (and the notify and timeout cells), never the renderer, so either can fire while a
  `&mut JustermRenderer` method is on the stack without a `RefCell` double-borrow. The
  `webglcontextlost` closure calls `preventDefault()` first: without it the browser never fires
  `webglcontextrestored`. beamterm's `context_loss.rs` and xterm.js's `WebglRenderer.ts` do the same
  (unpinned). The grace period (`DEFAULT_RESTORE_TIMEOUT_MS`, 3000, xterm.js parity) is read when a
  loss arms its deadline, so a changed timeout applies to the next loss only.
- **The restore deadline is never cancelled, and a loss epoch makes that safe** (#327).
  `clearTimeout` would work, but cancelling means owning the `Closure`, and the consumer's handler is
  exactly where the renderer gets destroyed (VSCode's `onContextLoss` calls
  `_disposeOfWebglRenderer()`); dropping the closure from inside its own body is what Rust, unlike
  JS, does not get away with. So `arm_restore_deadline` hands it to JS with `Closure::once_into_js`,
  which frees it after its single run, and `on_restore_deadline` rejects a deadline whose context came
  back, whose loss already notified, or whose epoch is older. A stale deadline costs one no-op task.
  The state borrow is released and the callback cloned out before calling JS, because the handler is
  re-entrant. `Drop` clears the callback slot, so a pending deadline finds nobody to call — the same
  observable contract as xterm.js's `clearTimeout` on dispose. The epoch guards
  **lost → restored → lost**: the first loss's timer would otherwise land inside the second loss's
  grace period (`a_deadline_left_over_from_a_previous_loss_never_notifies`). It does *not* make
  justerm stricter than xterm.js on a double `webglcontextlost` with no restore between: that
  antecedent is unreachable — two `loseContext()` calls produce exactly one event in headless
  Chromium, because the event fires on the transition into lost (measured 2026-08-04, #579).
- **"Is the context lost" has two answers and they disagree for a whole window — so the predicate
  is chosen per site** (#639). The browser destroys a context synchronously and only queues
  `webglcontextlost`; the mirror holds on the way back. Measured in Chromium right after
  `WEBGL_lose_context.loseContext()`: `gl.isContextLost()` already `true`, `drawingBufferWidth`
  already `0`, the flag still `false`. The rule: a caller that **has** the answer in hand tests it
  (`apply_surface_size` stops on a non-positive read-back, right for any cause of one); a caller that
  must **ask** consults both sources, each covering the window the other misses; `isContextLost`
  reports the flag, because to a consumer it is honest — "has a loss been reported". #639's first
  fix guarded on the flag, went green, and left the defect reachable verbatim.
- **`render` asks both sources, and the pure module is given the one it cannot fetch** (#695,
  ADR-0027 D3/D4). `ContextState::action` takes a `ContextLiveness` read off the context and
  composes it with the flag. Before #695 both arms ran on a dead context in the pre-dispatch window:
  `Rebuild` rebuilt and threw (an empty shader log, `"justerm-renderer: "`), `Draw` packed into a
  dead atlas (`packs()` +1). `demo/context-loss-race.html` asserts no throw and a `packs()` delta of
  0, in a window it first checks exists. The argument rather than a caller-side guard, because
  `webgl.rs` is wasm32-only and invisible to `cargo test`; the cost is that the argument can lie, which
  only the browser proof catches (call site pinned to `Usable`: 326 host tests green, proof red —
  #695). A dead context beats a pending rebuild: GL objects created on one come back invalidated, and
  beamterm's `render_frame` checks the opposite order (unpinned).
- **`gpu_work_must_wait` covered only one of its two windows until #772.** It asked the context and
  `is_lost`, and `on_restored` clears `is_lost` while setting `pending_rebuild` — so between
  `webglcontextrestored` and the rebuild both answered "fine" while the program, VAOs and atlases were
  still the destroyed ones, and a setter baked into resources `restore` replaced a frame later. Its
  doc claimed the window was covered. The composition now lives on the state machine as
  `ContextState::must_defer` (`Dead || is_lost || pending_rebuild`), where ADR-0027 D1 puts it
  (`gpu_work_defers_in_all_three_windows_including_the_one_is_lost_cannot_see`).

**Entry points while the context cannot be used.** Each one either defers, refuses, or pays.

- **Every entry point that changes the geometry takes the request and defers the GPU work.**
  At HEAD these are `setDevicePixelRatio`, the seven per-grid selector setters that funnel through
  `adopt_selectors` (`setFontSize`, `setFontFamily`, `setFontWeight`, `setFontWeightBold` (#928),
  `setSubpixelAntialiasing` (#961), `setLetterSpacing`, `setLineHeight`), and `resizeSurface`. None
  may reject: a consumer can see the loss (#579) but nothing obliges it to check, and a rejecting
  setter breaks every one that does not. Each stores what it was given and `restore` re-derives
  from it — the stored value is the queue. The DPR alone is not stored: `set_device_pixel_ratio`
  drops a mid-loss call, since re-baking commits atlases the dead context hands back invalidated, and
  `restore` re-reads the live `devicePixelRatio` (#269). `resizeSurface` commits the request and
  `apply_surface_size` skips adopting a zero read-back (#639). `resizeGrid` does no GPU work at all
  since #773 split the old `resize`; the grid is no longer derived from the drawing buffer, so the old
  "a zero read-back would floor the grid to one cell" no longer reaches the renderer.
- **A resize during a loss is provisional** (`resizeSurface`, and the widget's `resize`). The
  renderer commits the buffer asked for and defers the read-back; any clamp settles inside `restore`,
  which runs on the next `render`, not when `webglcontextrestored` fires. On a lost context
  `cssWidth()` is the committed request, so the widget's `applyGrid` shrinks nothing; after the
  restore the surface's `onContextRestored` re-derives every tenant and the provisional numbers are
  replaced with no consumer call. Measured on renderer 0.14.x, before anything did that (headless
  Chromium, `MAX_TEXTURE_SIZE` 8192, cell 9 device px, 4000 columns asked mid-loss): during the loss
  `cssWidth()` 36000 and `canvas.style.width` `36000px`; after the restoring `render()` the grid was
  910 and `cssWidth()` 8190 while the box stayed `36000px`, 4.4× too wide. Reachable only when the
  request exceeds the browser's buffer limits.
- **Construction is the one entry point that refuses instead of deferring, and the binding decides
  where** (#688). A constructor has no state machine, nothing to replay at `restore` and no object to
  hand back, so it returns `Err`. The guard sits above `glow::Context::from_webgl2_context`, because
  that enumerates extensions with `get_supported_extensions().unwrap()` and panics on a lost
  context's `null`; the `MAX_TEXTURE_SIZE` read after it is harmless (glow answers `0`, defended by
  `.max(1)`). A panic is also the one failure that leaves the family's error shape — a `RuntimeError`,
  not a bare string. `getContext` succeeding is not liveness: on a canvas whose context is lost it
  returns the same lost object. One check covers the whole constructor, not because a context cannot
  die inside it but because the *report* cannot arrive inside it (`webglcontextlost` dispatches at a
  task boundary) and every later glow call on the path fails cleanly — `create_*` return `Result`,
  `get_uniform_location` an `Option`, status getters `unwrap_or(false)` — so a mid-construction death
  yields a bare-string `Err`. The constructor asks the **context alone**, because a fresh state
  machine reports "live" unconditionally — consulting it there is a constant, not a weaker predicate
  (mutation measured red on `context-loss-construct.html`, #688). For the same reason the loss
  listener, attached before any GL work, cannot catch a loss during construction; an earlier comment
  claimed it could (#269). Since #773 the constructor bakes nothing — both registries start empty.
- **Registration is the one entry point that neither refuses nor defers, and pays instead** (#787).
  `add_grid` reaches `bake_config` (through `acquire_config`) with no liveness predicate. Refusing is
  the constructor's privilege; deferring would defer the **cell**, which a consumer reads back the
  moment `addGrid` returns and which is a CPU measurement a dead context does not obstruct. A grid
  asking mid-loss for a configuration nobody holds costs one thrown-away bake (`bakes()` +1 in the
  window, and the restore bakes it again) — bounded, since `render` cannot draw while the context must
  wait and a grid born on a configuration is that entry's key-matching holder, so the restore re-bakes
  it (only since #788). It pays rather than refuses because of two browser answers: Chromium's
  `createBuffer()` returns a non-null object on a lost context, before and after the event dispatches
  (2026-08-19, #770), and `createTexture` does the same (2026-08-20, #774) — had the latter answered
  `null`, `bake_config` would map it to `Err` and registration would refuse. `restore` gives every
  registered grid a fresh VAO and buffer and refills it, drawn or not; `demo/context-loss-grids.html`
  registers and feeds a grid inside the loss window beside three siblings, places it after the
  restore, and asserts its own ink.
- **Deleting a dead object mid-life is silent during a loss and loud after the restore.**
  `remove_grid` deletes a grid's VAO and instance buffer, and `release_config` the atlas of a
  configuration whose last grid left, with no liveness check. Measured 2026-09-29 (headless Chromium,
  raw WebGL, a no-delete control): a delete during a loss, in either window, raises nothing beyond
  `CONTEXT_LOST_WEBGL` and nothing is left over after the restore. The flag appears only when a dead
  object is deleted on a **live** context — `INVALID_OPERATION` — which for these two is the window
  between `webglcontextrestored` and `restore`'s rebuild. `gl.viewport` is silent during a loss too,
  even with a negative width that a live context answers `INVALID_VALUE` (same harness), so
  `apply_surface_size`'s viewport call mid-loss is a no-op. (The comments this replaced said
  "measured, #770 — an error flag, not a no-op" without naming the window.)
- **What still packs on a dead context: `apply_frame`, and only it.** It reaches
  `resolve_and_pack` → rasterise → `upload_glyph` → `upload_instances` with no liveness predicate for
  the whole loss. `apply_damage` cannot: it is a `GridTier` method, the tier holds buffer handles but
  not the `glow::Context` (#769), so it only scatters and sets `needs_repack` (#421) for `render` to
  pack. ADR-0027 records `apply_frame` as its one open conformance row. The hazard has no caller
  today — `justerm-web` calls `apply_damage` only, PenTerm uses the widget — and its clearance is
  conditional on two things `restore` does: `invalidate_baseline`, so the diff cannot skip
  re-uploading instances the GPU never received ([GPU upload](gpu-upload.md)), and `bake_config` with
  the configuration's cache (`bake_all_glyphs` over `cache.entries()`), so a slot marked resident but
  never uploaded is re-rasterised. Narrow either and a frame the consumer submitted never appears.
  #774 watched both halves hold: `demo/context-loss-grids.html` feeds `apply_frame` on a dead context
  with a glyph nothing else uses (λ), and after the restore the grid draws correctly with `packs()`
  +0 at its placement; each half was mutation-tested red. Whether the clearance is a design or an
  accident is undecided (Known holes).

**The rebuild.**

- **Loss destroys GPU state, not the CPU model.** The context object survives (the browser reuses
  it; xterm.js keeps its `_gl`, beamterm's re-`getContext` returns the same object), and so do the
  glyph caches, packed instances, `FrameGrid`s and palettes — which is what makes a restore a
  re-upload rather than a re-send from the engine ([frame adapter](frame-adapter.md)).
- **`restore` runs in four steps, and the order makes a failure harmless.** `render` calls it on
  `FrameAction::Rebuild` only, and clears the latch with `rebuilt()` only after it returns `Ok`; on
  any error the old resources stay and the next frame retries
  (`a_failed_rebuild_is_retried_on_the_next_frame`). The DPR is re-read first, so each atlas is baked
  once at the live density rather than baked and re-baked as beamterm's `restore_context` →
  `handle_pixel_ratio_change` does (unpinned).
  1. **Build every replacement without touching a live field**: the pipeline, a VAO and instance
     buffer per grid (#771 — rebuilding only one would leave another grid binding a dead VAO, the bind
     raises `INVALID_OPERATION`, and grid B draws grid A's cells), and an atlas per configuration at
     the live DPR keeping its glyph slots (#772). Each error arm calls `discard`, which deletes only
     what this step built on the live context. The configurations baked are those that will still
     have a holder after step 3, asked by key (`ConfigRegistry::ids_wanted_by`, #788). It used to
     require a *current* holder; two grids swapping configurations mid-loss then re-baked neither —
     `bakes()` 1 against `atlasCount()` 2, one grid's ink 168 where 183 is right, not self-healing
     until the next loss. The registry cannot express the old predicate, which is the point.
  2. **Commit.** Uniform locations come out of the new `Pipeline` **destructured**, so a location left
     uncopied is a compile error — #791 forgot `u_bleed_px`, every post-restore frame raised
     `INVALID_OPERATION`, and the band stopped drawing. Every grid's upload baseline is invalidated
     (#263, #774). The displaced objects are **not deleted** (#793): they died with the context, and
     deleting them on the live one raises `INVALID_OPERATION` — measured five times per restore on one
     grid and one configuration with correct pixels, invisible to the proof corpus. The flag matters
     because it is the channel a guard listens on (#791's uniform failure is the same error); a
     renderer that sets it on every restore leaves nothing to listen for. The #770 measurement under
     the old behaviour: raw WebGL deleting a pre-loss buffer → `0x0502`, a post-restore one → `0`;
     through `restore`, `0x0502` versus `0` for a renderer never lost; on headless SwiftShader and a
     real NVIDIA/D3D11 browser alike. That comment had judged the flag harmless; #793 reversed it.
  3. **Reconcile grids whose selectors moved while the context was dead** (#772): `select_config` per
     grid moves it onto the entry its key now names. It needs the committed live context; a failure
     returns `Err` with a self-consistent restore, the latch stays set, and the whole restore re-runs
     (idempotent).
  4. **Refill**: `apply_surface_size` re-asks for the stored `requested` buffer — which is also where
     a mid-loss `resizeSurface` gets its adopt-what-fits pass — and `upload_instances` runs for every
     grid against its invalidated baseline. Narrowing this or step 2 to the grids that draw leaves a
     hidden grid's rect blank past the restore with every other check green (#774, mutation-tested).
     With the `invalidate_baseline` call removed, 20 of 24 context-loss proof runs go red — every page
     but `context-loss-construct.html` at all four DPRs — against 24/24 green (measured 2026-09-28,
     #988).

**The consumer half of a restore.**

- **What "defer" costs: a value normally read back synchronously is settled at restore, and the
  renderer does not announce it.** A restore notification cannot be built from the consumer's own
  `webglcontextrestored` listener, because `restore` runs inside `render`, after that listener. The
  harm, though, is separable from the notification (#717): a display box sized from a provisional
  `cssWidth` is repaired by re-fitting once the context is back, with no export. "The consumer
  cannot observe X" is not "the consumer cannot fix what X causes".
- **A restore is also a density adoption** (#325). `restore` re-reads the live DPR, so the cell can
  move across a restore nobody asked for, and with no resize in flight nothing tells a consumer a
  re-fit is due. `TerminalSurface`'s `webglcontextrestored` listener therefore presents first (the
  rebuild happens inside that render; acting before it uses the pre-restore cell —
  mutation-checked), runs every tenant's `reapplySurface`, and presents again (a resized buffer is a
  cleared one). Since renderer 0.15.0 (#773) the buffer no longer resizes itself on a density change,
  caught by the #325 e2e going red with a 1369-tall grid in a 703-tall buffer. Before the fix, dpr
  1 → 2 across a loss left a `2556x1369` buffer under a `1278x703` box; only the height was wrong
  (`703` against `684.5`), because the width's cell doubled exactly (9 → 18) while the height went
  19 → 37 — and CI's Linux font takes 19 → 38, so there neither axis moves. The portable check is
  `canvas.style × dpr === drawing buffer`.
- **Only the sole tenant re-derives its own buffer; a shared host must be told** (#808). `applyGrid`
  sizes the surface only when `composedSurface`, so a terminal sharing a canvas gets none of the
  above: the buffer and rects are the host's (ADR-0021 D3). The surface keeps `announcedDpr` and,
  after the rebuild, calls `announceDensity` if the live ratio differs — which also keeps it silent on
  a restore that adopted nothing. Measured on `demo/shared-surface.html` before the fix (dpr 1 → 2,
  one lose/restore, `onDensityChange` calls 0): the cell went 6x12 → 11x23 while the buffer stayed
  `900x340` under a surface reporting `450x170` CSS, so the right pane started at device x=500 past
  the buffer's edge and read `0,0,0,0`, while the left pane still fit and looked right.
  `announceDensity` ends in a coalesced `requestRender()` on both density paths, because the host's
  obligation begins with a `resizeSurface` that clears the buffer after both paths have presented;
  with that line removed and a host that pushes no frame, both panes read `0,0,0,0` at a correct
  `1800x680` buffer — pane A, which fits at any density, is the witness (#808). `announcedDpr` is
  recorded only when a handler exists, since there is no second `webglcontextrestored` for one loss.
  PenTerm registers `onDensityChange` right after `TerminalSurface.open` and re-sizes and re-places
  every pane in it (`penterm 7dadc3c95 src/blocks/terminal/lib/nativeSurface.ts — open`).
- **Input bound.** The outside inputs are browser events (one deadline task per loss, freed after it
  runs) and the consumer's setter values, each stored as one value per grid or one per surface. The
  number of restores is unbounded, and each leaves glow handles behind (Known holes).

## Code

- `justerm-renderer/src/context_loss.rs` — `ContextState` (`on_lost`, `on_restored`,
  `on_restore_deadline`, `rebuilt`, `is_lost`, `restore_overdue`, `must_defer`, `action`),
  `FrameAction`, `ContextLiveness`, `DEFAULT_RESTORE_TIMEOUT_MS`; tests
  `gpu_work_defers_in_all_three_windows_including_the_one_is_lost_cannot_see`,
  `a_deadline_left_over_from_a_previous_loss_never_notifies`,
  `the_same_loss_before_its_event_is_dispatched_still_skips_the_rebuild`
- `justerm-renderer/src/webgl/context.rs` — `ContextLossHandler`, `arm_restore_deadline`,
  `JustermRenderer::gpu_work_must_wait`, `JustermRenderer::restore`, and the four exports
  `is_context_lost`, `is_restore_overdue`, `set_on_context_loss`, `set_context_restore_timeout_ms`
- `justerm-renderer/src/webgl/draw.rs` — `JustermRenderer::render` (the one caller of `restore`),
  `JustermRenderer::apply_frame`
- `justerm-renderer/src/webgl.rs` — `JustermRenderer::new` (the constructor guard)
- `justerm-renderer/src/webgl/surface.rs` — `JustermRenderer::resize_surface`,
  `JustermRenderer::apply_surface_size`
- `justerm-renderer/src/webgl/font.rs` — `JustermRenderer::set_device_pixel_ratio`,
  `JustermRenderer::adopt_selectors`, `JustermRenderer::bake_config`
- `justerm-renderer/src/webgl/grids.rs` — `JustermRenderer::add_grid`,
  `JustermRenderer::remove_grid`, `JustermRenderer::release_config`
- `justerm-renderer/src/config_registry.rs` — `ConfigRegistry::ids_wanted_by`
- `justerm-web/src/context-loss.ts` — `ContextLossRelay`
- `justerm-web/src/terminal-surface.ts` — `TerminalSurface` (`onContextRestored`,
  `announceDensity`, `setOnContextLoss`, `setContextRestoreTimeout`, `isContextLost`)
- `justerm-web/src/justerm-renderer.ts` — `applyGrid`, `reapplySurface`

Browser proofs: `justerm-renderer/demo/context-loss.html`, `context-loss-construct.html`,
`context-loss-race.html`, `context-loss-grids.html`, `context-loss-neighbour.html`,
`context-loss-timeout.html`; web e2e `demo.spec.ts` "a GL restore at a changed density re-applies the
canvas box (#325)" and `shared-surface.spec.ts` (#774, #808).

## Reference behaviour

- [Resizing while the GL context is lost](../../agents/reference-facts.md#resizing-while-the-gl-context-is-lost--the-reference-never-asks-the-question-639-verified-2026-08-03)
  — a negative result: xterm's resize runs unguarded through a loss because it never reads back what
  the driver granted, so it has no 0 to guard against. Absence of a guard is not evidence about one.
- [Reading a GL parameter that a lost context answers with `null`](../../agents/reference-facts.md#reading-a-gl-parameter-that-a-lost-context-answers-with-null-688-verified-2026-08-03)
  — xterm reads the same parameter unguarded in its constructor, so the shape is shared; the
  difference is the binding (JS carries `null` on, glow unwraps). xterm's other two reads are
  falsy-guarded.
- [A second loss during the restore](../../agents/reference-facts.md#a-second-loss-during-the-restore--the-reference-is-strictly-weaker-so-it-grants-nothing-688-lens-verified-2026-08-03)
  — xterm can half-commit a restore; `restore`'s build-then-commit cannot, and xterm is no licence.
- [Recovering a context loss when the resource is shared between terminals](../../agents/reference-facts.md#recovering-a-context-loss-when-the-resource-is-shared-between-terminals-774-verified-2026-08-20)
  — xterm.js shares a CPU-side atlas and rebuilds refcount-conditionally, three.js rebuilds lazily at
  next use; neither transfers (every texture here dies at once, and a hidden grid has no next use), so
  this restore is unconditional and eager. No reference loses one context across N terminals.

Not in the store, attributed: xterm.js arms a 3 s timeout on `webglcontextlost` and fires an emitter
if still lost, clearing it on dispose (`WebglRenderer.ts:125-136`, `:161-163`), the origin of this
crate's `Drop` contract; alacritty asks the driver's reset status at the point of use with no
deadline and nobody to notify — ADR-0027 D1 reached outside a browser (#579, 2026-08-04).

## Cross-cutting invariants

- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  every failure here (no document, no canvas, no WebGL2 context, a lost context at construction)
  crosses into JS as a string primitive: no `.message`, no `.stack`, `instanceof Error === false`.
  #662 fixed only the decoder's site, which ADR-0008 obliged. A panic is a third shape — a
  `RuntimeError` object — which that note's recurrence list does not reach; #688 removed this
  territory's only panic site by guarding above it.
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md) —
  the density watcher, the `webglcontextrestored` listener and the loss relay are surface-scoped,
  because one canvas has one context, one density and one loss. They sat on the per-terminal
  `JustermRenderer` until #775, so disposing the first terminal took recovery away from every sibling
  on its canvas — silently, until a loss it could no longer recover from.
- [a screen change no frame carries owes its own present](../invariant/an-unframed-screen-change-owes-a-present.md)
  — `announceDensity` re-presents after the host's handler, whose `resizeSurface` clears a buffer both density paths had already presented (#808)
- [a CPU record of GPU state is true only while that object lives](../invariant/a-gpu-mirror-dies-with-its-context.md)
  — `restore` replaces every GPU object, so it is where each CPU record is dropped (`invalidate_baseline`) or refilled (`bake_config(Some(cache))`)

## Blast radius

- [frame adapter](frame-adapter.md) — a restore re-packs from the retained `FrameGrid` and asks the
  engine for nothing; discarding it on loss would need a re-send nobody can request.
- [glyph atlas](glyph-atlas.md) — every atlas texture dies; `restore` re-bakes each live
  configuration from its cache, which assumes a cache entry marked resident can always be
  re-rasterised.
- [GPU upload](gpu-upload.md) — the upload baseline assumes its buffer still exists; any new path
  that recreates a buffer must invalidate it, or an identical frame never refills it.
- [multi-viewport](multi-viewport.md) — one context is one loss for every grid and configuration, so
  `restore` walks both registries and predicts the reconcile's outcome by key; a change to how grids
  hold configurations changes which atlases survive.
- [cell geometry](cell-geometry.md) — every deferring entry point is one of its setters or the
  surface size; whatever derives the cell is what a loss window must store and `restore` re-derive.
- [widget lifecycle](widget-lifecycle.md) — the widget's teardown never calls `free()`, which is the
  only unset `setOnContextLoss` has; the relay exists because of that.
- [colour policy](colour-policy.md) · [caret drawing](caret-drawing.md) — every uniform location
  belongs to the program that dies; a new uniform must be added to `Pipeline` so the destructuring
  commit carries it.

## Known holes / open

- **`apply_frame` still packs through a dead context**, safe only by the two-part validity condition
  above. Nobody has been asked whether the clearance is a design or an accident; that is a judgement,
  not a measurement.
- **Not deleting the displaced objects leaves their glow handles behind, once per restore.** glow
  keeps each GL object's JS handle in a `SlotMap` that only `delete_*` removes (glow 0.18.0,
  `src/web_sys.rs`), so every restore leaves the program, the quad VBO, each grid's VAO and instance
  buffer and each configuration's atlas in those maps for the renderer's lifetime — growth unbounded
  in the number of restores. The comment `restore` carried before #988 gave freeing those slots as the
  reason to delete; #793 weighed only the error flag. Found reading source during #988; not measured.
- **The pixel consequence of an un-rebaked atlas cannot be gated here.** When #788's defect was live,
  an un-rebaked atlas kept drawing correctly; most likely the harness — Chromium hands back a
  non-null `createTexture` on a lost context and its simulated loss does not appear to discard texture
  contents. The proofs gate the state (an entry that survives is re-baked), not the symptom.
- **A measurement read one rAF after a present is void** (#808): comparing two versions one frame
  after the restore returned `0,0,0,0` for both, because a presented buffer is gone. A read only
  means something inside the frame the library scheduled. And `demo/shared-surface.html`'s handler
  ends in a `pane.push()` that schedules exactly the missing frame — the #776 shape, where the proof
  supplies the thing under test — so the page takes that push behind a flag.
