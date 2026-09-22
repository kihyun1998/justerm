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
//! This ink scan is a divergence from both references, recorded here (from #361) so it is not later
//! rediscovered as a defect — and adjudicated in **ADR-0022**, which grades the evidence behind it:
//! alacritty sizes its cell from font metrics (`compute_cell_size`, `alacritty/src/display/mod.rs:1608-1615`
//! — `average_advance` + `line_height` from the font tables, where `average_advance` is the advance of
//! `'0'`; `builtin_font.rs:51` only *consumes* those metrics) and xterm from `CharSizeService`
//! (`measureText('W').width` + `fontBoundingBox*`, becoming the device cell in
//! `addons/addon-webgl/src/WebglRenderer.ts:646-671`). The method here is inherited from beamterm
//! (`canvas_rasterizer::measure_cell_metrics`) and justified only by beamterm's own unmeasured comment
//! that text metrics "have rounding issues" — so it is 1 of 3, not the consensus. So
//! if a font's `█` under- or over-fills its advance, justerm's grid ends up sized differently from
//! theirs. It is safe only as long as the **builtin** `█` never re-enters measurement: the scan
//! reads the *font's* `█` via `fill_text` + ink bounds, while `block_glyph` (the renderer's own
//! drawn `█`) is called from `Rasterizer::builtin` alone, never from `Rasterizer::new`. Were it
//! ever called during measurement, the cell would feed the glyph that defines it — a feedback loop.
//! Today there is none, and **nothing enforces that** (ADR-0022 records it as an invariant kept by
//! call-site discipline). The hazard is ours, not inherited from the references: both pass an
//! already-fixed cell *into* their custom-glyph drawing (alacritty `glyph_cache.rs:214-216`, xterm
//! `CustomGlyphRasterizer.ts:15-29`), so the loop is structurally impossible for them — they are silent
//! about it because their architecture forecloses it, not because they weighed it.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};

use crate::bitmap::{PADDING, cell_metrics, ink_bounds};
use crate::builtin::block_glyph;
use crate::css_font::{FontWeight, font_string};
use crate::glyph_cache::FontStyle;
use crate::lcd::{fit_dark_gamma, with_lcd};

/// The string drawn at both polarities to measure a configuration's dark-ink coverage curve (#961):
/// round and diagonal strokes, so every channel sees partial coverage at many levels.
const CALIBRATION_TEXT: &str = "Hamburgefonstiv 0123 {}[] @#%&";

/// A browser-backed glyph rasteriser bound to one font family, size and pair of weights.
pub struct Rasterizer {
    /// Held only to keep the JS canvas alive for the context's lifetime.
    #[allow(dead_code)]
    canvas: OffscreenCanvas,
    ctx: OffscreenCanvasRenderingContext2d,
    font_family: String,
    font_size: f32,
    /// The weights regular and bold text are drawn at (#928).
    font_weight: FontWeight,
    font_weight_bold: FontWeight,
    /// Physical (content) cell in device px — the on-screen grid cell.
    phys_w: u32,
    phys_h: u32,
    /// The face's declared line box against the baseline, device px (#791).
    font_ascent: u32,
    font_descent: u32,
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
    /// Band reserved above and below the cell for ink that leaves it (ADR-0019 R1.2, #791).
    bleed_y: u32,
    /// The opaque canvas per-channel (LCD) coverage is drawn on (#961), or `None` for a grayscale
    /// configuration. Sized like `canvas`.
    lcd: Option<(OffscreenCanvas, OffscreenCanvasRenderingContext2d)>,
    /// The dark-ink coverage exponent measured for this configuration (`lcd::fit_dark_gamma`), or
    /// `0.0` for a grayscale one — the value the shader's `u_lcd_gamma` carries.
    lcd_gamma: f32,
}

