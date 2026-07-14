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
//!
//! This ink scan is a deliberate divergence from both references, recorded here (from #361) so it
//! is not later rediscovered as a defect: alacritty sizes its cell from font metrics
//! (`average_advance` + `line_height`, `builtin_font.rs:51`) and xterm from `CharSizeService`. So
//! if a font's `█` under- or over-fills its advance, justerm's grid ends up sized differently from
//! theirs. It is safe only as long as the **builtin** `█` never re-enters measurement: the scan
//! reads the *font's* `█` via `fill_text` + ink bounds, while `block_glyph` (the renderer's own
//! drawn `█`) is called from `Rasterizer::builtin` alone, never from `Rasterizer::new`. Were it
//! ever called during measurement, the cell would feed the glyph that defines it — a feedback loop.
//! Today there is none.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};

use crate::bitmap::{PADDING, cell_metrics, ink_bounds};
use crate::builtin::block_glyph;
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
    /// The GRID cell in device px (#338): the glyph box plus the consumer's spacing policy. The
    /// atlas slot is this, padded — not the glyph box — so a bitmap carries its own margins and a
    /// builtin block element can fill the cell edge to edge (#359).
    cell_w: u32,
    cell_h: u32,
    /// Where the glyph box sits inside the cell, device px from its top-left.
    off_x: u32,
    off_y: u32,
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
            cell_w: phys_w,
            cell_h: phys_h,
            off_x: 0,
            off_y: 0,
        })
    }

    fn apply_state(ctx: &OffscreenCanvasRenderingContext2d, font: &str) {
        ctx.set_font(font);
        // Alphabetic baseline: draw at y = PADDING + ascent so the ink top lands at PADDING.
        ctx.set_text_baseline("alphabetic");
        ctx.set_text_align("left");
        ctx.set_fill_style_str("white");
    }

    /// The ink-scanned box of `█` in device px — the GLYPH box, which is the grid cell only while
    /// the spacing policy is the identity (#338).
    pub fn glyph_box(&self) -> (u32, u32) {
        (self.phys_w, self.phys_h)
    }

    /// Adopt a grid cell and the glyph's place inside it (#338/#359). Every bitmap this rasteriser
    /// produces afterwards is a padded CELL, with the glyph drawn at `off` and a builtin block
    /// element filling the cell outright. The caller must re-bake the atlas: its slots change size.
    pub fn set_cell(&mut self, cell: (u32, u32), off: (u32, u32)) -> Result<(), JsValue> {
        self.cell_w = cell.0.max(1);
        self.cell_h = cell.1.max(1);
        self.off_x = off.0;
        self.off_y = off.1;
        // A wide source is two padded cells minus the shared inner bands; the canvas must hold it.
        let (padded_w, padded_h) = self.padded_size();
        let (need_w, need_h) = (2 * padded_w, padded_h);
        if self.canvas.width() < need_w || self.canvas.height() < need_h {
            self.canvas.set_width(need_w.max(self.canvas.width()));
            self.canvas.set_height(need_h.max(self.canvas.height()));
            // Resizing an OffscreenCanvas resets its 2D state — baseline, align and fill style go
            // back to their defaults, and every glyph would then be drawn from the wrong origin.
            Self::apply_state(
                &self.ctx,
                &font_string(&self.font_family, self.font_size, FontStyle::Normal),
            );
        }
        Ok(())
    }

    /// The padded atlas slot in device px: the grid cell plus `PADDING` on each side.
    pub fn padded_size(&self) -> (u32, u32) {
        (self.cell_w + 2 * PADDING, self.cell_h + 2 * PADDING)
    }

    /// Rasterise one grapheme in the given font `style` into a white/coverage RGBA bitmap,
    /// row-major, with the glyph drawn inset by [`PADDING`]. A normal glyph is
    /// `padded_w × padded_h`; a `wide` glyph is `(2*phys_w + 2*PADDING) × padded_h` (its two
    /// content halves plus outer guard bands — see [`crate::bitmap::split_wide_bitmap`]).
    /// Rasterise one grapheme into a white/coverage RGBA bitmap, row-major, sized to the PADDED
    /// CELL with the glyph drawn at its offset inside it. A `wide` glyph is
    /// `(2*cell_w + 2*PADDING) x padded_h` (two content cells plus outer guard bands — see
    /// [`crate::bitmap::split_wide_bitmap`]), its ink centred over the two-cell advance, so the
    /// halves the splitter cuts at the cell boundary are contiguous by construction.
    ///
    /// A block element, sextant or extra eighth block never reaches the font: it is drawn to the cell,
    /// because it is meant to tile with its neighbours and the font would only fill an ink box
    /// (#359, #361).
    pub fn rasterize(&self, text: &str, style: FontStyle, wide: bool) -> Result<Vec<u8>, JsValue> {
        let (padded_w, padded_h) = self.padded_size();
        // A wide source keeps one PADDING band on each outer edge and 2*cell_w of content between
        // (== 2*padded_w - 2*PADDING). A normal source is one padded cell.
        let src_w = if wide {
            2 * padded_w - 2 * PADDING
        } else {
            padded_w
        };

        if !wide && let Some(bitmap) = self.builtin(text) {
            return Ok(self.pad(&bitmap, padded_w, padded_h));
        }

        self.ctx
            .set_font(&font_string(&self.font_family, self.font_size, style));
        // Clear the full (double padded) canvas so a previous wide glyph can't linger.
        self.ctx
            .clear_rect(0.0, 0.0, (padded_w * 2) as f64, padded_h as f64);
        // Inset by PADDING, then by the glyph's place inside the cell (#338). A wide glyph's ink is
        // `2 * phys_w` across a `2 * cell_w` advance, so its left margin is the whole slack, not half
        // of it — otherwise the letter sits off-centre in its own advance.
        //
        // It cannot TEAR, whatever this offset is: the ink is contiguous in this one source bitmap
        // and `split_wide_bitmap` merely cuts it at the cell boundary. That is what #359 bought over
        // #338, where each half was placed independently by the shader and the slack meant for the
        // outside of the letter opened a hole through its middle.
        let x_off = if wide {
            self.cell_w.saturating_sub(self.phys_w)
        } else {
            self.off_x
        };
        let x = (PADDING + x_off) as f64;
        let y = (PADDING + self.off_y) as f32 + self.ascent;
        self.ctx.fill_text(text, x, y as f64)?;
        let img = self
            .ctx
            .get_image_data(0.0, 0.0, src_w as f64, padded_h as f64)?;
        Ok(img.data().to_vec())
    }

    /// The built-in cell-sized bitmap for a lone block/sextant glyph, or `None` for what the font owns.
    fn builtin(&self, text: &str) -> Option<Vec<u8>> {
        let mut chars = text.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None; // a cluster, not a lone block element
        }
        block_glyph(c as u32, self.cell_w, self.cell_h)
    }

    /// Centre a cell-sized bitmap inside a padded slot, leaving the guard band transparent.
    fn pad(&self, cell: &[u8], padded_w: u32, padded_h: u32) -> Vec<u8> {
        let mut out = vec![0u8; (padded_w * padded_h * 4) as usize];
        let p = PADDING as usize;
        let (cw, ch) = (self.cell_w as usize, self.cell_h as usize);
        for row in 0..ch {
            let src = row * cw * 4;
            let dst = ((row + p) * padded_w as usize + p) * 4;
            out[dst..dst + cw * 4].copy_from_slice(&cell[src..src + cw * 4]);
        }
        out
    }
}
