# Territory — widget lifecycle

## What it is

Who owns the work that keeps happening after the call that started it returns — a rAF loop,
media-query and window listeners, a11y timers, resize observation — and who is responsible for
stopping it.

**One seam is composed; the rest are not.** Since #606 the widget ends what it was *handed* — the
frame source's subscriptions, its own DOM listeners, and the renderer — and that rule is written where
a consumer reads it (`justerm-web/README.md` § Tearing down, plus the `Renderer` port's doc). Every
other piece was added by the slice that needed it, has a perfectly good teardown handle, and still has
nobody calling it.

The distinction is the useful part: the composed seam is the one where a *type* could carry the
obligation. The rest depend on a consumer remembering, and the measurement below says none does.

## Governing decisions

**None.**

- The measured inventory this note routes to is in spine **#605** — *"justerm-web's background work
  has no lifecycle owner"*. A GitHub issue, so not a graph node

## Design model

**One rule is settled (#606); the rest of the inventory is still unowned.**

- **Ambient work must survive its own body throwing, and that is a scheduling-order property, not an
  error-handling one** (#696). A self-perpetuating rAF loop that re-arms *after* its body leaves a
  stale handle behind when the body throws — and a re-entry guard reading that handle then refuses
  every restart, so the loop is off for the life of the widget with nothing logged. Clearing the
  handle *first* (the reference's shape, `RenderDebouncer._innerRefresh`) makes the loop restartable
  without catching anything, so the error still reaches the browser. For this widget "restartable"
  means "restarted by the next frame": `updateCursor` calls `start()` on every decoded frame.
  Catching instead would force a choice between swallowing the error and re-raising it sixty times a
  second. The corollary: a loop body must not call `start()` on its own loop — during the body no
  frame is scheduled, so the guard would let a second loop begin and double the blink rate.
- **The context-loss notification goes through a relay, not the consumer's function** (#579), because
  of two asymmetries in the renderer's published surface this layer may not reach across.
  `setOnContextLoss(callback: Function)` has **no unset**, so detaching or replacing a handler has
  nothing to pass — registering `notify` once and swapping behind it is what makes `set(undefined)`
  expressible. And `dispose()` **cannot reach the renderer's callback slot**: the renderer clears it
  in `Drop`, which runs at the binding's `free()`, which `Terminal.dispose()` deliberately never
  calls (#606) — so without the relay's `end()` a deadline armed before disposal still delivers to an
  ended widget. That is parity, not invention: xterm.js's disposable clears its restore timeout
  (`addons/addon-webgl/src/WebglRenderer.ts:161-163`). **The post-`end` gate sits in `set`, not in
  `notify`**, and a mutation test is what chose it: with a gate in both plus `end`'s clear, neither
  gate can be made to fail because each masks the other; the two placements deliver identically but
  differ on what `end` promises — gate the delivery and a post-`end` `set(handler)` still parks the
  consumer's closure on an object the renderer holds until `free()`, for the life of the page. Gate
  the installation and the promise holds by construction, and every mechanism stays falsifiable.

- **What the widget is handed, the widget ends.** `Terminal` receives exactly three things
  (`source`, `renderer`, `options`), and `dispose()` now releases all of them: both `FrameSource`
  subscriptions, its own DOM listeners, and — since #606 — the renderer, through an optional
  `dispose?()` on the `Renderer` port. Derived, not invented: xterm.js disposes each
  consumer-constructed addon from `Terminal.dispose()`, and this repo's other injected port already
  worked this way through the `Unsubscribe` it returns. See
  [reference behaviour](#reference-behaviour).
- **State that only ever arrives as a *change* has to be established by whoever owns the
  transitions, and `mount()` is where that happens** (#912). Focus reaches the renderer as
  focus/blur intents, so a terminal nobody clicks is never described at all — and until #912 both
  holders of the flag assumed focus, which blinked the caret and painted the *active* selection tint
  in every pane the user had not touched. Reported from PenTerm with several panes on screen.

  **Measured before generalising, because the obvious rule over-reaches.** The `Renderer` port has
  exactly two members carrying pushed boolean state, and only one has this hole: a composition
  cannot be in progress at mount (`setComposing` is driven by a browser event that has not fired),
  while focus can already be anywhere. Every other initial value already had a path — the options
  ones are applied by `create`. So this is one fact with one instance, not a cross-cutting
  invariant, and it is written here rather than promoted.

  **Both halves, because they close different holes**, and the references say why the pair matters
  more than the value: the corpus splits 2-1 on the default and what all three share is a
  *correction path*, so a default alone is not a contract
  ([reference facts](../../agents/reference-facts.md#the-initial-focus-state--who-establishes-it-912-verified-2026-09-16)).
  The unfocused default is the only half that reaches a `Terminal` built without `element`, which
  never attaches; the `attach()` call is the only half that reaches a consumer-supplied `Renderer`,
  whose own default nobody here controls. It reports to the renderer directly rather than through
  the input sink — a mount is not a user action, and a consumer that encodes focus reports would
  otherwise write bytes to the PTY at mount.
- **`Terminal.dispose()` is end of life, not unmount.** `mount()` after it throws. Declared rather
  than left open because the alternative was already broken: `textareaCell` and `cursorAnchor`
  survive disposal (a remounted widget parks the IME candidate window at the previous mount's anchor;
  since #631 only until the next focus or composition start re-syncs it, which shortens that window
  rather than closing it), and a
  re-mounted renderer would have lost its `prefers-reduced-motion` listener permanently — its only
  registration is in a private constructor.
- **It stops work and releases its grid** (since #770 added `removeGrid`). The renderer's wasm
  instance, GL context and the canvas context-loss listeners its Rust side owns survive `dispose()`;
  they belong to the binding's `free()`, which is unsafe while the consumer still holds the object.

Inventory, re-measured 2026-07-29 — the sweep #605 asked for:

| Ambient work | teardown handle | who calls it |
|---|---|---|
| the renderer's rAF blink loop + reduced-motion listener | `dispose()` | **`Terminal`** (#606) |
| the context-loss notification the renderer holds for the widget's life (#579) | `dispose()`, via the relay's `end()` | **`TerminalSurface`** since #775 — it moved off the terminal because `setOnContextLoss` takes no grid: one context means one loss, and a per-terminal channel would both deliver it N times and let the first terminal to end close it for its siblings. It still *must* be closed by hand, for #579's original reason — the renderer's own teardown of that slot is in `Drop`, i.e. at `free()`, which nothing here calls |
| the density watcher and the `webglcontextrestored` listener (#325) | `dispose()` | **`TerminalSurface`** since #775, and for the same reason: both are surface-scoped (`setDevicePixelRatio` takes no grid, and there is one canvas), and both *draw*. On the terminal they were a live defect the moment a second terminal existed — the first `dispose()` took density tracking and context recovery away from every sibling |
| input attachment | returns a disposer | `Terminal`, via `detach` |
| the frame source's two subscriptions | returns `Unsubscribe` | `Terminal` |
| the scrollbar's window listeners | `dispose()` | nobody |
| a live pointer gesture's window listeners and the selection tick interval (#902) | `dispose()` | `Terminal`, via `detach` |
| resize observation | returns a disposer | nobody — the demo writes `void disposeFit;` |
| the a11y controller's announce timer | `dispose()` | nobody |
| the accessible view's keydown | **none** | — |
| the search debounce | **none** | — |
| the marker index's in-flight pull (`MarkerIndexCache`) | `reset()` — which orphans the flight rather than cancelling it, since a `Promise` has no cancel | nobody. `terminal.ts` has **zero** references to the cache: it is consumer-constructed and never handed to `Terminal` at all. Added 2026-08-06 (#746), where the row became load-bearing: an *orphaned* pull's rejection used to clear the flag the replacing pull owned |

- **The remaining rows share one cause and it is not the renderer's.** Every collaborator above is
  consumer-constructed and exported individually; the only thing with a `dispose()` a consumer is
  plausibly told to call is `Terminal`, and it owns only what it builds. Measured (2026-07-29): every
  `.dispose()` call in production code is a *decoration handle* (`lineDecoration`, `full`, `gutter`) —
  no collaborator lifecycle is ended anywhere, and `Terminal.dispose()` itself is called nowhere
  outside tests.
  **This bullet said "there is no composition root" until #775, and that half is now answered** — see
  `## Known holes` for what the answer covers and what it does not. The measurement above is unchanged
  and is still the reason the rows below have no caller: a surface owns the canvas and the context, not
  the scrollbar, the fit observer or the a11y timers.
- **Why #606 was separable from that.** The renderer's was the only row a consumer could not close by
  discipline: `dispose` was not on the port, a **type-level** obstacle rather than a missing call.
  The rest are callable and uncalled, which is a different question — tracked on the spine #605.
- **How a `JustermRenderer` is built** (`create`, `attach`, `build`, `assemble`). It is a thin
  translator because the renderer owns the compositing — the full-stack pivot's payoff: the beamterm
  adapter composited in TypeScript (`CellMirror` + `makeRenderPolicy` + `composeOverlayDraws`) because
  beamterm had no such concepts. Overlay, cursor and decoration state is consumer-pushed every frame
  and retained by the renderer, exactly like the cursor (#273), and set before the frame's damage so
  the frame packs once.
  - *Both entry points take one construction path*, so a sole tenant and a shared one differ by a
    parameter (`composedSurface`) rather than by a second body that can drift. `create` composes the
    surface and keeps it in a private field, so it is the surface's only possible tenant; `attach`
    receives a surface a host opened, which may have siblings. Composing it sizes the buffer to the
    one grid (#331's exactness), presents synchronously, and ends the surface on dispose — the last
    being [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md).
    This used to be mirrored on the surface as an `ownsExtent` option with a guard refusing a second
    tenant; #802 deleted both, since the guard defended a state that cannot be constructed, and
    `test/published-seam.types.ts` §3 pins that unreachability. "Sole tenant" is anchored on the flag
    for the same reason.
  - *The wasm modules load with dynamic `import()`.* Two top-level wasm-bindgen "bundler" imports race
    their init and the second fails (`__wbindgen_externrefs` undefined), so deferring to runtime lets
    vite instantiate each cleanly — the beamterm adapter's reason too. The renderer's module is the
    surface's (constructing it binds the context to a canvas); `create` starts the decoder's import
    in parallel with opening the surface, because splitting the old `Promise.all` across two objects
    made them serial — pure startup latency. The adapter is not exercised by vitest (it needs a GL
    context and the wasm); its pure wire logic is unit-tested and the whole path is proven by the
    demo's headless e2e and the renderer's own GL proofs.
  - *The first frame packs once*: `setOverlay`'s re-pack is a no-op until the first `apply_damage`,
    and the frame's overlay, decorations and cursor are set before its damage.
  - *The grid is named at birth* (#773, #928, #961): the seven selectors go into `addGrid`, one bake,
    where pushing them by setter afterwards baked up to eight, each of the first seven freed by the
    next. The values are the ones the setters used, defaults included, so the initial fit is still
    computed at the consumer's final cell. That also retired an ordering question — font had once to precede spacing, a dependency the
    renderer had already removed, since every path that changes the glyph box, the DPR or either
    spacing funnels through one function (`recompute_cell` up to 0.14.x, `bake_config` after, #772).
  - *`build` and `assemble` are an error boundary.* A grid is GPU memory — a VAO, an instance buffer and
    a refcount on its configuration's atlas (4.2 MiB at an 8x16 cell, 12.8 MiB at 15x30 on a dpr-2
    display, measured for #773's follow-up) — and nothing holds it if assembly throws, so `build`
    releases it on a throw, and `assemble` is split out so that `try` stays readable. `create` does the
    same for the surface: a throw after it exists would strand a bound WebGL2 context, a running
    density watcher and a canvas listener, and a retry on the same canvas would get the same context
    back with the orphan's listeners firing beside the new surface's.
  - *A density change and a context restore reach a terminal through one registration* (`onReapply`),
    because from the terminal they are one obligation: re-derive its geometry at whatever the cell
    became. The surface owns *when*; only the terminal knows *what*. The terminal registers `onEnd`
    too, so a host that disposes the surface ends the widgets on it — otherwise each keeps its blink
    loop and reduced-motion listener while holding an id the renderer has retired, and every per-grid
    call throws `UnknownGrid` on a timer.
  - *A terminal starts unfocused*, and `Terminal` also reports focus once at mount. A renderer is told
    about focus *changes*, so a never-focused pane is never told anything, and the old `true` stood for
    the life of every pane the user had not clicked. The corpus splits 2-1 on the initial value and
    what the three share is a correction path
    ([the INITIAL focus state](../../agents/reference-facts.md#the-initial-focus-state--who-establishes-it-912-verified-2026-09-16)). It also fails safe: a focused
    terminal that reads as blurred recovers on the first keystroke, a blurred one reading as focused
    never did.
  - *The rect's origin is stored and its extent re-derived*: the extent follows from the grid and the
    cell, while the origin is the host's measurement. The host re-supplies the origin whenever the box
    moves because WebGL binds one context to one canvas — a terminal is a transparent overlay over its
    viewport, and nothing inside the GL layer can observe the overlay drifting from it.
- **The blink loop and the present** (`JustermRenderer.render`, `blinkTick`, `trackBlinkCells`,
  `setFocused`, `dispose`).
  - *`render()` presents the whole canvas*, since it takes no grid, so how it presents follows from
    whether the terminal is alone. A sole tenant presents synchronously, exactly as before #775: one
    terminal is one present per frame either way, coalescing would only add a frame of latency, and
    the whole e2e suite drives a frame and reads pixels in the same turn, so deferring would silently
    change what many unrelated assertions mean. A terminal sharing a surface requests a present on the
    surface's loop, coalesced with its siblings' — N synchronous presents would redraw the canvas N
    times a frame. The two agree at N=1, which makes this a derivation rather than a mode switch;
    `Terminal` calls `render` on every decoded frame and cannot choose, so without it a shared widget
    could not reach the loop the surface exists to run.
  - *The SGR 5 phase re-pack is gated on the grid possibly holding a `BLINK` cell* — xterm.js's
    `needsBlinkInViewport` (`TextBlinkStateManager.ts:67`) adapted to frame mode. xterm.js answers
    exactly by scanning the viewport it owns; a frame-mode consumer holds damage, not the grid, so the
    gate is **conservative**: a false positive costs one redundant re-pack, a false negative would
    freeze blinking text. A Full frame replaces the answer, a Partial one can only add to it, and the
    answer decays only at the next Full frame. The Full case's exactness rests on core emitting full
    damage as every row at full width (`justerm-core/src/term.rs`, `TermDamage::Full`), which
    `FrameKind::Full` states as its contract (*"Every row is present"*, `serialize.rs`) — if a Full frame
    became a subset, this gate would produce the one error it must not. Without the gate an opted-in
    consumer pays a full re-pack per half-period on every terminal; measured (demo, 600 ms interval,
    3.0 s, presenting rAF turns, identical conditions): 16 with three blinking cells on screen, 11 with
    none — a delta of 5, exactly the 5 phase flips in the window, each proportional to `cols x rows`.
  - *A hidden terminal's tick does nothing* (#801). Both halves of a tick end in `backend.render()`,
    which presents the whole canvas, so on a shared surface a hidden pane's blink redrew its siblings
    twice a second for pixels not on screen. The re-pack was already gated in the renderer's draw
    loop, which is why this was invisible — what was wasted was the present, and no counter at this
    layer reports presents. The cursor half looked self-gated (`CursorBlink.isVisible` is solid when
    blurred, and `display: none` blurs the textarea), but `TextBlink.isVisible` has no focus gate, and
    `hide()` with no DOM change — which the README recommends for `visibility: hidden` — leaves the
    terminal focused. rAF stops for a hidden *document*, but a terminal scrolled out of view inside a
    visible page keeps ticking — not xterm.js's guarantee, whose `setViewportVisible` is fed by an
    `IntersectionObserver` on the screen element; the gap covers the cursor half equally, so it is a
    widget-level decision, left as it was. A throw in the body stops the loop with no handle left and
    the next `startBlinkLoop` restarts it (`FrameLoop`, #696); before that the re-arm sat at the
    bottom of the body and a throw latched the loop off permanently.
  - *`setFocused` presents even with no caret on screen*, when the tint moved. It is the one setter
    that changes something other than the cursor, and `issueOverlay` only retains and re-packs —
    `redrawCursor` is what presents, which is why `setTheme` pairs the two. Guarding the present on a
    cursor, as `setCursorBlink` and `setComposing` legitimately do, dropped the tint flip whenever the
    application had hidden the caret, and on an idle hidden-caret terminal there may be no next
    present at all. The mount-time call takes neither branch, since `focused` starts `false`. xterm.js
    re-shows the caret from its own focus handler for the same reason
    (`browser/CoreBrowserTerminal.ts:310`, `_showCursor()`).
  - *`dispose` releases the grid* (called by `Terminal.dispose()` since #606; the doc once said
    "nothing calls this yet", which was the defect), so every per-grid method throws after it — the
    honest answer rather than a regression. It used to say "stops work, does not release memory", on
    the ground that `free()` is the only release — untrue since #770 added `removeGrid`; what keeping
    it cost, at the time, was the atlas above per closed terminal until the tab went away: a fixed
    `tex_storage_3d(RGBA8, paddedW, paddedH * 32, 192)` allocation whose size does not depend on how
    many glyphs were used (4.2 / 12.8 MiB), plus the rasteriser and glyph cache on the wasm heap. The
    wasm instance, the GL context and the Rust-side canvas listeners survive until the binding's
    `free()`. The context-loss notification is ended by the surface's own `dispose`, which only a
    terminal that composed its surface reaches — a shared surface's channel belongs to the surface.
    The renderer's `ContextLossHandler` would clear it only in `Drop`, at `free()`, too late; the
    contract matched is xterm.js's, whose disposable clears its pending restore timeout
    (`addons/addon-webgl/src/WebglRenderer.ts:161-163`). `isContextLost()` / `isRestoreOverdue()` keep
    answering, since they read the state machine the surviving canvas listeners still feed.

## Code

- `justerm-web/src/terminal.ts` — `Terminal`, `dispose`, and the listeners it owns. `setSuggestion`
  (#972) refuses after `dispose`: a consumer's ranker answers asynchronously and can land after the
  pane is gone, and the renderer throws on a disposed grid. `track` also drops the suggestion on
  entering the alternate screen
- `justerm-web/src/renderer.ts` — the port, and the `dispose?()` that made the renderer reachable
- `justerm-web/src/justerm-renderer.ts` — the blink tick and the reduced-motion listener, and
  the `dispose` `Terminal` now calls
- `justerm-web/src/terminal-surface.ts` — `TerminalSurface`, the composition root this territory spent
  two issues without (#775). It owns the canvas, the context, the grid registry, the density watcher,
  the context-loss relay and the one presenting loop, and its `dispose` ends every grid still attached
  before its own ambient work. **It is also the first thing here with an instantiation seam**:
  `SurfaceDeps` injects the backend, `raf`/`caf` and the resolution query, so the *composition* is
  host-tested rather than browser-only — the trade #696 and #579 each declined, taken the third time
  because a composition root has nothing left over to extract
- `justerm-web/src/frame-loop.ts` — `FrameLoop`, which owns the rAF handle for that tick. Host-tested
  with an injected `raf`/`caf` pair, because the widget around it has no instantiation seam (#696)
- `justerm-web/src/context-loss.ts` — `ContextLossRelay`, the channel `dispose` closes. Extracted for
  the same reason `FrameLoop` was; the two published-surface asymmetries that make the indirection
  necessary are in the design model above (#579)
- `justerm-web/src/scrollbar.ts` · `fit.ts` — collaborators with their own disposers
- `justerm-web/src/accessibility-dom.ts` — the a11y timers and their teardown

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated**.

- [Who ends a component the consumer handed over](../../agents/reference-facts.md#widget-teardown--who-ends-a-handed-over-component-606-verified-2026-07-29)
  — xterm.js disposes each consumer-constructed addon from `Terminal.dispose()`, idempotently, and
  its lifecycle is one-shot by construction. That last part is a **condition** on the rule, not
  decoration: the reference is safe to copy only because its dispose is end-of-life, which is why
  #606 had to declare the same thing rather than assume it.

## Cross-cutting invariants

- [an awaited in-page promise needs an anchor](../invariant/an-awaited-in-page-promise-needs-an-anchor.md)
  — this territory's contracts (context loss, dispose, restore) are provable only in a browser, and
  two of them are read through parked hooks. A read path that can fail for a reason it does not
  report is a hole in the evidence for everything below, not in the behaviour itself
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — `textareaCell` is a cached *decision* that outlives the geometry it was computed from, so a cell
  change with a stationary cursor left the IME anchor stale (#578, fixed by #631). It is a lifecycle
  fact rather than a geometry one: the cache still has no invalidation path, and #631's answer was
  not to give it one — the widget cannot observe a cell change, because `getGeometry` is a
  consumer-supplied *pull* callback. Instead the anchor is re-read at the moments something reads it
  (composition start, focus). So the cached decision still outlives its geometry between those
  moments **by design**; what changed is that nothing reads it while it is stale.

- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — the anchor's point-of-use re-read (#631) sits in a `compositionstart` handler because that is the
  moment the OS reads it, and the widget can only learn of it from a browser event. The same fact is why
  the frame-driven path has no composition gate to key on (#637).

- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — **promoted out of this territory by #775, and the promotion corrected it.** This section used to
  read *"no invariant originates here"* and carried the rule the territory gained at #606 — *"what a
  layer is handed across a port, that layer ends"* — with a standing instruction to promote it the day
  a second injected-collaborator port existed, naming #287's `TerminalSurface` as the likely candidate.
  That is exactly what arrived, and it **falsified the phrasing**: `JustermRenderer.attach` is handed a
  surface and deliberately does *not* end it, because the surface is shared. The criterion is
  **exclusivity**, not provenance — a layer ends what it is the only holder of, whether it built it or
  was handed it. Both of this territory's sites still derive from the corrected rule; what changed is
  that the composition case (`create` builds the surface, so it ends it) stopped needing a clause of
  its own.

  Kept here rather than replaced by the link, because it is the part the note cannot say about
  *itself*: the reach check that found one site (#606) was right to leave it, and the one-site
  phrasing was wrong. A rule promoted on the strength of one site would have shipped the provenance
  wording into the first host that attaches two terminals.

## Blast radius

Everything the widget attaches, because the missing owner is a property of the composition rather
than of any one collaborator.

- [caret drawing](caret-drawing.md) · [caret report](caret-report.md) — the blink phase is driven by
  the rAF loop this territory owns. `Terminal` can now **end** it (#606); what it still cannot do is
  **pause** it, so neither blink loop stops while the terminal is off-screen (#607, closed
  `NOT_PLANNED` on a measurement — see `## Blast radius`). Ending and pausing turned out to be
  separable questions, which is why one shipped without the other
- [GL context lifecycle](gl-context-lifecycle.md) — the consumer sets the restore timeout and reacts
  to the callback. Since #579 that owner **is** stated: the widget registers the channel and closes
  it on `dispose`, which is the second rule this territory has (after #606's) and the first one that
  had to be *added* rather than merely written down — the renderer's teardown for it fires at a
  moment the widget never reaches
- [events & replies](events-and-replies.md) — both queues are drained on a cadence the consumer
  chooses, and a widget that is disposed but still queueing has no defined behaviour
- [accessibility](accessibility.md) — its timers are ambient work, and `reactivate()` already carries
  a reset obligation that a lifecycle owner would otherwise hold
- [release](release.md) — `justerm-web` consumes the *published* wasm decoder, so its startup path
  depends on a version it does not control

## Known holes / open

- **One rule now exists; the territory is still mostly convention.** #606 settled what happens to
  what the widget is *handed*. Six collaborators the consumer keeps have teardown nobody calls, and
  two have none at all — that is a **composition-root** question, and it now lives here rather than
  on an issue.
  **Measured 2026-08-21, which is why #605 closed.** The half that was a hypothesis is answered: the
  anchor predicted that each new slice would re-decide ownership locally, and the three ambient
  modules added *after* it was filed did the opposite — `frame-loop.ts` (2026-08-03),
  `context-loss.ts` (08-04) and `dpr-watcher.ts` (08-10) each carry a teardown, and the last two cite
  #606 in their own source. Five source files now quote that rule. The half that remains is real and
  unchanged — the only `Terminal.dispose()` calls outside tests sit inside the demo's *proof* probes,
  and the `Scrollbar` and the resize observer are never disposed on any ordinary path — but nothing
  can move it: a composition root is a question a **host application** asks, and `justerm-web` had no
  consumer when this was written. **penterm adopted the widget on 2026-09-15** for its Native (beta)
  panes (`../penterm/src/blocks/terminal/components/NativeTerminalSurface.tsx` mounts a `Terminal`
  and pushes its `dispose` into the effect's cleanups), so this is now live; this bullet is where
  that reader is standing.
  **#775 moved it, and the way it moved is worth recording because #605's closing note predicted it
  could not be.** That note was right about the reason and wrong about the reach: a composition root
  *is* a question a host asks, and what changed is that this package now has to answer it for itself.
  One WebGL2 context serving N terminals means something must own the canvas, the registry, the
  density and the recovery for longer than any one widget lives — so `TerminalSurface` is that root,
  and its ownership rule fell out of building it rather than being decided in the abstract:
  [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md).
  **What it does NOT cover, unchanged:** the surface owns what it composes, and the six
  consumer-constructed collaborators in the table above are not among them. `Scrollbar`, the resize
  observer, the a11y timers, the accessible view's keydown, the search debounce and the marker index
  are still callable and uncalled, and still wait on a host that adopts the widget. The root answers
  *who ends the canvas*, not *who ends everything*.
- ~~`Terminal.dispose()` cannot reach the renderer's blink loop.~~ Closed by #606: `dispose?()` is on
  the `Renderer` port and `Terminal.dispose()` calls it, proven in a real browser by counting the
  loop's presenting rAF turns before (>0) and after (0) disposal.
- **Neither blink loop pauses when the terminal is off-screen** — but read the two corrections
  before acting on it. A backgrounded *tab* does **not** keep animating: `rAF` stops for a hidden
  document, which is why #607 was about element visibility inside a *visible* page, and it closed
  `NOT_PLANNED` on a measurement — 5 presents in 1500 ms, all 5 from the demo's own frame timer and
  **0** from the blink loop, because the shipped defaults are a steady cursor and text blink off.
  So this is not tracked work; it is a validity condition, and #607 stated it: a consumer setting
  `cursorBlink: true` or `textBlinkInterval > 0` gets a real flip about twice a second.

  **#801 answered half of it, and the halves are not interchangeable.** That slice gave the widget
  the visibility input #607 said it did not have — the `hidden` flag — and `blinkTick` now returns on
  it, so a terminal the host has **set aside** drives nothing: measured on the two-terminal page, 4
  whole-canvas presents in 2100 ms before the gate and 0 after, with a shown sibling holding at 3
  either way. What is *still* open is #607's actual question, **scrolled out of view inside a visible
  page**, which no flag here can see because nobody tells the widget about it. The two references do
  not agree on the boundary either, and neither contains the other: xterm.js's `IntersectionObserver`
  catches a scrolled-away pane and `display: none` but **not** `visibility: hidden` (the box survives
  and still intersects); our flag catches whatever the host reports, which via `observeViewportRect`
  is `display: none` and via `hide()` is anything at all. So #607 is not reopened by this — it would
  need its own observer, and that is the decision it was closed on.

  The half that *was* closed had a narrower trigger than it looks, and the two blink clocks differ:
  `CursorBlink.isVisible` parks solid when `!focused` and `display: none` blurs, so the ordinary hide
  path self-gated; `TextBlink.isVisible` has no focus gate at all. The path that made it reachable is
  the one `justerm-web`'s README points at — `hide()` called directly for `visibility: hidden`, where
  no observer fires and focus is kept.
- **Focus is defined by the element alone, and nothing says that is the whole definition.** The
  widget drives `setFocused` from the hidden textarea's focus/blur events, so a terminal whose
  textarea is still `document.activeElement` while the *window* is not the user's active window
  keeps blinking and keeps the **active** selection tint — the same harm #912 removed, one state
  later, and one `rAF` does not park because the page is still visible. xterm.js ANDs its read with
  `ownerDocument.hasFocus()` and re-derives it once per microtask
  ([reference facts](../../agents/reference-facts.md#the-initial-focus-state--who-establishes-it-912-verified-2026-09-16)).
  **Unmeasured, and the instrument is the reason**: two independent attempts during #912 failed the
  same way — headless Chromium reports neither a `blur` on the focused textarea nor a change in
  `document.hasFocus()` when another tab is selected, while a positive control (moving focus to
  another element) does log `blur`. So the recorder is on the call path and headless simply does not
  model window deactivation. Settling it needs a headed browser with two real OS windows, reading
  the blur log and `document.hasFocus()` in one turn. Recorded rather than filed, because a gap
  whose measurement failed is a gap in the evidence and not yet a defect.
- **No reference comparison** for teardown composition, which is the one thing a widget library is
  usually judged on by its consumers. (Its *scheduling* half now has one — see below.)
- **The widget still cannot be constructed in a test.** `vitest.config.ts` runs the `node`
  environment and the constructor reads `window.matchMedia`, so there is no `RendererBackend`-fake
  path into `JustermRenderer` and none of its behaviour is unit-covered. #696 worked *around* this
  by extracting the one piece that had to be tested rather than by building the seam, and said so.
  **#579 took the same trade a second time** (`context-loss.ts`, the relay), which is the part worth
  recording: the workaround is the established shape now rather than one slice's expedient, and each
  use leaves the *composition* — what `create` registers, what `dispose` closes — provable only in a
  browser. For #579 both are asserted in `e2e/demo.spec.ts` against a real `WEBGL_lose_context` and
  both are mutation-verified, so this is coverage by a slower gate rather than absence. The slice
  that reaches for the extraction a third time should price building the seam against it.
