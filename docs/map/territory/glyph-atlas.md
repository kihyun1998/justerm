# Territory — glyph atlas

## What it is

Turning a grapheme into a slot in a GPU texture: classify it, allocate a slot, rasterise it with the
browser's own text engine, and evict when the atlas is full. The stateful half of drawing text —
[cell compositing](cell-compositing.md) assumes a slot is already resolved, and this is what resolves
it.

## Governing decisions

- [**ADR-0021 — one WebGL2 context draws N terminal grids as viewports**](../../adr/0021-single-context-multi-viewport.md)
  — governs the atlas's **tier and lifetime** as of #772, and nothing else here: D2 puts the atlas,
  rasteriser, glyph cache and the cell derived from them in the per-config tier, keyed by the four
  consumer selectors and refcounted, and its *Consequences* own what LRU eviction costs once the cache
  has more than one writer. The atlas's own design — two regions, the ASCII fast path, the eviction
  policy itself — is still governed by nothing
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the crate exists and
  reimplements beamterm; the atlas design is inherited rather than decided here

> **This section read "None." until #772.** That was true of the atlas as a *design* and stopped being
> true of it as a *resource*: the moment two grids can share one cache, who owns it and when it dies
> are decided somewhere, and that somewhere is ADR-0021.

## Design model

- **Rasterisation is the browser's text engine, deliberately.** Drawing through OffscreenCanvas
  brings font fallback, shaping and colour emoji for free — capabilities a hand-rolled rasteriser
  would have to reproduce.
- **Two LRU regions, normal and double-width**, with O(1) lookup / insert / evict, mirroring
  beamterm. Normal-styled ASCII gets **pre-allocated fixed slots**, so the common case never churns
  the cache.
- **The cache allocates; it does not classify.** `GlyphKind` arrives from the caller, and the
  unicode-width / emoji decision lives a layer up. That separation is what lets the cache be pure and
  host-testable with no GL at all.
- **The hot loop was lifted out of the browser-only layer on purpose** (#280) so `cargo test` can
  reach it. Three correctness gaps the #264 adversarial pass found were unreachable from the host
  before that: within-frame LRU eviction corrupting earlier cells, a rasterise failure stranding a
  committed-but-unuploaded slot, and one more the module doc enumerates.
- **A within-frame eviction can corrupt cells already packed in the same frame** — the atlas is
  mutable during a pass over the grid, so a slot handed out early can be reused before the frame is
  drawn. This is the hazard the split exists to make testable. Within one frame it is *refused*
  rather than repaired: a frame needing more distinct glyphs than a region holds is surfaced
  (`FrameExceedsCapacity`) instead of drawn wrong.
- **Since #772 the same eviction is a *cross-grid* event, and that half cannot be refused the same
  way** — the cache is shared by every grid on one font configuration, so one grid's pack can repoint
  a slot another grid's already-packed instances still address. The upload diff, which exists to
  catch a slot changing under an undamaged cell, cannot see it: the instance floats did not change,
  only what the atlas holds at the index they name. So the cache **counts its evictions**, a grid
  records the count it packed against, and the render loop re-packs the difference away. See
  [multi-viewport](multi-viewport.md)'s known holes for what that does and does not settle.

- **A slot is taller than its cell since #791.** It holds `bleed | cell | bleed` rows, the band
  sized per font configuration, so ink that leaves a cell survives the bake and the neighbouring
  cell can read it (ADR-0019 R1.2). Slot *count* is untouched — `GLYPHS_PER_LAYER`, `WIDE_BASE` and
  the eviction bookkeeping all key off slot ids — but the texture is taller: measured +44 % at
  dpr 1 and +23 % at dpr 2 on the reference face, and the ceiling `fit_cell_to_atlas` enforces drops
  by `2 * bleed`.

- **A glyph that will not fit its box is condensed before it is baked** (#792). The rasteriser
  measures the grapheme's ink with `measureText` — no readback, so it costs one call on the
  cache-miss path — and draws under `scale(s, 1)` when the ink exceeds the glyph box. It changes
  nothing else here: the slot keeps its size, the `GlyphKey` keeps its two fields (the constraint is
  a pure function of the grapheme and of the configuration's geometry, and the geometry *is* the
  `ConfigKey`), and every rebuild path — ASCII prebake, `bake_all_glyphs`, a cache miss, a DPR or
  font change, a context-loss restore — reaches it through the one `rasterize` seam.
- **`builtin` is outside it by construction, not by a list.** The builtin check precedes `fill_text`,
  so no fit can fire on a glyph the font never drew — the same shape as #507's dependency inversion,
  where the classifier *asks* `builtin::owns` rather than restating its ranges.

## Code

- `justerm-renderer/src/glyph_resolve.rs` — the pure per-cell slot resolution (host-testable)
- `justerm-renderer/src/glyph_cache.rs` — the slot map and LRU regions
- `justerm-renderer/src/rasterizer.rs` — OffscreenCanvas rasterisation (**wasm32/browser only**)
- `justerm-renderer/src/bitmap.rs` — `InkBounds` and the pure bitmap helpers
- `justerm-renderer/src/emoji.rs` · `bitmap.rs` — the two halves of the classification the cache
  takes as input: `is_emoji_text` decides by unicode, `is_color_bitmap` by what the font actually
  rendered (#284). The type it arrives as is owned by
  [emoji classification](emoji-classification.md)

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The design mirrors beamterm's
`beamterm-core/src/gl/glyph_cache.rs`, cited by path in the module doc — a **reimplementation
target**, not one of the three reference terminals, and pinned by nothing.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — this crate is outside the root workspace, so no `--workspace` or `--all` command reaches it;
  every gate it has is named for it by `--manifest-path`

## Blast radius

- [cell compositing](cell-compositing.md) — consumes the resolved slot and assumes it is stable for
  the frame
- [GPU upload](gpu-upload.md) — an LRU eviction changes the slot of a cell that was **not
  damaged**, which is why the upload planner diffs instead of trusting damage
- [cell geometry](cell-geometry.md) — slot size comes from the same ink scan that defines the cell
- [built-in block glyphs](builtin-block-glyphs.md) — occupy slots like any glyph but are produced
  geometrically rather than rasterised
- [wide glyph](wide-glyph.md) — the double-width LRU region exists for these, and the emoji
  classification cannot use core's `wide` flag because width is computed per character there

## Known holes / open

- **Zero governing records** for the stateful half of the renderer, including the eviction policy
  whose failure mode is corrupted cells rather than a missing glyph.
- **The rasteriser is browser-only**, so the correctness of what it produces is provable only in a
  headless browser proof — and those proofs have their own trap, since a fractional CSS canvas
  composites white under SwiftShader and reads as *sharpest* to a blur metric.
- **beamterm is cited by file path with no pin.** It is the design's origin and the citation cannot
  be checked.
