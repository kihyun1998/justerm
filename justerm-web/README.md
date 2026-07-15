# justerm-web

Browser terminal widget for the [justerm](../) engine: it consumes a
`DecodedFrame` (structure-of-arrays cells + span directory, as produced by
`justerm-wasm-decode`) and renders it with the first-party
[`justerm-renderer`](../justerm-renderer) (WASM + WebGL2). Frame mode now (decode wire
frames in the consumer), in-wasm later (engine compiled to WASM in the browser) — both
behind the `FrameSource` seam. (The renderer was `beamterm` until the #273 switch;
ADR-0018 pivoted it to the first-party renderer.)

This is a folder in the justerm repo, **not** a Cargo workspace member — it has
its own `package.json` version and ships to npm separately.

## Develop

```bash
pnpm install
pnpm test         # vitest — the pure render core (no GL/wasm)
pnpm typecheck    # tsc --noEmit
pnpm build        # tsup → dist/
```

## Demo (manual, in-browser)

```bash
pnpm demo         # NOT `vite demo` / `pnpm dlx vite demo`
```

Open the printed `http://localhost:5173/`. You should see **"hi"** at the
top-left of the grid.

> **Use `pnpm demo`, not `vite demo`.** `pnpm demo` runs the project's Vite with
> `vite.config.ts`, which sets `root: demo` and loads `vite-plugin-wasm` +
> `vite-plugin-top-level-await` (required to instantiate the two wasm-bindgen
> modules) and excludes them from esbuild dep-optimization. `vite demo` passes
> `demo` as the *root*, so Vite looks for config at `demo/vite.config.ts` (absent)
> and runs config-less — the wasm modules then fail to instantiate
> (`Cannot read properties of undefined (reading '__wbindgen_externrefs')`).

## Architecture

- **`FrameSource`** (`src/types.ts`) — abstract source of `DecodedFrame`s. Frame
  mode wires it to the consumer's IPC channel; in-wasm mode to an in-browser
  engine. `StubFrameSource` drives it by hand for tests/demos.
- **`Renderer`** port (`src/renderer.ts`) — the small interface the widget
  drives. `JustermRenderer` is the real adapter (wraps `justerm-renderer`, WASM +
  WebGL2); a fake covers the widget's wiring without a GL context.
- **`frameToDrawOps`** (`src/render-core.ts`) — the pure `DecodedFrame` → draw-op
  mapping (span walk, colour resolve, flags, wide-char, grapheme). No GL, no
  wasm, so the vitest suite covers it with golden frames. A `RenderPolicy` seam
  is where the theme/render policy (inverse, selection, dim, contrast) plugs in.
- **`Terminal`** (`src/terminal.ts`) — wires a `FrameSource` to a `Renderer`.
