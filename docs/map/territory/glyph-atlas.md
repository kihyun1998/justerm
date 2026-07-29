# Territory — glyph atlas

## What it is

Turning a grapheme into a slot in a GPU texture: classify it, allocate a slot, rasterise it with the
browser's own text engine, and evict when the atlas is full. The stateful half of drawing text —
[cell compositing](cell-compositing.md) assumes a slot is already resolved, and this is what resolves
it.

## Governing decisions

**None.**

- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the crate exists and
  reimplements beamterm; the atlas design is inherited rather than decided here

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
  drawn. This is the hazard the split exists to make testable.

## Code

- `justerm-renderer/src/glyph_resolve.rs` — the pure per-cell slot resolution (host-testable)
- `justerm-renderer/src/glyph_cache.rs` — the slot map and LRU regions
- `justerm-renderer/src/rasterizer.rs` — OffscreenCanvas rasterisation (**wasm32/browser only**)
- `justerm-renderer/src/bitmap.rs` — `InkBounds` and the pure bitmap helpers
- `justerm-renderer/src/emoji.rs` · `glyph_class.rs` — the classification the cache takes as input

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The design mirrors beamterm's
`beamterm-core/src/gl/glyph_cache.rs`, cited by path in the module doc — a **reimplementation
target**, not one of the three reference terminals, and pinned by nothing.

## Cross-cutting invariants

*(none identified yet)*

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
