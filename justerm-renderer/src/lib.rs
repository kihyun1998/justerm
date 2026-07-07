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
//! Pure, host-testable logic ([`color`]) is kept separate from the thin
//! `#[wasm_bindgen]` + WebGL glue ([`webgl`], browser-only, verified in the demo).
//! This scaffold slice (#259) establishes the crate, the `JustermRenderer` public
//! skeleton, and a stub that clears the canvas to the injected default background;
//! the GPU pipeline (instanced grid, glyph atlas, shaders) lands in #260+.

pub mod color;
pub mod frame;
pub mod mat4;
pub mod palette;

// The browser/GL glue is wasm32-only (web-sys/glow-web). Host builds skip it so the
// crate's pure core stays `cargo test`-able without a wasm runtime — the same split
// justerm-wasm-decode uses (pure `flatten` vs the `#[wasm_bindgen]` layer).
#[cfg(target_arch = "wasm32")]
mod webgl;
#[cfg(target_arch = "wasm32")]
pub use webgl::JustermRenderer;
