//! OffscreenCanvas glyph rasteriser — wasm32/browser only.
//!
//! Draws a grapheme with the browser's own text engine (which brings font fallback,
//! shaping, and — later, #268 — colour emoji) and returns its coverage bitmap: white
//! pixels with the coverage in the alpha channel.
//!
//! Cell metrics come from an **ink scan** of the full-block glyph `█` (#288): the cell is the
//! block's real pixel bounds (alpha ≥ threshold), which avoids the rounding / box-gap issues
//! of `fontBoundingBox`. Every glyph is rasterised into a **padded** cell — the physical cell
//! grown by [`PADDING`] on each side — with the glyph drawn inset, so the atlas carries a
//! transparent guard band that stops band bleed and gives tall/fallback glyphs room.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};

use crate::bitmap::{PADDING, cell_metrics, ink_bounds};
use crate::glyph_cache::FontStyle;

/// A browser-backed glyph rasteriser bound to one font family + size.
pub struct Rasterizer {
    /// Held only to keep the JS canvas alive for the context's lifetime.
    #[allow(dead_code)]
    canvas: OffscreenCanvas,
    ctx: OffscreenCanvasRenderingContext2d,
    font_family: String,
    font_size: f32,
    /// Physical (content) cell in device px — the on-screen grid cell.
    phys_w: u32,
    phys_h: u32,
    /// Baseline ascent (px the ink rises above the draw point), for alphabetic-baseline draws.
    ascent: f32,
}

/// A CSS `font` string for the family/size/style (mirrors beamterm `build_font_string`).
fn font_string(family: &str, size: f32, style: FontStyle) -> String {
    let (bold, italic) = match style {
        FontStyle::Normal => (false, false),
        FontStyle::Bold => (true, false),
        FontStyle::Italic => (false, true),
        FontStyle::BoldItalic => (true, true),
    };
    let italic = if italic { "italic " } else { "" };
    let bold = if bold { "bold " } else { "" };
    format!("{italic}{bold}{size}px {family}, monospace")
}

impl Rasterizer {
    /// Build a rasteriser for `font_family` at `font_size` (CSS px). Measures the cell from the
    /// `█` ink bounds and sizes an internal double-width padded canvas.
    pub fn new(font_family: &str, font_size: f32) -> Result<Rasterizer, JsValue> {
        // A generous square measuring buffer so `█` (drawn at an offset to catch any negative
        // positioning) fits with headroom above and below the baseline.
        let buf = ((font_size * 4.0).ceil() as u32).max(64);
        let canvas = OffscreenCanvas::new(buf, buf)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no 2d context"))?
            .dyn_into::<OffscreenCanvasRenderingContext2d>()?;

        // Cell metrics are style-independent for monospace (bold/italic keep the advance).
        Self::apply_state(
            &ctx,
            &font_string(font_family, font_size, FontStyle::Normal),
        );

        // Ink-scan the full block: draw at an offset with the (default) alphabetic baseline,
        // read back the buffer, and take the tight alpha bounds. The offset gives `2*font_size`
        // of headroom above the baseline so a tall `█` (ascent > font_size) can't clip at y<0;
        // `buf = 4*font_size` leaves the same below the draw point for the descent.
        let draw_offset = font_size * 2.0;
        ctx.clear_rect(0.0, 0.0, buf as f64, buf as f64);
        ctx.fill_text("\u{2588}", draw_offset as f64, draw_offset as f64)?;
        let img = ctx.get_image_data(0.0, 0.0, buf as f64, buf as f64)?;
        let bounds = ink_bounds(&img.data(), buf, buf, 128)
            .ok_or_else(|| JsValue::from_str("justerm-renderer: `█` produced no ink"))?;
        let m = cell_metrics(bounds, draw_offset);
        // Clamp all three like siblings: a negative ascent (ink below the draw point) would draw
        // glyphs above the padded cell and clip their tops.
        let (phys_w, phys_h, ascent) = (m.width.max(1), m.height.max(1), m.ascent.max(0.0));
        let (padded_w, padded_h) = (phys_w + 2 * PADDING, phys_h + 2 * PADDING);

        // Size the canvas to a DOUBLE padded cell (so a wide glyph fits); resizing clears the
        // 2D state, so re-apply it.
        canvas.set_width(padded_w * 2);
        canvas.set_height(padded_h);
        Self::apply_state(
            &ctx,
            &font_string(font_family, font_size, FontStyle::Normal),
        );

        Ok(Rasterizer {
            canvas,
            ctx,
            font_family: font_family.to_string(),
            font_size,
            phys_w,
            phys_h,
            ascent,
        })
    }

    fn apply_state(ctx: &OffscreenCanvasRenderingContext2d, font: &str) {
        ctx.set_font(font);
        // Alphabetic baseline: draw at y = PADDING + ascent so the ink top lands at PADDING.
        ctx.set_text_baseline("alphabetic");
        ctx.set_text_align("left");
        ctx.set_fill_style_str("white");
    }

    /// The physical (content) cell size in device px — the on-screen grid cell.
    pub fn cell_size(&self) -> (u32, u32) {
        (self.phys_w, self.phys_h)
    }

    /// The padded atlas cell size in device px (physical + `2*PADDING` each axis).
    pub fn padded_size(&self) -> (u32, u32) {
        (self.phys_w + 2 * PADDING, self.phys_h + 2 * PADDING)
    }

    /// Rasterise one grapheme in the given font `style` into a white/coverage RGBA bitmap,
    /// row-major, with the glyph drawn inset by [`PADDING`]. A normal glyph is
    /// `padded_w × padded_h`; a `wide` glyph is `(2*phys_w + 2*PADDING) × padded_h` (its two
    /// content halves plus outer guard bands — see [`crate::bitmap::split_wide_bitmap`]).
    pub fn rasterize(&self, text: &str, style: FontStyle, wide: bool) -> Result<Vec<u8>, JsValue> {
        let (padded_w, padded_h) = self.padded_size();
        // A wide source keeps one PADDING band on each outer edge and 2*phys_w of content
        // between (== 2*padded_w - 2*PADDING). A normal source is one padded cell.
        let src_w = if wide {
            2 * padded_w - 2 * PADDING
        } else {
            padded_w
        };

        self.ctx
            .set_font(&font_string(&self.font_family, self.font_size, style));
        // Clear the full (double padded) canvas so a previous wide glyph can't linger.
        self.ctx
            .clear_rect(0.0, 0.0, (padded_w * 2) as f64, padded_h as f64);
        // Draw inset: x = PADDING, baseline y = PADDING + ascent (ink top lands at PADDING).
        self.ctx
            .fill_text(text, PADDING as f64, (PADDING as f32 + self.ascent) as f64)?;
        let img = self
            .ctx
            .get_image_data(0.0, 0.0, src_w as f64, padded_h as f64)?;
        Ok(img.data().to_vec())
    }
}
