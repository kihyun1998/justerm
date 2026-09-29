//! Font configurations — baking an atlas, the device pixel ratio, and the font and spacing selectors that choose a configuration.

use crate::bitmap::{PADDING, split_wide_bitmap};
use crate::config_registry::ConfigKey;
use crate::dpr::dpr_changed;
use crate::glyph_cache::{FontStyle, GLYPHS_PER_LAYER, GlyphCache, GlyphSlot, WIDE_BASE};
use crate::metrics::{device_cell, fit_cell_to_atlas, glyph_offset};
use crate::rasterizer::Rasterizer;
use glow::HasContext;
use wasm_bindgen::prelude::*;

use super::{BakedConfig, GridTier, JustermRenderer, upload_glyph, weight_from_js};

#[wasm_bindgen]
impl JustermRenderer {
    /// Build one configuration's resources: rasteriser, cell geometry, atlas texture, and the
    /// glyphs baked into it (#772). Nothing here touches a live field, so every caller commits it
    /// atomically or throws it away.
    ///
    /// `resident` decides what gets baked, and that is the only difference between the two callers:
    /// `None` builds a **new** configuration and primes it with the 95 ASCII fast-path glyphs;
    /// `Some(cache)` rebuilds an **existing** one and re-bakes every resident glyph into the SAME
    /// slot it already occupies, so the instances that address them survive the rebuild.
    ///
    /// An associated function, reading no tier. Why: `docs/map/territory/glyph-atlas.md` § One
    /// bake function builds every configuration.
    pub(super) fn bake_config(
        gl: &glow::Context,
        max_texture_size: u32,
        key: &ConfigKey,
        resident: Option<&GlyphCache>,
        dpr: f32,
    ) -> Result<BakedConfig, JsValue> {
        let mut rasterizer = Rasterizer::new(
            key.font_family(),
            key.font_size() * dpr,
            key.font_weight(),
            key.font_weight_bold(),
            key.subpixel(),
        )?;
        // The cell is the glyph box plus the spacing policy (#338), fitted to what the atlas texture
        // can hold (#339/#359); the rasteriser learns it before anything is sized from
        // `padded_size()`, the atlas slot being the padded cell. Why:
        // `docs/map/territory/cell-geometry.md` § The cell is bounded twice.
        let char_size = rasterizer.glyph_box();
        // The bands this face needs, measured before the cell is sized because they are spent out of
        // the same texture budget (#791 the per-layer height, #966 the width).
        let bleed = (rasterizer.bleed_x(), rasterizer.bleed_y());
        let cell_size = fit_cell_to_atlas(
            device_cell(char_size, key.letter_spacing(), key.line_height(), dpr),
            PADDING,
            bleed,
            GLYPHS_PER_LAYER as u32,
            max_texture_size,
        );
        let char_offset = glyph_offset(cell_size, char_size);
        rasterizer.set_cell(cell_size, char_offset, bleed)?;
        let atlas_cell = rasterizer.padded_size();
        let atlas = Self::build_atlas(gl, atlas_cell.0, atlas_cell.1)?;
        let baked = match resident {
            Some(cache) => Self::bake_all_glyphs(gl, cache, &rasterizer, atlas, atlas_cell),
            None => Self::prebake_ascii_into(gl, &rasterizer, atlas, atlas_cell),
        };
        if let Err(e) = baked {
            unsafe { gl.delete_texture(atlas) }; // don't leak the half-built atlas
            return Err(e);
        }
        Ok(BakedConfig {
            atlas,
            rasterizer,
            cell_size,
            char_size,
            char_offset,
            atlas_cell,
        })
    }

    /// Rasterise + upload the 95 normal-styled ASCII glyphs into `atlas`, at their fixed fast-path
    /// slots (`0..=94`), so a cell using the ASCII fast path samples a real bitmap.
    fn prebake_ascii_into(
        gl: &glow::Context,
        rasterizer: &Rasterizer,
        atlas: glow::Texture,
        atlas_cell: (u32, u32),
    ) -> Result<(), JsValue> {
        for cp in 0x20u32..=0x7E {
            let ch = char::from_u32(cp).unwrap();
            let text = ch.to_string();
            let rgba = rasterizer.rasterize(&text, FontStyle::Normal, false)?;
            let rgba = rasterizer.finish(rgba, &text, FontStyle::Normal, false, false)?;
            upload_glyph(gl, atlas, atlas_cell, (cp - 0x20) as u16, &rgba);
        }
        Ok(())
    }

