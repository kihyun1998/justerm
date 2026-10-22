# Territory — widget lifecycle

## What it is

How a `justerm-web` widget comes into being, and who stops the work it leaves running after the call
that started it returns: the `Terminal` that pumps a `FrameSource` into a `Renderer`, the
`JustermRenderer` that is the first-party `Renderer`, the rAF blink loop, the media-query and window
listeners, the context-loss channel, and the collaborators a host constructs beside them (scrollbar,
resize observer, a11y controllers, search, marker index).

**One rule is composed; the rest are convention.** The widget ends what it was *handed* — the frame
source's subscriptions, its own DOM listeners, and the renderer — and that rule is written where a
consumer reads it (`justerm-web/README.md` § Tearing down, and the `Renderer` port's `dispose` doc).
Every collaborator a host builds beside the widget has a teardown handle and nothing in this package
calls it; the host is the composition root (see `## Design model`, *Who ends the rest*).

## Governing decisions

**None.**

- [ADR-0027 — liveness is answered by the source that owns it](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  rules what `isContextLost()` *means*, and says in its own text that who stops the notification
  arriving "is the consumer's lifecycle question" and not that record's
- [ADR-0021 — one WebGL2 context draws N terminal grids](../../adr/0021-single-context-multi-viewport.md)
  decides the surface/grid split this lifecycle rides on (a terminal is a grid lease on a surface), not
  who ends what; that rule arrived by promotion instead, as
  [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
- [ADR-0028 — composition surfaces have one writer each](../../adr/0028-composition-surfaces-have-one-writer-each.md)
  governs the textarea anchor and preedit run `Terminal` drives, not their teardown
- The measured inventory this note grew from was spine #605 (*"justerm-web's background work has no
  lifecycle owner"*), closed 2026-08-21 on the measurement recorded under `## Known holes / open`

## Design model

Status: the rules below come from the source and its doc-comments plus the issues cited; no record
decides them.

**What the widget is handed, the widget ends** (#606). `Terminal` receives exactly three things
(`source`, `renderer`, `options`). `Terminal.dispose()` unsubscribes both `FrameSource` channels,
ends the `ClipboardController` (an in-flight OSC 52 read outlives the event subscription), runs every
DOM detacher, disposes the `LinkTracker`, restores the element's inline `cursor`, removes the hidden
textarea, and calls the renderer's optional `dispose?()` **last**, after the widget has stopped feeding
it. Derived rather than invented: xterm.js disposes each consumer-constructed addon from
`Terminal.dispose()`, and this package's other injected port already worked this way through the
`Unsubscribe` it returns ([reference](#reference-behaviour)). The port requires `dispose` to be
idempotent, and the one consumer exercises that: PenTerm pushes `renderer.dispose()` and then
`term.dispose()` onto one cleanup list it runs in reverse, so the renderer is disposed by the widget and
then again by the host (penterm 7dadc3c95 `src/blocks/terminal/lib/nativeTerminalCache.ts` — `build`,
`disposeNativeTerminal`).

**`Terminal.dispose()` is end of life, not unmount** (#606). `mount()` after it throws. Declared
rather than left open because remounting was already broken: `textareaCell` and `cursorAnchor` survive
disposal (a remounted widget would park the IME candidate window at the previous mount's anchor until
the next focus or composition start re-syncs it, #631), and a remounted renderer would have lost its
`prefers-reduced-motion` listener, whose only registration is in a private constructor. A host that
wants a pane to outlive its mount keeps the widget and moves its element — PenTerm builds one widget
per session, hides its grid on unmount and disposes only when the session ends
(penterm 7dadc3c95 `src/blocks/terminal/lib/nativeTerminalCache.ts` — `mountNativeTerminal`).

**Ambient work must survive its own body throwing, and that is a scheduling-order property** (#696).
`FrameLoop` clears its handle *before* running the body, so a throw leaves nothing for the re-entry
guard to read and the loop is restartable without a `try`/`catch` — the error still reaches the
browser. That is the reference's shape (`RenderDebouncer._innerRefresh`, see
[reference](#reference-behaviour)). Before #696 the re-arm sat at the bottom of the body and one throw
latched the loop off for the widget's life with nothing logged. Catching instead would force a choice
between swallowing the error and re-raising it sixty times a second. *Restartable* here means
restarted by the next frame that starts the loop: `updateCursor` starts it on a frame that carries a
visible cursor, and `applyFrame` starts it on every frame while text blink is enabled — not, as an
earlier version of this note said, on every decoded frame. The corollary is in `FrameLoop`'s own doc:
a body must not call `start()` on its own loop, since during the body no frame is scheduled and the
guard would let a second loop begin (double blink rate).

**The context-loss notification goes through a relay** (#579), because of two asymmetries in the
renderer's published surface. `setOnContextLoss` takes a `Function` and has **no unset**, so
registering `ContextLossRelay.notify` once and swapping the handler behind it is what makes
`set(undefined)` expressible. And `dispose` **cannot reach the renderer's slot**: the renderer clears
it in `Drop`, at the binding's `free()`, which nothing here calls — so without `ContextLossRelay.end`
a deadline armed before disposal still delivers to an ended widget. That is parity: xterm.js's
disposable clears its restore timeout (cited in ADR-0027 at `addons/addon-webgl/src/WebglRenderer.ts:161-163`).
**The post-`end` gate sits in `set`, not in `notify`, and a mutation test chose it** (#579): with a
gate in both plus `end`'s clear, neither gate can be made to fail because each masks the other; gating
the delivery would still let a post-`end` `set(handler)` park the consumer's closure on an object the
renderer holds until `free()`. Gating the installation keeps `end`'s promise by construction
(`justerm-web/test/context-loss.test.ts`, *cannot be revived by a handler registered after the end*).
Since #775 the relay is the **surface's**, not the terminal's: `setOnContextLoss` takes no grid, one
context means one loss, and a per-terminal channel would both deliver N times and let the first
terminal to end close it for its siblings.

**State that only ever arrives as a change is established by whoever owns the transitions — at
`mount()`** (#912). Focus reaches the renderer as focus/blur intents, so a pane nobody clicks was never
described; until #912 both holders of the flag assumed focus, blinking the caret and painting the
*active* selection tint in every untouched pane (reported from PenTerm with several panes on screen).
Both halves exist because they close different holes: `JustermRenderer` and `CursorBlink` start
unfocused (the only half that reaches a `Terminal` built without `element`, which never attaches), and
`Terminal.attach` calls `setFocused(false)` once (the only half that reaches a consumer-supplied
renderer, whose default nobody here controls). It reports to the renderer directly rather than through
the input sink — a mount is not a user action, and a consumer encoding focus reports would otherwise
write bytes to the PTY at mount. The value is `false`, not `document.activeElement === ta`: the
textarea was appended a moment earlier and cannot be active. It also fails safe — a focused terminal
read as blurred recovers on the first keystroke; a blurred one read as focused never did. **Measured
before generalising** (#912): the port has two members carrying pushed boolean state and only focus has
this hole — a composition cannot be in progress at mount (`setComposing` is driven by an event that has
not fired), and every other initial value is applied by `create`. One fact, one instance; not promoted.

**How a `JustermRenderer` is built** (`create`, `attach`, `build`, `assemble`). A thin translator,
because the renderer composites in wasm (#273, the full-stack pivot); overlay, cursor and decoration
state is pushed every frame, retained by the renderer, and set before the frame's damage so the frame
packs once (`applyFrame`).
- *Both entry points take one construction path*, so a sole tenant and a shared one differ by a
  parameter (`composedSurface`) rather than by a second body. `create` composes the surface and keeps it
  in a private field with no accessor, so it is the surface's only possible tenant; `attach` receives a
  surface a host opened. The flag decides three things: sizing the drawing buffer to its own grid
  (#331's exactness), presenting synchronously, and ending the surface on dispose — the last being
  [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md). The
  surface once mirrored this as an `ownsExtent` option with a guard refusing a second tenant; #802
  deleted both, because the guard defended a state that cannot be constructed, and
  `justerm-web/test/published-seam.types.ts` §3 pins that unreachability.
- *The wasm modules load with dynamic `import()`.* Two top-level wasm-bindgen "bundler" imports race
  their init and the second fails (`__wbindgen_externrefs` undefined) — the beamterm adapter's reason
  too (attributed to that adapter's history; not re-run here). `create` starts the decoder's import in
  parallel with `TerminalSurface.open`, since splitting the old `Promise.all` across two objects made them
  serial.
- *The grid is named at birth* (#773, #928, #961): font family, size, both spacings, both weights and
  subpixel go into `TerminalSurface.addGrid` — one bake, where pushing them by setter afterwards baked up
  to eight. That retired an old ordering constraint (font before spacing), which the renderer had already
  removed by funnelling every cell-moving change through one function (`bake_config` since #772).
- *`build` and `assemble` are an error boundary.* A grid is GPU memory (a VAO, an instance buffer, a
  refcount on its configuration's atlas — 4.2 MiB at an 8x16 cell, 12.8 MiB at 15x30 on a dpr-2 display,
  measured for #773's follow-up), and nothing holds it if assembly throws, so `build` calls
  `lease.release()` on a throw; `create` likewise disposes a surface it composed, since a throw after it
  exists strands a bound context, a density watcher and a canvas listener.
- *A density change and a context restore reach a terminal through one registration*
  (`GridLease.onReapply` → `reapplySurface`): the surface owns *when*, only the terminal knows *what*.
  The constructor also registers `onEnd` → `JustermRenderer.dispose`, so `TerminalSurface.dispose` ends
  each **renderer** on it rather than retiring grids under a live blink loop. It does **not** end the
  `Terminal` widgets: the README measured `surface.dispose()` alone leaving every widget mounted and
  subscribed, so the next pushed frame throws `no grid with id N` — hence its order, *terminals first,
  then the surface*. (This note said "ends the widgets" before; it ends the renderers.)
- *The rect's origin is stored and its extent re-derived*: the extent follows from grid and cell; the
  origin is the host's measurement, re-supplied when the box moves because one context binds one canvas
  and nothing in the GL layer can see the overlay drift.

**The blink loop and the present** (`JustermRenderer.render`, `blinkTick`, `trackBlinkCells`,
`setFocused`, `dispose`).
- *`render()` presents the whole canvas*, since it takes no grid: a sole tenant presents synchronously
  (one terminal is one present per frame either way, and the e2e suite drives a frame and reads pixels
  in the same turn); a shared tenant calls `TerminalSurface.requestRender`, coalesced with its siblings'.
  The two agree at N=1, so this is a derivation, not a mode.
- *The SGR 5 phase re-pack is gated on the grid possibly holding a `BLINK` cell* — xterm.js's
  `needsBlinkInViewport` adapted to frame mode (#576). A frame-mode consumer holds damage, not the grid,
  so the gate is **conservative**: a Full frame replaces the answer, a Partial one can only add to it
  (`trackBlinkCells`). A false positive costs one redundant re-pack; a false negative would freeze blinking
  text. The Full case rests on core emitting full damage as every row at full width, which
  `FrameKind::Full` states as its contract — if a Full frame became a subset, this gate would produce the
  one error it must not. Measured for #576 (demo, 600 ms interval, 3.0 s): 16 presenting rAF turns with
  three blinking cells against 11 with none — exactly the 5 phase flips.
- *A hidden terminal's tick does nothing* (#801). Both halves of a tick end in `backend.render()`, which
  presents the whole canvas, so on a shared surface a hidden pane's blink redrew its siblings twice a
  second. `CursorBlink.isVisible` is solid when unfocused, but `TextBlink.isVisible` has no focus gate, and
  `hide()` with no DOM change (what the README recommends for `visibility: hidden`) keeps focus. Measured
  for #801: 4 whole-canvas presents in 2100 ms before the gate, 0 after, a shown sibling at 3 either way.
- *`setFocused` presents even with no caret on screen* when the tint moved. It is the one setter that
  changes something other than the cursor, and `issueOverlay` only retains and re-packs — guarding the
  present on a cursor dropped the tint flip whenever the application had hidden the caret. xterm.js
  re-shows the caret from its own focus handler for the same reason (#912).
- *`dispose` releases the grid* (since #770 added `removeGrid`, now via `GridLease.release`), stops
  the loop and detaches the reduced-motion listener — which *draws* since #576, so leaving it attached
  let a disposed widget repaint. Every per-grid method throws afterwards. The wasm instance, the GL
  context and the Rust-side canvas listeners survive until the binding's `free()`, which is unsafe while
  the consumer still holds the object; `isContextLost()` / `isRestoreOverdue()` keep answering from the
  state machine those listeners feed.

**Who ends the rest** — the inventory, re-read at HEAD (2026-10-22). Rows the widget or the surface own
are closed by code; the others are ended only by a host that kept the handle.

| Ambient work | Teardown handle | Ended by |
|---|---|---|
| frame + event subscriptions, DOM listeners, live-gesture window listeners, selection tick (#902) | `Unsubscribe` / detachers | `Terminal.dispose` |
| OSC 52 clipboard read in flight | `ClipboardController.dispose` | `Terminal.dispose` |
| link hover state | `LinkTracker.dispose` | `Terminal.dispose` |
| rAF blink loop + reduced-motion listener | `JustermRenderer.dispose` | `Terminal.dispose` (#606), or `TerminalSurface.dispose` via `onEnd` |
| context-loss relay, density watcher, `webglcontextrestored` listener, present loop | `TerminalSurface.dispose` | the surface — `create`'s renderer for a sole tenant, the host otherwise (#775) |
| `Scrollbar` window listeners | `Scrollbar.dispose` | host — PenTerm does (`nativeTerminalCache.ts` — `build`) |
| `observeResize` | returned disposer, plus `FitController.dispose` | host — PenTerm does not use it (its own `ResizeObserver`, disconnected in `build`'s cleanups); the demo writes `void disposeFit;` |
| a11y announce timer, `selectionchange` listener | `Accessibility.dispose` | host — PenTerm wires no `Accessibility` |
| accessible view's `Escape` keydown | **none** (on its own element) | — |
| search debounce | **none** (`SearchController` has no `dispose`) | — |
| marker index pull in flight | `MarkerIndexCache.reset` — orphans the flight rather than cancelling it, since a `Promise` has no cancel (#746) | host; `terminal.ts` never sees the cache |

The rows that need a host share one cause, and it is not the renderer's: every such collaborator is
consumer-constructed and exported individually, and the widget owns only what it builds. #606 was
separable from them because the renderer's row was the only one a consumer could not close by
discipline — `dispose` was not on the port, a type-level obstacle rather than a missing call.

**The composition writers `Terminal` drives** (ADR-0028; moved here from input encoding, because they
push to the renderer rather than produce an intent):
- **A composition freezes the textarea anchor for every writer, forced or not** (#637 for the frame
  stream, #649 for the point-of-use re-sync). The predicate is "a candidate window is open" —
  `CompositionController`'s `isComposing`, not the broader `active` (`isComposing ||
  isSendingComposition`), which outlives it by one deferred read and would swallow the
  `compositionstart` re-sync in continuous CJK.
- **A composition's origin comes from the composition before it while a commit is in flight** (#911,
  ADR-0028 D4): the committed text has not been sent, so the frame stream cannot know it. The value is
  the previous run's end — `JustermRenderer.setPreedit`'s returned caret column, since the widget has
  no `wcwidth` — chosen by `preeditLatch` (`composition.ts`) on `active`, the mirror of the freeze
  above. The only gate on the DOM wiring is the #911 e2e ("a continuous burst starts each syllable
  where the last one ended").
- **`preeditIntent`** answers *nothing* or *these codepoints at this origin*: it skips unchanged data,
  pushes nothing without an origin (no frame has reported a cursor yet, so `rows` is still `0`), and
  splits by **code point** so an astral scalar is one cell. `preeditIntent` and `preeditLatch` are pure
  because the unit suite runs in `environment: "node"`, the blind spot #649 measured.
- **The drawn run is cleared at `compositionend`**, before the commit reaches the grid; waiting for a
  frame would leave the last syllable drawn twice (timing measured, #249). The run is pushed from
  `compositionupdate.data`, never the textarea value, so for Korean the drawn run and the eventual
  commit can disagree — the IME's behaviour, not a defect (#249).
- **Against a renderer older than #249**, `setPreedit` returns `col` when the backend lacks the
  binding, so #911's handoff degrades silently to the frame stream's answer — benign, and the two
  packages release on separate tracks (#918, closed `not_planned`).

## Code

- `justerm-web/src/terminal.ts` — `Terminal`, `Terminal.mount`, `Terminal.dispose`, `TerminalOptions`,
  `rendererNotifyingSink`, `INPUT_ATTRIBUTE`; `Terminal.setSuggestion` refuses after dispose (#972)
- `justerm-web/src/renderer.ts` — `Renderer`, with the optional `dispose` and `setFocused` contracts
- `justerm-web/src/frame-source.ts` — `StubFrameSource`; the `FrameSource` and `Unsubscribe` types live
  in `types.ts`
- `justerm-web/src/frame-loop.ts` — `FrameLoop`, host-tested with injected `raf`/`caf`
- `justerm-web/src/context-loss.ts` — `ContextLossRelay`
- `justerm-web/src/justerm-renderer.ts` — `JustermRenderer.create`, `JustermRenderer.attach`,
  `build`, `assemble`, `blinkTick`, `trackBlinkCells`, `setFocused`, `dispose`
- `justerm-web/src/terminal-surface.ts` — `TerminalSurface.dispose`, `GridLease` (`onReapply`, `onEnd`,
  `release`), `SurfaceDeps` — the instantiation seam that makes the composition host-testable
- `justerm-web/src/dpr-watcher.ts` — `DprWatcher`, surface-scoped since #775
- `justerm-web/src/composition.ts` — `CompositionController`, `preeditIntent`, `preeditLatch`
- Tests: `justerm-web/test/terminal.test.ts` (fake renderer), `frame-loop.test.ts`,
  `context-loss.test.ts`, `terminal-surface.test.ts`; browser proofs in `justerm-web/e2e/demo.spec.ts`
  and `shared-surface.spec.ts` (`#606`, `#579`, `#801`, `#912` titles)

## Reference behaviour

- [Widget teardown — who ends a handed-over component](../../agents/reference-facts.md#widget-teardown--who-ends-a-handed-over-component-606-verified-2026-07-29)
  — xterm.js disposes each consumer-constructed addon from `Terminal.dispose()`, idempotently, and its
  lifecycle is one-shot. The one-shot part is a **condition** on copying it, which is why #606 had to
  declare end-of-life rather than assume it
- [Surviving a throw from inside a rAF loop](../../agents/reference-facts.md#surviving-a-throw-from-inside-a-raf-loop--the-reference-clears-its-handle-first-696-verified-2026-08-03)
  — the reference clears its handle first; `FrameLoop` follows
- [The initial focus state — who establishes it](../../agents/reference-facts.md#the-initial-focus-state--who-establishes-it-912-verified-2026-09-16)
  — the corpus splits 2-1 on the default; what all three share is a correction path, so a default alone
  is not a contract
- [Text blink — SGR 5](../../agents/reference-facts.md#text-blink--sgr-5-576-verified-2026-07-29)
  — the viewport gate the conservative `mayHaveBlinkCells` adapts

## Cross-cutting invariants

- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — promoted out of this territory by #775, and the promotion corrected it: the rule #606 gave this note
  read *"what a layer is handed across a port, that layer ends"*, and `JustermRenderer.attach` falsified
  it the day it existed — handed a shared surface, it must not end it. The criterion is exclusivity, not
  provenance; both of this territory's sites (`Terminal.dispose` ending the renderer, `create`'s
  renderer ending its surface) derive from the corrected form. The ambient-work table's host rows are
  that note's § A teardown the host must remember: no host calls them yet
- [an awaited in-page promise needs an anchor](../invariant/an-awaited-in-page-promise-needs-an-anchor.md)
  — this territory's contracts (context loss, dispose, restore) are provable only in a browser, and some
  are read through parked hooks; a read path that can fail without saying why is a hole in the evidence
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — `textareaCell` is a cached decision with no invalidation path, because `getGeometry` is a
  consumer-supplied *pull*; #631 re-reads the anchor at the moments something reads it (composition
  start, focus) instead, so the cache is stale between them **by design** and nothing reads it then
- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — the anchor re-read sits in the `compositionstart` handler because that is when the OS reads it, and
  the widget can only learn of it from a browser event; the same fact is why the frame path has no
  composition gate to key on (#637)
- [a screen change no frame carries owes its own present](../invariant/an-unframed-screen-change-owes-a-present.md)
  — `render()`'s two shapes (a sole tenant presents, a shared tenant coalesces) are what every unframed change reaches; the hidden-tick gate is the converse (#801)

## Blast radius

- [multi-viewport](multi-viewport.md) — the surface ends grids, not widgets; a change to what `onEnd`
  reaches or to the lease's release decides whether a disposed surface leaves live widgets throwing
  `no grid with id N` on the next frame
- [GL context lifecycle](gl-context-lifecycle.md) — the renderer clears its notification slot only at
  `free()`, a moment this package never reaches; the relay's `end` is the only stop, so a change to who
  holds the relay changes whether a notification lands on an ended widget
- [caret drawing](caret-drawing.md) — the blink phase is clocked by the loop this territory owns and
  gated on the focus state it establishes at mount; a change to either moves when the caret is drawn
- [events & replies](events-and-replies.md) — an OSC 52 read is answered asynchronously after the event
  subscription; `Terminal.dispose` must keep ending the controller or a disposed widget answers the PTY
- [accessibility](accessibility.md) — its timers and `selectionchange` listener are host-ended ambient
  work, and `reactivate()` carries a reset obligation a lifecycle owner would otherwise hold
- [hyperlinks](hyperlinks.md) — the hover state restores the element's inline `cursor` on dispose; a
  link-state change that adds DOM state must add its undo here
- [release](release.md) — `justerm-web` consumes the *published* wasm decoder through a dynamic
  `import()`, so its startup path depends on a version it does not control

## Known holes / open

- **A composition root exists for the canvas, not for everything** (#605 closed 2026-08-21 on this).
  #775's `TerminalSurface` answers *who ends the canvas, the context, the density and the recovery*;
  it does not own the host-constructed collaborators in the table. Measured on #605's close: the three
  ambient modules added after the spine (`frame-loop.ts`, `context-loss.ts`, `dpr-watcher.ts`) each
  carry a teardown, and `context-loss.ts` and `dpr-watcher.ts` cite #606 in their own source. PenTerm,
  the one host, adopted the widget on 2026-09-15 and ends the scrollbar, its resize observer, the
  widget, the renderer and its placement (penterm 7dadc3c95 `nativeTerminalCache.ts` —
  `disposeNativeTerminal`); it wires no a11y, search debounce teardown or marker index, so those rows
  have no ending caller anywhere.
- **Two collaborators have no teardown at all**: the accessible view's keydown (harmless while its
  element is dropped with it) and the search debounce (`SearchController` has no `dispose`; a timer
  armed before the host drops it still fires).
- **No pause for a terminal scrolled out of view inside a visible page.** A hidden *document* stops
  rAF; #607 closed `NOT_PLANNED` on a measurement (5 presents in 1500 ms, all from the demo's frame
  timer, 0 from the blink loop, because the shipped defaults are a steady cursor and no text blink), so
  this is a validity condition: a consumer setting `cursorBlink: true` or `textBlinkInterval > 0` gets a
  real flip about twice a second. #801 gave the widget the `hidden` input for a pane the host sets aside;
  a pane scrolled away is still not reported to anyone. The references do not agree on the boundary:
  xterm.js's `IntersectionObserver` catches a scrolled-away pane and `display: none` but not
  `visibility: hidden`; this package catches whatever the host reports.
- **Focus is the textarea's alone, and window deactivation is not modelled.** A textarea that stays
  `document.activeElement` while the window is inactive keeps blinking and keeps the active tint —
  #912's harm, one state later. xterm.js ANDs with `ownerDocument.hasFocus()`. **Unmeasured**: two
  attempts during #912 failed because headless Chromium reports neither a `blur` nor a `hasFocus()`
  change when another tab is selected, while a positive control (focus moved to another element) does
  log `blur`. Settling it needs a headed browser with two OS windows.
- **`JustermRenderer` cannot be constructed in a unit test.** `vitest.config.ts` runs `node` and the
  private constructor reads `window.matchMedia`, so none of its behaviour is unit-covered; #696 and #579
  each extracted the one piece that had to be tested (`FrameLoop`, `ContextLossRelay`) and left the
  composition — what `create` registers, what `dispose` closes — to browser proofs, which assert #579's
  half against a real `WEBGL_lose_context`. `TerminalSurface` took the other road (`SurfaceDeps`), because
  a composition root has nothing left to extract.
- **No reference comparison for teardown *composition*** — which collaborators a widget library ends for
  its host, the thing consumers usually judge it on.
- **The IME anchor's *other* readers are only partly known.** Measured (#649): the browser's focus
  steps are a second reader — focusing the textarea scrolls the nearest scrollable ancestor by the
  anchor. Whether an AT tool or magnifier is a third is unmeasured; it is the open question on spine
  #640 that decides whether the focus-time re-sync can give way to `focus({ preventScroll: true })`.
