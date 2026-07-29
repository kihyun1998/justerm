# Territory — multi-viewport rendering

## What it is

One WebGL2 context drawing **N terminal grids** as viewports, instead of one context per terminal.
A tabbed or split-pane host needs many grids on screen; browsers cap the number of live WebGL
contexts, and each one carries its own glyph atlas and shader program.

**This territory is a record with no code.** It is the inverse of every other note here — those
describe behaviour that exists and hunt for the decision behind it; this one has the decision and
nothing implementing it.

## Governing decisions

- [**ADR-0021 — one WebGL2 context draws N terminal grids as viewports (`TerminalSurface`)**](../../adr/0021-single-context-multi-viewport.md)
  — the direction, the `TerminalSurface` shape, a three-layer resource split and the rules for
  assigning a resource to a layer. Read its `Status:` line rather than any restatement
- Live epic: **#287** (a GitHub issue, so not a graph node)

## Design model

Everything below is **designed and not built** — read ADR-0021 for the authoritative form.

- **One context, N viewports**, with per-grid `scissor` + `viewport` rectangles rather than per-grid
  contexts. The browser's context cap is the forcing constraint.
- **Resources sort into three layers** by how widely they can be shared: per-context (the program,
  the atlas texture), per-surface, per-frame. The record's contribution is the *rule for assigning*
  something to a layer, not the layer list — that rule is what a future implementation will be
  measured against.
- **Named prior art, cross-checked**: three.js's multiple-views (scissor + viewport), Ghostty's
  shared grid, WezTerm's glyph cache. `virtual-webgl` is recorded as a **counter-example** —
  multiplexing contexts underneath rather than sharing one honestly.

## Code

**None.**

No TerminalSurface, no scissor path, no per-surface resource split — the renderer draws one grid per
context today. `justerm-renderer/src/webgl.rs` is where the single-context assumption lives, and it
is the file a first slice would open.

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. ADR-0021 names its prior art with reasoning, and the
comparison lives inside the record rather than as pinned rows — which for an unbuilt design means
the argument for the shape is unverifiable at the moment someone starts building it.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

Nothing today, and that is the point: a territory with no code has no blast radius, so it cannot
appear in any other note's checklist. It is reachable only from here and from the hub.

When built, it lands squarely on:

- [glyph atlas](glyph-atlas.md) — the atlas becomes per-context and shared across grids, which is the
  whole economy of the design
- [GL context lifecycle](gl-context-lifecycle.md) — one loss would take down **every** grid at once
  rather than one, and the restore path is currently written for a single surface
- [cell geometry](cell-geometry.md) — each surface has its own cell size and device-pixel ratio, so
  the geometry stops being a property of the renderer
- [GPU upload](gpu-upload.md) — the diff baseline becomes per-surface
- [widget lifecycle](widget-lifecycle.md) — a shared context outlives any one widget, which is a
  lifecycle question nothing currently owns

## Known holes / open

- **A record with no implementation is its own state**, and this map had no way to represent it until
  this note: the code-first passes could not see this territory at all, because there is nothing to
  read.
- **The design is unverified against its own prior art.** The comparison is inside the record; if
  three.js or Ghostty has moved, nothing would notice before someone builds against it.
- **The single-context assumption is not marked anywhere in the renderer.** A first slice has to find
  it by reading, since no comment says "this assumes one grid".
