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
- [ADR-0011 — justerm-web keeps a viewport cell mirror](../../adr/0011-justerm-web-viewport-cell-mirror.md)
  — decided the web's mirror, whose scatter algorithm (`cell-mirror.ts`: wipe on Full, shift before
  spans, scatter span-ordered cells) `FrameGrid` ports; it decides nothing about this module

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
- **Every index is bounded as a caller error, because the caller is JS** (#355). The wire caps
  `cols`/`rows` at `u16`, so no frame from core can name more than 65535×65535 = 4_294_836_225 cells
  (just under `u32::MAX`) — but `apply_damage` reads its header from JS and `apply_frame` takes bare
  `u32`s, so nothing binds a caller that does not come through core. `cell_count` is checked before
  any per-cell vector is reserved, and span/scroll indices are validated **before the first write**:
  an unchecked `line == rows` — an off-by-one, not an exotic value — used to trap the module
  (`RuntimeError: unreachable`) and poison it for good ("recursive use of an object"), and checking
  as the scatter went left a refused frame half-applied. Up-front validation makes a refusal total;
  `resolve_frame` holds the same discipline (rasterise before commit).
- **A Full frame ignores its scroll op.** It is authoritative — core ships every row as a span — and
  an alt-screen switch can leave a stale scroll set on a Full frame (`justerm-core` marks full damage
  without clearing it), so shifting against a full repaint would be meaningless.
- **An over-height scroll `count` is tolerated, not refused**: every row's source lands outside the
  region, so the shift blanks it and the spans repaint it. Core stopped *producing* one in #661
  (`Term::scroll_delta` caps at the region height), but `decode` does not reject it and the frame
  arrives from JS, so the tolerance is what keeps a foreign frame from over-reading.
- **A cell's cluster index is resolved to text at scatter time**, because `extra` indexes the
  *frame's* `side_table` and a later frame's table differs. The side table holds only the trailing
  combining marks, so the stored text is base codepoint + marks. The column is `u32`, as core emits
  it (#621/#627): the table has one entry per combining cell of the viewport and the header admits a
  viewport far wider than `u16::MAX` cells, so the old `u16` column read index 65536 as "no cluster"
  and 65537 as the wrong one, silently.

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
