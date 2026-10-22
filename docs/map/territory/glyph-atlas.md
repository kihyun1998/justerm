# Territory — glyph atlas

## What it is

Turning a cell's grapheme into a slot in a GPU texture array: key it, allocate a slot from an LRU
region, rasterise it with the browser's own text engine (or the builtin drawer), bake it into the
slot — with a subpixel light mask when the configuration asks for one — and evict when a region is
full. It is the stateful half of drawing text: [cell compositing](cell-compositing.md) assumes every
cell's slot is already resolved, and this is what resolves it. Classification (emoji or not) is an
input it takes, not a decision it makes ([emoji classification](emoji-classification.md)).

## Governing decisions

- [ADR-0021 — one WebGL2 context draws N terminal grids](../../adr/0021-single-context-multi-viewport.md)
  governs the atlas's **tier and lifetime** since #772: D2 puts the atlas, rasteriser, glyph cache
  and the cell derived from them in the per-config tier, and its *Consequences* own what LRU eviction
  costs once the cache has more than one writer. Nothing else here.
- [ADR-0031 — glyph bake geometry answers to our own model](../../adr/0031-glyph-bake-geometry-answers-to-our-own-model.md)
  governs what the bake may do to a glyph before the composite sees it — place, scale, refuse — which
  is where #792's condense lives.
- [ADR-0033 — subpixel coverage is one mask and a measured curve](../../adr/0033-subpixel-coverage-is-one-mask-and-a-measured-curve.md)
  — what a slot's channels hold when a configuration opts into LCD text (#961), and why it is not
  xterm.js's bake-over-background.
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the crate exists and
  reimplements beamterm; the atlas design (two regions, the ASCII fast path, the eviction policy) is
  inherited from beamterm and decided by no record.

## Design model

Status: the tier (ADR-0021), the bake's latitude (ADR-0031) and the subpixel layout (ADR-0033) are
recorded; the cache design and the resolver's rules are from source and tests only.

- **Rasterisation is the browser's text engine, deliberately.** OffscreenCanvas brings font
  fallback, shaping and colour emoji — what a hand-rolled rasteriser would have to reproduce. The
  cost is that `rasterizer.rs` is wasm32-only, so what it produces is provable only in a browser.
- **Two LRU regions, normal and double-width** (`NORMAL_CAPACITY` 2048 slots, `WIDE_CAPACITY` 2048
  two-slot glyphs from `WIDE_BASE`), O(1) lookup / insert / evict, mirroring beamterm. The 95
  normal-styled ASCII glyphs have **pre-allocated fixed slots** (`ascii_fast_path`), so the common
  case never churns the cache. The shader addresses a slot with 13 bits; compile-time asserts keep
  the regions inside it and keep a wide pair from straddling a texture layer.
- **The cache allocates; it does not classify.** `GlyphKind` arrives from the caller; width comes
  from core's `WIDE_CHAR` flag, never a renderer width table (two tables can disagree). A key is the
  grapheme text — NFC-normalised for a cluster — plus `FontStyle`.
- **The hot loop was lifted out of the browser-only layer on purpose** (#280) so `cargo test` reaches
  it. Three gaps the #264 adversarial pass found were unreachable from the host before: within-frame
  eviction corrupting earlier cells (P0), a rasterise failure stranding a committed-but-unuploaded
  slot (P1), and control codepoints burning a normal-region slot (S — C0, DEL and C1 fold to space in
  `sanitize_codepoint`). Rasterise and upload are injected closures into `resolve_frame`.
- **A within-frame eviction can corrupt cells already packed in the same frame** — the atlas is
  mutable during a pass, so a slot handed out early could be reused before the frame is drawn. It is
  *refused*, not repaired: a frame needing more distinct glyphs than a region holds returns
  `FrameExceedsCapacity` instead of drawing wrong. The pin set (`FramePins`) is the caller's, so
  within one `render` it spans every grid packed in the frame.
- **Since #772 the same eviction is also a cross-grid event**, and that half cannot be refused the
  same way: the cache is shared by every grid on one font configuration, so one grid's pack can
  repoint a slot another grid's packed instances still name, and the upload diff cannot see it (the
  instance floats did not change, only what the atlas holds at that index). So the cache counts its
  evictions (`GlyphCache::evictions`), a grid records the count it packed against
  (`packed_at_evictions`), and `render` re-packs a grid whose count is stale. What that does and does
  not settle: [multi-viewport](multi-viewport.md).
