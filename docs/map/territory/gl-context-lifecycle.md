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
  restore raised it **five** times with the pixels perfectly correct, so nothing in the proof corpus
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
  `addGrid` returns and the cell is a CPU ink-scan a dead context does not obstruct. So a grid asking
  mid-loss for a configuration nobody holds costs **one thrown-away bake** (measured: `bakes()` +1 in
  the loss window, and the restore bakes it again). Bounded, not merely small: `render` cannot draw
  while `gpu_work_must_wait()` holds, and a grid born on a configuration is that configuration's own
  key-matching holder, so the restore always re-bakes it — the second half only true since #788.
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
- **Every entry point that changes the geometry takes the request and defers the GPU work.** Five of
  them can arrive mid-loss — the DPR, the font size, the font family, the spacing policy and the
  resize — and none may reject the call, because a consumer has no obligation to hold it back. It can
  now *see* the loss (#579 wired the surface), but seeing is not the same as being expected to act on
  it: nothing in the contract says a consumer must check, and a setter that rejected would break every
  one that does not. So each stores what it was given and lets `restore` re-derive
  from it; nothing is queued, because the stored value *is* the queue. The four setters skip an
  atlas re-bake that a dead context would return invalidated; `resize` skips reading the drawing
  buffer back, which on a dead context answers 0 and would floor the grid to one cell (#639).
- **"Is the context lost" has two answers and they disagree for a whole window — so the predicate
  is chosen per site, never shared out of habit** (#639). The browser destroys a context
  *synchronously* and merely **queues** `webglcontextlost`; the mirror holds on the way back. So the
  state machine's flag — the honest thing to report to a *consumer*, since it tracks what we have
  been told — lags the context itself, and an internal caller guarding on it is guarding on the
  wrong thing. Measured: in the pre-dispatch window `gl.isContextLost()` is already `true`,
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
  Measured before the fix: dpr 1 → 2 across a loss left a `2556x1369` buffer under a canvas styled
  `1278x703`. Note which half was wrong — the **width** was accidentally correct because the cell
  doubled exactly (9 → 18), and only the height was off (`703` against `684.5`, the cell having gone
  19 → 37). A width-only check would have reported this area clean. **And the accident is font
  dependent, not a property of the fix**: CI's Linux font takes the same 19 to 38, so *both* axes
  divide back evenly there and the box does not move at all. The portable statement is
  `canvas.style x dpr === drawing buffer`, never that the box changed.

## Code

- `justerm-renderer/src/context_loss.rs` — the state machine (pure, host-tested)
- `justerm-renderer/src/webgl.rs` — the event closures, resource recreation, and the four exports
  (browser-only)

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
- **No reference comparison at all**, and the usual comparison set does not apply cleanly — see
  ADR-0027's *Named prior art* for why the absence is itself the finding.
- **The interaction with the upload planner is stated here and nowhere else.** That a restore must
  invalidate the diff baseline is exactly the kind of cross-territory rule this map exists to hold,
  and it currently has no test naming it.
