# Invariant — a layer ends what it exclusively holds, and never what it shares

## The fact

Teardown follows **exclusivity, not provenance**. A layer holding a collaborator that runs work of its
own — a loop, a listener, a timer, a registered GPU resource — ends that collaborator iff it is the only
holder. Whether the layer built it or was handed it decides nothing.

Both halves have a real site (re-verified at HEAD 7bb31f7):

1. **Handed over and exclusive → the receiver ends it.** `justerm-web/src/terminal.ts — Terminal.dispose`
   calls `this.renderer.dispose?.()` on a `Renderer` it did not construct (#606), because the port's
   own doc rules out sharing one across two `Terminal`s.
2. **Handed over and shared → the receiver must not end it.** `justerm-renderer.ts — JustermRenderer.attach`
   is handed a `TerminalSurface` and leaves it running (#775), because ending it would take down the
   canvas every sibling terminal draws on.

A provenance rule answers (1) and gets (2) backwards. The composition case is clause (1) again:
`JustermRenderer.create` composes a surface, is its only holder, and ends it — the `composedSurface`
flag gates `this.surface.dispose()` in `JustermRenderer.dispose`.

The check is one question, answerable from the type: can a second holder exist? A collaborator
reachable from more than one live object is shared.

### A teardown the host must remember is uncalled until a host exists

The rule names who ends a collaborator; it cannot make that party exist. Where the exclusive holder is
the *host* — the collaborator is consumer-constructed and exported individually, and `Terminal` never
sees it — the teardown runs only if a host remembers to call it. Three such teardowns at HEAD 7bb31f7,
none reached from `justerm-web/src/terminal.ts` (which imports none of the three classes):

- `justerm-web/src/accessibility-dom.ts — Accessibility.dispose` — the announce timer and the
  `selectionchange` listener;
- `justerm-web/src/fit.ts — FitController.dispose` and the disposer `observeResize` returns — the
  debounce timer and the `ResizeObserver`;
- `justerm-web/src/marker-index.ts — MarkerIndexCache.reset` — orphans an in-flight pull (#746).

No caller exists for any of them. PenTerm constructs none of the three (no match for `Accessibility`,
`FitController`, `observeResize` or `MarkerIndexCache` under `src/` at penterm 7dadc3c95; it runs its
own `ResizeObserver`, ended in `nativeTerminalCache.ts — build`'s cleanups). The package's own demo
constructs all three and ends none: `justerm-web/demo/main.ts` writes `void disposeFit;`. `FitController.dispose`
and `MarkerIndexCache.reset` have unit tests (`test/fit.test.ts`, `test/marker-index.test.ts`);
`Accessibility.dispose` (only its inner `AccessibilityController.dispose` is unit-tested) and
`observeResize`'s disposer have no caller at all. The first host that adopts one is their first caller, and
nothing will tell it the call is owed except the doc-comment. The widget-lifecycle territory's ambient
work table is the inventory.

## Why it is cross-cutting

The sites share no call path, and the fact reads as a different question from each: in widget
lifecycle "who calls dispose?", in multi-viewport "can one terminal ending break its siblings?", in GL
context lifecycle "whose listener is this?". `Terminal.dispose` and `TerminalSurface.dispose` do not
call each other; they share only the assumption that a collaborator's holders are known.

The rule was predicted from one site and the prediction was wrong. Widget lifecycle recorded "what a
layer is handed across a port, that layer ends", found one site, and left an instruction to promote it
when a second injected-collaborator port appeared (#287 named as the likely one). #775 supplied that
second site and contradicted the provenance phrasing; a note promoted on one site would have shipped
it, and the first host attaching two terminals would have disposed a shared surface.

## Territories it holds in

- [widget lifecycle](../territory/widget-lifecycle.md) — owns the mechanism: the `Renderer` port's
  optional `dispose?()` (`justerm-web/src/renderer.ts — Renderer.dispose`), `Terminal.dispose` calling it
  once, and the inventory of ambient work. Clause (1).
- [multi-viewport](../territory/multi-viewport.md) — clause (2). `JustermRenderer` carries
  `composedSurface` and only the composing path ends the surface. One level down, the registry: a
  terminal releases its own grid lease, and `justerm-web/src/terminal-surface.ts — TerminalSurface.dispose`
  ends and releases every lease still registered.
- [GL context lifecycle](../territory/gl-context-lifecycle.md) — the density watcher (`dprWatcher`),
  the `webglcontextrestored` listener and the context-loss relay (`ContextLossRelay`) are surface-scoped
  and stopped by `TerminalSurface.dispose`. Before #775 all three sat on the per-terminal object, which is
  where a violation was reachable.
- [events & replies](../territory/events-and-replies.md) — `Terminal.mount` constructs a
  `ClipboardController` the widget alone holds, and `Terminal.dispose` ends it, because an in-flight
  clipboard read is already past the event subscription and dropping the subscription cannot stop it.
  (This territory linked here already; the old note did not list it.)
- [accessibility](../territory/accessibility.md) — `Accessibility.dispose` is a host-held teardown no
  host calls (§ A teardown the host must remember).
- [fit](../territory/fit.md) — `FitController.dispose` and `observeResize`'s disposer, the same.
- [decoration](../territory/decoration.md) — `MarkerIndexCache.reset`, the same; the cache is what keeps
  the decoration registry anchored, and no consumer builds one.

## What a violation looks like

Silent, and it damages a bystander. Measured while working #775: with the density watcher, restore
listener and loss relay on the per-terminal object, the first terminal disposed takes context recovery
away from every sibling sharing its canvas; the survivor keeps drawing and looks healthy until a
context loss it can no longer recover from.

The mirror is a leak: ending nothing because "someone handed it to me" leaves a widget that stops
consuming frames while its renderer's blink loop keeps repainting a canvas nobody drives (#606, before
the port gained `dispose?()`).

## Discovery history

- **#606** (2026-07-29) — clause (1) at its only site, through a new optional `dispose?()` on the port.
  Derived from xterm.js disposing each consumer-constructed addon from `Terminal.dispose()`
  ([reference facts](../../agents/reference-facts.md#widget-teardown--who-ends-a-handed-over-component-606-verified-2026-07-29)).
- **#605** (closed 2026-08-21) — measured the surrounding inventory; closed with the composition-root
  half parked as a question a host application asks.
- **#775** (2026-08-24) — built the host-shaped object, supplied the second site, and falsified the
  provenance phrasing. The prior art always held the exclusivity rule: ghostty's root ends every
  terminal it holds and then its own shared machinery (`src/App.zig:107`), asserting the shared tier is
  empty by then (`:115`), while each terminal releases only its own claim (`src/Surface.zig:833`), at
  `e6e26e1`.

## Where it will recur

- **Any second tenant on a shared thing.** A host keeping hidden terminals registered (penterm's
  adoption shape, #770 "registered but not drawn") holds surfaces outliving individual terminals; every
  teardown path there answers this question.
- **The per-config tier.** A glyph atlas is shared by every grid on one font configuration and released
  by refcount when the last leaves (#772; `justerm-renderer/src/config_registry.rs`,
  `webgl/grids.rs — release_config`). A consumer path that released it directly would be this violation
  in GPU memory.
- **A collaborator gaining ambient work it did not have.** A port whose implementations are inert
  becomes a site the day one starts a timer — how the `Renderer` port became one.
- **Any new `dispose()`**: before writing it, ask which of the things it touches a second live object
  can also reach.
