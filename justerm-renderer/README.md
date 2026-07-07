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

Under construction, sliced under Epic #258. This is the scaffold (#259): crate + public skeleton
(`JustermRenderer::new` / `resize` / `apply_frame` / `render`) + a stub that clears the canvas to the
injected default background. The GPU pipeline (instanced grid, glyph atlas, shaders, cursor, selection)
lands in #260+.

## Build & test

This crate is **excluded from the root cargo workspace** (its `web-sys`/`glow` deps are wasm32-only).

```bash
# pure logic (host)
cargo test --manifest-path justerm-renderer/Cargo.toml
# full crate incl. the WebGL glue (wasm32 gate)
cargo build --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown
```
