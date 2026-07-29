# Territory — widget lifecycle

## What it is

Who owns the work that keeps happening after the call that started it returns — a rAF loop,
media-query and window listeners, a11y timers, resize observation — and who is responsible for
stopping it.

**Nothing composes them today.** Each piece was added by the slice that needed it, each has a
perfectly good teardown handle, and no artifact says whose job it is to call them.

## Governing decisions

**None.**

- The measured inventory this note routes to is in spine **#605** — *"justerm-web's background work
  has no lifecycle owner"*. A GitHub issue, so not a graph node

## Design model

**One rule is settled (#606); the rest of the inventory is still unowned.**

- **What the widget is handed, the widget ends.** `Terminal` receives exactly three things
  (`source`, `renderer`, `options`), and `dispose()` now releases all of them: both `FrameSource`
  subscriptions, its own DOM listeners, and — since #606 — the renderer, through an optional
  `dispose?()` on the `Renderer` port. Derived, not invented: xterm.js disposes each
  consumer-constructed addon from `Terminal.dispose()`, and this repo's other injected port already
  worked this way through the `Unsubscribe` it returns. See
  [reference behaviour](#reference-behaviour).
- **`Terminal.dispose()` is end of life, not unmount.** `mount()` after it throws. Declared rather
  than left open because the alternative was already broken: `textareaCell` survives disposal (a
  remounted widget parks the IME candidate window at the canvas origin until the cursor moves), and a
  re-mounted renderer would have lost its `prefers-reduced-motion` listener permanently — its only
  registration is in a private constructor.
- **It stops work, not memory.** The renderer's wasm instance, GL context, glyph atlas and the
  canvas context-loss listeners its Rust side owns all survive `dispose()`; they belong to the
  binding's `free()`, which is unsafe while the consumer still holds the object.

Inventory, re-measured 2026-07-29 — the sweep #605 asked for:

| Ambient work | teardown handle | who calls it |
|---|---|---|
| the renderer's rAF blink loop + reduced-motion listener | `dispose()` | **`Terminal`** (#606) |
| input attachment | returns a disposer | `Terminal`, via `detach` |
| the frame source's two subscriptions | returns `Unsubscribe` | `Terminal` |
| the scrollbar's window listeners | `dispose()` | nobody |
| resize observation | returns a disposer | nobody — the demo writes `void disposeFit;` |
| the a11y controller's announce timer | `dispose()` | nobody |
| the accessible view's keydown | **none** | — |
| the search debounce | **none** | — |

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
- `justerm-web/src/justerm-renderer.ts` — the rAF blink loop and the reduced-motion listener, and
  the `dispose` `Terminal` now calls
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

*(none identified yet)*

## Blast radius

Everything the widget attaches, because the missing owner is a property of the composition rather
than of any one collaborator.

- [caret drawing](caret-drawing.md) · [caret report](caret-report.md) — the blink phase is driven by
  the loop `Terminal` cannot reach, and neither blink loop pauses when the terminal is off-screen
  (tracked: #606, #607)
- [GL context lifecycle](gl-context-lifecycle.md) — the consumer sets the restore timeout and reacts
  to the callback, so context recovery is one more thing with no stated owner
- [events & replies](events-and-replies.md) — both queues are drained on a cadence the consumer
  chooses, and a widget that is disposed but still queueing has no defined behaviour
- [accessibility](accessibility.md) — its timers are ambient work, and `reactivate()` already carries
  a reset obligation that a lifecycle owner would otherwise hold
- [release](release.md) — `justerm-web` consumes the *published* wasm decoder, so its startup path
  depends on a version it does not control

## Known holes / open

- **One rule now exists; the territory is still mostly convention.** #606 settled what happens to
  what the widget is *handed*. Six collaborators the consumer keeps have teardown nobody calls, and
  two have none at all — that is a **composition-root** question, still tracked on #605.
- ~~`Terminal.dispose()` cannot reach the renderer's blink loop.~~ Closed by #606: `dispose?()` is on
  the `Renderer` port and `Terminal.dispose()` calls it, proven in a real browser by counting the
  loop's presenting rAF turns before (>0) and after (0) disposal.
- **Neither blink loop pauses when the terminal is off-screen**, so a backgrounded tab keeps
  animating. Tracked: #607.
- **No reference comparison** for teardown composition, which is the one thing a widget library is
  usually judged on by its consumers.
