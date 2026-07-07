//! Thin `#[wasm_bindgen]` + WebGL2 glue — browser-only (wasm32), verified in the demo.
//!
//! Scaffold (#259): initialise the GL context from a canvas and clear to the injected
//! default background. The instanced grid pipeline (shaders, atlas) replaces the bare
//! clear in #260+; `apply_frame` is a documented seam until the frame type lands in #261.

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::color::gl_rgb;

/// The justerm-family WebGL2 terminal renderer.
///
/// Consumer-side (ADR-0018): consumes a decoded frame + an injected palette and paints
/// via WebGL2. `justerm-core` stays render-agnostic. This scaffold exposes the public
/// skeleton (`new` / `resize` / `apply_frame` / `render`); the GPU pipeline lands in #260+.
#[wasm_bindgen]
pub struct JustermRenderer {
    gl: glow::Context,
    /// Injected default background (`0xRRGGBB`) — the consumer owns the theme (ADR-0002).
    default_bg: u32,
    /// Drawing-buffer size in device pixels.
    size: (i32, i32),
}

#[wasm_bindgen]
impl JustermRenderer {
    /// Bind a renderer to the canvas matched by `canvas_selector`, clearing to
    /// `default_bg` (`0xRRGGBB`, injected by the consumer).
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_selector: &str, default_bg: u32) -> Result<JustermRenderer, JsValue> {
        console_error_panic_hook::set_once();

        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no document"))?;
        let canvas: HtmlCanvasElement = document
            .query_selector(canvas_selector)?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: canvas not found"))?
            .dyn_into()?;
        let webgl2: WebGl2RenderingContext = canvas
            .get_context("webgl2")?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no webgl2 context"))?
            .dyn_into()?;

        let gl = glow::Context::from_webgl2_context(webgl2);
        let size = (canvas.width() as i32, canvas.height() as i32);

        Ok(JustermRenderer {
            gl,
            default_bg,
            size,
        })
    }

    /// Resize the drawing buffer to `width`×`height` device pixels.
    pub fn resize(&mut self, width: i32, height: i32) {
        self.size = (width.max(1), height.max(1));
        unsafe {
            self.gl.viewport(0, 0, self.size.0, self.size.1);
        }
    }

    /// Apply a decoded frame. Seam only in this scaffold — #261 decodes the frame +
    /// injected palette into per-cell instances (reference→RGB resolution in Rust).
    pub fn apply_frame(&mut self, _frame: &[u8]) {
        // #261: decode → instance packing (A-ii, hot path in wasm).
    }

    /// Clear the canvas to the injected default background and present. The instanced
    /// grid (cell quads) replaces this bare clear in #260+.
    pub fn render(&self) {
        let [r, g, b] = gl_rgb(self.default_bg);
        unsafe {
            self.gl.clear_color(r, g, b, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }
}
