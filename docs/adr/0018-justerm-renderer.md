# ADR-0018: Build `justerm-renderer` (first-party WebGL2 renderer), supersede ADR-0002

Status: proposed (2026-07-07) — supersedes ADR-0002

## Context

ADR-0002 adopted the third-party `beamterm` renderer, weighing "adopt beamterm" against "write a fresh
WebGL2 renderer (a lot of GPU code)" and choosing adopt — explicitly flagging the residual risk as
"single-author bus-factor → vendor/fork worst case". Two things changed:

1. **The risk materialised as observed stagnation.** `junkdog/beamterm` is a single-author MIT project
   that went quiet after v1.0.0 (2026-03-30, last commit 2026-04-01). We cannot co-evolve the renderer
   contract with the engine.
2. **justerm pivoted to a first-party full-stack terminal.** `justerm-web` (Epic #103) was step one.
   A third-party renderer leaves a seam we do not own.

The pain is empirical and recurring — all of it GL *behaviour* impedance the consumer-side adapter
(`justerm-web/src/beamterm-renderer.ts`) works around, not data-format impedance:
- `batch.clear(bg)` does not back-fill un-drawn cells → GL-default blue (#255).
- beamterm has no cursor primitive → the adapter cell-inverts and hand-manages old/new damage.
- beamterm's built-in selection sees only on-screen cells → justerm owns the selection model (ADR-0002).

## Decision

**Build `justerm-renderer`** — a first-party WebGL2 terminal grid renderer, a new Rust crate and a
sibling family member (like beamterm was). **Reimplement, not fork**: study beamterm's real source as
prior-art (as `justerm-core` studied xterm.js) and build fresh in justerm's own architecture.

- **The engine boundary holds.** `justerm-core` still does NOT render. The change is the renderer's
  *provenance* (third-party → first-party), not the core's boundary invariant.
- **Rust → wasm, GL via `glow`.** wasm + WebGL2 is the same target for a plain browser and a Tauri
  webview, so pure-web support is retained exactly as beamterm provided it (no Tauri dependency).
- **A-ii: hot path in wasm.** The renderer consumes a decoded frame + an *injected* palette and does
  reference→RGB resolution + instance packing in Rust, drawing with a single instanced draw call; the
  wasm↔JS boundary is crossed only for the handful of GL calls per frame. This is the fastest webview
  CPU path (decode is already wasm, so the frame stays in linear memory through packing). The palette
  is injected by the consumer, so ADR-0002's "the consumer, not justerm, owns hex" survives — the
  renderer merely *applies* it and stays theme-agnostic.
- **Impedance is designed out** because we own the GL layer: a native cursor primitive, a clear that
  back-fills the default background, and selection highlights fed from the engine's model.
- **Crate placement (this slice, #259): excluded from the cargo workspace, independent version track.**
  The renderer depends on `web-sys`/`glow` (browser/GL-heavy). Keeping it out of `members` preserves
  the clean host `cargo test --workspace` gate — the same reason `fuzz` and `justerm-facade` are
  excluded. Its own gate is `cargo build -p justerm-renderer --target wasm32-unknown-unknown` (a
  documented blind spot, like the wasm32-only `justerm-wasm-decode/tests/web.rs`). Independent semver
  also matches beamterm's own model and the renderer's distinct release cadence.

## Coordinate spaces: device pixels are the source of truth

Decided in #331/#335 after the CSS-first shape of #252/#265 produced a real clipping bug.

The cell is *measured* in device pixels — the rasteriser ink-scans `█` at `FONT_SIZE * devicePixelRatio`
— and that integer is what the shader receives as `u_cell_size`. Everything else is derived from it:

- `cell_width()` / `cell_height()` → **device px, `u32`**. The exact cell. The bare name carries it, as in
  xterm.js's `dimensions.device.cell` and beamterm's `cell_size()`.
- `cssCellWidth()` / `cssCellHeight()` → **CSS px, `f32`, unrounded**. The consumer divides its available
  box by these to decide how many columns fit (xterm.js's `FitAddon` does exactly this against
  `dimensions.css.cell.width`) and maps mouse coordinates through them.
- `resize(cols, rows)` → the drawing buffer becomes `cols * cell_width()` × `rows * cell_height()`, an
  exact multiple of the cell. xterm.js sizes its canvas the same way
  (`device.canvas.width = cols * device.cell.width`).
- `cssWidth()` / `cssHeight()` → the CSS display box for that buffer, **unrounded** (#337). The canvas's
  CSS box stays the consumer's to set (as with beamterm's `auto_resize_canvas_css = false`).

**Why the CSS view must be a float.** Rounding it to a whole CSS pixel destroys the cell: the ink-scan is
16 device px tall at dpr 1 and 33 at dpr 2, i.e. 16.5 CSS px, and a rounded 17 does not scale back to 33.

**And why the CSS canvas *box* is a float too** (#337, decided against xterm.js). xterm rounds it:
`css.canvas.width = Math.round(device.canvas.width / dpr)` (`WebglRenderer.ts:687`). We do not, on three
grounds, in order of weight:

1. **Rounding's error is absolute, the alternative's is not.** A rounded box misses the buffer by up to
   `dpr/2` device px whatever the canvas size; an unrounded one misses only by the browser's layout grain
   — a CSS length snaps to 1/64 px before layout (Blink `layout_unit.h:473`, `FixedPoint<6, int32_t>`) —
   i.e. `<= dpr/128`. Measured in headed Chromium at dpr 1.1 against a 36-device-px buffer: unrounded
   `style=32.727px` used **35.9906** device px; rounded `style=33px` used **36.3000** — 0.3 px *larger than
   the buffer feeding it*, so the image is stretched. That is the very failure xterm's comment blames for
   blurriness ("the backing canvas image is 1 pixel too large for the canvas element size") — it blames
   `ceil`, but `round` overshoots half the time. (Gecko's grain is said to be 1/60; not read from source.)
2. **Every *derived* CSS length in both references is already fractional.** xterm's
   `css.cell = device.cell / dpr` (`WebglRenderer.ts:694-695`) and beamterm's `css_cell_size()`
   (`terminal_grid.rs:405-413`) both divide device px by the DPR and keep the float. `cssWidth()` is a
   derived length of exactly that kind.
3. **xterm's rounded `css.canvas` is a different animal, and its reason does not transfer.** That value
   also sizes `screenElement` (`WebglRenderer.ts:211-212`) and is read by `MouseCoordsService:38`,
   `SelectionService:419`, `DomRenderer:146-149`, `AccessibilityManager:405` and
   `OverviewRulerRenderer:148` — so an integer costs xterm nothing there. But that is a co-benefit, **not
   its stated reason**: the comment at `WebglRenderer.ts:682-686` argues only `round` over `ceil`. *xterm
   never evaluates a fractional box at all* — which is a gap in its reasoning, not evidence against one.
   We own no DOM, so the overshoot trade-off is all that is left, and it favours leaving the float alone.
4. **beamterm does not round a CSS box either.** Its CSS box is an integer *input*
   (`renderer.rs:87,96-101`) and the device buffer is the derived, rounded quantity (`physical_size()`,
   `renderer.rs:164-168`), with the sub-cell remainder letterboxed (`terminal_grid.rs:240-241`). That
   route is closed to us: #331 made the grid the source of truth.

**The browser may refuse the buffer, and then the canvas attribute must follow it down** (#339). WebGL
grants "a drawing buffer with smaller dimensions" whenever the request cannot be satisfied, with no error
and no lost context; `canvas.width` keeps the request while `drawingBufferWidth` reports the grant. The
limit is not knowable in advance — Chromium clamps each axis to
`min(max_texture_size, max_renderbuffer_size, max_viewport_dims[axis])` and then applies a hard-coded
`5760×5760` area budget — so `resize()` reads the granted buffer back and adopts the grid that fits it,
reporting it through `cols()`/`rows()`.

This diverges from every reference. xterm.js (`WebglRenderer.ts:205-206,679-680`), beamterm
(`renderer.rs:86-104`) and three.js (`WebGLRenderer.js:667-708`) all leave `canvas.width` at the request
and merely viewport into the grant, so their grids overhang a clamped buffer silently. We re-set the canvas
down because the CSS box above is derived from `canvas.width`: an attribute describing a buffer that does
not exist would make `cssWidth()` a lie. The second allocation is the price of that coupling, not an
oversight.

**Nothing here is exact, and the old proof pretended otherwise.** `demo/dpr.html` asserted
`Math.round(cssWidth() * dpr) === cols * cell`. Since `cssWidth()` *is* `cols*cell/dpr`, that reduces to
`round(x/d*d) === x`; the browser never appears in it. Measured: it stays green at dpr 1, 1.1 and 1.5 with
the accessor rounding. It now asks `getBoundingClientRect()` instead, and reddens at dpr 1.1. In truth no
CSS length maps a 360-device-px buffer onto whole device pixels at dpr 1.1 — only boxes that are multiples
of 10 CSS px do, and `cols * cell` is not generally a multiple of 11. There is a nearest answer, not an
exact one, and `cssWidth()` returns it.

**What the bug actually was.** Not "rounding". The buffer came from `round(cssBox * dpr)` while the layout
came from `cols * device_cell` — two quantities with no reason to agree. At `devicePixelRatio = 1.1`
(browser zoom at 110 %) every grid overhung its buffer by 1–2 device px and the last column was clipped.

Two cures are sound, and both ship:

- **buffer ← grid** (xterm.js, `device.canvas.width = cols * device.cell.width`), leftover container space
  outside the canvas. This is ours: the overhang becomes unrepresentable rather than absorbed.
- **grid ← buffer, letterbox the remainder** (beamterm, `cols = canvas_width / cell_width`, sub-cell
  remainder painted with `canvas_padding_color`). beamterm keeps a rounded CSS pixel box and is correct.

We chose xterm's because it needs no padding colour and because the consumer already knows `cols`/`rows`
— it computed them by dividing its box by the CSS cell (xterm's `FitAddon`; our `justerm-web/src/fit.ts`).
The cost: the canvas is exactly the grid, so a consumer whose container is larger must size the container
itself (`cssWidth()`/`cssHeight()`), or the gutter shows the page through (with #298 translucency, literally).

The consumer still lays out in CSS px (#252 survives); it just no longer sizes the drawing buffer in them.

## Consequences

- A GPU-discipline effort (glyph atlas, instancing, shaders, context-loss) — larger and longer than the
  parsing engine. Sliced incrementally under Epic #258 (#259 scaffold → GPU core → glyph pipeline →
  browser integration → cursor/selection → the `justerm-web` switch → docs flip), each slice tracer-bullet
  demoable, mirroring how #103 sliced justerm-web ("compliance is cumulative").
- beamterm and justerm-renderer run in parallel until parity; the switch (#273) is its own slice — no
  big-bang. `docs/architecture.md` + `CLAUDE.md` keep saying "beamterm renders" until then (#274).
- xterm-parity colour policy (contrast/dim/tile-glyph/inverse, #223–#241) currently lives tested in
  `justerm-web` TS; it ports to Rust cumulatively (#272), TS-fed hybrid allowed in the interim.
- beamterm is MIT (© 2025 Adrian Papari); studied as prior-art only. If any substantial portion is
  ever copied, its copyright notice must be preserved — reimplementation from reading avoids this.

## Prior art

Renderer-owning projects are whole terminals/apps (Alacritty, Warp, Wezterm, Kitty) or the renderer is
the product (xterm.js webgl-addon, beamterm); engine *libraries* rent or pair (VS Code = xterm+webgl).
Building justerm-renderer is the deliberate move of justerm from the "engine library" quadrant into the
"first-party full-stack" quadrant — a vision choice, recorded here, not a mechanical refactor.
