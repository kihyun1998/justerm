//! The grid registry — registering, removing and placing grids, and the configurations they hold.

use crate::config_registry::{ConfigId, ConfigKey};
use crate::css_font::FontWeight;
use crate::palette::Palette;
use crate::registry::{GridId, Viewport};
use glow::HasContext;
use wasm_bindgen::prelude::*;

use super::{
    ConfigTier, DEFAULT_FONT_FAMILY, FONT_SIZE, GridTier, JustermRenderer, weight_from_js,
};

#[wasm_bindgen]
impl JustermRenderer {
    /// The configuration a grid's seven selectors ask for (#772).
    pub(super) fn key_of(&self, at: usize) -> ConfigKey {
        let grid = self.grid_at(at);
        ConfigKey::new(
            &grid.font_family,
            grid.font_size,
            grid.font_weight,
            grid.font_weight_bold,
            grid.letter_spacing,
            grid.line_height,
            grid.subpixel,
        )
    }

    /// The entry serving `key`, joining an existing one or building a new one — and **only**
    /// building when nothing already serves it (#772 AC 4). The returned id carries one reference,
    /// which the caller owes to a grid or to a `release`.
    fn acquire_config(&mut self, key: ConfigKey) -> Result<ConfigId, JsValue> {
        if let Some(id) = self.configs.find(&key) {
            self.configs.retain(id);
            return Ok(id);
        }
        let baked = Self::bake_config(
            &self.global.gl,
            self.global.max_texture_size,
            &key,
            None,
            self.global.dpr,
        )?;
        self.bake_count = self.bake_count.wrapping_add(1);
        Ok(self.configs.insert(key, ConfigTier::fresh(baked)))
    }

    /// Drop one grid's reference to a configuration, deleting the atlas when the last grid leaves.
    fn release_config(&mut self, id: ConfigId) {
        if let Some(tier) = self.configs.release(id) {
            // Safety: live GL context — or a dead one, where the delete has no state effect and
            // sets `INVALID_OPERATION` (`docs/map/territory/gl-context-lifecycle.md` § Deleting a dead object mid-life).
            unsafe { self.global.gl.delete_texture(tier.atlas) };
        }
    }

    /// Move the grid in slot `at` onto the configuration its selectors now ask for: acquire the new
    /// entry, then release the old, and never edit the shared entry to follow the grid. A failed
    /// build leaves the grid where it was. The grid is marked for re-pack and its instance count is
    /// dropped to zero, unconditionally. Why: `docs/map/territory/multi-viewport.md` § A
    /// configuration change is a move.
    pub(super) fn select_config(&mut self, at: usize, key: ConfigKey) -> Result<(), JsValue> {
        let old = self.grid_at(at).config;
        if *self.configs.key(old) == key {
            return Ok(());
        }
        let new = self.acquire_config(key)?;
        self.grids.grid_at_mut(at).config = new;
        self.release_config(old);
        let grid = self.grids.grid_at_mut(at);
        grid.needs_repack = true;
        grid.instance_count = 0;
        Ok(())
    }

    /// Register a terminal grid and return its id.
    ///
    /// The new grid is **registered but not drawn**: it holds its own per-grid state from this
    /// moment, and draws only once `setViewport` says where. Until then it is not packed either —
    /// feeding a hidden grid costs the scatter and nothing after it.
    ///
    /// It costs one GPU instance buffer and the VAO that points at it. The atlas, rasteriser, glyph
    /// cache, program and shared quad buffer stay one per context / per configuration.
    ///
    /// **The seven selectors are this grid's font, and they are optional and trailing**:
    /// `addGrid(palette, fg, bg)` takes the defaults (`"monospace"`, 16 CSS px, no letter spacing,
    /// line height 1, weights `"normal"` / `"bold"`, grayscale text), and any of the seven may be
    /// given instead. A weight takes what `setFontWeight` takes, and the last argument is what
    /// `setSubpixelAntialiasing` takes. They are what the grid's atlas is keyed by, so a grid whose
    /// selectors match a sibling's **joins that sibling's atlas and bakes nothing**.
    ///
    /// A non-finite selector, or a weight outside those values, is ignored in favour of the default,
    /// and size / line height are floored exactly as their setters floor them.
    ///
    /// Why: [`docs/map/territory/multi-viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/multi-viewport.md)
    /// § Registration.
    // Three palette columns plus the seven optional, trailing font/metric selectors.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = addGrid)]
    pub fn add_grid(
        &mut self,
        palette_colors: Vec<u32>,
        default_fg: u32,
        default_bg: u32,
        font_family: Option<String>,
        font_size: Option<f32>,
        letter_spacing: Option<f32>,
        line_height: Option<f32>,
        font_weight: Option<JsValue>,
        font_weight_bold: Option<JsValue>,
        subpixel: Option<bool>,
    ) -> Result<u32, JsValue> {
        let palette =
            Palette::from_colors(&palette_colors, default_fg, default_bg).map_err(|e| {
                JsValue::from_str(&format!(
                    "justerm-renderer: palette must be 256 colours, got {}",
                    e.got
                ))
            })?;
        // Safety: live GL context — not checked here: a registration during a loss succeeds, and
        // one that misses the configuration cache pays a thrown-away bake
        // (`docs/map/territory/gl-context-lifecycle.md` § Registration is the one entry point that
        // neither refuses nor defers).
        let key = ConfigKey::new(
            font_family.as_deref().unwrap_or(DEFAULT_FONT_FAMILY),
            font_size
                .filter(|v| v.is_finite())
                .map_or(FONT_SIZE, |v| v.max(1.0)),
            font_weight
                .as_ref()
                .and_then(weight_from_js)
                .unwrap_or(FontWeight::NORMAL),
            font_weight_bold
                .as_ref()
                .and_then(weight_from_js)
                .unwrap_or(FontWeight::BOLD),
            letter_spacing.filter(|v| v.is_finite()).unwrap_or(0.0),
            line_height
                .filter(|v| v.is_finite())
                .map_or(1.0, |v| v.max(1.0)),
            subpixel.unwrap_or(false),
        );
        let config = self.acquire_config(key.clone())?;
        let buffers = match Self::build_grid_buffers(&self.global.gl, self.global.quad_vbo) {
            Ok(b) => b,
            // Hand back the reference just taken.
            Err(e) => {
                self.release_config(config);
                return Err(e);
            }
        };
        let id = self.grids.register(GridTier::new(
            buffers,
            config,
            &key,
            palette,
            // No cells until this grid is sized: `cols`/`rows` answer 0.
            (0, 0),
        ));
        Ok(id.raw())
    }

