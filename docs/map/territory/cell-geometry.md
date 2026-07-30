# Territory — cell geometry

## What it is

How big a cell is, where the glyph sits inside it, and how those numbers survive the trip between
device pixels and CSS pixels. Everything geometric in the renderer derives from **one measurement**:
an ink scan of the font's `█`.

## Governing decisions

- [**ADR-0022 — the grid cell is the ink box of the font's `█`**](../../adr/0022-cell-geometry-from-an-ink-scan.md)
  — and everything geometric follows from it. The measurement method is inherited from beamterm and
  its grounds are **marked unverified in the record itself**
- [**ADR-0023 — a spacing setting is CSS pixels**](../../adr/0023-spacing-settings-are-css-pixels.md)
  — because the font description it belongs to is. Both references express spacing in *device* px,
  so one font description would otherwise speak two units

## Design model

- **Device pixels are the source of truth; the CSS view is derived.** The rasteriser ink-scans `█` at
  `FONT_SIZE * dpr`, the shader lays the grid out in device px (`u_cell_size`), and the drawing
  buffer is an exact multiple of them. `cssCellWidth()` is a **float** on purpose, so the derivation
  can be undone — a consumer's `cols * cssCellWidth()` box scales back to `cols * cell` device px
  exactly.
- **The cell box and the glyph box used to be the same rectangle, and are not any more** (#338).
  While they were identical the shader could stretch one glyph quad across one cell and be right by
  construction. `letterSpacing` and `lineHeight` break that identity: the cell grows, the glyph does
  not, and something has to say where inside the cell the glyph sits.
- **That split is prior-art consensus, unlike the measurement.** Both references carry a char box
  beside a cell box — this is one of the few places the renderer can point at agreement rather than
  at its own reasoning.
- **The nesting is why tiling glyphs are a separate problem.** Once the glyph box sits *inside* the
  cell, anything meant to tile the cell must be drawn to the **cell** instead — see built-in block
  glyphs, which exists entirely because of this.
- **Spacing settings are CSS px** even though the geometry is device px, because they belong to the
  same description as `font_size`. The conversion happens on the way in, once.
- **The grid dimensions are outputs, not inputs.** A consumer sets a CSS box and reads `cols` / `rows`
  back — the resize contract runs in that direction.

## Code

- `justerm-renderer/src/rasterizer.rs` — the ink scan of `█` (browser-only)
- `justerm-renderer/src/metrics.rs` — the cell box / glyph box nesting
- `justerm-renderer/src/dpr.rs` — `css_px`, `grid_px`, the device↔CSS derivation
- `justerm-renderer/src/webgl.rs` — `css_cell_width`, `css_cell_height`, `css_width`, `css_height`,
  `cols`, `rows`, `set_font_size`, `set_font_family`, `set_line_height`, `set_letter_spacing`,
  `set_device_pixel_ratio` — the largest single share of the crate's wasm exports
  (`rg -c '#\[wasm_bindgen' justerm-renderer/src/webgl.rs` for the total)

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Renderer ink channels](../../agents/reference-facts.md#renderer-ink-channels)

The cell/glyph box split is quoted directly in `metrics.rs`'s module doc from both references. **The
ink-scan measurement itself has no such backing** — ADR-0022 records it as inherited and grades its
grounds as unverified, which is unusual enough to be worth knowing before building on it.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — this territory *produces* the value; the invariant is about everything downstream that divides
  by it and cannot tell that it moved (#578)
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — this crate is outside the root workspace, so no `--workspace` or `--all` command reaches it;
  every gate it has is named for it by `--manifest-path`

## Blast radius

- [built-in block glyphs](builtin-block-glyphs.md) — exists *because* of the box split above; a change to
  the nesting changes what those glyphs must be drawn to
- [cell compositing](cell-compositing.md) — supplies the box every instance is drawn into
- [glyph atlas](glyph-atlas.md) — rasterises into slots sized by this measurement
- [caret report](caret-report.md) — `setCursorThickness` and the caret rects are expressed in this
  geometry
- [fit](fit.md) — the resize contract runs consumer→CSS box, renderer→`cols`
  / `rows`, which is the reverse of what a grid-first API would do
- [widget lifecycle](widget-lifecycle.md) — where a cell change *arrives* in the consumer, and the one
  place it meets a cache that outlives it: the hidden textarea's anchor is re-read at a point of use
  rather than invalidated, because nothing here pushes (#631)

## Known holes / open

- **The founding measurement is graded unverified in its own record.** Everything geometric derives
  from the `█` ink scan, and ADR-0022 says its grounds were inherited rather than established.
- **Headless proofs cannot be trusted naively here.** A fractional CSS canvas composites white under
  SwiftShader, and a sharpness metric will read that as *the sharpest* result — so a geometry proof
  needs to state what it is actually measuring.
- **No record for the resize direction.** "Consumer sets the CSS box, renderer reports the grid" is a
  contract inversion relative to a grid-first API, and it is documented only by its parameters.
