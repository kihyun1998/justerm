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
