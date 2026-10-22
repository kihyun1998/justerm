# Territory — cell geometry

## What it is

How big a cell is, where the glyph sits inside it, how big the atlas slot around it is, and how
those numbers survive the trip between device pixels and CSS pixels — for the cell, the grid and the
drawing buffer. Everything derives from one measurement of the face *per font configuration*: its
advance, floored, for the width (#962); its line box, read at the CSS size, scaled by the DPR and
ceiled, for the height and baseline (#986); and an ink scan of its `█` for the horizontal bleed band
only (#966). Since #772 the renderer keys that measurement by the grid's font selectors, so two
terminals in two fonts have two cell geometries on one canvas, and since #773 every cell reader names
its grid (`cellWidth(grid)`, `cssCellWidth(grid)`, `cols(grid)`). On the web side it includes the
density-change signal (`DprWatcher`) and the surface's re-derivation after one.

## Governing decisions

- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) —
  the founding record, amended by #962 (the width is the face's floored advance, as xterm.js and
  alacritty size it) and #986 (the height is xterm.js's line box — a maintainer's judgement recorded
  there with what it was decided on). The ink scan of `█` survives only to size `bleed_x`, and the
  record keeps the invariant that the builtin `█` never re-enters measurement.
- [ADR-0023 — a spacing setting is CSS pixels](../../adr/0023-spacing-settings-are-css-pixels.md) —
  because the font description it belongs to is; both references take device px.
- [ADR-0021 — one WebGL2 context draws N grids](../../adr/0021-single-context-multi-viewport.md) — D2
  puts the cell in the per-config tier; D3 ("tier the fields") is what split the grid's size from the
  drawing buffer's (#773) and says a density change invalidates every device-px quantity the consumer
  gave.
- [ADR-0032 — a shared fact is owned by its producer](../../adr/0032-a-shared-fact-is-owned-by-its-producer.md)
  — why the surface request is stored verbatim in device px rather than as a CSS box.
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) § "Coordinate spaces:
  device pixels are the source of truth" — the device/CSS direction and the layout-grain error.

## Design model

Status: the measurement, spacing unit and tiering are recorded (ADR-0022/0023/0021); the slot
layout, fit and surface rules are from source and doc-comments, several of them measured on issues.

