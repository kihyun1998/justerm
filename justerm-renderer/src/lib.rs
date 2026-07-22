//! `justerm-renderer` — first-party WebGL2 terminal grid renderer for the justerm
//! family. Reimplements the third-party `beamterm` renderer in justerm's own
//! architecture (ADR-0018, supersedes ADR-0002).
//!
//! ## Boundary
//! This is a *consumer-side* renderer — a sibling family member, exactly as
//! beamterm was. `justerm-core` still does NOT render (the engine's boundary
//! invariant holds). The renderer consumes a decoded frame + an *injected*
//! palette (the consumer owns the theme; ADR-0002's "consumer owns hex" survives)
//! and paints via WebGL2. The hot path stays in wasm (ADR-0018 "A-ii").
//!
//! ## Structure (family idiom, mirrors `justerm-wasm-decode`)
//! Pure, host-testable logic (`color`, `render_policy`, `metrics`, …) is kept
//! separate from the thin `#[wasm_bindgen]` + WebGL glue (`webgl`, browser-only,
//! verified by the `demo/*.html` pixel proofs). The GPU pipeline is in: glyph
//! atlas + rasterizer, a single instanced draw call, cursor, selection / search /
//! active-match overlays and decorations. `JustermRenderer` is the public surface;
//! the modules behind it are internal (#465).

// Dead-code analysis is only trustworthy on **wasm32**, so it is silenced elsewhere (#465).
//
// `rasterizer` and `webgl` are `#[cfg(target_arch = "wasm32")]`, so a host build compiles the crate
// with its entire GL half missing — and then reports every item only that half uses as unused. That
// is 158 warnings on host versus **0** on wasm32, none of them real. wasm32 is the target that
// compiles the whole crate *and* the one that actually ships, so that is where the analysis runs;
// silencing it on host is removing noise, not lowering a gate. Verify with
// `cargo build --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown`.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

// `pub(crate)`, not `pub` (#465). The crate is published to **npm only** — `publish-crate.yml`
// uploads `-p justerm-core` and nothing else — so no Rust dependent can exist and these modules are
// an implementation detail, not an API. Keeping them `pub` made every internal rename a "breaking
// public API change" (two `renderer-v*` minors were cut on symbols nobody imports, and in 0.x a minor
// breaks a caret range, forcing a `justerm-web` bump each time). It also **disabled dead-code
// analysis**: a `pub` item is assumed used by someone outside, so the compiler stops asking.
// `docs/agents/release.md` now states that semver here is measured against the wasm/JS class.
//
// Nothing is lost for testing: every test module is an inner `#[cfg(test)] mod tests` in its own
// file, which reaches `super::*` regardless of visibility, and there is no `tests/` directory.
pub(crate) mod attrs;
pub(crate) mod bitmap;
pub(crate) mod builtin;
pub(crate) mod color;
pub(crate) mod context_loss;
pub(crate) mod contrast;
pub(crate) mod cursor;
pub(crate) mod decoration;
pub(crate) mod dpr;
pub(crate) mod emoji;
pub(crate) mod frame;
pub(crate) mod frame_grid;
pub(crate) mod glyph_cache;
pub(crate) mod glyph_class;
pub(crate) mod glyph_resolve;
pub(crate) mod mat4;
pub(crate) mod metrics;
pub(crate) mod overlay;
pub(crate) mod palette;
pub(crate) mod render_policy;
pub(crate) mod upload;

// The browser/GL glue is wasm32-only (web-sys/glow-web). Host builds skip it so the
// crate's pure core stays `cargo test`-able without a wasm runtime — the same split
// justerm-wasm-decode uses (pure `flatten` vs the `#[wasm_bindgen]` layer).
#[cfg(target_arch = "wasm32")]
mod rasterizer;
#[cfg(target_arch = "wasm32")]
mod webgl;
#[cfg(target_arch = "wasm32")]
pub use webgl::JustermRenderer;
