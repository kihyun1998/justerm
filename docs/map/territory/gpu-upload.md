# Territory — GPU upload

## What it is

Deciding and executing the minimal GPU transfer that reconciles one grid's instance buffer with its
freshly packed instances. The renderer packs a whole grid at a time; this area diffs the new pack
against a CPU mirror of what the GPU holds and uploads only the contiguous instance ranges that
changed, or the whole buffer when its size changed or its baseline was invalidated.

## Governing decisions

**None.**

- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) decides the crate exists,
  not how it uploads.
- [ADR-0003 — damage model](../../adr/0003-damage-model-incremental-bounds.md) is the contrast: the
  engine's damage serves the wire, and this planner deliberately does not consume it.
- [ADR-0027 — liveness is answered by the source that owns it](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  records the `apply_frame` row whose validity rests on this area's baseline invalidation; it governs
  liveness, not the upload ([GL context lifecycle](gl-context-lifecycle.md)).

## Design model

Status: from the source and its tests only; no record decides it.

- **Diffing, not marking.** `plan_upload` compares packed instances stride by stride and collapses
  the changed ones into maximal half-open `[start, end)` instance ranges; a length change returns
  `UploadPlan::Full`. beamterm marks a dirty bitmask from frame damage instead (named in
  `upload.rs`'s module doc, unpinned). Diffing catches a glyph-slot change on a cell the frame did
  not damage — an atlas LRU eviction re-slots it — which a damage-keyed mark would miss and leave the
  wrong glyph on screen (#263, 2026-07-09, commit 07bcbea).
- **That defence holds within one grid only.** The diff sees a re-slot because this grid re-packed
  and the float changed. Once one glyph cache serves every grid on a font configuration (#772), a
  sibling's pack can repoint a slot while this grid's floats stay byte-identical, so there is nothing
  to diff. That half is answered before the diff: the grid records the cache's `evictions()` count it
  packed against (`packed_at_evictions`), and `render` re-packs any drawn grid whose configuration's
  count moved ([glyph atlas](glyph-atlas.md), [multi-viewport](multi-viewport.md)).
- **Diffing does not defend against a stale grid.** The pack reads the persistent `FrameGrid`; a cell
  the engine did not damage is never scattered into it, so an engine damage under-report leaves the
  grid — and therefore the pack and the diff — unchanged ([frame adapter](frame-adapter.md)).
- **The diff runs only after a pack.** `upload_instances` is called from `resolve_and_pack` and from
  `restore`'s refill; `render` packs a drawn grid only when `needs_repack` is set or its eviction
  count is stale, so an untouched frame does no diff and no GL work. The premise is that packing is
  cheap and pure and the GPU transfer is the cost, so cleverness goes into what to upload, not what
  to pack. No measurement of either cost is recorded (Known holes).
- **The plan is executed elsewhere.** `upload.rs` is pure and host-tested; the GL calls
  (`buffer_data_u8_slice` for `Full`, `buffer_sub_data_u8_slice` per range) live in the wasm32-only
  `webgl/draw.rs`, which copies each uploaded range into `uploaded` as it goes. Incremental repaint
  from the previous renderer was not ported: the engine's damage targets the wire, and this is the
  renderer's answer to its own problem (#263).
- **The instance layout is stated once.** `INSTANCE_STRIDE` is derived from `frame`'s
  `INSTANCE_FLOATS` rather than written out: the two drifting apart mis-addresses every attribute
  silently. It was a literal `9 * 4` until #513 widened the record, then #455 (`bg_default`) and
  #525 (the second line ink) widened it again in one release cycle; #966 added the left/right
  neighbours, and `INSTANCE_FLOATS` is 20 at HEAD. `build_grid_buffers` takes the attribute byte
  offsets from `frame`'s named float offsets (#791) except three literals: fg at `5 * F` although
  `frame::FG_RGB` names it, and the two line inks at `9 * F` and `10 * F`, which `frame` does not
  name — a float inserted before them moves them by hand.
- **The upload baseline is valid only while its buffer persists.** `uploaded` mirrors the live
  `instance_vbo`. A context loss destroys that buffer and `restore` builds an empty one; without
  `invalidate_baseline` an identical frame diffs to zero ranges and never refills it — a blank render
  that does not self-heal (found by #263's adversarial pass, fixed in #269, commit 17a4ac9).
  `restore` invalidates every grid's baseline and re-uploads the existing instances without
  re-packing ([GL context lifecycle](gl-context-lifecycle.md)).
- **Input bound.** The plan's size is the grid's instance count, `cols × rows × INSTANCE_FLOATS`
  floats; the grid size is the consumer's, bounded where the frame adapter sizes the `FrameGrid`.

## Code

- `justerm-renderer/src/upload.rs` — `plan_upload`, `UploadPlan`, `invalidate_baseline`; tests
  `a_restored_context_forces_a_full_reupload_of_an_identical_frame`,
  `an_identical_frame_plans_no_upload`, `adjacent_changes_merge_into_one_range`
- `justerm-renderer/src/webgl/draw.rs` — `JustermRenderer::upload_instances`,
  `JustermRenderer::resolve_and_pack` (records `packed_at_evictions`), `JustermRenderer::render`
  (the stale-eviction re-pack)
- `justerm-renderer/src/webgl.rs` — `INSTANCE_STRIDE`, `JustermRenderer::build_grid_buffers`,
  `GridTier` (`uploaded`, `instance_vbo`, `packed_at_evictions`)
- `justerm-renderer/src/frame.rs` — `INSTANCE_FLOATS`, `FG_RGB`, `pack_instances`
- `justerm-renderer/src/webgl/context.rs` — `JustermRenderer::restore` (invalidate, then refill)

`upload.rs`'s docs say "buffer_sub_data", the `web-sys` family name; the method called is
`buffer_sub_data_u8_slice`.

## Reference behaviour

No entry in `docs/agents/reference-facts.md`. The comparison lives unpinned in `upload.rs`'s docs:
beamterm's mark-bitmask is the rejected alternative, and `invalidate_baseline` is named as the
diff-based analogue of beamterm's `DirtyRegions::mark_all()` in `recreate_resources` and xterm.js's
`_requestRedrawViewport()` on `webglcontextrestored`.

## Cross-cutting invariants

- [a CPU record of GPU state is true only while that object lives](../invariant/a-gpu-mirror-dies-with-its-context.md)
  — `GridTier::uploaded` describes the live instance buffer only; a restore drops it (`invalidate_baseline`) so the refill is `Full`

## Blast radius

- [glyph atlas](glyph-atlas.md) — eviction re-slots cells the frame never damaged; the diff catches
  it within one grid and the eviction count catches it across grids, so a change to eviction or its
  counter changes what must re-pack.
- [cell compositing](cell-compositing.md) — the packer defines the instance layout the diff compares
  and the attribute table addresses; a float inserted before offsets 5, 9 or 10 misplaces the three
  literal offsets.
- [GL context lifecycle](gl-context-lifecycle.md) — the baseline assumes the buffer it mirrors still
  exists; any new path that recreates a buffer must invalidate it.
- [multi-viewport](multi-viewport.md) — the baseline and buffer are per grid while the glyph cache is
  per configuration; a change to what grids share changes which re-slots the diff can see.
- [frame adapter](frame-adapter.md) · [damage](damage.md) — the diff assumes the `FrameGrid` is
  current; it cannot recover a cell the engine's damage left out.

## Known holes / open

- No record governs a design whose central claim — diffing catches what marking cannot — is a
  correctness argument, not a performance one.
- The rejected alternative and the two analogues are unpinned: no `reference-facts.md` row.
- No measurement: the premise that packing is cheap and upload is the cost is not measured anywhere
  in the repo.
- Three attribute offsets are literals (`5`, `9`, `10`) beside named ones.
