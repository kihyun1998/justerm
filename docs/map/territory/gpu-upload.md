# Territory — GPU upload planning

## What it is

Deciding the minimal GPU work to reconcile the instance buffer between two frames. The renderer
re-packs the **whole** grid every frame, so this is what stops that costing a full buffer upload each
time — it diffs the freshly packed instances against what was last uploaded and returns the
contiguous ranges that actually changed.

## Governing decisions

**None.**

- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the crate, not this
- [ADR-0003 — damage model](../../adr/0003-damage-model-incremental-bounds.md) is the *contrast*:
  the engine's damage exists for the wire and this planner deliberately does **not** consume it

## Design model

- **Diffing, not marking — and the reason is a real defect class, not a preference.** beamterm marks
  a dirty bitmask from frame damage. This planner compares packed instances instead, which catches
  something a mark cannot: **a glyph-slot change on an *undamaged* cell**, caused by atlas LRU
  eviction. A mark keyed off frame damage would miss it and leave the wrong glyph on screen.
  **Valid within one grid, and only within one** (bounded #772). The diff sees a slot change because
  the *packed instance* changed — this grid re-packed, and the new slot is a different float. Once a
  glyph cache is shared by every grid on one font configuration, a **sibling's** pack can repoint a
  slot while this grid's floats stay byte-identical, and there is nothing for the diff to find. That
  half is answered outside this planner: the cache counts its evictions and the render loop re-packs
  any grid whose configuration has moved on ([glyph atlas](glyph-atlas.md)). Worth knowing before
  reading the sentence above as a complete defence, which it was until a second writer existed.
- **Whole-grid re-pack is the premise, not a cost to be optimised away.** Because packing is cheap
  and pure, the expensive resource is the GPU transfer — so the design pushes all cleverness into
  *what to upload* rather than *what to pack*.
- **The output is a plan, executed elsewhere.** This module returns contiguous ranges; the GL layer
  runs them through `buffer_sub_data`. That split keeps the planning host-testable while the GL call
  stays browser-only.
- **This is why incremental-repaint work from the previous renderer was not ported.** The engine's
  incremental damage is aimed at the *wire*; the renderer's equivalent problem is solved here, by
  different means, against a different failure mode.

## Code

- `justerm-renderer/src/upload.rs` — the planner (pure, host-testable)
- `justerm-renderer/src/webgl.rs` — executes the plan via `buffer_sub_data_u8_slice` (browser-only).
  The module docs on both sides shorten that to "buffer_sub_data", which is the `web-sys` family
  name and not a method that exists — quoted rather than backticked here, because a name under this
  heading is meant to be one you can grep
- `justerm-renderer/src/frame.rs` — produces the freshly packed instances being diffed

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. beamterm's mark-bitmask is named as the approach this
one **rejects**, cited in prose with no pin — and a rejected alternative is exactly the kind of claim
worth pinning, because it is the argument for the current design.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [glyph atlas](glyph-atlas.md) — LRU eviction is the source of the undamaged-cell change this
  planner exists to catch; a change to eviction changes what the diff must find
- [cell compositing](cell-compositing.md) — supplies the instances; the instance layout is what is
  being diffed, so widening it changes the comparison
- [damage](damage.md) — the deliberate non-relationship. Engine damage is a wire concern and this
  planner ignores it by design

## Known holes / open

- **Zero governing records** for a design whose central claim — "diffing catches what marking cannot"
  — is a correctness argument, not a performance one.
- **The rejected alternative is unpinned.** beamterm's mark-bitmask is the thing this is measured
  against and it is named only in a module comment.
- **No measurement is recorded.** The premise is that whole-grid re-pack is cheap enough that upload
  is the bottleneck; nothing in the repo states what either costs.
