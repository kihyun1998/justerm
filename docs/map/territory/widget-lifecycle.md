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

- **What the widget is handed, the widget ends.** `Terminal` receives exactly three things
  (`source`, `renderer`, `options`), and `dispose()` now releases all of them: both `FrameSource`
  subscriptions, its own DOM listeners, and — since #606 — the renderer, through an optional
  `dispose?()` on the `Renderer` port. Derived, not invented: xterm.js disposes each
  consumer-constructed addon from `Terminal.dispose()`, and this repo's other injected port already
  worked this way through the `Unsubscribe` it returns. See
  [reference behaviour](#reference-behaviour).
- **`Terminal.dispose()` is end of life, not unmount.** `mount()` after it throws. Declared rather
  than left open because the alternative was already broken: `textareaCell` and `cursorAnchor`
  survive disposal (a remounted widget parks the IME candidate window at the previous mount's anchor;
  since #631 only until the next focus or composition start re-syncs it, which shortens that window
  rather than closing it), and a
  re-mounted renderer would have lost its `prefers-reduced-motion` listener permanently — its only
  registration is in a private constructor.
- **It stops work, not memory.** The renderer's wasm instance, GL context, glyph atlas and the
  canvas context-loss listeners its Rust side owns all survive `dispose()`; they belong to the
  binding's `free()`, which is unsafe while the consumer still holds the object.

Inventory, re-measured 2026-07-29 — the sweep #605 asked for:

| Ambient work | teardown handle | who calls it |
|---|---|---|
| the renderer's rAF blink loop + reduced-motion listener | `dispose()` | **`Terminal`** (#606) |
| the context-loss notification the renderer holds for the widget's life (#579) | `dispose()`, via the relay's `end()` | **`Terminal`** — and it *must* be, because the renderer's own teardown of that slot is in `Drop`, i.e. at `free()`, which nothing here calls |
| input attachment | returns a disposer | `Terminal`, via `detach` |
| the frame source's two subscriptions | returns `Unsubscribe` | `Terminal` |
| the scrollbar's window listeners | `dispose()` | nobody |
| resize observation | returns a disposer | nobody — the demo writes `void disposeFit;` |
| the a11y controller's announce timer | `dispose()` | nobody |
| the accessible view's keydown | **none** | — |
| the search debounce | **none** | — |
| the marker index's in-flight pull (`MarkerIndexCache`) | `reset()` — which orphans the flight rather than cancelling it, since a `Promise` has no cancel | nobody. `terminal.ts` has **zero** references to the cache: it is consumer-constructed and never handed to `Terminal` at all. Added 2026-08-06 (#746), where the row became load-bearing: an *orphaned* pull's rejection used to clear the flag the replacing pull owned |

- **The remaining rows share one cause and it is not the renderer's.** Every collaborator above is
  consumer-constructed and exported individually; the only thing with a `dispose()` a consumer is
  plausibly told to call is `Terminal`, and it owns only what it builds. **There is no composition
  root.** Measured: every `.dispose()` call in production code is a *decoration handle*
  (`lineDecoration`, `full`, `gutter`) — no collaborator lifecycle is ended anywhere, and
  `Terminal.dispose()` itself is called nowhere outside tests.
- **Why #606 was separable from that.** The renderer's was the only row a consumer could not close by
  discipline: `dispose` was not on the port, a **type-level** obstacle rather than a missing call.
  The rest are callable and uncalled, which is a different question — tracked on the spine #605.

## Code

- `justerm-web/src/terminal.ts` — `Terminal`, `dispose`, and the listeners it owns
- `justerm-web/src/renderer.ts` — the port, and the `dispose?()` that made the renderer reachable
- `justerm-web/src/justerm-renderer.ts` — the blink tick and the reduced-motion listener, and
  the `dispose` `Terminal` now calls
- `justerm-web/src/frame-loop.ts` — `FrameLoop`, which owns the rAF handle for that tick. Host-tested
  with an injected `raf`/`caf` pair, because the widget around it has no instantiation seam (#696)
- `justerm-web/src/context-loss.ts` — `ContextLossRelay`, the channel `dispose` closes. Extracted for
  the same reason `FrameLoop` was, and its doc carries the two published-surface asymmetries that
  make the indirection necessary rather than stylistic (#579)
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

**No invariant originates here — checked, not assumed (#606).** The rule this territory gained ("what a layer is handed
across a port, that layer ends") was tested for reach before being left here, because a fact that
holds in N territories belongs in `invariant/` and is invisible from the other N-1 if it is not.

It has exactly one site. The criterion is *a port through which a collaborator with its own ambient
work is injected*, and `justerm-web` is the only layer in the family with one: `justerm-renderer`
takes a canvas **selector** and a theme, not a collaborator; `justerm-core` takes bytes and holds no
listener, timer or loop by design (the no-I/O invariant in `CLAUDE.md`). The `DecorationRegistry`
handle is the mirror image — the widget hands *out* something the consumer ends — which is a
different rule, not this one at a second site.

Promote it the day a second injected-collaborator port exists. The multi-viewport work (#287) is the
likely candidate: a `TerminalSurface` holding N grids would be exactly that shape.

## Blast radius

Everything the widget attaches, because the missing owner is a property of the composition rather
than of any one collaborator.

- [caret drawing](caret-drawing.md) · [caret report](caret-report.md) — the blink phase is driven by
  the rAF loop this territory owns. `Terminal` can now **end** it (#606); what it still cannot do is
  **pause** it, so neither blink loop stops while the terminal is off-screen (#607). Ending and
  pausing turned out to be separable questions, which is why one shipped without the other
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
  can move it: a composition root is a question a **host application** asks, and `justerm-web` has no
  consumer (penterm's webview is still xterm.js). It becomes live the day one adopts the widget, and
  this bullet is where that reader will be standing.
- ~~`Terminal.dispose()` cannot reach the renderer's blink loop.~~ Closed by #606: `dispose?()` is on
  the `Renderer` port and `Terminal.dispose()` calls it, proven in a real browser by counting the
  loop's presenting rAF turns before (>0) and after (0) disposal.
- **Neither blink loop pauses when the terminal is off-screen**, so a backgrounded tab keeps
  animating. Tracked: #607.
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