    /// Bake one configuration's ENTIRE glyph set — the 95 prebaked ASCII plus every glyph resident
    /// in `cache` — into `atlas`, each into the SAME slot it already occupies. Preserving the slots
    /// is what lets the packed `instances` stay valid across the re-bake (no re-pack, no
    /// re-resolve). Shared by the DPR re-bake (#322) and the context-loss restore (#269), which
    /// both need "a fresh, correctly-sized atlas holding what the old one held".
    fn bake_all_glyphs(
        gl: &glow::Context,
        cache: &GlyphCache,
        rasterizer: &Rasterizer,
        atlas: glow::Texture,
        atlas_cell: (u32, u32),
    ) -> Result<(), JsValue> {
        let (pad_w, pad_h) = atlas_cell;
        Self::prebake_ascii_into(gl, rasterizer, atlas, atlas_cell)?;
        for (k, slot) in cache.entries() {
            // Two-cell iff the slot lives in the wide region — a `Wide` (CJK) glyph or a *wide*
            // `Emoji`; a narrow `Emoji` (#297) sits in the normal region and is one cell.
            let wide = slot.slot_id() >= WIDE_BASE;
            let rgba = rasterizer.rasterize(&k.text, k.style, wide)?;
            let colour = matches!(slot, GlyphSlot::Emoji(_));
            let rgba = rasterizer.finish(rgba, &k.text, k.style, wide, colour)?;
            let base = slot.slot_id();
            if wide {
                let m = rasterizer.margin_x();
                let (left, right) = split_wide_bitmap(&rgba, 2 * pad_w - 2 * m, pad_w, pad_h, m);
                upload_glyph(gl, atlas, atlas_cell, base, &left);
                upload_glyph(gl, atlas, atlas_cell, base + 1, &right);
            } else {
                upload_glyph(gl, atlas, atlas_cell, base, &rgba);
            }
        }
        Ok(())
    }

    /// Notify the renderer that `window.devicePixelRatio` changed to `dpr`. The consumer
    /// drives this from a resolution `matchMedia` listener — a DPR change at the *same* CSS size
    /// (dragging to another-density monitor) does not fire a resize, so it must be signalled
    /// explicitly. **Every** configuration is re-baked in place at the new device size, each keeping
    /// its own glyph slots (so nothing has to re-pack). A no-op if the ratio is unchanged; on error
    /// every old atlas is left intact and `dpr` unadvanced, so the next notification retries
    /// (self-healing). While the context is lost the notification is dropped: the restore bakes at
    /// the ratio live at that moment.
    ///
    /// **The drawing buffer and every viewport rect are left alone.** They are the consumer's
    /// device-pixel measurements, so after a density change the consumer re-issues the surface size
    /// and every rect — which it must anyway, the cell having just moved. Why:
    /// [`docs/map/territory/multi-viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/multi-viewport.md)
    /// § The selector funnel.
    #[wasm_bindgen(js_name = setDevicePixelRatio)]
    pub fn set_device_pixel_ratio(&mut self, dpr: f32) -> Result<(), JsValue> {
        if !dpr_changed(self.global.dpr, dpr) {
            return Ok(());
        }
        // Dropped, not stored: `restore` re-reads the live DPR (#269). Why:
        // `docs/map/territory/gl-context-lifecycle.md` § Every entry point that changes the geometry.
        if self.gpu_work_must_wait() {
            return Ok(());
        }
        self.rebuild_all_configs(dpr)?;
        self.global.dpr = dpr;
        Ok(())
    }

    /// Re-bake **every** live configuration at `dpr`, each keeping its own glyph slots (#772).
    ///
    /// Atomic across the whole registry: every replacement is built before any is committed, so a
    /// failure part-way leaves every entry exactly as it was and the caller retries. Why:
    /// `docs/map/territory/multi-viewport.md` § The selector funnel.
    ///
    /// Does **not** advance `self.global.dpr`; the caller commits that once this returns.
    fn rebuild_all_configs(&mut self, dpr: f32) -> Result<(), JsValue> {
        let ids = self.configs.ids();
        let mut baked = Vec::with_capacity(ids.len());
        for &id in &ids {
            let key = self.configs.key(id).clone();
            let built = Self::bake_config(
                &self.global.gl,
                self.global.max_texture_size,
                &key,
                Some(&self.configs.get(id).cache),
                dpr,
            );
            match built {
                Ok(b) => baked.push(b),
                Err(e) => {
                    // Safety: live GL context; these textures are this function's own and unpublished.
                    unsafe {
                        for b in &baked {
                            self.global.gl.delete_texture(b.atlas);
                        }
                    }
                    return Err(e);
                }
            }
        }
        for (id, b) in ids.into_iter().zip(baked) {
            let old = self.configs.get_mut(id).adopt(b);
            // Safety: live GL context; the outgoing texture is no longer referenced.
            unsafe { self.global.gl.delete_texture(old) };
            self.bake_count = self.bake_count.wrapping_add(1);
        }
        Ok(())
    }

