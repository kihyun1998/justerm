//! OffscreenCanvas glyph rasteriser — wasm32/browser only.
//!
//! Draws a grapheme with the browser's own text engine (which brings font fallback,
//! shaping, and — later, #268 — colour emoji) and returns its coverage bitmap: white
//! pixels with the coverage in the alpha channel. The cell size is measured from the
//! full-block glyph `█` so a cell exactly holds one monospace glyph.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};

/// A browser-backed glyph rasteriser bound to one font + size.
pub struct Rasterizer {
    /// Held only to keep the JS canvas alive for the context's lifetime.
    #[allow(dead_code)]
    canvas: OffscreenCanvas,
    ctx: OffscreenCanvasRenderingContext2d,
    cell_w: u32,
    cell_h: u32,
}

impl Rasterizer {
    /// Build a rasteriser for `font_family` at `font_size` (CSS px). Measures the cell
    /// from `█` and sizes an internal one-cell canvas.
    pub fn new(font_family: &str, font_size: f32) -> Result<Rasterizer, JsValue> {
        let canvas = OffscreenCanvas::new(8, 8)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no 2d context"))?
            .dyn_into::<OffscreenCanvasRenderingContext2d>()?;

        let font = format!("{font_size}px {font_family}");
        Self::apply_state(&ctx, &font);

        // Measure the cell from the full block (fills the em box in monospace fonts).
        let m = ctx.measure_text("\u{2588}")?;
        let cell_w = (m.width().ceil() as u32).max(1);
        let cell_h =
            ((m.font_bounding_box_ascent() + m.font_bounding_box_descent()).ceil() as u32).max(1);

        // Resize the canvas to one cell; a resize clears the 2D state, so re-apply it.
        canvas.set_width(cell_w);
        canvas.set_height(cell_h);
        Self::apply_state(&ctx, &font);

        Ok(Rasterizer {
            canvas,
            ctx,
            cell_w,
            cell_h,
        })
    }

    fn apply_state(ctx: &OffscreenCanvasRenderingContext2d, font: &str) {
        ctx.set_font(font);
        ctx.set_text_baseline("top");
        ctx.set_text_align("left");
        ctx.set_fill_style_str("white");
    }

    /// The measured cell size in CSS px.
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// Rasterise one grapheme into a `cell_w × cell_h` RGBA bitmap (white, alpha =
    /// coverage). Row-major, 4 bytes/pixel — ready for a texture-array upload.
    pub fn rasterize(&self, text: &str) -> Result<Vec<u8>, JsValue> {
        let (w, h) = (self.cell_w as f64, self.cell_h as f64);
        self.ctx.clear_rect(0.0, 0.0, w, h);
        self.ctx.fill_text(text, 0.0, 0.0)?;
        let img = self.ctx.get_image_data(0.0, 0.0, w, h)?;
        Ok(img.data().to_vec())
    }
}
