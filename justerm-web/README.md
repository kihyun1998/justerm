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
  // Every length is CSS px, because that is what `clientX`/`clientY` are — `renderer.cellSize()`
  // is DEVICE px, so divide it by the ratio or the pointer lands on the wrong cell at dpr != 1.
  // Return measured values: the cell must be positive and finite, the counts non-negative
  // integers. A `0` or `NaN` cell (an unlaid-out or hidden container) makes every pointer event
  // resolve to a garbage cell; the widget warns once per field rather than failing.
  getGeometry: () => {
    const r = canvas.getBoundingClientRect();
    const cell = renderer.cellSize(); // device px
    const dpr = window.devicePixelRatio || 1;
    return {
      originX: r.left,
      originY: r.top,
      cellWidth: cell.width / dpr,
      cellHeight: cell.height / dpr,
      cols,
      rows,
    };
  },
});
```

`Terminal` takes many more options (scroll, selection, search, links, accessibility) —
each is an injected seam rather than a built-in policy, so the host stays in control of
transport, clipboard and theme. See the [demo](https://github.com/kihyun1998/justerm/blob/master/justerm-web/demo/main.ts)
for a fully wired example.

## Tearing down

`Terminal.dispose()` is **end of life**, not unmount. It stops consuming frames, detaches the
listeners it attached, and disposes the renderer you handed it — so nothing the widget started can
still draw. Build a new `Terminal` (and a new renderer) rather than mounting a disposed one; it
throws if you try.

```ts
term.dispose(); // also disposes `renderer`
```

It also stops the context-loss notification below, so a disposed widget never calls back into your
handler — the same thing xterm.js does by clearing its pending restore timeout on dispose.

Two things it does **not** cover, so they are yours:

- **Anything you constructed and kept** — a `Scrollbar`, the resize observer returned by
  `observeResize`, the accessibility controllers. The widget never saw them, so it cannot end them.
- **GPU memory.** Disposing stops the renderer's work; the wasm instance, its GL context and glyph
  atlas live until you drop your own reference and let the page collect it.

## Surviving a lost GL context

A browser may destroy a WebGL context at any moment — GPU reset, driver eviction, a backgrounded
tab — and every GL object goes with it. **You do not have to do anything about it.** The renderer
rebuilds itself when the browser fires `webglcontextrestored`, keeping the terminal's content,
because that content lives on the CPU side and never left.

What has no other signal is the context that does **not** come back. It leaves a blank canvas with
nothing to distinguish it from a quiet terminal, so the widget will tell you:

```ts
const renderer = await JustermRenderer.create({
  canvasSelector: "#term",
  fontFamily: "monospace",
  fontSize: 16,
  theme,
  contextRestoreTimeout: 3000, // ms; this is the default, xterm.js's value
  onContextLoss: () => showBanner("The GPU dropped this terminal. Reload to recover."),
});
```

`onContextLoss` fires **at most once per loss**, and only if the context is still gone when the
deadline passes. Treat it as a warning rather than a verdict: Chromium keeps re-attempting a real
restore roughly once a second indefinitely, so the context may still come back afterwards and the
terminal will repaint by itself. What to do meanwhile is yours — dim the terminal, show a message,
or tear the widget down and fall back (VSCode swaps in a DOM renderer at this point).

To ask instead of being told — polling a status line, or attaching after a loss already happened:

```ts
renderer.isContextLost();    // has a loss been REPORTED to us
renderer.isRestoreOverdue(); // …and did it miss its deadline
```

`isContextLost()` answers *"was I told"*, not *"is the GPU usable right now"*, and the difference is
real rather than pedantic: a browser destroys a context synchronously and only *queues* the event, so
for a short window this returns `false` while every GL call is already dead. It is the honest thing
to show a user and the wrong thing to gate drawing on — which is why the renderer guards its own work
on a stricter predicate it does not export
([ADR-0027](https://github.com/kihyun1998/justerm/blob/master/docs/adr/)).

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
