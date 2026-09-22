# justerm-renderer

WebGL2 terminal grid renderer for the [justerm](https://github.com/kihyun1998/justerm) family,
compiled to WASM. Hand it a decoded frame and a palette; it paints the grid.

- **One context draws N terminal grids.** A browser caps live WebGL contexts at roughly sixteen, so
  a renderer holds a registry of grids and places each one on a shared drawing buffer rather than
  taking a canvas per terminal.
- **The theme is injected.** The renderer never picks a colour. You pass a 256-entry palette plus a
  default foreground and background, all packed `0xRRGGBB`, and it resolves every colour reference
  against them.
- **The hot path stays in WASM.** Colour resolution and instance packing happen in Rust; the
  WASM↔JS boundary is crossed only for the handful of GL calls per frame, and a drawn grid is one
  instanced draw call.

Most applications do not use this package directly —
[`justerm-web`](https://www.npmjs.com/package/justerm-web) wraps it in a terminal widget that also
owns focus, input, selection and accessibility. Reach for this package when you are building that
layer yourself.

## Install

```bash
npm install justerm-renderer
```

It is a wasm-bindgen bundler-target module, so a bundler needs WASM and top-level-await support
(with Vite: `vite-plugin-wasm` + `vite-plugin-top-level-await`, and list the package in
`optimizeDeps.exclude`).

## Usage

```js
import { JustermRenderer } from "justerm-renderer";

// The renderer binds to one canvas and owns its GL context.
const canvas = document.querySelector("#term");
const r = new JustermRenderer("#term");

// A grid is registered explicitly; a renderer starts holding none. The palette is
// 256 entries of 0xRRGGBB, followed by the default fg/bg the `Default` colour ref
// resolves to. Font arguments are optional and default to the built-in monospace.
const palette = new Uint32Array(256);
const grid = r.addGrid(palette, 0xcdd6f4, 0x1e1e2e, "monospace", 16);

// Size the grid, then the shared drawing buffer, then place the grid on it.
// `cell_width`/`cell_height` are DEVICE pixels, the same space `setViewport` takes.
const [cols, rows] = [80, 24];
r.resizeGrid(grid, cols, rows);
r.resizeSurface(cols * r.cell_width(grid), rows * r.cell_height(grid));
r.setViewport(grid, 0, 0, cols * r.cell_width(grid), rows * r.cell_height(grid));

// Display the buffer at the CSS size it reports.
canvas.style.width = `${r.cssWidth()}px`;
canvas.style.height = `${r.cssHeight()}px`;

// A dense row-major frame: `cols * rows` entries per column. `bg`/`fg` are colour
// references (not RGB — the renderer resolves them), `codepoints` one base codepoint
// per cell, `flags` the cell attribute bits.
r.apply_frame(
  grid,
  cols,
  rows,
  bg,          // Uint32Array
  fg,          // Uint32Array
  codepoints,  // Uint32Array
  flags,       // Uint16Array
  true,        // blinkOn — you own the blink clock
);

r.render();
```

To drive it from justerm's wire format instead, decode with
[`justerm-wasm-decode`](https://www.npmjs.com/package/justerm-wasm-decode) and pass the damage frame
to `apply_damage`, which scatters a span-ordered frame into the grid it already holds rather than
requiring a dense one.

> **Four methods are snake_case**: `apply_frame`, `apply_damage`, `cell_width` and `cell_height`.
> Everything else is camelCase. The bundled `justerm_renderer.d.ts` is authoritative.

## Several terminals on one canvas

Every per-grid call names the grid it acts on and throws on an id it does not know:
`apply_frame`, `apply_damage`, `setPalette`, `setOverlay`, `setActiveMatch`, `setDecorations`,
`setCursor`, `clearCursor`, `setPreedit`, `setLinkHover`, `cols`/`rows`,
`cell_width`/`cell_height`/`cssCellWidth`/`cssCellHeight`, the font setters and the colour/cursor
policy scalars. The calls that belong to the surface rather than to any one grid —
`render`, `resizeSurface`, `setDevicePixelRatio`, `cssWidth`/`cssHeight` and the context-loss
handlers — take no grid.

Sizing is therefore two calls, not one, because a buffer holding several grids has no single cell
size to be a multiple of:

| Call | Sizes | Units |
|---|---|---|
| `resizeGrid(grid, cols, rows)` | one grid's dimensions | cells |
| `resizeSurface(width, height)` | the shared drawing buffer | device px |
| `setViewport(grid, x, y, width, height)` | where a grid lands on that buffer | device px |

`clearViewport(grid)` hides a grid while keeping every byte of its state — its cells stay resident
and its glyph atlas stays alive, so showing it again is a `setViewport` and costs no atlas bake.
`removeGrid(grid)` is the end of its life.

**Terminals in the same font share one glyph atlas.** Resources are keyed by the whole font
configuration — family, size, the regular and bold weights, letter-spacing, line-height and subpixel text
together — and refcounted, so six terminals in one font hold one atlas, rasteriser and glyph cache between
them, and the last one to leave a configuration releases it. Changing one terminal's font moves it
to a different entry rather than editing the one its neighbours draw through, which is what lets two
terminals in two different fonts — and so two different cell geometries — sit side by side on one
canvas. `atlasCount()` reports how many configurations are live and `bakes()` counts atlas builds,
so the sharing is something you can measure rather than assume.

## Subpixel text

`setSubpixelAntialiasing(grid, true)` draws a grid's text with per-channel (LCD / ClearType)
coverage where the browser produces it, which reads sharper on a subpixel display. It is off by
default, it is part of the font configuration (so it bakes or joins an atlas like a font change), and
it does not move the cell. `addGrid`'s trailing argument sets it at birth.

It applies only over an **opaque** background: a default-background cell under a translucent
`setBgAlpha` keeps grayscale, because one alpha cannot carry three coverages. Colour emoji and the
built-in block glyphs are unchanged. The browser decides where it draws LCD text at all — Chromium on
Windows does for antialiased text of ordinary sizes (measured: at 28–42 device px, and not at 49 or
above), and a face it draws aliased gets none. Where it draws none, no colour fringe appears, though
dark ink is still drawn at the lighter weight the browser gives it rather than grayscale's.

## Device pixels belong to you

Every size above is a measurement you made, so a density change invalidates all of them.
`setDevicePixelRatio(dpr)` re-bakes every atlas and touches nothing else: the drawing buffer keeps
the size it was asked for and every viewport rect stays where it was placed, because only you can
re-make those measurements. Re-issue `resizeSurface`, `resizeGrid` and `setViewport` after one —
which you are doing anyway, the cell having just moved.

## Losing the GL context

A browser may destroy a WebGL context at any moment. The renderer rebuilds itself on
`webglcontextrestored`, keeping each grid's content, because that content lives on the CPU side and
never left. What has no other signal is the context that does not come back:

```js
r.setContextRestoreTimeoutMs(3000);
r.setOnContextLoss(() => showBanner("The GPU dropped this terminal."));

r.isContextLost();    // has a loss been reported to us
r.isRestoreOverdue(); // …and did it miss its deadline
```

`isContextLost()` answers *"was I told"*, not *"is the GPU usable right now"*: a browser destroys a
context synchronously and only queues the event, so for a short window this reads `false` while
every GL call is already dead. It is the honest thing to show a user and the wrong thing to gate
drawing on — the renderer guards its own work on a stricter predicate it does not export.

## License

Dual-licensed under [MIT](https://github.com/kihyun1998/justerm/blob/master/LICENSE-MIT) or
[Apache-2.0](https://github.com/kihyun1998/justerm/blob/master/LICENSE-APACHE), at your option.