- **A malformed dense frame is refused, deliberately unlike beamterm** (#355). beamterm tolerates —
  `update_cells` zips and silently truncates, `update_cells_by_index` filters out-of-range writes —
  but `apply_frame` is a dense contract taken straight from a JS caller, so a frame shorter than its
  grid is a caller defect and inventing cells hides it. `resolve_frame` bounds the frame by the data
  it carries (`GridOverflows`, `FrameShorterThanGrid`); trusting `cols × rows` once allocated a
  million cells for a 1000×1000 grid backed by two codepoints (#355). The sparse `apply_damage` path
  is tolerant but validates its span directory first ([frame adapter](frame-adapter.md)).
- **The resolver resets a pending wide right-half at every row start, and that is load-bearing.**
  A wide lead in the final column is legal core state (ADR-0025; `Row::resize` truncates through a
  pair, and the alt screen resizes without reflowing, #529), so a lead can set a right half no spacer
  consumes; without the reset the next row's first cell inherits it. The lead draws as its left half.
  Handling it here is conformance, not cover: a lead's spacer is decidable from the viewport, so it
  is the consumer's under ADR-0017.
- **A slot is larger than its cell**: `bleed | cell | bleed` rows since #791 and the same
  horizontally since #966, each band sized per font configuration, so ink that leaves a cell
  survives the bake and the neighbour can read it (ADR-0019 R1.2). Slot *count* is untouched — slot
  ids key everything — but the texture grows: +44 % at dpr 1 and +23 % at dpr 2 on the reference face
  for the vertical band (#791, 2026-08-20), and `fit_cell_to_atlas`'s ceiling drops by `2 * bleed` on
  each axis.
- **A glyph wider than its box is condensed before it is baked** (#792, ADR-0031). `Rasterizer::draw`
  measures the grapheme's ink with `measureText` — no readback, one call on the cache-miss path — and
  draws under `scale(s, 1)` past the box plus `bleed_x`. The slot keeps its size and `GlyphKey` its
  two fields: the fit is a pure function of the grapheme and the configuration geometry, and every
  rebuild path reaches it through the one `rasterize` seam. A builtin glyph is outside it by
  construction — the builtin check precedes `fill_text`.
- **A subpixel configuration's slot holds two coverages** (#961, ADR-0033): grayscale in A and the
  light mask — the glyph drawn white over opaque black — in RGB (`lcd::with_lcd`). Every bake path
  reaches it through `Rasterizer::finish`, called after classification because a mask in RGB would
  read as colour to `is_color_bitmap`; a colour emoji and a builtin skip it. The dark-ink exponent is
  fitted once per configuration bake from a calibration draw (`Rasterizer::calibrate`,
  `lcd::fit_dark_gamma`) — the platform's text gamma, 2.65 on the Windows 11 machine ADR-0033 records.
- **Every 2D context the rasteriser creates is `willReadFrequently`** (#1019), through the one
  `Rasterizer::context_2d`. Every bake reads back with `getImageData`, and Chromium starts a 2D
  context GPU-backed without the flag, making each read a synchronous GPU→CPU readback; a font-size
  change at 80×24 cost a median 143–221 ms to paint on a GTX 1050 Ti, 31–37 ms with the flag, and all
  305 readbacks of a subpixel bake were byte-identical with and without it (#1019, Windows 11,
  2026-09-29). The flag's usual cost — a slower canvas→texture upload — does not apply: `upload_glyph`
  uploads the bytes `getImageData` returned. `demo/read-frequently.html` asserts the flag on every read
  it drives (construction, rebuild, subpixel, cache miss); it spies only `OffscreenCanvas`, and a
  context-loss restore is covered because it rebakes through the same `bake_config`.
- **How the fragment stage reads a slot** (`shader.rs` `FRAG_SRC`, `slot_texel`). 32 glyphs stack
  vertically per texture-array layer, so a slot is `(layer = slot >> 5, band = slot & 31)`. The
  cell-local texcoord is inset into the slot's content region through `u_cell_uv = (origin.xy,
  span.xy)` (#288, #791), so the guard band is never sampled while the content maps edge to edge of
  the cell — block elements are baked at cell size and tile. It is **not a symmetric inset**: with a
  band the slot's edges differ, and a symmetric inset would stretch the cell over the band. The
  texcoord is nudged `+0.001` off the exact texel edge so `NEAREST` cannot round to a neighbour
  (beamterm `cell.frag`; DPR != 1, #265); a neighbour's slot is read with the same nudge.
- **One bake function builds every configuration** (`JustermRenderer::bake_config`). A new
  configuration primes the 95 ASCII glyphs; a rebuild (density change, context restore) re-bakes
  every resident glyph into the slot it already occupies (`bake_all_glyphs`), so packed instances
  survive with no re-pack. It is an associated function because the constructor has no `self` yet,
  and so it cannot read a live tier by accident — which lets every caller commit atomically or throw
  the result away. A resident glyph bakes across two slots iff its slot id is in the wide region
  (`slot_id() >= WIDE_BASE`), not iff it is `Wide`: that is what catches a wide colour emoji while a
  narrow one (`GlyphKind::EmojiNarrow`, #297) stays one slot.
- **Input bound.** Distinct glyphs are bounded by the two region capacities (over-capacity frames are
  refused); a cluster's length by core's `MAX_CLUSTER_TAIL` before it reaches the wire; the frame's
  shape by the data it carries. The slot's pixel size is bounded by `fit_cell_to_atlas` against
  `MAX_TEXTURE_SIZE`, and the calibration canvas by `CALIBRATION_MAX_PX`.

## Code

- `justerm-renderer/src/glyph_resolve.rs` — `resolve_frame`, `Cells`, `FramePins`, `ResolveError`,
  `sanitize_codepoint` (host-testable)
- `justerm-renderer/src/glyph_cache.rs` — `GlyphCache::get_or_insert`, `GlyphCache::touch`,
  `GlyphCache::next_eviction`, `GlyphCache::evictions`, `GlyphCache::entries`, `ascii_fast_path`,
  `GlyphKey`, `GlyphKind`, `GlyphSlot`, `slot_texcoord`, `EMOJI_FLAG`, `WIDE_BASE`
- `justerm-renderer/src/rasterizer.rs` — `Rasterizer::new`, `Rasterizer::rasterize`,
  `Rasterizer::finish`, `Rasterizer::draw`, `Rasterizer::context_2d`, `Rasterizer::calibrate`
  (**wasm32 only**)
- `justerm-renderer/src/bitmap.rs` — `split_wide_bitmap`, `ink_bounds`, `PADDING`
- `justerm-renderer/src/lcd.rs` — `with_lcd`, `fit_dark_gamma` (host-testable)
- `justerm-renderer/src/metrics.rs` — `fit_cell_to_atlas`, `horizontal_fit`
- `justerm-renderer/src/shader.rs` — `FRAG_SRC`'s slot read: `u_cell_uv`, `slot_texel`
- `justerm-renderer/src/webgl/font.rs` — `bake_config`, `prebake_ascii_into`, `bake_all_glyphs`
- `justerm-renderer/src/webgl/draw.rs` — `apply_frame`, `resolve_and_pack` (the closures handed to
  `resolve_frame`), the `packed_at_evictions` stamp
- `justerm-renderer/src/webgl.rs` — `build_atlas`, `upload_glyph`

## Reference behaviour

- [A 2D canvas the glyph bake reads back](../../agents/reference-facts.md#a-2d-canvas-the-glyph-bake-reads-back-1019-verified-2026-09-29)
  — xterm.js creates its atlas scratch canvas `willReadFrequently` too.
- [Subpixel (LCD) text coverage in the glyph atlas](../../agents/reference-facts.md#subpixel-lcd-text-coverage-in-the-glyph-atlas-961-verified-2026-09-22)
  — what the references bake for LCD text, against ADR-0033's one-mask design.
- [A glyph the font draws wider than its cell](../../agents/reference-facts.md#a-glyph-the-font-draws-wider-than-its-cell-792-verified-2026-08-21)
  — the references' squeeze mechanisms ADR-0031 reads as a catalogue.
- The cache design mirrors beamterm `beamterm-core/src/gl/glyph_cache.rs`, cited by path in the module
  doc — a reimplementation target, not one of the reference terminals, and pinned by nothing.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the renderer is outside the root workspace, so no `--workspace` or `--all` command reaches the
  cache or the resolver; every gate names the crate by `--manifest-path`.
- [a CPU record of GPU state is true only while that object lives](../invariant/a-gpu-mirror-dies-with-its-context.md)
  — `GlyphCache`'s resident slots outlive the atlas texture, so every replacement re-bakes them into the same slots (`bake_config(Some(cache))`)
- [every atlas-slot writer and the shader agree on one origin](../invariant/an-atlas-slot-has-one-origin.md)
  — the font writer places at `slot_geometry`'s origin and the shader samples at `cell_uv`'s; they agree by algebra, not shared code

## Blast radius

- [cell compositing](cell-compositing.md) — the packer assumes every slot it names holds the same
  glyph until the frame is drawn; anything that lets a slot be repointed mid-frame breaks every
  cell packed before it.
- [GPU upload](gpu-upload.md) — an eviction changes the slot of a cell that was not damaged, which is
  why the upload planner diffs instead of trusting damage; a cross-grid eviction is the case the diff
  cannot see.
- [multi-viewport](multi-viewport.md) — the cache is shared per configuration, so its eviction count
  is the only signal a sibling grid has that its packed slots went stale.
- [cell geometry](cell-geometry.md) — the slot is the padded cell plus both bleed bands; a change to
  the cell or band size changes the texture's size and the `u_cell_uv` origin the shader insets by.
- [built-in block glyphs](builtin-block-glyphs.md) — produced geometrically, but laid into the slot
  by their own `pad()` at the same origin the shader assumes.
- [emoji classification](emoji-classification.md) — the verdict picks the region and the flag; a new
  kind must still be region-consistent between allocation and lookup.
- [GL context lifecycle](gl-context-lifecycle.md) — a restore re-bakes every resident glyph into its
  old slot; the bake must stay a pure function of the configuration for that to be correct.

## Known holes / open

- **No record governs the cache design itself** — two regions, the fast path, the LRU policy —
  although its failure mode is corrupted cells rather than a missing glyph.
- **The rasteriser is browser-only**, so the correctness of what it produces is provable only in a
  headless-browser proof — whose own trap is that a fractional CSS canvas composites white under
  SwiftShader and reads as *sharpest* to a blur metric.
- **beamterm is cited by file path with no pin.** It is the design's origin and the citation cannot
  be checked.
- **Subpixel text is measured on two platforms.** ADR-0033's numbers are Windows 11 + Chromium; the
  Linux CI runner's headless Chromium draws LCD text with a near-flat dark-ink curve and meets the same
  1 % bounds. macOS is unmeasured; wherever the canvas draws no LCD text the proof checks only that no
  fringe appears. The browser declines LCD text for large glyphs (none at 49 device px) and for a face
  it draws aliased, so `demo/subpixel.html` mounts its grid at 28 CSS px to exercise it at all. The
  Linux measurement predates `willReadFrequently` (#1019); readback bytes were compared with and
  without it on Windows 11 only, and no gate would notice a platform that stops drawing LCD text on a
  CPU-backed canvas, since `subpixel.html` does not require fringes.
- **A refused frame may already have displaced glyphs the last one drew** (`apply_frame`'s doc): the
  refusal keeps the grid's last packed instances, but slots evicted before the refusal can show other
  glyphs until a frame is accepted.
