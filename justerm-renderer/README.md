# justerm-renderer

First-party **WebGL2 terminal grid renderer** for the [justerm](https://github.com/kihyun1998/justerm)
family. Reimplements the third-party `beamterm` renderer in justerm's own architecture — see
**ADR-0018** (supersedes ADR-0002) and **Epic #258**.

- **Consumer-side, sibling family member.** `justerm-core` still does not render; this crate does.
  It consumes a decoded frame + an *injected* palette (the consumer owns the theme) and paints via
  WebGL2. Rust → wasm, GL via [`glow`] — the same target for a plain browser and a Tauri webview.
- **A-ii (hot path in wasm).** Reference→RGB resolution + instance packing happen in Rust; the wasm↔JS
  boundary is crossed only for the handful of GL calls per frame (one instanced draw call per drawn
  grid).

## Status

Shipping — Epic #258 is closed and this is the family's active renderer. The GPU pipeline is in:
glyph atlas + rasterizer, an instanced grid draw call, cursor, selection / search / active-match
overlays, decorations, and live palette / font / metric setters. `justerm-web` renders through it
(#273), and it composites every layer itself — the widget no longer resolves per-cell colour.

**One context holds and draws N terminal grids** (Epic #287, in progress). A renderer starts holding
none: `addGrid` registers a terminal grid and returns its id, `setViewport` places it on the shared
drawing buffer in device pixels, `clearViewport` hides it while keeping every byte of its state, and
`render` draws each placed grid into its own rect. Every grid is equal — there is no first grid with
privileges, and no grid at all until the consumer asks for one.

**Terminals in the same font share one glyph atlas.** Resources are keyed by font configuration —
family, size, letter-spacing and line-height together — and refcounted, so six terminals in one font
hold one atlas, rasteriser and glyph cache between them, and the last one to leave a configuration
releases it. Changing one terminal's font moves it to a different entry rather than editing the one
its neighbours are drawing through, which is also what makes two terminals in two different fonts —
and therefore two different cell geometries — drawable side by side on the same canvas. `atlasCount()`
reports how many configurations are live and `bakes()` counts atlas builds, so the sharing is
something you can measure rather than assume.

**Every per-grid export names the grid it acts on** (0.15.0, breaking). `applyFrame`, `applyDamage`,
`setPalette`, `setOverlay`, `setActiveMatch`, `setDecorations`, `setCursor`, `clearCursor`,
`setPreedit`, `cols`/`rows`, `cellWidth`/`cellHeight`/`cssCellWidth`/`cssCellHeight`, the four
font/metric setters and the colour/cursor policy scalars all take a grid id first and throw on one
they do not know. The exports that belong to the surface — `render`, `setDevicePixelRatio`, the
context-loss handlers, `cssWidth`/`cssHeight` — do not.

Two consequences worth knowing before upgrading:

- **`resize(cols, rows)` split in two.** It used to size a grid *and* the drawing buffer, which is
  one number only while one grid owns the canvas. `resizeGrid(grid, cols, rows)` sets a grid's
  dimensions; `resizeSurface(width, height)` sizes the shared drawing buffer in **device pixels**,
  the same space `setViewport` takes. The single-grid arrangement is now three calls the consumer
  makes — `resizeGrid`, then `resizeSurface(cols * cellWidth(grid), rows * cellHeight(grid))`, then
  `setViewport(grid, 0, 0, …)` over the whole buffer — and asking for `cols * cellWidth` keeps the
  exactness the old call guaranteed. `cssWidth`/`cssHeight` still report the CSS box to display the
  buffer at.
- **Device pixels belong to the consumer, so a density change invalidates all of them.**
  `setDevicePixelRatio` re-bakes every atlas and touches nothing else: the drawing buffer stays the
  size it was asked for and every viewport rect stays where it was placed, because both are
  measurements the consumer made at the old density and only the consumer can re-make them. Re-issue
  `resizeSurface`, `resizeGrid` and `setViewport` after one — which you are doing anyway, the cell
  having just moved.

`addGrid(palette, defaultFg, defaultBg, fontFamily?, fontSize?, letterSpacing?, lineHeight?)` takes
the grid's font up front, so a grid joins a sibling's atlas instead of baking one it would abandon a
line later. `applyDamageTo` is retired — `applyDamage` addresses a grid itself.

Published to npm as **`justerm-renderer`** on its own **`renderer-v*`** tag track. That track is
deliberately separate from the workspace `v*` tags (which publish `justerm-core` +
`justerm-wasm-decode`): this crate's `web-sys`/`glow` deps are wasm32-only, so it carries its own
version line and ships on its own cadence.

## Build & test

This crate is **excluded from the root cargo workspace** (its `web-sys`/`glow` deps are wasm32-only),
so `cargo test --workspace` at the repo root does **not** reach it — always gate it by manifest path.

```bash
# pure logic (host) — the GL/wasm layer is 0-compile here
cargo test --manifest-path justerm-renderer/Cargo.toml
cargo fmt --manifest-path justerm-renderer/Cargo.toml --check
# full crate incl. the WebGL glue (wasm32 gate)
cargo build --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown
```

The GL layer is proved in a real browser rather than by unit test — `demo/*.html` pages that draw and
then read pixels back, swept across device pixel ratios:

```bash
pnpm run test:unit    # the pixel helpers the proofs read their evidence through (browserless)
pnpm run test:proofs  # builds the wasm, then drives the demo pages in headless Chromium
```
