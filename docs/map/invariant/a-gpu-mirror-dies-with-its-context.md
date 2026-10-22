# Invariant — a CPU-side record of GPU state is true only while that GPU object lives

## The fact

The renderer keeps CPU copies that describe what a GPU object holds, so it can skip work: what is
already in a grid's instance buffer, which glyph sits in which atlas slot. Each copy is true only while
the object it describes is the same object. When the object is destroyed and replaced — a WebGL context
loss destroys every one; a density change replaces every atlas — each copy must be **either dropped,
or made true again by refilling the new object from it**. Kept as it is, it describes an object that no
longer exists, and the work it lets the renderer skip is exactly the work the new object needs.

Two records at HEAD 7bb31f7, answered the two different ways:

1. **Dropped.** `justerm-renderer/src/webgl.rs — GridTier::uploaded` is the baseline `plan_upload` diffs
   the next pack against (#263). `JustermRenderer::restore` (`webgl/context.rs`) calls
   `justerm-renderer/src/upload.rs — invalidate_baseline` on every grid after swapping in a fresh
   instance buffer, so the refill plans a `Full` upload (#269; every grid since #774). The field's own
   doc-comment states the invariant.
2. **Refilled.** `justerm-renderer/src/glyph_cache.rs — GlyphCache` records each resident glyph's slot,
   and packed instances address those slots. `webgl/font.rs — JustermRenderer::bake_config` with
   `Some(cache)` builds a new atlas and re-bakes every resident glyph (`GlyphCache::entries`) into the
   same slot it already held, so the cache and the instances stay true (#322 for density, #772 per
   configuration). `restore` and `rebuild_all_configs` (density) both pass `Some(cache)`; only a new
   configuration (`acquire_config`) passes `None`.

The choice between the two is cost: the baseline is cheap to rebuild (one full upload); the glyph cache
is not, because instances name slots and a fresh cache would re-number them.

## Why it is cross-cutting

The two records belong to different tiers — `uploaded` to a grid, the cache to a font configuration —
and neither reads the other. What they share is that the object they mirror can be swapped underneath
them by a path that is not theirs: the context-loss path rebuilds everything, and the density path
rebuilds atlases while leaving instance buffers alone (so it must not drop `uploaded`, and does not).
`restore` services both records, but by enumeration: there is no registry of mirrors, so a third one is
correct only if whoever adds it also adds its line to every path that replaces its object.

## Territories it holds in

- [GL context lifecycle](../territory/gl-context-lifecycle.md) — `restore` is the path that replaces
  every GPU object, and is where each record is dropped or refilled.
- [GPU upload](../territory/gpu-upload.md) — `GridTier::uploaded` and `invalidate_baseline`: the record
  that is dropped.
- [glyph atlas](../territory/glyph-atlas.md) — `GlyphCache`'s resident slots and `bake_config(Some(cache))`:
  the record that is refilled, on restore and on a density change.

## What a violation looks like

For a kept baseline: after a context restore the first identical frame diffs to zero ranges, nothing
is uploaded into the empty buffer, and the grid renders blank and does not heal until enough cells
change. For a dropped or re-numbered glyph cache: instances point at slots that hold another glyph or
nothing — wrong characters or holes after a restore or a density change, in the cells that did not
change. Both reproduce only after a loss or density change, which no ordinary session triggers on
demand; the browser proofs force one through `WEBGL_lose_context`.

## Discovery history

- **#269** (2026-07-09, `17a4ac9`) — context-loss recovery; `invalidate_baseline` exists for it, the
  diff-based analogue of beamterm's `DirtyRegions::mark_all()` and xterm.js's
  `_requestRedrawViewport()` on restore (cited in its doc-comment).
- **#322** (2026-07-09, `bdf0b00`) — a density change re-bakes resident glyphs into the same slots.
- **#772** (2026-08-19, `eb186e6`) and **#774** (2026-08-20, `a42af85`) — per-configuration atlases, and
  a proof that one loss recovers every registered grid, made each record plural; restore walks every
  grid and every configuration.

## Where it will recur

Any new CPU-side field whose value describes a GPU buffer, texture or uniform: name the paths that
replace that object (`restore`, `rebuild_all_configs`, a grid resize) and decide at each whether the
field is dropped or the object is refilled from it. Grep for `restore` and `rebuild_all_configs` when
adding one; nothing else will list it.