    /// Write one grid's font/metric selectors through `edit` and move that grid onto the
    /// configuration they now name (#772, per-grid since #773) — the single site all seven selector
    /// setters go through. It does not touch the drawing buffer.
    ///
    /// - **Atomic.** On failure the selectors are put back, so the grid and the entry it names
    ///   still agree and the consumer can retry.
    /// - **Deferred while the context is dead.** The selectors still advance and
    ///   [`restore`](Self::restore) re-selects from them; the cell does not move until then.
    /// - **It never edits the entry it is leaving** — `select_config` moves the grid.
    ///
    /// Why each: `docs/map/territory/multi-viewport.md` § The selector funnel.
    fn adopt_selectors(
        &mut self,
        at: usize,
        edit: impl FnOnce(&mut GridTier),
    ) -> Result<(), JsValue> {
        let prev = {
            let g = self.grid_at(at);
            (
                g.font_size,
                g.font_family.clone(),
                g.font_weight,
                g.font_weight_bold,
                g.letter_spacing,
                g.line_height,
                g.subpixel,
            )
        };
        edit(self.grid_at_mut(at));
        if self.gpu_work_must_wait() {
            return Ok(());
        }
        let key = self.key_of(at);
        if let Err(e) = self.select_config(at, key) {
            let g = self.grid_at_mut(at);
            (
                g.font_size,
                g.font_family,
                g.font_weight,
                g.font_weight_bold,
                g.letter_spacing,
                g.line_height,
                g.subpixel,
            ) = prev;
            return Err(e);
        }
        Ok(())
    }

    /// Set **one grid's** font size in **CSS px** — it joins the configuration keyed by the
    /// new size, baking one only if no grid already stands on it. Consumer policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)):
    /// the size is the consumer's, the atlas mechanism the renderer's. A non-finite size is ignored;
    /// a smaller-than-`1.0` one is clamped (a zero/negative size would rasterise a degenerate
    /// atlas). A no-op if unchanged.
    ///
    /// **It moves that grid only**: a sibling on the configuration this grid is leaving keeps it,
    /// untouched, so two terminals in two fonts draw side by side.
    ///
    /// The cell size changes, so `cssCellWidth`/`cssCellHeight` move and
    /// **the consumer must re-fit**: re-divide its box, `resizeGrid`, and
    /// re-place the grid — nothing here resizes the surface for it, because a surface drawing N
    /// grids belongs to none of them. Takes effect on the next `render`.
    #[wasm_bindgen(js_name = setFontSize)]
    pub fn set_font_size(&mut self, grid: u32, css_px: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        if !css_px.is_finite() {
            return Ok(());
        }
        let css_px = css_px.max(1.0);
        if (css_px - self.grid_at(at).font_size).abs() < f32::EPSILON {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_size = css_px)
    }