impl Rasterizer {
    /// Build a rasteriser for `font_family` at `font_size` (CSS px), drawing regular text at
    /// `font_weight` and bold text at `font_weight_bold`. Measures the cell from the `█` ink bounds
    /// at the `normal` weight and sizes an internal double-width padded canvas. With `subpixel` it
    /// also holds an opaque canvas for per-channel coverage and measures the dark-ink curve (#961).
    pub fn new(
        font_family: &str,
        font_size: f32,
        font_weight: FontWeight,
        font_weight_bold: FontWeight,
        subpixel: bool,
    ) -> Result<Rasterizer, JsValue> {
        // A generous square measuring buffer so `█` (drawn at an offset to catch any negative
        // positioning) fits with headroom above and below the baseline.
        let buf = ((font_size * 4.0).ceil() as u32).max(64);
        let canvas = OffscreenCanvas::new(buf, buf)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no 2d context"))?
            .dyn_into::<OffscreenCanvasRenderingContext2d>()?;

        // Cell metrics are style-independent for monospace (bold/italic keep the advance), and are
        // measured at the `normal` weight whatever this rasteriser's weights are (#928).
        let measuring = font_string(
            font_family,
            font_size,
            FontStyle::Normal,
            FontWeight::NORMAL,
            FontWeight::BOLD,
        );
        Self::apply_state(&ctx, &measuring);

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
        // The face's own DECLARED line box, which the bleed band is sized against (#791). It is a
        // property of the font rather than of the string, so which glyph is measured here is
        // immaterial — and it is the deepest vertical metric a browser exposes: there is no Canvas
        // equivalent of the OS/2 win-ascent GDI reports, so this is the ceiling for a browser-side
        // renderer, and why the band needs an empirical headroom on top of it.
        let fm = ctx.measure_text("\u{2588}")?;
        let font_ascent = fm.font_bounding_box_ascent().max(0.0).round() as u32;
        let font_descent = fm.font_bounding_box_descent().max(0.0).round() as u32;
        // Clamp all three like siblings: a negative ascent (ink below the draw point) would draw
        // glyphs above the padded cell and clip their tops.
        let (phys_w, phys_h, ascent) = (m.width.max(1), m.height.max(1), m.ascent.max(0.0));
        let (padded_w, padded_h) = (phys_w + 2 * PADDING, phys_h + 2 * PADDING);

        // Size the canvas to a DOUBLE padded cell (so a wide glyph fits); resizing clears the
        // 2D state, so re-apply it.
        canvas.set_width(padded_w * 2);
        canvas.set_height(padded_h);
        Self::apply_state(&ctx, &measuring);

        let (lcd, lcd_gamma) = if subpixel {
            let regular = font_string(
                font_family,
                font_size,
                FontStyle::Normal,
                font_weight,
                font_weight_bold,
            );
            let gamma = Self::calibrate(&regular, font_size)?;
            (
                Some(Self::opaque_canvas(padded_w * 2, padded_h, &measuring)?),
                gamma,
            )
        } else {
            (None, 0.0)
        };

        Ok(Rasterizer {
            canvas,
            ctx,
            font_family: font_family.to_string(),
            font_size,
            font_weight,
            font_weight_bold,
            phys_w,
            phys_h,
            ascent,
            font_ascent,
            font_descent,
            cell_w: phys_w,
            cell_h: phys_h,
            off_x: 0,
            off_y: 0,
            bleed_y: 0,
            lcd,
            lcd_gamma,
        })
    }

    /// A `w`×`h` canvas whose 2D context is opaque (`alpha: false`) — the only kind the browser
    /// draws per-channel (LCD) text into — with the drawing state applied.
    fn opaque_canvas(
        w: u32,
        h: u32,
        font: &str,
    ) -> Result<(OffscreenCanvas, OffscreenCanvasRenderingContext2d), JsValue> {
        let canvas = OffscreenCanvas::new(w, h)?;
        let opts = js_sys::Object::new();
        js_sys::Reflect::set(&opts, &"alpha".into(), &JsValue::FALSE)?;
        let ctx = canvas
            .get_context_with_context_options("2d", &opts)?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no 2d context"))?
            .dyn_into::<OffscreenCanvasRenderingContext2d>()?;
        Self::apply_state(&ctx, font);
        Ok((canvas, ctx))
    }

    /// The dark-ink coverage exponent for text drawn in `font` (#961): [`CALIBRATION_TEXT`] white
    /// over black and black over white, fitted by [`fit_dark_gamma`].
    fn calibrate(font: &str, font_size: f32) -> Result<f32, JsValue> {
        let w = (font_size * 24.0).ceil() as u32;
        let h = (font_size * 3.0).ceil() as u32;
        let (_canvas, ctx) = Self::opaque_canvas(w, h, font)?;
        let draw = |ink: &str, ground: &str| -> Result<Vec<u8>, JsValue> {
            ctx.set_fill_style_str(ground);
            ctx.fill_rect(0.0, 0.0, w as f64, h as f64);
            ctx.set_fill_style_str(ink);
            ctx.fill_text(
                CALIBRATION_TEXT,
                font_size as f64 * 0.5,
                font_size as f64 * 2.0,
            )?;
            Ok(ctx
                .get_image_data(0.0, 0.0, w as f64, h as f64)?
                .data()
                .to_vec())
        };
        let light = draw("white", "black")?;
        let dark = draw("black", "white")?;
        Ok(fit_dark_gamma(&light, &dark))
    }

