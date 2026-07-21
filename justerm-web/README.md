# justerm-web

Browser terminal widget for the [justerm](https://github.com/kihyun1998/justerm) engine.
It consumes a `DecodedFrame` (structure-of-arrays cells + span directory, produced by
[`justerm-wasm-decode`](https://www.npmjs.com/package/justerm-wasm-decode)) and paints it
with the first-party
[`justerm-renderer`](https://www.npmjs.com/package/justerm-renderer) (WASM + WebGL2).

justerm-web is the *consumer* half of the family: the engine parses VT and produces frames,
this widget renders them and turns user input into intent. It does no I/O — you feed it
frames and it hands you back what the user did.

## Install

```bash
npm install justerm-web
```

`justerm-renderer` and `justerm-wasm-decode` come along as dependencies. Both are
wasm-bindgen modules, so a bundler needs WASM + top-level-await support (with Vite:
`vite-plugin-wasm` + `vite-plugin-top-level-await`, and list both packages in
`optimizeDeps.exclude`).

## Usage

```ts
import { JustermRenderer, StubFrameSource, Terminal } from "justerm-web";

// 1. The renderer owns the canvas. The theme is injected — justerm is theme-agnostic
//    and never guesses a colour (all values are packed 0xRRGGBB).
const renderer = await JustermRenderer.create({
  canvasSelector: "#term",
  fontFamily: "monospace",
  fontSize: 16,
  theme: {
    ansi: [
      0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5,
      0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ],
    defaultFg: 0xcdd6f4,
    defaultBg: 0x1e1e2e,
    selectionBg: 0x45475a,
  },
});

// 2. A FrameSource supplies DecodedFrames. In production this is your IPC channel
//    (PTY -> engine -> wire -> decode); StubFrameSource drives it by hand.
const source = new StubFrameSource();

// 3. The Terminal wires the two together and owns focus, input and selection.
//    `input` receives an *Intent* (a key press, paste, mouse report...), not bytes:
//    encoding intent for your backend is the host's job, not the widget's.
const term = new Terminal(source, renderer, {
  element: document.getElementById("term-container")!,
  input: { send: (intent) => myBackend.send(intent) },
  // Pixel -> cell is host policy too, so the widget asks for the geometry it needs.
  getGeometry: () => {
    const r = canvas.getBoundingClientRect();
    const { width: cellWidth, height: cellHeight } = renderer.cellSize();
    return { originX: r.left, originY: r.top, cellWidth, cellHeight, cols, rows };
  },
});
```

`Terminal` takes many more options (scroll, selection, search, links, accessibility) —
each is an injected seam rather than a built-in policy, so the host stays in control of
transport, clipboard and theme. See the [demo](https://github.com/kihyun1998/justerm/blob/master/justerm-web/demo/main.ts)
for a fully wired example.

## What it does and does not do

**Does**: renders frames, resolves the injected theme, tracks selection and search
highlights, exposes a screen-reader mirror and an accessible view, turns pointer/keyboard
events into intent.

**Does not**: read a PTY, own a transport, pick colours, or run the terminal engine. Those
are the host's — that boundary is why the engine stays independently testable
([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/)).

## Links

- [Repository](https://github.com/kihyun1998/justerm) ·
  [Issues](https://github.com/kihyun1998/justerm/issues)
- [`justerm-core`](https://crates.io/crates/justerm-core) — the engine (Rust)
- Architecture: [`docs/architecture.md`](https://github.com/kihyun1998/justerm/blob/master/docs/architecture.md)

## Develop (in the repo)

```bash
pnpm install
pnpm test         # vitest — the pure render core (no GL/wasm)
pnpm typecheck    # tsc --noEmit, three tsconfig projects
pnpm build        # tsup -> dist/
pnpm demo         # NOT `vite demo` / `pnpm dlx vite demo`
pnpm test:e2e     # playwright, drives the real wasm in headless Chromium
```

> **Use `pnpm demo`, not `vite demo`.** `pnpm demo` runs the project's Vite with
> `vite.config.ts`, which sets `root: demo` and loads `vite-plugin-wasm` +
> `vite-plugin-top-level-await` (required to instantiate the two wasm-bindgen modules)
> and excludes them from esbuild dep-optimization. `vite demo` passes `demo` as the
> *root*, so Vite looks for config at `demo/vite.config.ts` (absent) and runs
> config-less — the wasm modules then fail to instantiate
> (`Cannot read properties of undefined (reading '__wbindgen_externrefs')`).

## Architecture

- **`FrameSource`** (`src/types.ts`) — abstract source of `DecodedFrame`s. Frame mode
  wires it to the consumer's IPC channel; in-wasm mode to an in-browser engine.
  `StubFrameSource` drives it by hand for tests/demos.
- **`Renderer`** port (`src/renderer.ts`) — the small interface the widget drives.
  `JustermRenderer` is the real adapter (wraps `justerm-renderer`, WASM + WebGL2); a fake
  covers the widget's wiring without a GL context.
- **`CellMirror`** (`src/cell-mirror.ts`) — a viewport-sized **text** mirror (ADR-0011): it
  applies a frame's scroll op so the screen-reader row tree stays correct across scroll, and
  serves row text + the column map (#152). Text-only since #504 — colour resolve and
  compositing live in the renderer's wasm (#273), so the widget maps no cells to draw ops.
- **`Terminal`** (`src/terminal.ts`) — wires a `FrameSource` to a `Renderer`.

## Licence

MIT OR Apache-2.0.