- **Device pixels are the source of truth; the CSS view is derived** (`dpr.rs`). The renderer owns
  the DPR the atlas is baked at; the rasteriser reads the advance at `FONT_SIZE * dpr` and the line box
  at `FONT_SIZE` (scaled and ceiled); the shader lays the grid out in device px (`u_cell_size`);
  `css_px` is a **float** so the derivation can be undone — a consumer's `cols * cssCellWidth()` box
  scales back to `cols * cell` device px
  (`a_css_box_built_from_the_float_cell_scales_back_to_the_device_grid`). "Scales back" is
  arithmetic, not physics (#337): no CSS length lands exactly on the device grid at a fractional DPR
  (`L × 1.1` is whole only when `10 | L`, and the browser reports `1.100000023841858`); the used box
  misses by the layout grain, measured 0.0016–0.0156 device px in headed Chromium at dpr 1.1 (#337).
- **"The face's line box" is not one number** (#986, `metrics::line_box`). Chromium rounds
  `fontBoundingBox{Ascent,Descent}` to whole pixels at the size asked for, so the CSS-size read scaled
  is not the drawn-size read: Consolas at 14 CSS px × 1.5 is `ceil(17 × 1.5)` = 26 against 19 + 5 = 24
  (#986). `line_box` takes xterm.js's side: the CSS-size read, the product in double precision as JS
  takes it (single precision turns a line box of 10 at dpr 1.1 into 11, where xterm gets 12), and the
  alphabetic baseline the *drawn* size's `ideographicBaseline` above the box's bottom, where xterm
  draws (a descent rule sat Yu Gothic 2–5 px high). Consequences: the height is not a function of the
  device size alone; the box can be up to 2 px shorter than the face's `█` (harmless — builtin draws
  `█` to the cell) or than the drawn-size line box (the vertical band takes that gap); and
  `demo/line-box-cell.html` needs a size where the two reads disagree, which no size at dpr 1 is.
- **The width is the advance of `W`, floored** (`metrics::advance_width`, #962), after a WebView2 app
  measured `█` 2 px wider than Consolas's advance (`rasterizer.rs` module docs). History: the ink box
  was never the better cell — on DejaVu Sans Mono it destroyed ink on 439 of 1579 sampled codepoints
  against the line box's 84 (#791, recorded in ADR-0022); #792's "at most 13 %" horizontal clipping
  was withdrawn by #966 because it had measured the advance rounded *up* (on Consolas, the ink box
  again); at the floored advance clipping rises 21–75 %, which the horizontal band #966 pays for.
- **The cell is measured at the `normal` weight, whatever weights the configuration draws at**
  (#928; `Rasterizer::new` passes `FontWeight::NORMAL`). The weights key the configuration, so a
  weight change re-bakes the atlas but never moves the grid — no re-fit owed, and a profile draws the
  same grid in a native pane as in an xterm.js pane, whose `CharSizeService` also measures without the
  weight. PenTerm relies on that (penterm 7dadc3c95 `src/blocks/terminal/lib/nativePaneView.ts` —
  `applyNativePaneView`: "A weight re-bakes the atlas but never moves the cell"). Decided by the
  maintainer on a headless-Chromium table showing the other choice moves the cell on real faces
  (`monospace` 8×16 / 8×16 / 9×16 at weights 100/400/900, dpr 1; Courier New 10×18 / 10×18 / 11×16;
  Consolas unchanged) and the xterm.js fact only; alacritty, read afterwards, measures at the
  configured regular face, and the maintainer kept `normal` (#928, 2026-09-17). Heavy regular text is
  drawn into a box measured for 400, as bold always was, so the condense and band paths carry it.
- **A weight is validated before it reaches a `font` string** (#928, `css_font::FontWeight`). An
  invalid CSS `font` assignment is dropped whole, so a bad weight would silently swap the face and
  move the cell. xterm.js's set is taken (`normal`, `bold`, `100`..`900`, a number in `[1, 1000]`); a
  setter ignores anything else; `bolder`/`lighter` are refused as xterm.js refuses them.
- **The cell box and the glyph box are two rectangles** (#338). `letterSpacing` and `lineHeight`
  grow the cell, not the glyph (`metrics::device_cell`), and something has to say where the glyph
  sits inside. That split is prior-art consensus — both references carry a char box beside a cell box.
  Once the glyph box is inside the cell, anything meant to tile must be drawn to the **cell** instead
  ([built-in block glyphs](builtin-block-glyphs.md)).
- **The glyph is centred in a grown cell, as xterm does and alacritty does not**
  (`metrics::glyph_offset`). alacritty baseline-anchors (`glyph_cache.rs:256`) and leaves vertical
  placement to `glyph_offset`; xterm, a browser rasteriser feeding a GPU atlas, splits the room. The
  halves split as xterm splits them — `floor` across, `round` down — so an odd remainder lands right
  and top: arbitrary, and mirrored so as not to invent a different arbitrary. A cell narrower than the
  glyph (negative spacing) offsets by zero, and the glyph reaches into its neighbour up to the band.
- **The atlas slot *is* the padded cell plus the bands** (#359, #791, #966; `metrics::slot_geometry`).
  The bitmap already carries the glyph at its offset, so the shader neither places nor masks it: wider
  cells space the text because the bitmap has wider margins, and a wide glyph's halves touch because
  it was baked centred over its two-cell advance. The vertex stage pixel-snaps each quad corner
  (`floor(... + 0.5)` in `VERT_SRC`). Every site that places something into a slot must use the same
  origin (`metrics::cell_uv` for the shader; `Rasterizer::pad` for a builtin bitmap — which did not,
  for one commit, and every block glyph sat a band too high).
- **Each band is derived per configuration** — `metrics::vertical_bleed` from the gap between the
  cell's ascent/descent and the face's declared line box at the drawn size, `metrics::horizontal_bleed`
  from what the face's `█` overhangs the glyph box, each plus `BLEED_HEADROOM_PX` (4, empirical: the
  line-box budget alone still clipped 168 glyphs on Cascadia Mono and 263 on Lucida Console at em 24,
  and xterm.js reserves the same 4 px, #791). Each band is spent out of its own axis of the texture, so
  it lowers the largest cell the atlas holds. They reach the shader as `u_bleed_px`; both floor at the
  headroom, so the fragment stage's `armed` guards defend a value the pipeline does not produce.
- **The bake may change the glyph on one axis, only to make it fit** (#792, ADR-0031;
  `metrics::horizontal_fit`). Faces draw many single-cell glyphs wider than their box — over 2095
  codepoints at em 24 device px, 252 on DejaVu Sans Mono and 1153 on the demo face, 35 to 629 losing
  more than 30 % of their ink (#792) — and the horizontal band holds only a few pixels, because the
  Canvas API exposes no horizontal counterpart to `fontBoundingBox`. So a glyph is condensed into its
  window (box plus band each side), or translated if it fits the window but sits outside it. Three
  load-bearing properties, each a defect the proof caught: keyed on the **glyph box**, never the cell
  (negative `letter_spacing` narrows the cell past the glyph on purpose); **x only**, so the vertical
  band is untouched by construction; and it fires on nothing that fits (`.` and `i` guard it in the
  proof), with `FIT_TOLERANCE_PX` = 1 for `measureText`'s one-row bias. A width-2 glyph runs the same
  fit with a two-cell box and has almost nothing to do: over 880 wide codepoints at em 24, zero
  exceed two cells on Consolas, Cascadia Mono, Courier New, Lucida Console or DejaVu Sans Mono, and
  eight do on the demo face by one device px, inside the tolerance (#792).
- **Underline and strikethrough thickness is xterm.js's rule**, `max(1, round(font_px / 15))`
  (`metrics::line_thickness`, #517): a Canvas renderer has no font file and so no
  `underline_thickness` metric. It replaced beamterm's `0.05 × glyph_box`, measured at 11.3 % of the
  cell against xterm's 6.2 %. `round`, not `floor`, so a 15-px em is 1.
- **Spacing settings are CSS px** (ADR-0023) even though the geometry is device px; the conversion
  happens once, on the way in (`round(letter_spacing * dpr)` in `device_cell`).
- **Input bound:** every consumer-supplied size is clamped or refused at its setter — font size `>= 1`
  and non-finite ignored, letter spacing non-finite → 0, line height `>= 1` and non-finite → 1, the
  cell to `[1, MAX_CELL_PX]` then to what the atlas holds, a weight to `[1, 1000]`, a surface to
  positive (non-positive refused), a grid to at least 1×1.

### The cell is bounded twice

Both times by asking and then adopting. `MAX_CELL_PX` (4096) exists because `setLetterSpacing(1e9)` is
finite, so no setter check stops it, and a `u32::MAX` cell makes the adopt-what-fits loop
unsatisfiable (#339); it sits far above any real cell and below the smallest `MAX_TEXTURE_SIZE`
measured (8192, headless SwiftShader). `fit_cell_to_atlas` then shrinks the cell until the atlas
holding it — one padded slot wide, `GLYPHS_PER_LAYER` slots tall, both bands included — fits the
texture limit (#359): `glTexStorage3D` over the limit raises `GL_INVALID_VALUE` without throwing, glow
does not look, and the storage-less texture samples `(0,0,0,1)` — the terminal solid with
foreground, invisible to a proof drawn with `█`. Measured: `lineHeight = 16` on a 16-px glyph asked
for a `258 × 8256` texture and an `M` came back fully lit (#359). The adopted cell is what
`cell_height()` reports.

### The grid dimensions were outputs

And since #773 they are inputs reported back. A consumer still measures a CSS box and divides it by
the CSS cell to get `cols` / `rows`. While the renderer sized the drawing buffer from the grid, it
could refuse a grid it could not draw and `cols()` reported the grid adopted (#339). The buffer is the
*surface's* now — one canvas holds N grids in M cell sizes — so `resize_surface` adopts the browser's
grant, `resize_grid` records what it was told, and `cols()` is an echo: a consumer that asks for more
than fits learns it from `cssWidth`. The resize direction is the reverse of a grid-first API
([fit](fit.md)).

### A spacing change is a new configuration

`setLetterSpacing` and `setLineHeight`: the slot is the padded cell, so a spacing change is a
different configuration, costing an atlas bake rather than a uniform — and two grids can hold two
spacings at once. Neither setter rejects: `setLineHeight` clamps to `>= 1` where xterm.js throws
([who re-fits after a spacing change](../../agents/reference-facts.md#who-re-fits-after-a-spacing-change-578-verified-2026-07-29)),
and `setLetterSpacing` maps a non-finite value to 0. A failed bake leaves the policy unchanged, so
the next call retries. The consumer reads the adopted result back from the cell, and must re-fit.

### The surface's size is the consumer's device-px request

(`resizeSurface`, `resizeGrid`, `apply_surface_size`.) Until #773 one call, `resize(cols, rows)`, wrote
two tiers: the implicit grid's dimensions and the drawing buffer, snapped to `cols * cell`. ADR-0021
D3 separates them, and multi-viewport makes the separation load-bearing.

- *`resizeGrid` is not clamped to the rect, and that is a decision.* Keeping every column inside the
  buffer was the renderer's while the buffer was derived from the grid; the rect is now the consumer's
  measured box. An oversized grid corrupts nothing — per-cell reads are bounds-checked and each grid
  draws under its own `gl.scissor` — but its mouse mapping and reflow are wrong.
- *Device px rather than a CSS box.* A CSS box (three.js's `setSize`) would make this the one
  canvas-addressing export in CSS px while every rect on that canvas is device px. It keeps the exact
  fit reachable: `cols * cellWidth(grid)` is two integers this crate handed out. The consumer sets the
  canvas's CSS display box itself (beamterm's `auto_resize_canvas_css = false` is the same split).
- *Ask, then adopt; at most two passes.* Pass 2 asks for a buffer already granted, so it cannot be
  clamped again. `canvas.width` / `height` are re-set **down** to the grant — where xterm.js, beamterm
  and three.js leave them oversized — because #337 couples the CSS display box to them. The request is
  stored, not passed, because one of its three callers (construction, `resizeSurface`, a context
  restore) is the restore; and stored **verbatim**, because a CSS box stored through the renderer's
  copy of the density would come back wrong by the ratio (that copy lags — `setDevicePixelRatio` drops
  a notification while the context is lost; ADR-0032). A grant smaller than the request moves every
  placed rect, since a rect's GL y is measured from the buffer's bottom.
- *A read-back of no size is not a grant* (#639). A lost context reports 0×0; adopting it would commit
  a 1×1 surface the restore then rebuilds at. So the request stays committed and only verification
  defers; in that window `cssWidth` describes a buffer that does not exist yet, and a consumer sizing
  its canvas from it must repeat its fit after the restore.
- *The CSS readbacks are unrounded.* A rounded `cssCellWidth` loses the device cell for good (33 device
  px at dpr 2 is 16.5); a rounded `cssWidth` misses the buffer by up to `dpr/2` device px and can round
  *up*, stretching the image (`rounding_the_css_box_moves_it_further_off_the_device_grid_than_leaving_it_alone`).
  `cellWidth` carries the bare name because it is the exact measured cell.

### A density change moves the cell and may move the CSS box

(`JustermRenderer.setDevicePixelRatio`, #325, the consumer half of #322.) The renderer re-bakes every
configuration at the new ratio, keeping each glyph's slot, and stops: it leaves the drawing buffer as
asked and never touches the DOM. A no-op at an unchanged ratio (`dpr_changed`), dropped while the
context is lost (the restore re-reads the live DPR).

- *Who notices.* `DprWatcher` (`justerm-web/src/dpr-watcher.ts`) listens on a `matchMedia` resolution
  query and **re-arms a new query at the new ratio on every change** — a resolution query is bound to
  the ratio it was created with, so the obvious implementation fires once and goes quiet. A port of
  xterm.js's `CoreBrowserService`.
- *Who re-derives.* The density is the surface's, so `TerminalSurface.setDevicePixelRatio` re-bakes,
  re-derives every attached terminal's buffer and CSS box, presents once, and only then fires
  `onDensityChange` — the host owes a fresh `resizeSurface` and a fresh rect per terminal there, since
  nothing else can re-measure them (ADR-0021 D3). Doing it per terminal would issue two
  `resizeSurface` calls, each clearing the buffer. Without the re-ask a move to a denser monitor would
  halve the displayed terminal; without the CSS rewrite the browser scales a stale box, the blur #322
  exists to remove.
- *The CSS box can move.* The device cell is `round(metric * dpr)`, and dividing it back need not land
  on the old CSS cell (font 16, 25×6 grid: CSS height 96 at dpr 1 and 1.5, 99 at dpr 2). Whether it
  moves is font dependent — an e2e assertion that it had moved went red on CI's Linux font (19 → 38
  where the local one went 19 → 37). What always holds, and what to assert, is
  `canvas.style × dpr === drawing buffer`.
- *No re-fit, deliberately.* Re-deriving the grid needs the container's measurements, which the
  widget does not hold (#417, #578); xterm.js draws the same line — `handleDevicePixelRatioChange`
  re-measures, repaints and calls no `resize` — and its `FitAddon` stays manual.

## Code

- `justerm-renderer/src/metrics.rs` — `advance_width`, `line_box`, `device_cell`, `glyph_offset`,
  `fit_cell_to_atlas`, `slot_geometry`, `cell_uv`, `vertical_bleed`, `horizontal_bleed`,
  `horizontal_fit`, `line_thickness`, `MAX_CELL_PX`, `BLEED_HEADROOM_PX`, `FIT_TOLERANCE_PX`
  (host-testable)
- `justerm-renderer/src/dpr.rs` — `css_px`, `dpr_changed`
- `justerm-renderer/src/css_font.rs` — `FontWeight`, `font_string`
- `justerm-renderer/src/rasterizer.rs` — `Rasterizer::new` (the advance, line-box and `█` reads),
  `Rasterizer::set_cell`, `Rasterizer::bleed_x`, `Rasterizer::bleed_y` (wasm32 only)
- `justerm-renderer/src/shader.rs` — `VERT_SRC`'s pixel snap, `FRAG_SRC`'s `u_bleed_px`
- `justerm-renderer/src/webgl/surface.rs` — `cell_width`, `cell_height`, `css_cell_width`,
  `css_cell_height`, `cols`, `rows`, `resize_grid`, `resize_surface`, `apply_surface_size`,
  `css_width`, `css_height`
- `justerm-renderer/src/webgl/font.rs` — `bake_config`, `set_font_size`, `set_font_family`,
  `set_font_weight`, `set_font_weight_bold`, `set_letter_spacing`, `set_line_height`,
  `set_device_pixel_ratio`
- `justerm-web/src/dpr-watcher.ts` — `DprWatcher`
- `justerm-web/src/terminal-surface.ts` — `setDevicePixelRatio`, `onDensityChange`
- `justerm-web/src/justerm-renderer.ts` — `setDevicePixelRatio` (the per-terminal entry, routed
  through the surface)

## Reference behaviour

- [Renderer ink channels](../../agents/reference-facts.md#renderer-ink-channels)
- [Font weight — what it reaches, and what the cell is measured at](../../agents/reference-facts.md#font-weight--what-it-reaches-and-what-the-cell-is-measured-at-928-verified-2026-09-17)
- [A device-pixel-ratio change — who notices, and what they do about it](../../agents/reference-facts.md#a-device-pixel-ratio-change--who-notices-and-what-they-do-about-it-325-verified-2026-08-10)
- [Who re-fits after a spacing change](../../agents/reference-facts.md#who-re-fits-after-a-spacing-change-578-verified-2026-07-29)
- [When is a resize redundant — box, grid, or cell](../../agents/reference-facts.md#when-is-a-resize-redundant--box-grid-or-cell-632-verified-2026-07-30)
- [Resizing while the GL context is lost](../../agents/reference-facts.md#resizing-while-the-gl-context-is-lost--the-reference-never-asks-the-question-639-verified-2026-08-03)
- [A glyph the font draws wider than its cell](../../agents/reference-facts.md#a-glyph-the-font-draws-wider-than-its-cell-792-verified-2026-08-21)
- The cell/glyph-box split and the floored-advance width are backed by both references, cited in
  ADR-0022. The height is xterm.js's rule only: it follows that engine's rounding rather than the
  face, because a consumer runs both engines from one setting — a judgement recorded in ADR-0022.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md) — this territory
  *produces* the value; the invariant is about everything downstream that divides by it and cannot
  tell that it moved (#578).
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the renderer is outside the root workspace; its host tests run only where a gate names it by
  `--manifest-path`.
- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  the two ways measurement fails here (no 2d context, a `█` that rasterises to no ink) reach JS as
  string primitives, with no `.message` or `.stack`, on the one failure whose value is the diagnostic.
- [every atlas-slot writer and the shader agree on one origin](../invariant/an-atlas-slot-has-one-origin.md)
  — owns the slot's shape (`slot_geometry`) and the shader's origin (`cell_uv`) that every writer must match

## Blast radius

- [built-in block glyphs](builtin-block-glyphs.md) — exists because the glyph box sits inside the
  cell; its `pad()` hard-codes the slot origin `cell_uv` owns, and its `█` must never feed the scan
  that sizes `bleed_x`.
- [glyph atlas](glyph-atlas.md) — the slot is the padded cell plus both bands; any change here changes
  the texture's size, the largest cell it holds, and re-bakes every resident glyph.
- [cell compositing](cell-compositing.md) — the neighbour-ink read and the underline marks are in
  cell-local device px (`u_cell_size`, `u_bleed_px`, `dots_per_cell`); a band of 0 or a cell that
  stops being whole pixels breaks their sampling.
- [caret drawing](caret-drawing.md) — caret rects and `setCursorThickness` are expressed in this
  device-px cell.
- [fit](fit.md) — the consumer divides its box by `cssCellWidth`; a cell that moves without the
  consumer re-fitting leaves `cols` describing a grid that no longer fills the box.
- [multi-viewport](multi-viewport.md) — every viewport rect is a device-px measurement the host made;
  a density change invalidates all of them at once and only the host can re-issue them.
- [GL context lifecycle](gl-context-lifecycle.md) — a restore re-applies the stored surface request
  and re-reads the live DPR; storing anything derived through the renderer's density breaks it.
- [widget lifecycle](widget-lifecycle.md) — where a cell change arrives in the consumer; the hidden
  textarea's anchor is re-read at a point of use rather than invalidated, because nothing here pushes
  (#631).

## Known holes / open

- **The bake's decisions share no written rule, and one attempt is on record as failed.** #791 (a
  band for ink that leaves the cell), #792 (condense what will not fit) and #359 (a tiling family
  bypasses the font) rhyme, so #798 proposed one rule: *the bake may place a glyph, may scale it on an
  axis with no other budget, may refuse the font for a family whose correctness is about neighbours —
  but may never change what the box is.* Measured against every geometry site in the bake on
  2026-08-21, it explained four of seven: it is silent about ink destroyed outside the readback window
  and about the slot's band, and `fit_cell_to_atlas` shrinks the cell when the texture limit refuses
  it — the one thing the sentence forbids. #798 was closed as falsified; start from the refutation.
  (ADR-0031 since records what the bake may do to a glyph, not what it may do to the box.)
- **Headless proofs cannot be trusted naively here.** A fractional CSS canvas composites white under
  SwiftShader, and a sharpness metric reads that as the sharpest result — a geometry proof must state
  what it measures.
- **Which call a consumer must re-issue after a font or density change is written only in
  doc-comments.** The surface and the grid are sized by two calls in two units; ADR-0021 D3 tiers the
  fields but no record states the consumer's re-issue obligation as a contract.
- **`DprWatcher`'s composition is proven only in a browser** — the class is unit-tested, but
  `JustermRenderer` reads `window.matchMedia` in its constructor and cannot be built in a unit test
  (`dpr-watcher.ts` module doc).
