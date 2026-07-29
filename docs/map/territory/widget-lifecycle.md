# Territory — widget lifecycle

## What it is

Who owns the work that keeps happening after the call that started it returns — a rAF loop,
media-query and window listeners, a11y timers, resize observation — and who is responsible for
stopping it.

**Nothing composes them today.** Each piece was added by the slice that needed it, each has a
perfectly good teardown handle, and no artifact says whose job it is to call them.

## Governing decisions

**None.**

- Live spine: **#605** — *"justerm-web's background work has no lifecycle owner"*. It holds the
  measured inventory this note routes to; a GitHub issue, so not a graph node

## Design model

There is no model yet — which is the finding, not a gap in this note. What exists is an inventory,
measured 2026-07-29:

| Ambient work | teardown handle | who calls it |
|---|---|---|
| the renderer's rAF blink loop + reduced-motion listener | `dispose()` | **nobody** |
| the scrollbar | `dispose()` | the consumer |
| resize observation | returns a disposer | the consumer |
| input attachment | returns a disposer | `Terminal`, via `detach` |
| the a11y controller and accessible view | `dispose()` | the consumer |

- **`Terminal.dispose()` tears down exactly what `Terminal` itself attached** — its own listeners and
  the input attachment — and nothing else. That is a defensible scope; the problem is that no other
  scope exists.
- **The renderer's loop is unreachable by construction.** `dispose` is not on the `Renderer` port, so
  `Terminal` *cannot* call it even if it wanted to. This is a **type-level** obstacle, not an
  oversight in a call site, which is why it survives review.
- **The obligation is currently split three ways** — `Terminal`, the consumer, and nobody — with no
  document stating the division. A consumer that disposes the widget and expects the page to go quiet
  gets a rAF loop that keeps running.

## Code

- `justerm-web/src/terminal.ts` — `Terminal`, `dispose`, and the listeners it owns
- `justerm-web/src/justerm-renderer.ts` — the rAF blink loop and the reduced-motion listener, and
  its own `dispose` that nothing reaches
- `justerm-web/src/scrollbar.ts` · `fit.ts` — collaborators with their own disposers
- `justerm-web/src/accessibility-dom.ts` — the a11y timers and their teardown

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. How xterm.js composes teardown across its addons —
whether a single `dispose` cascades, and what it guarantees about ambient work — has never been
checked, and it is the closest available prior art for exactly this problem.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

Everything the widget attaches, because the missing owner is a property of the composition rather
than of any one collaborator.

- [caret drawing](caret-drawing.md) · [caret report](caret-report.md) — the blink phase is driven by
  the loop nothing can stop (#606), and neither blink loop pauses when the terminal is off-screen
  (#607)
- [GL context lifecycle](gl-context-lifecycle.md) — the consumer sets the restore timeout and reacts
  to the callback, so context recovery is one more thing with no stated owner
- [events & replies](events-and-replies.md) — both queues are drained on a cadence the consumer
  chooses, and a widget that is disposed but still queueing has no defined behaviour
- [accessibility](accessibility.md) — its timers are ambient work, and `reactivate()` already carries
  a reset obligation that a lifecycle owner would otherwise hold
- [release](release.md) — `justerm-web` consumes the *published* wasm decoder, so its startup path
  depends on a version it does not control

## Known holes / open

- **The whole territory is a hole.** There is an inventory and no model; #605 exists to decide who
  owns what, and until it does every entry above is a convention.
- **#606 — `Terminal.dispose()` cannot stop the renderer's blink loop**, because the port does not
  expose `dispose`. Fixing the call site is not possible without changing the type.
- **#607 — neither blink loop pauses when the terminal is off-screen**, so a backgrounded tab keeps
  animating.
- **No reference comparison** for teardown composition, which is the one thing a widget library is
  usually judged on by its consumers.
