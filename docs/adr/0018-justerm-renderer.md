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
- `cssWidth()` / `cssHeight()` → the CSS display box for that buffer. The canvas's CSS box stays the
  consumer's to set (as with beamterm's `auto_resize_canvas_css = false`).

**Why the CSS view must be a float.** Rounding it to a whole CSS pixel destroys the cell: the ink-scan is
16 device px tall at dpr 1 and 33 at dpr 2, i.e. 16.5 CSS px, and a rounded 17 does not scale back to 33.

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
