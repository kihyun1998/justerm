# Territory — GL context lifecycle

## What it is

Surviving the browser taking the GPU away. A WebGL context can be lost at any moment — GPU reset, tab
backgrounded, driver eviction — and **every GL object it owned is destroyed with it**. The browser
fires `webglcontextlost`, and *may* later fire `webglcontextrestored`. This territory is the state
machine that decides what the renderer does in between.

## Governing decisions

- [ADR-0027 — a liveness question is answered by the source that owns the answer](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  — **when GPU work may be attempted, and which predicate each site asks.** Its conformance map
  resolves every entry point in this territory, and it *derives* the two that are still wrong rather
  than listing them. Promoted from spine #689 (closed) after the rule produced a site — #695 — that
  nobody had reported
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — owning a GL context at
  all is this crate's premise; it decides nothing about the loss behaviour

## Design model

- **"May later fire" is the whole difficulty.** Restoration is not promised, so the machine cannot be
  written as "wait for the event" — it needs a timeout and a way to tell a consumer that recovery is
  overdue rather than pending.
- **The state machine is pure and host-tested; the browser wiring is not.** Event closures and GL
  resource recreation live in the wasm layer, while what the renderer *should do with the current
  frame* is a value this module returns. That is the same split the packer, the upload planner and
  the frame adapter use.
- **The consumer is told, not guessed at.** `is_context_lost`, `is_restore_overdue`,
  `set_on_context_loss` and `set_context_restore_timeout_ms` are four of the crate's exports — the
  timeout is a consumer policy, and overdue-ness is a question a consumer can ask rather than infer.
  **All four have a consumer as of #579** ([widget lifecycle](widget-lifecycle.md)), and the one thing
  that took adapting is a shape this crate cannot change without breaking callers:
  `set_on_context_loss` takes a `Function` and offers no unset, and it clears its own slot in `Drop`
  — which runs at `free()`, a call the widget deliberately never makes. So the consumer registers an
  indirection once and swaps behind it. Worth knowing before adding a fifth export of this shape: a
  push channel whose only teardown is `Drop` pushes that teardown onto whoever holds it.
- **A restore deletes nothing it displaces, and that is deliberate** (#793). Every handle the rebuild
  replaces — the program, the quad VBO, each grid's VAO and instance buffer, each configuration's
  atlas — belonged to the context that died, so it is already gone; asking GL to delete it is a no-op
  that raises `INVALID_OPERATION`. Measured on master before the change: the first frame after every
  restore raised it **five** times — one atlas, the program, the quad VBO, one VAO, one instance VBO,
  so one grid on one configuration — with the pixels perfectly correct, so nothing in the proof corpus
  could see it. The reason it is worth naming rather than tolerating is the *channel*: a uniform
  location that survives a restore pointing at the dead program raises the same
  `INVALID_OPERATION` — that is how #791's `u_bleed_px` failed — so a renderer that leaves the error
  flag set on every restore has nothing left for a guard to listen to. The one deletion that stays is
  `restore`'s own `discard`, which frees what that function built on the **live** context and never
  published.
- **Loss destroys GPU state, not the CPU-side model.** The persistent dense grid in the
  [frame adapter](frame-adapter.md) survives, which is what makes a restore a re-upload rather than a
  re-send from the engine.
- **Registration is the one entry point that neither refuses nor defers, and pays instead** (#787).
  `add_grid` reaches `bake_config` with no liveness predicate, unlike every other mid-life entry
  point. Both alternatives are closed to it: refusing is the constructor's privilege and only because
  a constructor has nothing to defer into, and deferring would defer the **cell** — which the five
  deferring setters can do and this one cannot, since a consumer reads `cellWidth` back the moment
  `addGrid` returns and the cell is a CPU measurement a dead context does not obstruct. So a grid asking
  mid-loss for a configuration nobody holds costs **one thrown-away bake** (measured: `bakes()` +1 in
  the loss window, and the restore bakes it again). Bounded, not merely small: `render` cannot draw
  while `gpu_work_must_wait()` holds, and a grid born on a configuration is that configuration's own
  key-matching holder, so the restore always re-bakes it — the second half only true since #788.
  **What makes it pay rather than refuse is two browser answers, measured because the obvious
  assumption is false.** Chromium's `createBuffer()` hands back a **non-null** object on a lost
  context, both in the synchronous window before `webglcontextlost` dispatches and after it
  (2026-08-19, #770), so the buffer build succeeds and `add_grid`'s `Err` arm — glow's `null` path —
  is not taken. `createTexture` answers the same way (2026-08-20, #774), and that half is
  load-bearing rather than symmetric: the bake on a cache miss creates a texture, and had it answered
  `null`, `bake_config` would map that to `Err` and the registration would **refuse** — the contract
  ruled out above. Not refusing is the contract because a consumer registering a terminal while the
  context happens to be dead wants the grid, and `restore` gives **every** registered grid a fresh
  VAO and buffer and refills it, drawn or not (#771 had to, since a stale per-grid VAO draws the
  *wrong* grid once there is a draw loop). `demo/context-loss-grids.html` watches it rather than
  reasoning it: it registers *and feeds* a grid inside the loss window with three siblings already on
  the registry, places it after the restore, and asserts it draws its own ink rather than a
  neighbour's.
- **Deleting a dead object mid-life has no state effect and raises `INVALID_OPERATION`** (measured,
  #770). `remove_grid` deletes the grid's VAO and instance buffer, and `release_config` the atlas of
  a configuration whose last grid left, with no liveness check, so on a lost context each delete
  leaves the error flag set. That flag is not free: it is the channel a guard listens on, which is
  why `restore` deletes nothing it displaces (the #793 bullet above).
- **Construction is the one entry point that refuses instead of deferring, and it is the only one
  where the *binding* decides the failure shape.** The five below can defer because there is a
  renderer to defer *into*; a constructor has no state machine yet, nothing to replay at `restore`,
  and no object to hand back — so it returns `Err` (#688). What forces the guard's exact position is
  not this crate's code but glow's: `Context::from_webgl2_context` enumerates the extensions
  (`get_supported_extensions().unwrap()`) and **panics** on the `null` a lost context answers with,
  so the check sits above *that* call, not above the first parameter this crate reads. The read
  itself is harmless — `get_parameter_i32` answers `0` for a `null`. A panic is also the one failure
  here that leaves the family's error shape: it arrives as a `RuntimeError`, not as the bare string
  every other fallible path throws.
- **Every entry point that changes the geometry takes the request and defers the GPU work.** Seven of
  them can arrive mid-loss — the DPR, the font size, the font family, the font weights (#928), the
  subpixel setting (#961), the spacing policy and the resize — and none may reject the call, because a consumer has no obligation to hold it back. It can
  now *see* the loss (#579 wired the surface), but seeing is not the same as being expected to act on
  it: nothing in the contract says a consumer must check, and a setter that rejected would break every
  one that does not. So each stores what it was given and lets `restore` re-derive
  from it; nothing is queued, because the stored value *is* the queue. The font and spacing setters skip an
  atlas re-bake that a dead context would return invalidated; `resize` skips reading the drawing
  buffer back, which on a dead context answers 0 and would floor the grid to one cell (#639).
- **"Is the context lost" has two answers and they disagree for a whole window — so the predicate
  is chosen per site, never shared out of habit** (#639). The browser destroys a context
  *synchronously* and merely **queues** `webglcontextlost`; the mirror holds on the way back. So the
  state machine's flag — the honest thing to report to a *consumer*, since it tracks what we have
  been told — lags the context itself, and an internal caller guarding on it is guarding on the
  wrong thing. Measured in Chromium, immediately after `WEBGL_lose_context.loseContext()`: in the
  pre-dispatch window `gl.isContextLost()` is already `true`,
  `drawingBufferWidth` already `0`, and the flag still `false`. The rule that falls out:
  a caller that **has** the answer in hand tests that (`resize` rejects a non-positive read-back,
  which is also right for any other cause of one), and a caller that must **ask** consults both
  sources, since each covers the window the other misses. The constructor is the third case and it
  falls out of the same rule rather than adding one: it asks the **context alone**, because the flag
  it would also consult does not exist yet — a freshly built state machine reports "live"
  unconditionally, so consulting it there is not a weaker predicate but a constant (#688, measured
  red as a mutation on `context-loss-construct.html`).
  This is the territory's most expensive shape so far: #639's first fix guarded on the flag, went
  green, and left the defect it was written for reachable verbatim.
- **`render` asks both sources, and the pure module is *given* the one it cannot fetch** (#695,
  ADR-0027 D3/D4). `ContextState::action` takes a `ContextLiveness` the wasm layer reads off the
  context and composes it with the flag it owns. Both arms of that decision used to run on a dead
  context in the pre-dispatch window: the `Rebuild` arm rebuilt and **threw** (an empty
  shader-compile log, `"justerm-renderer: "`), and the `Draw` arm packed — `packs()` +1, measured —
  resolving glyphs into a dead atlas. Both now skip; `demo/context-loss-race.html` asserts the
  no-throw and the `packs()` delta of **0**, in a window whose existence it checks first.
  **Why the argument rather than a guard at the caller**: `webgl.rs` is wasm32-only, so a guard
  there is invisible to `cargo test` — the composition lives in the pure module precisely so both
  windows have a host test. The cost taken with it is that the argument is a place to lie, which no
  host test can catch; that is what the browser section covers, and a mutation confirms the split
  (call site pinned to `Usable` → 326 host tests green, proof red).
- **What still runs the pack on a dead context: `apply_frame` — and *only* `apply_frame`.** It
  reaches the pack → rasterise → `upload_glyph` → `upload_instances` chain from its own call with
  **no liveness predicate at all**, for the *whole* loss rather than one window.
  **This bullet said "`apply_frame` and `apply_damage`" until #774, and so does ADR-0027's
  conformance row; both were wrong, and the correction halves the open defect.** `apply_damage` is
  an inherent method on `GridTier`, which holds the buffer *handles* and not the `glow::Context` —
  so the tier split (#769) makes it **structurally incapable** of a GL call. It scatters into the
  retained grid, sets `needs_repack` and returns (#421); the pack behind it belongs to `render`,
  which a lost context skips. `docs/map/territory/multi-viewport.md` had the accurate version the
  whole time (*"`apply_frame` and `repack_from_grid` reach `resolve_and_pack`"*), so this was two
  notes disagreeing rather than an unknown.
  What that costs is the reachability sentence this bullet used to carry — *"a consumer streaming
  output through a multi-second GPU recovery pumps every frame through it"* — which is **false for
  the consumer we have**: `justerm-web` calls `apply_damage` and never `apply_frame`. The hazard is
  real for a direct-path caller and there is not one today. **A cleared concern, not
  a safe design**, and the clearance is conditional: it holds only because `restore` does two
  separate things — `invalidate_baseline`, so the #263 diff cannot skip the re-upload of instances
  the GPU never received, and `bake_all_glyphs` over `cache.entries()`, so a slot marked resident
  but never uploaded is re-rasterised. Remove or narrow either and this becomes a silent defect: a
  frame the consumer submitted, saw acknowledged, and never sees. It is the one row of ADR-0027's
  conformance map still resolving as ✗.
  **#774 watched the clearance hold, which is not the same as retiring it.**
  `demo/context-loss-grids.html` feeds a grid through `apply_frame` *while the context is dead*,
  with a glyph nothing else on the page uses (λ — not ASCII, so a cache slot rather than a prebake).
  That call rasterises into the dead atlas, marks the slot resident, and records an upload baseline
  for bytes the GPU never received; the grid is then placed after the restore and draws correctly
  **without re-packing** (`packs()` +0 at its placement), so both halves of the condition are
  observed rather than argued for. Each half was mutation-tested: baking the restore's atlases with
  the cache dropped blanks that glyph, and refilling only the grids that draw blanks the whole grid.
  What is unchanged is that it *is* a validity condition — the frames still go through a dead
  context — so the ✗ stands until somebody decides whether the clearance is a design or an accident.
  Nobody has been asked.
- **What "defer" costs, stated once because each site pays it.** A value the consumer normally reads
  back synchronously — a clamped grid, an atlas-shrunk cell — is settled at restore instead, and the
  consumer is not told. This used to be filed as "the same missing signal as #579, reached from the
  other side"; **#579 has landed and it is not the same signal**, which is the more useful fact. The
  loss half needed nothing from this crate — the four exports were already there — while a *restore*
  **notification** cannot be built in the consumer at all: `restore` runs inside `render`, not in the
  `webglcontextrestored` listener, so a consumer-side listener fires before the deferred read-back has
  settled and would report the grid it had before. Measured while wiring #579.
  **This bullet ended "whoever fixes this owns a new export here, not a widget change" for about an
  hour, and #717 disproved it the same day** — kept as written because the correction is the content.
  The notification and the *harm* are separable, and only the first one is ours. The harm is a display
  box the consumer sized from a provisional `cssWidth`, which nothing here can rewrite; the consumer
  repeats its fit when `isContextLost()` goes false and it is gone, with no export involved. What went
  wrong in the original sentence is a shape worth watching for: *"the consumer cannot observe X"* was
  turned into *"the consumer cannot fix what X causes"*, and those are different claims. The export
  question reopens only for a consumer that cannot poll.
- **A restore is also a *density* adoption, and that half had no consumer trigger at all** (#325).
  `restore()` re-reads the **live** device pixel ratio rather than the one the renderer was built
  with — deliberately, because a DPR notification arriving during a loss is *dropped* rather than
  queued — so the **cell** can move across a restore nobody asked for. The bullet above resolves the
  *clamp* case by having the consumer repeat its fit when `isContextLost()` goes false; this one
  cannot be reached that way, because with no resize in flight nothing tells a consumer a repeat is
  due. So the widget handles it on `webglcontextrestored`: it **drives one render first** — the
  rebuild happens inside that render, so acting before it uses the pre-restore cell
  (mutation-checked) — then re-derives the drawing buffer from the grid it holds, re-applies the
  canvas display box, and renders again (a resized buffer is a cleared one).

  **The buffer stopped moving on its own at renderer 0.15.0 (#773)**, which is why this bullet now
  names three steps where it named one: the renderer re-bakes at the live density and leaves the
  buffer as asked, so a restore that adopts a new density leaves a grid too large for the buffer
  holding it until the widget re-derives. Caught by this territory's own e2e rather than by reading
  — the #325 test went red at the migration with a 1369-tall grid inside a 703-tall buffer.
  **And all three of those steps are the SOLE tenant's, which is the half #808 found.** They run
  inside `reapplySurface`'s `composedSurface` branch, so a terminal sharing a canvas gets none of
  them: the buffer and every viewport rect belong to whoever measured the container (ADR-0021 D3),
  in device px at a ratio that has just stopped being true. Until #808 that host was never told —
  `onDensityChange` was `setDevicePixelRatio`'s alone, and a restore calls no setter. The surface now
  keeps the density it last **announced** and compares it against the live one after the rebuild,
  which is also what keeps the notification from firing on a restore that adopted nothing. Measured
  on `demo/shared-surface.html` (dpr 1 → 2, one lose/restore, `onDensityChange` calls **0**): the
  cell went 6x12 → 11x23 while the buffer stayed `900x340` under a surface now reporting `450x170`
  CSS, so the right-hand pane's 63 columns started at device x=500 and ran past the buffer's edge —
  the scissor discarded all of it and its centre read `0,0,0,0`. No error, and the left-hand pane
  narrow enough to still fit and look correct, which is what made it quiet.
  **And the announcement carries a repaint, which is the half that was nearly shipped missing.** What
  it asks a host for begins with a fresh `resizeSurface`, which re-creates the drawing buffer and
  therefore **clears** it — while both density paths present *before* the handler, for the reason two
  paragraphs up (the cell is not readable until a render has run). So a host doing exactly what the
  contract lists was left staring at the clear. `announceDensity` ends in a coalesced
  `requestRender()`, on both paths at once: two rules for one mechanism is what this area has
  repeatedly paid for, and the pre-existing half of it was `setDevicePixelRatio`'s since #775.
  Measured with that one line removed, on a host that pushes no frame: the buffer is correctly
  `1800x680` and **both** panes read `0,0,0,0` — so the witness is pane A, the one that fits at any
  density, and a check that only watched the pane #808 is about would have reported this clean.

  Measured before the fix: dpr 1 → 2 across a loss left a `2556x1369` buffer under a canvas styled
  `1278x703`. Note which half was wrong — the **width** was accidentally correct because the cell
  doubled exactly (9 → 18), and only the height was off (`703` against `684.5`, the cell having gone
  19 → 37). A width-only check would have reported this area clean. **And the accident is font
  dependent, not a property of the fix**: CI's Linux font takes the same 19 to 38, so *both* axes
  divide back evenly there and the box does not move at all. The portable statement is
  `canvas.style x dpr === drawing buffer`, never that the box changed.
- **The listeners hold only the shared state, never the renderer** (#269). The two closures capture
  the `Rc`'d `ContextState` and nothing else, so either can fire while a `&mut JustermRenderer`
  method is on the stack without a `RefCell` double-borrow. The `webglcontextlost` one calls
  `preventDefault()` **first**: without it the browser never fires `webglcontextrestored` and the
  context stays dead for good. Every reference implementation does this first — beamterm's
  `context_loss.rs`, xterm.js's `WebglRenderer.ts`. The grace period is a consumer-injected value
  (ADR-0017: the renderer times, the consumer decides how long), read at the moment a loss arms
  its deadline — which is why a changed timeout applies to the *next* loss only.
- **The restore deadline is never cancelled, and a loss epoch is what makes that safe** (#327).
  `clearTimeout` would work — a merely-queued timer task aborts when it finds its id gone from the
  map (HTML spec, timer initialization steps), which is how xterm.js does it — but cancelling means
  *owning* the `Closure`, and the consumer's notification handler is exactly the place that destroys
  the renderer (VSCode's `onContextLoss` calls `_disposeOfWebglRenderer()`). Dropping the handler
  would free the very closure whose body is running; JS gets away with this because its closures
  are garbage-collected, and Rust's are not. So the closure is handed to JS instead
  (`Closure::once_into_js` keeps it alive through an internal `Rc` cycle that the single invocation
  breaks, freeing it *after* the body returns), and every deadline with nothing to say identifies
  itself: `on_restore_deadline` rejects it if the context came back, if it already notified, or if
  it belongs to an earlier loss. A stale deadline costs one no-op task. Inside the body the state
  borrow is released and the callback cloned out **before** calling into JS, because the consumer's
  handler runs re-entrantly and may dispose the renderer, poll `isRestoreOverdue` or replace itself.
  `Drop` disarms from the other end: it clears the callback slot, so a deadline still pending finds
  nobody to call, while the `Rc`s it captured keep its state alive until it runs once and frees
  itself — the same observable contract as xterm.js's `clearTimeout` on dispose
  (`WebglRenderer.ts:161-163`).
  The epoch guards the **lost → restored → lost** order: the first loss's timer is still pending
  when the second loss arms its own, and without the stamp it would land inside the second loss's
  grace period and cut it short. That order is reachable — every transition into "lost" dispatches
  — and `context_loss.rs`'s `a_deadline_left_over_from_a_previous_loss_never_notifies` is written on
  it. **This used to claim more, and the extra claim was wrong** (measured 2026-08-04, #579): it said
  the epoch made us stricter than xterm.js, whose single `_contextRestorationTimeout` is overwritten
  without being cleared when a second `webglcontextlost` arrives *with no restore between*
  (`WebglRenderer.ts:131`), so "both timers then fire and its `onContextLoss` is delivered twice".
  The overwrite is real in its source; the antecedent is not reachable. A second
  `WEBGL_lose_context.loseContext()` on an already-lost context delivers **no** second event
  (headless Chromium: two `loseContext()` calls with no restore between produce exactly **1**
  `webglcontextlost`), because the event fires on the transition into lost and an already-lost
  context has none to make. So that comparison described a state neither implementation can be put
  into. The epoch still earns its place on the order above — a favourable comparison is just the
  kind nobody re-checks.
- **`gpu_work_must_wait` covered only one of its two windows until #772.** The #639 bullet above
  names both sources and the window each covers; the mirror one — a context back before
  `webglcontextrestored` is processed, so GL answers "live" while the program, VAO and atlas it
  owned are still the destroyed ones — was described and not covered. The function asked
  `is_lost()`, and `on_restored` clears exactly that flag while setting `pending_rebuild`, so in the
  post-restored, pre-rebuild window both sources answered "fine" and a setter baked into resources
  `restore` replaced on the next frame. The composition now lives on the state machine
  (`must_defer`), beside `action`, which is where ADR-0027 D1 puts it: the source that owns the
  flags answers the question about them. `apply_surface_size` does not use it — it holds the actual
  answer, having just read the drawing buffer back, and guards on that.
- **`restore` runs in four steps, and the order is what makes a failure harmless.** The context
  *object* survives a loss (the browser reuses it; xterm.js keeps its `_gl`, and beamterm's
  re-`getContext` hands back the same object), so only what it owned is rebuilt — the program, the
  VAOs and buffers, the atlas textures and the uniform locations bound to that program — while CPU
  state (glyph cache, instances, grid, palette) survives untouched, which is what preserves the
  terminal's content. The DPR is re-read *before* anything is built, so the fresh atlas is baked
  once at the live density rather than baked at the stale one and immediately re-baked, as
  beamterm's `restore_context` → `handle_pixel_ratio_change` does (#322 is the same re-bake driven
  by a `matchMedia` notification). On any failure the old resources stay in place and
  `pending_rebuild` stays set, so the next frame retries — self-healing, mirroring
  `set_device_pixel_ratio`.
  1. **Build every replacement without touching a live field.** Every grid gets its own buffers,
     because the VAO and instance buffer are per-grid (#771) and both died: rebuilding only the
     default's would leave a registered grid binding a dead VAO, the bind raises
     `INVALID_OPERATION` and leaves the *previous* grid's VAO bound, so grid B silently draws grid
     A's cells. Every **configuration** gets its own atlas at the live DPR, keeping its own glyph
     slots (#772) — one atlas would restore one grid's font and leave the others sampling a dead
     texture. Each error arm deletes what the step built (`discard`: built on the **live** context
     and never published, so this deletion is real), which makes the order of the three free.
     The configurations baked are the ones that will **still** have a holder after step 3, asked
     of the registry by key (`ids_wanted_by`, #788) — an entry whose every grid drifted off its key
     is released by the reconcile, and baking it would rasterise a glyph set into a texture deleted
     a few lines later, once per restore after a mid-loss font change. **It used to also require a
     *current* holder, which is the set as of now rather than as of after**: step 3 can place a grid
     on an entry nobody holds at this instant, so two grids swapping configurations mid-loss
     re-baked neither and one came back drawing through a dead texture. Measured: `bakes()` 1
     against `atlasCount()` 2, and that grid's ink 168 where a correctly baked atlas gives 183 —
     silent, and not self-healing until the next loss. #774 made the rest of this step measured
     rather than reasoned: `demo/context-loss-grids.html` loses one context with four grids in four
     states (drawn / hidden / never drawn / registered mid-loss) and reads each grid's own rect back
     — one bake per *live* configuration, including one whose only holder is hidden, and `packs()`
     unmoved across the whole restore.
  2. **Commit.** The uniform locations come out of the new pipeline **destructured**, not copied
     field by field: a location that survives a restore by being copied is one an author has to
     remember — `demo/context-loss.html` says in as many words that forgetting one leaves a location
     belonging to the dead program — and #791 forgot `u_bleed_px` exactly that way: every frame after a restore raised
     `INVALID_OPERATION` and the band silently stopped drawing. Binding every field by name makes
     the next omission a compile error. Every grid's upload baseline is invalidated here, not just
     the default's (#263, #774: narrowing either this or step 4's refill to the grids that draw
     leaves a hidden grid's rect blank past the restore, with every other check on that page
     green). The displaced objects are **not deleted** — the #793 bullet above. The measurement
     under it (#770, before #793 acted on it) was taken two ways: raw WebGL with no wasm involved
     (delete a pre-loss buffer → `0x0502`; delete one created after the restore → `0`) and through
     `restore` itself (the restoring `render` leaves `0x0502`, a renderer that never lost its
     context leaves `0`), on headless SwiftShader and on a real NVIDIA/D3D11 browser alike. The comment
     that carried this measurement judged the flag harmless — no state effect, the next frame reads clean, the only cost a
     consumer polling `getError` around a restore seeing a failure that is not one — and #793
     reversed that for the channel reason in its bullet. What not deleting costs on the glow side
     is under *Known holes*.
  3. **Reconcile grids whose selectors moved while the context was dead** (#772). A mid-loss
     `setFontSize` / `setLetterSpacing` writes the selector and defers the rest, so the grid now
     names a configuration whose key it no longer matches; steps 1–2 rebuilt the entries that exist,
     and this moves grids between them. It runs after the commit because it needs a live context,
     which it has — `restore` is only reached on a `Rebuild`. A failure here leaves a committed,
     self-consistent restore and returns `Err`, so the retry latch stays set and the next frame
     runs the whole restore again (idempotent).
  4. **Refill.** The loss reset the drawing-buffer size and the viewport, so `apply_surface_size`
     re-asks for the buffer the consumer last requested — in device px, as given — and every grid's
     buffer is re-uploaded so `render` draws the pre-loss frame. This is also where a
     `resizeSurface` that arrived *during* the loss gets its adopt-what-fits pass: it committed the
     request and skipped the read-back (#639). Nothing extra is stored for it — the buffer it asked
     for *is* `requested`.

## Code

- `justerm-renderer/src/context_loss.rs` — the state machine (pure, host-tested)
- `justerm-renderer/src/webgl/context.rs` — `ContextLossHandler`, `arm_restore_deadline`,
  `gpu_work_must_wait`, `restore`, and the four exports (browser-only)
- `justerm-renderer/src/webgl/draw.rs` — `render`, the one caller of `restore` (`FrameAction::Rebuild`)

## Reference behaviour

Two questions have been checked; the rest of the territory has not. The comparison set here is smaller
than for the rest of the crate — but **smaller is not empty, and this note said empty until 2026-08-04**
(#579). alacritty has a context-loss concept and a recovery path, and asks the driver's reset status at
the point of use rather than a queued-event flag: ADR-0027's D1 reached independently, outside a
browser. What it has no analogue for is the *consumer* half — it recovers synchronously with nobody to
tell — which is the distinction the original sentence flattened.

- [Resizing while the GL context is lost](../../agents/reference-facts.md#resizing-while-the-gl-context-is-lost--the-reference-never-asks-the-question-639-verified-2026-08-03)
  — a **negative** result, and the useful kind: xterm's resize handler runs unguarded through a loss,
  which reads as permission until you see *why* it can. It never asks the driver what it granted, so
  the read that a dead context answers with 0 does not exist there. Absence of a guard is not
  evidence about the guard
- [Reading a GL parameter that a lost context answers with `null`](../../agents/reference-facts.md#reading-a-gl-parameter-that-a-lost-context-answers-with-null-688-verified-2026-08-03)
  — the reference reads the *same* parameter in its *own* constructor with no guard, so the shape is
  shared and this layer is not the one that drifted. What differs is entirely the binding: JS carries
  a `null` on, glow unwraps it. Not indifferent, though — xterm's other two parameter reads *are*
  falsy-guarded, so a `null` there becomes a throw
- [Recovering a context loss when the resource is shared between terminals](../../agents/reference-facts.md#recovering-a-context-loss-when-the-resource-is-shared-between-terminals-774-verified-2026-08-20)
  — the one reference that shares a texture atlas across terminals shares the **CPU-side** one and
  keeps its GL objects per terminal, so its restore drops one reference and asks its consumer for a
  full redraw. Neither half transfers: our shared entry is the GPU texture, and our consumer has no
  retained state to be asked. Also the bound on the whole comparison — no reference loses *one*
  context across *N* terminals, so "registered but not drawn when the context died" has no comparand

Checked since (#579, 2026-08-04): **the #327 comparison has an answer, and it is that only xterm has
the concept.** xterm arms a 3 s timeout on `webglcontextlost` and fires an emitter if it is still lost
(`WebglRenderer.ts:125-136`), clearing it on dispose (`:161-163`) — which is where this crate's own
`Drop` contract came from. alacritty has nothing to compare: it recovers at the point of use with no
deadline and nobody to notify.

~~Still unchecked: what any of them does with GPU resources it cannot rebuild.~~ **Answered in #774,
and the two answers disagree with each other rather than with us.** xterm.js rebuilds
*refcount-conditionally* — a terminal whose sibling still holds its atlas rejoins the entry it just
left, so its restore touches no atlas at all; three.js rebuilds **lazily, at next use**
(`WebGLRenderer.js:1119-1131`). Neither transfers: on one context every entry's texture died, so
conditional is wrong here, and a hidden grid has no next use, so lazy is wrong here. Both are
recorded in the reference-facts section linked above, as reasons this restore is unconditional and
eager rather than as shapes to follow.

## Cross-cutting invariants

- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  every failure this territory reports (no document, no canvas, no WebGL2 context) crosses into JS
  as a **string primitive**, so a consumer's `catch` sees no `.message`, no `.stack` and
  `instanceof Error === false`. Unchanged by #662, which fixed the decoder's single site because
  ADR-0008 obliged that shape there; nothing obliges it here yet. **A panic is a third shape and
  that note's recurrence list does not reach it** — it is neither a new fallible export nor a
  `map_err`: it crosses as a `RuntimeError` *object*, i.e. the one thing here that a consumer's
  `catch` could tell apart from the rest. #688 removed this territory's only site, by guarding above
  the glow call that produced it rather than by changing what anything throws
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — **this is where a violation of it was actually reachable.** The three things a consumer holds for
  the context's sake — the density watcher, the `webglcontextrestored` listener and the context-loss
  relay — are all **surface-scoped**, because one canvas has one context, one density and one loss.
  They sat on the per-terminal `JustermRenderer` until #775, so the first terminal disposed took
  context recovery away from every sibling on its canvas: silent, and damaging a bystander rather than
  the caller. The survivor kept drawing correctly right up to a loss it could no longer recover from

## Blast radius

- [frame adapter](frame-adapter.md) — its persistent grid is what a restore replays from; if that
  were ever discarded on loss, recovery would need the engine's cooperation
- [glyph atlas](glyph-atlas.md) — every slot is a GPU resource and does not survive; the atlas has to
  be rebuilt, not merely re-bound
- [GPU upload](gpu-upload.md) — the "last uploaded" state it diffs against is invalidated by a loss,
  so a restore has to force a full upload rather than a diff
- [cell geometry](cell-geometry.md) — every deferring entry point above is one of its setters or the
  resize, so a change to what derives the cell changes what a loss window has to hold
- [widget lifecycle](widget-lifecycle.md) — the consumer sets the timeout and reacts to the callback
- [multi-viewport rendering](multi-viewport.md) — one context means **one loss for every grid and
  every font configuration at once**, so `restore` walks two registries rather than acting on a
  single grid: since #771 it rebuilds every registered grid's VAO and instance buffer and drops every
  upload baseline, and since #772 it also re-bakes every live configuration's atlas at the live
  density — keeping each one's glyph slots, so no grid has to re-pack — and then runs a **reconcile**
  pass over the grids. That last step exists because a font or spacing setter arriving while the
  context is dead writes its selector and defers the rest (an atlas cannot be baked on a dead
  context), leaving that grid naming a configuration whose key it no longer matches. The reconcile
  runs *after* the commit and propagates its error, so a failure leaves a self-consistent restore,
  the retry latch set, and the whole function re-run on the next frame — idempotent by construction.
  It also skips re-baking a configuration that the reconcile is about to release, which is the whole
  glyph set of a font nobody is on any more.
  **Which configurations those are is a *prediction*, and getting it wrong is how a surviving entry
  goes un-rebaked** (#788). The bake runs before the reconcile — the reconcile acquires entries and
  needs the committed live context, so the order is forced — and it therefore has to answer a
  question about the reconcile's outcome. It used to ask *"does a grid hold this entry now, and
  still want it"*, which is the set as of **now**; the reconcile places a grid on the entry whose
  **key** it matches, so a grid could join an entry the bake step had excluded. Two grids swapping
  configurations mid-loss re-baked **neither**: measured, `bakes()` 1 against `atlasCount()` 2.
  The prediction now lives on the registry (`ConfigRegistry::ids_wanted_by`), where the old
  predicate is not merely wrong but **unwritable** — that type does not know which grid holds what,
  so the only question it can ask is the one it should.
  **The pixel consequence was looked for and not found, which is worth knowing before it is assumed
  again**: an un-rebaked atlas kept drawing correctly here, and the reason is most likely the
  harness rather than the renderer — this browser hands back a non-null `createTexture` on a lost
  context and its *simulated* loss does not appear to discard texture contents. So this territory's
  proofs can gate the **state** (an entry that survives is re-baked) and structurally cannot gate
  the symptom.
- **The setters' deferral guard was missing a third window, and it is the one this territory is
  named for** (#772). `gpu_work_must_wait` asked the context and the `is_lost` flag; `on_restored`
  clears `is_lost` and sets `pending_rebuild`, so between `webglcontextrestored` and the rebuild both
  sources answered *"fine"* while the program, VAO and atlas were still the destroyed ones. Its own
  doc-comment claimed that window was covered. The composition now lives on the state machine beside
  `action` (`ContextState::must_defer`), which is where ADR-0027 D1 puts it — the source that owns
  the flags answers the question about them — and a setter in that window defers instead of building
  into resources `restore` replaces one frame later. It had
  to, because a draw loop turns a stale per-grid GPU object from *nothing draws it* into *the wrong
  grid's cells are drawn* — binding a VAO from the dead context raises `INVALID_OPERATION` and leaves
  the previously bound one in place. The refill came with it (`restore` ends in an
  `upload_instances` per slot, against a baseline invalidated for every grid), so what was owed was
  not more *code* but the **evidence**. **Supplied by #774**, and the shape of it is the part worth
  keeping: a pass that reads the drawing buffer can only see the grids that paint, so the recovery of
  a grid with no viewport is not a hard thing to assert — it is an *unobservable* one, until the
  proof places the grid after the restore and reads what appears. `demo/context-loss-grids.html`
  loses one context with four grids in four states (drawn · drawn-then-hidden · fed but never drawn ·
  registered *and* fed inside the loss window) and compares each grid's **own rect**, because a
  whole-buffer comparison passes as long as the visible grids are right, which is the claim in doubt.
  The load-bearing case is the drawn-then-hidden one: it was packed once and nothing has dirtied it,
  so `render` will not re-pack it and cannot repair anything — measured, `packs()` moves by **0** at
  its placement — leaving `restore`'s own refill as the only thing that can have filled the new
  buffer. Narrowing either half of that refill to the grids that draw was mutation-tested and turns
  exactly that rect blank, with every other check on the page green

## Known holes / open

- ~~**Zero governing records**~~ — **closed 2026-08-03 by ADR-0027.** The anchor was spine `#689`,
  opened on an explicit falsifier: *derive a fourth site nobody had to be told about, or settle a
  question before it is asked*. Both halves fired — #695 was found by asking the rule of every entry
  point, and the same pass classified two further sites without being asked — so the spine promoted
  and closed. Kept here rather than deleted because the *shape* is the reusable part: this territory
  went from zero records to one by opening a cheap hypothesis at the second rhyming issue instead of
  waiting for the archaeology that produced the repo's other two records at cluster sizes of 20 and 9.
- ~~**Nothing draws a glyph whose ink leaves its cell across a restore**~~ — **closed 2026-08-21
  (#793)** by `demo/context-loss-neighbour.html`, which is also the page that made the spurious
  `INVALID_OPERATION` above visible. It asserts two things that fail for different reasons: the band's
  ink comes back unchanged, and the post-restore frame raises no GL error at all.
- **One site still resolves against ADR-0027 as a defect**, not as an open question: the unguarded
  `apply_frame` / `apply_damage` chain, safe *only* by the validity condition stated in the design
  model above. (`render`/`action()` was the other; #695 closed it.) Nobody has been asked whether it
  is worth fixing — the answer turns on whether the clearance is a design or an accident, and that
  is a judgement, not a measurement.
- ~~**Nothing presents after the host pays a density change's obligations, on either path.**~~
  **Closed in #808** by the coalesced `requestRender()` in the design model above. Kept because
  *how it was found* is the reusable part: it was invisible to the whole browser corpus, because
  `demo/shared-surface.html`'s handler ends with a `pane.push()` that a real host does anyway and
  that happens to schedule exactly the missing frame — the #776 shape, where the proof supplies the
  thing under test. The page now takes that push behind a flag so a probe can construct the
  contract-minimal host, which is the only shape in which the defect is observable at all.
  **And the first attempt to measure it was invalid, which is worth more than the fix.** Comparing
  the two library versions one rAF *after* the restore turn returned `0,0,0,0` for both, because a
  presented frame is composited and the buffer is gone — so it could not tell "cleared" from
  "already discarded". A read of an unpresented buffer only means something inside the frame the
  library itself scheduled.
- **Not deleting the displaced objects leaves their glow handles behind, once per restore.** glow
  keeps each GL object's JS handle in a `SlotMap` that only `delete_*` removes (`glow` 0.18.0,
  `src/web_sys.rs`, `delete_buffer` / `delete_texture` / `delete_program` /
  `delete_vertex_array`), so since #793 every restore leaves the program, the quad VBO, each
  grid's VAO and instance buffer, and each configuration's atlas in those maps for the renderer's
  lifetime. The comment `restore` carried before #988 gave freeing those slots as *the* reason to
  delete, and #793's record weighs only the `INVALID_OPERATION` flag. Found reading source during
  #988; not measured, and nobody has been asked whether it matters.
- **No reference comparison at all**, and the usual comparison set does not apply cleanly — see
  ADR-0027's *Named prior art* for why the absence is itself the finding.
- ~~**The interaction with the upload planner is stated here and nowhere else.** That a restore must
  invalidate the diff baseline is exactly the kind of cross-territory rule this map exists to hold,
  and it currently has no test naming it.~~ — **stale since #774, measured 2026-09-28 (#988).**
  `demo/context-loss-grids.html` names the rule, and it is not the only page that holds it: with
  `restore`'s `invalidate_baseline` call removed, **20 of 24** context-loss proof runs go red — every
  page but `context-loss-construct.html` (which never restores) at all four DPRs — against 24/24
  green on the unmutated build.