    /// Unregister a grid and release the GPU buffer it owned, and its share of its configuration —
    /// the atlas goes only with the last grid standing on it.
    ///
    /// This is the *session-close* operation, not the hide one: hiding is
    /// `clearViewport`, which keeps every byte resident so coming back is
    /// a placement rather than a rebuild.
    ///
    /// Errors on an unknown id. **Every** grid is removable, the first one included.
    #[wasm_bindgen(js_name = removeGrid)]
    pub fn remove_grid(&mut self, grid: u32) -> Result<(), JsValue> {
        let removed = self
            .grids
            .remove(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))?;
        // Safety: live GL context — or a dead one, where the deletes have no state effect and set
        // `INVALID_OPERATION` (`docs/map/territory/gl-context-lifecycle.md` § Deleting a dead object mid-life).
        unsafe {
            self.global.gl.delete_vertex_array(removed.vao);
            self.global.gl.delete_buffer(removed.instance_vbo);
        }
        // …and give up its share of the configuration (#772).
        self.release_config(removed.config);
        Ok(())
    }

    /// Place a grid on the shared drawing buffer, in **device pixels**, top-left origin.
    ///
    /// A placed grid is a drawn grid. The rect is stored as the consumer measured it; the flip to
    /// GL's bottom-origin y happens where the grid is drawn.
    ///
    /// Errors on an unknown id and on a rect with no area — a grid with no rect is
    /// `clearViewport`'s state. Every grid is placeable, the first one included.
    #[wasm_bindgen(js_name = setViewport)]
    pub fn set_viewport(
        &mut self,
        grid: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), JsValue> {
        // Why a rect with no area is refused: `docs/map/territory/multi-viewport.md` § Placement.
        if width <= 0 || height <= 0 {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: a viewport must have area, got {width}x{height}"
            )));
        }
        self.grids
            .set_viewport(
                GridId::from_raw(grid),
                Viewport {
                    x,
                    y,
                    width,
                    height,
                },
            )
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// Stop drawing a grid **without unregistering it** — the hidden-workspace state.
    ///
    /// Every byte of the grid's state survives: its packed instances, its upload baseline, its
    /// palette, its cursor and overlays. Nothing is re-baked when it comes back, because nothing
    /// was released. The rect is not retained: showing the grid again is a `setViewport` with a
    /// freshly measured rect.
    ///
    /// Errors on an unknown id. Every grid is hideable, the first one included.
    #[wasm_bindgen(js_name = clearViewport)]
    pub fn clear_viewport(&mut self, grid: u32) -> Result<(), JsValue> {
        self.grids
            .clear_viewport(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// How many grids are registered, drawn or not. Zero on a fresh renderer. Registry *state*,
    /// not a diagnostic counter.
    #[wasm_bindgen(js_name = gridCount)]
    pub fn grid_count(&self) -> usize {
        self.grids.len()
    }

    /// How many distinct font configurations this renderer holds resources for — i.e. how many
    /// glyph atlases exist.
    ///
    /// Six terminals in one font answer `1`, and a seventh that changes its font answers `2`.
    /// Registry *state*, like `gridCount` — not a diagnostic counter.
    #[wasm_bindgen(js_name = atlasCount)]
    pub fn atlas_count(&self) -> usize {
        self.configs.len()
    }

    /// Number of atlas bakes run so far (a diagnostic) — every configuration built from nothing,
    /// plus every in-place rebuild of one (a DPR change, a context restore).
    ///
    /// Read the **delta** across an operation, as with `packs`: a grid *joining* an existing
    /// configuration moves this by zero.
    ///
    /// It counts **committed** bakes. A rebuild that fails part-way discards every replacement it
    /// built and leaves this where it was. Not a stable API surface; a counter for verification.
    /// Wraps harmlessly.
    #[wasm_bindgen(js_name = bakes)]
    pub fn bakes(&self) -> u32 {
        self.bake_count
    }

    /// Whether a grid currently has a viewport, i.e. whether it draws. Errors on an unknown
    /// id — the same answer `setViewport` gives, so a stale handle cannot read as "not drawn".
    #[wasm_bindgen(js_name = isGridDrawn)]
    pub fn is_grid_drawn(&self, grid: u32) -> Result<bool, JsValue> {
        self.grids
            .is_drawn(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))
    }
}