    /// Set **one grid's** font family — a CSS `font-family` string (`"monospace"`,
    /// `"'Fira Code', monospace"`, …) the browser's text engine resolves, with its own fallback. It
    /// joins the configuration keyed by the new family, exactly as a size change does. Consumer policy
    /// ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) — the renderer stays font-agnostic; loading a webfont (`@font-face` / `FontFace`)
    /// before calling is the consumer's job (an unloaded family silently falls back). A no-op if
    /// unchanged, and like a size change it moves **this grid only**.
    ///
    /// The cell size may change, so `cssCellWidth`/`cssCellHeight` can move
    /// and **the consumer must re-fit**, exactly as after a size change. Takes effect on the
    /// next `render`.
    #[wasm_bindgen(js_name = setFontFamily)]
    pub fn set_font_family(&mut self, grid: u32, family: String) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        if family == self.grid_at(at).font_family {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_family = family)
    }

    /// Set the weight **one grid's** regular text is drawn at: CSS `"normal"` / `"bold"`, a
    /// `"100"`..`"900"` keyword, or a number in `[1, 1000]` — the values xterm.js's `fontWeight`
    /// takes. Anything else is ignored. It joins the configuration keyed by the new weight, exactly
    /// as a family change does, and moves this grid only.
    ///
    /// The cell is measured at the `normal` weight whatever this is, so it does not move and no re-fit
    /// is needed. Takes effect on the next `render`.
    #[wasm_bindgen(js_name = setFontWeight)]
    pub fn set_font_weight(&mut self, grid: u32, weight: JsValue) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let Some(weight) = weight_from_js(&weight) else {
            return Ok(());
        };
        if weight == self.grid_at(at).font_weight {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_weight = weight)
    }

    /// Set the weight **one grid's** bold (SGR 1) text is drawn at. Takes the same values as
    /// `setFontWeight`, and ignores anything else. Like it, this joins the configuration keyed by the
    /// new weight, moves this grid only, and does not move the cell.
    #[wasm_bindgen(js_name = setFontWeightBold)]
    pub fn set_font_weight_bold(&mut self, grid: u32, weight: JsValue) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let Some(weight) = weight_from_js(&weight) else {
            return Ok(());
        };
        if weight == self.grid_at(at).font_weight_bold {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_weight_bold = weight)
    }

    /// Draw **one grid's** text with per-channel (LCD / subpixel) coverage, or back to grayscale.
    /// Off by default. It joins the configuration keyed by the new setting, exactly as a family
    /// change does, and moves this grid only; the cell does not move, so no re-fit is needed. Takes
    /// effect on the next `render`.
    ///
    /// Per-channel coverage applies only where the cell's background is opaque: a default-background
    /// cell under a translucent `setBgAlpha` keeps grayscale, since one alpha cannot carry three
    /// coverages. Colour emoji and builtin block glyphs are unchanged. Where the browser draws no LCD
    /// text the three channels come back equal and no colour fringe appears, though dark ink is still
    /// drawn at the lighter weight the browser gives it.
    #[wasm_bindgen(js_name = setSubpixelAntialiasing)]
    pub fn set_subpixel_antialiasing(&mut self, grid: u32, on: bool) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        if on == self.grid_at(at).subpixel {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.subpixel = on)
    }

    /// Adopt a spacing policy on one grid (#338/#359), or leave every field as it was — a spacing
    /// change is a different configuration. [`adopt_selectors`](Self::adopt_selectors) owns the
    /// atomicity and the lost-context deferral; this only decides which fields move.
    ///
    /// The error is dropped: both public setters return `()`, and a failed bake leaves the policy
    /// unchanged, so the next call retries.
    fn adopt_spacing(&mut self, at: usize, letter_spacing: f32, line_height: f32) {
        let _ = self.adopt_selectors(at, |g| {
            g.letter_spacing = letter_spacing;
            g.line_height = line_height;
        });
    }

    /// Extra space between columns, in **CSS pixels** — the consumer's policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)), applied
    /// as `round(letter_spacing * dpr)` device px on the cell. May be negative, which
    /// narrows the cell and crops the glyph rather than stretching it; the cell never reaches zero.
    /// Why CSS pixels: [ADR-0023](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0023-spacing-settings-are-css-pixels.md).
    #[wasm_bindgen(js_name = setLetterSpacing)]
    pub fn set_letter_spacing(&mut self, grid: u32, css_px: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let ls = if css_px.is_finite() { css_px } else { 0.0 };
        self.adopt_spacing(at, ls, self.grid_at(at).line_height);
        Ok(())
    }

    /// A multiplier on the glyph height, `>= 1` — the consumer's policy. Clamped rather than
    /// rejected; a non-finite value is `1`. Read the result back with `cell_height` — it may be
    /// smaller than asked, because a cell the atlas texture cannot hold is shrunk to one it can.
    /// Why: [`docs/map/territory/cell-geometry.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-geometry.md)
    /// § A spacing change is a new configuration.
    #[wasm_bindgen(js_name = setLineHeight)]
    pub fn set_line_height(&mut self, grid: u32, multiplier: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let lh = if multiplier.is_finite() {
            multiplier.max(1.0)
        } else {
            1.0
        };
        self.adopt_spacing(at, self.grid_at(at).letter_spacing, lh);
        Ok(())
    }
}