    /// The dark-ink coverage exponent the shader raises the light mask to, or `0.0` when this
    /// configuration is grayscale (#961).
    pub fn lcd_gamma(&self) -> f32 {
        self.lcd_gamma
    }

    /// This rasteriser's CSS `font` string for `style`.
    fn font(&self, style: FontStyle) -> String {
        font_string(
            &self.font_family,
            self.font_size,
            style,
            self.font_weight,
            self.font_weight_bold,
        )
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
    /// How deep a band this configuration's slots reserve for ink that leaves the cell (#791).
    ///
    /// Derived rather than fixed: the gap between the ink box of this face's block glyph and the
    /// face's declared line box is a property of how one glyph was drawn, and differs by several
    /// device px between faces at the same size.
    pub fn bleed_y(&self) -> u32 {
        let cell_ascent = self.ascent.max(0.0).round() as u32;
        let cell_descent = self.phys_h.saturating_sub(cell_ascent);
        crate::metrics::vertical_bleed(
            cell_ascent,
            cell_descent,
            self.font_ascent,
            self.font_descent,
        )
    }

    pub fn glyph_box(&self) -> (u32, u32) {
        (self.phys_w, self.phys_h)
    }

    /// Adopt a grid cell and the glyph's place inside it (#338/#359). Every bitmap this rasteriser
    /// produces afterwards is a padded CELL, with the glyph drawn at `off` and a builtin block
    /// element filling the cell outright. The caller must re-bake the atlas: its slots change size.
    pub fn set_cell(
        &mut self,
        cell: (u32, u32),
        off: (u32, u32),
        bleed_y: u32,
    ) -> Result<(), JsValue> {
        self.cell_w = cell.0.max(1);
        self.cell_h = cell.1.max(1);
        self.off_x = off.0;
        self.off_y = off.1;
        self.bleed_y = bleed_y;
        // A wide source is two padded cells minus the shared inner bands; the canvas must hold it.
        let (padded_w, padded_h) = self.padded_size();
        let (need_w, need_h) = (2 * padded_w, padded_h);
        if self.canvas.width() < need_w || self.canvas.height() < need_h {
            self.canvas.set_width(need_w.max(self.canvas.width()));
            self.canvas.set_height(need_h.max(self.canvas.height()));
            // Resizing an OffscreenCanvas resets its 2D state — baseline, align and fill style go
            // back to their defaults, and every glyph would then be drawn from the wrong origin.
            Self::apply_state(&self.ctx, &self.font(FontStyle::Normal));
        }
        if let Some((canvas, ctx)) = &self.lcd
            && (canvas.width() < need_w || canvas.height() < need_h)
        {
            canvas.set_width(need_w.max(canvas.width()));
            canvas.set_height(need_h.max(canvas.height()));
            Self::apply_state(ctx, &self.font(FontStyle::Normal));
        }
        Ok(())
    }

    /// The padded atlas slot in device px: the grid cell plus `PADDING` on each side.
    pub fn padded_size(&self) -> (u32, u32) {
        self.geometry().padded
    }

    /// This configuration's slot layout — see [`crate::metrics::slot_geometry`], which owns the
    /// arithmetic so it can be tested without a browser.
    fn geometry(&self) -> crate::metrics::SlotGeometry {
        crate::metrics::slot_geometry(
            (self.cell_w, self.cell_h),
            (self.off_x, self.off_y),
            self.bleed_y,
            PADDING,
        )
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

        if !wide && let Some(bitmap) = self.builtin(text) {
            return Ok(self.pad(&bitmap, padded_w, padded_h));
        }

        // Clear the full (double padded) canvas so a previous wide glyph can't linger.
        self.ctx
            .clear_rect(0.0, 0.0, (padded_w * 2) as f64, padded_h as f64);
        self.draw(&self.ctx, text, style, wide)?;
        let img = self
            .ctx
            .get_image_data(0.0, 0.0, self.src_w(wide) as f64, padded_h as f64)?;
        Ok(img.data().to_vec())
    }

    /// A [`rasterize`](Self::rasterize)d bitmap in this configuration's slot layout (#961).
    /// Unchanged for a grayscale configuration or a `colour` glyph; otherwise its RGB becomes the
    /// light mask — the same glyph drawn white over opaque black — per [`with_lcd`], or the alpha
    /// repeated for a builtin glyph, which the font never draws.
    pub fn finish(
        &self,
        rgba: Vec<u8>,
        text: &str,
        style: FontStyle,
        wide: bool,
        colour: bool,
    ) -> Result<Vec<u8>, JsValue> {
        let Some((_, ctx)) = &self.lcd else {
            return Ok(rgba);
        };
        if colour {
            return Ok(rgba);
        }
        if !wide && self.builtin(text).is_some() {
            return Ok(with_lcd(&rgba, None));
        }
        let (padded_w, padded_h) = self.padded_size();
        ctx.set_fill_style_str("black");
        ctx.fill_rect(0.0, 0.0, (padded_w * 2) as f64, padded_h as f64);
        ctx.set_fill_style_str("white");
        self.draw(ctx, text, style, wide)?;
        let mask = ctx.get_image_data(0.0, 0.0, self.src_w(wide) as f64, padded_h as f64)?;
        Ok(with_lcd(&rgba, Some(&mask.data())))
    }

    /// The width of a rasterised source: one padded cell, or for a `wide` glyph one PADDING band on
    /// each outer edge and `2 * cell_w` of content between (`== 2*padded_w - 2*PADDING`).
    fn src_w(&self, wide: bool) -> u32 {
        let padded_w = self.padded_size().0;
        if wide {
            2 * padded_w - 2 * PADDING
        } else {
            padded_w
        }
    }

    /// Draw `text` into `ctx` at this configuration's glyph origin, condensed to its box when the
    /// ink is wider. The caller prepares the canvas and reads it back.
    fn draw(
        &self,
        ctx: &OffscreenCanvasRenderingContext2d,
        text: &str,
        style: FontStyle,
        wide: bool,
    ) -> Result<(), JsValue> {
        ctx.set_font(&self.font(style));
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
        let origin = self.geometry().draw_origin;
        // `x_off` replaces the glyph's own horizontal offset for a wide source (its ink spans two
        // cells), so only the vertical half of the geometry is taken wholesale.
        let x = (PADDING + x_off) as f64;
        let y = origin.1 as f32 + self.ascent;
        // Horizontally the slot has no band and never can (#792): the Canvas API exposes no
        // face-level counterpart to `fontBoundingBox{Ascent,Descent}` for one to be sized from, so
        // ink past this window is destroyed here rather than landing on a neighbour. Condense it
        // instead — on this axis alone, because the vertical one already has somewhere to go
        // (ADR-0019 R1.2). The box is the GLYPH box, never the cell: `metrics::device_cell` lets a
        // negative `letter_spacing` narrow the cell past the glyph and crop it, and a predicate
        // keyed on the cell would read that consumer policy as a font fact.
        let box_w = if wide { 2 * self.phys_w } else { self.phys_w };
        let metrics = ctx.measure_text(text)?;
        let fit = crate::metrics::horizontal_fit(
            metrics.actual_bounding_box_left() as f32,
            metrics.actual_bounding_box_right() as f32,
            box_w,
        );
        if fit.scale_x == 1.0 && fit.pen_offset == 0.0 {
            ctx.fill_text(text, x, y as f64)?;
        } else {
            // `pen_offset` is pre-scale, so both the bearing and the ink scale together and the
            // condensed left edge lands on the box origin. `scale(_, 1.0)` leaves the baseline and
            // every vertical extent untouched, which is what keeps #791's band uninvolved.
            ctx.save();
            ctx.translate(x, 0.0)?;
            ctx.scale(fit.scale_x as f64, 1.0)?;
            ctx.fill_text(text, fit.pen_offset as f64, y as f64)?;
            ctx.restore();
        }
        Ok(())
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
        // The cell band starts past the guard band across, and past the guard band AND the bleed
        // down — the same origin `metrics::cell_uv` hands the shader. `PADDING` on both axes was
        // right only while the slot's content *was* the cell: with a band reserved it puts every
        // builtin glyph `bleed_y` rows too high, which is a run of `█` that no longer meets its
        // neighbour. Caught by `spacing.html` and `cursor.html` at all four densities (#791).
        let (px, py) = (PADDING as usize, (PADDING + self.bleed_y) as usize);
        let (cw, ch) = (self.cell_w as usize, self.cell_h as usize);
        for row in 0..ch {
            let src = row * cw * 4;
            let dst = ((row + py) * padded_w as usize + px) * 4;
            out[dst..dst + cw * 4].copy_from_slice(&cell[src..src + cw * 4]);
        }
        out
    }
}
