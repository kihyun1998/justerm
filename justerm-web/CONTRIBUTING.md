# Working on justerm-web

This file is for people with the repository checked out. It is deliberately not part of the npm
package — `package.json`'s `files` ships `dist/` and the licences, and npm force-includes only
`README.md` and `LICENSE*` beyond that, so nothing here reaches a consumer's `node_modules`.

```bash
pnpm install
pnpm test         # vitest — the pure render core (no GL/wasm)
pnpm typecheck    # tsc --noEmit, three tsconfig projects
pnpm build        # tsup -> dist/
pnpm demo         # NOT `vite demo` / `pnpm dlx vite demo`
pnpm test:e2e     # playwright, drives the real wasm in headless Chromium
```

> **Use `pnpm demo`, not `vite demo`.** `pnpm demo` runs the project's Vite with `vite.config.ts`,
> which sets `root: demo` and loads `vite-plugin-wasm` + `vite-plugin-top-level-await` (required to
> instantiate the two wasm-bindgen modules) and excludes them from esbuild dep-optimization.
> `vite demo` passes `demo` as the *root*, so Vite looks for config at `demo/vite.config.ts`
> (absent) and runs config-less — the wasm modules then fail to instantiate
> (`Cannot read properties of undefined (reading '__wbindgen_externrefs')`).

The demo of a shared surface is `demo/shared-surface.html` — two terminals at two font sizes on one
canvas. Run `pnpm demo`, then open `/shared-surface.html`.

## Source map

- `src/types.ts` — `FrameSource`, `DecodedFrame` (web's source-agnostic mirror of the decoder).
- `src/renderer.ts` — the `Renderer` port the widget drives.
- `src/justerm-renderer.ts` — the real adapter over `justerm-renderer` (WASM + WebGL2).
- `src/terminal-surface.ts` — `TerminalSurface`, `observeViewportRect`, `viewportOrigin`.
- `src/cell-mirror.ts` — the viewport-sized text mirror behind the accessible view and link state.
- `src/link-tracker.ts` — `LinkTracker`, internal; not a public export.
- `src/terminal.ts` — `Terminal`, which wires a `FrameSource` to a `Renderer`.
