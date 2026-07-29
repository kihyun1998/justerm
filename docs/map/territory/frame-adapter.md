# Territory — frame adapter

## What it is

The join between what the engine sends and what the renderer needs. `apply_frame` consumes a **dense
row-major** grid; a decoded `Partial` frame — the common case after the first — carries only the
**damaged cells in span order**, with a directory saying where each span belongs. Something has to
turn the second into the first, and this is it.

It keeps a persistent dense grid and scatters each frame's damage into it, so the packer always sees
a coherent full viewport.

## Governing decisions

**None.**

- [ADR-0003 — damage model](../../adr/0003-damage-model-incremental-bounds.md) creates the shape this
  adapts *from* — spans with column bounds
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the crate, not this

## Design model

- **The mismatch is the whole territory.** The engine's frame is sparse and ordered by span; the
  renderer's packer is dense and indexed by `row * cols + col`. Neither shape is wrong — they answer
  different needs, and the cost of the seam is this module.
- **Feeding spans straight through does not fail loudly.** It misaligns `bg[row * cols + col]` and
  **silently repaints undamaged cells as Default** — a wrong picture, not a crash, which is the
  failure class this map treats as most expensive.
- **The dense grid is persistent across frames**, which is what makes a `Partial` meaningful at all:
  a frame that carries three spans is a statement about *change*, and the unchanged remainder has to
  come from somewhere.
- **Pure and host-testable**, so the scatter can be tested without a browser — the same split the
  packer and the upload planner use.

## Code

- `justerm-renderer/src/frame_grid.rs` — `FrameGrid`, the persistent dense grid and the scatter
- `justerm-renderer/src/webgl.rs` — `apply_frame`, which consumes the dense result

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. Whether the references carry an equivalent seam — or
avoid it by having their frontend hold the model — has never been checked.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [damage](damage.md) — the span shape this adapts from. A change to what a `Partial` contains lands
  here first, and the failure is silent
- [frame](frame.md) · [wire format](wire-format.md) — the decoded shape is this module's input
  contract
- [cell compositing](cell-compositing.md) — consumes the dense grid and would misread a sparse one
  without ever erroring
- [GL context lifecycle](gl-context-lifecycle.md) — a context loss invalidates GPU state but **not** this
  grid, which is what allows a restore without a full re-send from the engine

## Known holes / open

- **Zero governing records** for a seam whose failure mode is a plausible-looking wrong picture.
- **Nothing states who owns re-synchronisation.** If the persistent grid and the engine's screen ever
  disagree — a dropped frame, a restore, a resize race — no document says which side is authoritative
  or how a consumer would detect it.
- **The `Partial` vs full-frame decision is the engine's**, and the adapter simply copes. Whether a
  consumer can *request* a full frame is not stated anywhere in this direction.
