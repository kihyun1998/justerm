# justerm-renderer

First-party **WebGL2 terminal grid renderer** for the [justerm](https://github.com/kihyun1998/justerm)
family. Reimplements the third-party `beamterm` renderer in justerm's own architecture — see
**ADR-0018** (supersedes ADR-0002) and **Epic #258**.

- **Consumer-side, sibling family member.** `justerm-core` still does not render; this crate does.
  It consumes a decoded frame + an *injected* palette (the consumer owns the theme) and paints via
  WebGL2. Rust → wasm, GL via [`glow`] — the same target for a plain browser and a Tauri webview.
- **A-ii (hot path in wasm).** Reference→RGB resolution + instance packing happen in Rust; the wasm↔JS
  boundary is crossed only for the handful of GL calls per frame (single instanced draw call).

## Status

Shipping — Epic #258 is closed and this is the family's active renderer. The GPU pipeline is in:
glyph atlas + rasterizer, a single instanced grid draw call, cursor, selection / search / active-match
overlays, decorations, and live palette / font / metric setters. `justerm-web` renders through it
(#273), and it composites every layer itself — the widget no longer resolves per-cell colour.

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
