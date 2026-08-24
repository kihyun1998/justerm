# Cross-cutting invariant — a layer ends what it exclusively holds, and never what it shares

## The fact

**Teardown follows exclusivity, not provenance.** A layer holding a collaborator that runs work of its
own — a loop, a listener, a timer, a registered GPU resource — ends that collaborator **iff it is the
only holder**. Whether the layer *built* it or was *handed* it decides nothing.

The two halves are separately load-bearing, and each one has a real site:

1. **Handed over and exclusive → the receiving layer ends it.** `Terminal` is handed a `Renderer` it
   did not construct and disposes it (#606), because that renderer is this widget's alone — the port's
   own doc rules out sharing one across two `Terminal`s.
2. **Handed over and shared → the receiving layer must NOT end it.** `JustermRenderer.attach` is
   handed a `TerminalSurface` and deliberately leaves it running (#775), because ending it would take
   down the canvas every sibling terminal is drawing on.

So a rule phrased on provenance answers (1) and gets (2) exactly backwards. Phrased on exclusivity it
derives both, plus the composition case: `JustermRenderer.create` **composes** a surface and therefore
is its only holder, so it ends it — and that is the same clause as (1), not a third rule.

## Why it is cross-cutting

**Three territories, no shared call path, and the fact is invisible from each of them.** From inside
widget lifecycle the question reads *"who calls dispose?"*; from inside multi-viewport it reads *"can
one terminal ending break its siblings?"*; from inside GL context lifecycle it reads *"whose listener
is this?"*. They are the same question and nothing in any one of them says so.

The count is doing work here in the way this map asks it to. **The rule was predicted from one site and
the prediction was wrong**, which is only visible with the second site in hand:
[widget lifecycle](../territory/widget-lifecycle.md) recorded *"what a layer is handed across a port,
that layer ends"*, checked it for reach, found exactly one site, and left a standing instruction to
promote it *"the day a second injected-collaborator port exists — the multi-viewport work (#287) is the
likely candidate"*. That day arrived with #775, and the second site **contradicted the rule as
written** rather than confirming it. A note promoted on the strength of one site would have shipped the
provenance phrasing, and the first host to attach two terminals would have followed it into disposing a
shared surface.

## Territories it holds in

- [widget lifecycle](../territory/widget-lifecycle.md) — **owns the mechanism**: the `Renderer` port's
  optional `dispose?()`, `Terminal.dispose()` calling it exactly once, and the inventory of ambient
  work with teardown nobody calls. Clause (1)'s site
- [multi-viewport](../territory/multi-viewport.md) — clause (2)'s site. A surface is shared by
  construction, so `JustermRenderer` carries `ownsSurface` and only the composing path ends it. The
  registry half is the same fact one level down: a terminal releases **its own** grid and the surface
  releases every grid still registered when it ends
- [GL context lifecycle](../territory/gl-context-lifecycle.md) — the density watcher, the
  `webglcontextrestored` listener and the context-loss relay are **surface-scoped**, so they belong to
  the shared holder. This is where a violation was actually reachable: before #775 all three sat on the
  per-terminal object and its `dispose()` stopped them

## What a violation looks like

**Silent, and it damages a bystander rather than the caller.** Nothing throws — the layer that ended
too much is fine, and the layer that loses its ambient work has no way to notice.

The concrete shape, measured while working #775: with the density watcher, the restore listener and
the loss relay on the per-terminal object, the **first** terminal to be disposed takes context recovery
away from every sibling sharing its canvas. The surviving terminal keeps drawing correctly and looks
healthy, right up until a context loss it can no longer recover from.

The mirror violation is a leak rather than a break: end nothing on the ground that "someone handed it
to me", and a widget stops consuming frames while its renderer's blink loop keeps repainting a canvas
nobody is driving (#606, before the port gained `dispose?()`).

The tell that distinguishes them is one question — *can a second holder exist?* — and it is answerable
from the type. A collaborator reachable from more than one live object is shared; one reachable from
exactly one is not.

## Discovery history

- **#606** (2026-07-29) — established clause (1) at its only site: `Terminal.dispose()` reaches the
  renderer it was handed, through a new optional `dispose?()` on the port. Derived from xterm.js, which
  disposes each consumer-constructed addon from `Terminal.dispose()`
  ([reference facts](../../agents/reference-facts.md#widget-teardown--who-ends-a-handed-over-component-606-verified-2026-07-29)).
  Checked for reach, found one site, and left the promotion instruction rather than a note
- **#605** (closed 2026-08-21) — measured the surrounding inventory and closed with the
  composition-root half explicitly parked as *"a question a host application asks"*, with nothing able
  to move it
- **#775** (2026-08-24) — built the host-shaped object, which supplied the second site and **falsified
  the predicted phrasing**. Exclusivity replaced provenance. The prior art converges on the sharper
  rule and always did: ghostty's root ends every terminal it holds and then its own shared machinery
  (`src/App.zig:107`), asserting the shared tier is empty by then (`:115`), while each terminal
  releases only **its own** claim on that tier (`src/Surface.zig:833`) — three statements that are one
  rule about exclusivity, at SHA `e6e26e1`

## Where it will recur

- **Any second tenant on a shared thing.** The next one is already designed: a host that keeps hidden
  terminals registered (penterm's adoption shape) holds surfaces outliving individual terminals, and
  every teardown path there answers this question
- **The per-config tier**, one level below the surface. A glyph atlas is shared by every grid on the
  same font configuration and released by refcount when the last one leaves — the same rule, already
  implemented in the renderer (#772), and a consumer-side path that released it directly would be this
  violation in GPU memory
- **Whenever a collaborator gains ambient work it did not have.** Exclusivity is a property of the
  *composition*, but what makes it matter is the collaborator having something to stop. A port whose
  implementations are inert today becomes a site of this invariant the day one of them starts a timer —
  which is how the `Renderer` port became one
- **Any new `dispose()`**, as the check rather than as a recurrence: before writing one, ask which of
  the things it touches a second live object can also reach
