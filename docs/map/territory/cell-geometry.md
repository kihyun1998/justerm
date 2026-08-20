# Territory — cell geometry

## What it is

How big a cell is, where the glyph sits inside it, and how those numbers survive the trip between
device pixels and CSS pixels. Everything geometric derives from **one measurement**: an ink scan of
the font's `█`.

One measurement *per font configuration*, since #772 — the renderer keys the ink scan, the cell and
the glyph box by (family, size, letter-spacing, line-height) and refcounts them, so two terminals in
two fonts have two cell geometries on one canvas. Nothing in the derivation changed; what changed is
that "the renderer's cell" is no longer a phrase with one referent. Every export here still reports
the **implicit default grid's** — `cellWidth`, `cellHeight`, `cssCellWidth`, `cssCellHeight`, `cols`,
`rows` — and no export reports another grid's, so a single-grid consumer sees exactly what it did.
See [multi-viewport](multi-viewport.md) for the tier and its lifetime.

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
- **The slot is no longer the cell plus a guard band** (#791). It carries a **bleed band** above and
  below as well — room for ink that leaves the cell, which the receiving cell reads back
  (ADR-0019 R1.2). Three consequences worth knowing before touching anything here: the band is
  derived *per font configuration* (`metrics::vertical_bleed`, from the gap between this face's `█`
  ink box and its declared line box, plus an empirical headroom), it is spent out of the same
  per-layer height as the guard band so it **lowers** the tallest cell the atlas can hold, and every
  site that places something into a slot must use the same origin — `pad()`, the path that lays a
  builtin bitmap in without going through the font, did not, and every block glyph sat a band too
  high until the pixel proofs said so.
- **The nesting is why tiling glyphs are a separate problem.** Once the glyph box sits *inside* the
  cell, anything meant to tile the cell must be drawn to the **cell** instead — see built-in block
  glyphs, which exists entirely because of this.
- **Spacing settings are CSS px** even though the geometry is device px, because they belong to the
  same description as `font_size`. The conversion happens on the way in, once.
- **The grid dimensions were outputs, and since #773 they are inputs reported back.** A consumer
  still measures a CSS box and divides it by the CSS cell to get `cols` / `rows`; what changed is
  what happens next. While the renderer sized the drawing buffer from the grid it could refuse one
  it could not draw, and `cols()` reported the grid actually adopted (#339). The buffer is the
  *surface's* now — one canvas holds N grids in M cell sizes, so there is no cell it can be a
  multiple of — and `resize_surface` adopts the browser's grant while `resize_grid` records what it
  was told. So `cols()` is an echo, and a consumer that asks for more than fits learns it from
  `cssWidth` rather than from `cols`.
- **The cell is per font configuration, so a surface can hold several at once** (#772/#773). Every
  cell reader takes a grid: `cellWidth(grid)`, `cssCellWidth(grid)`. See the cross-cutting invariant
  below for what a *reader* of one owes.

## Code

- `justerm-renderer/src/rasterizer.rs` — the ink scan of `█` (browser-only)
- `justerm-renderer/src/metrics.rs` — the cell box / glyph box nesting
- `justerm-renderer/src/dpr.rs` — `css_px`, the device→CSS derivation. It is the only direction
  left: grid_px, cells_that_fit and device_px were all retired with the grid-derived buffer (#773),
  because the surface is now asked for in device px and kept as asked, so nothing in this crate
  converts *into* device px any more. (Retired names un-backticked on purpose — this heading
  resolves every code-span against the source.)
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
- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  the two ways measurement can fail here (no 2d context, a `█` that rasterizes to no ink) are
  reported to JS as **string primitives**, so a consumer catching them gets no `.message` and no
  `.stack` — on the one failure whose whole value is the diagnostic, since there is nothing to
  retry and nothing to fall back to

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

- ~~**The founding measurement is graded unverified in its own record.**~~ — **measured 2026-08-20
  (#791)**. ADR-0022 now carries the comparison it asked for: the ink box is never the better cell
  and on DejaVu Sans Mono it destroys ink on 439 of 1579 sampled codepoints against the line box's
  84. The *decision* is still unchanged — adopting the line-box metric would move every grid's size
  and does not fix clipping on its own — so alternative (A) remains open, but it is no longer open
  for lack of evidence.
- **Headless proofs cannot be trusted naively here.** A fractional CSS canvas composites white under
  SwiftShader, and a sharpness metric will read that as *the sharpest* result — so a geometry proof
  needs to state what it is actually measuring.
- **No record for the resize direction**, and #773 made the question sharper rather than answering
  it. "Consumer sets the CSS box, renderer reports the grid" was already a contract inversion
  relative to a grid-first API, documented only by its parameters; now the surface and the grid are
  sized by two different calls in two different units, and *which* of them a consumer is obliged to
  re-issue after a font or density change is likewise documented only by the doc-comments on them.
