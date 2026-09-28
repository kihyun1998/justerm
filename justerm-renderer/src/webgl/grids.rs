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
            // Safety: live GL context. A texture that died with a lost context deletes as an error
            // flag with no state effect (measured, #770).
            unsafe { self.global.gl.delete_texture(tier.atlas) };
        }
    }

    /// Move the grid in slot `at` onto the configuration its selectors now ask for.
    ///
    /// **The shared entry is never edited to follow it.** Ghostty states the reason in one line —
    /// *"increasing the font size in one would increase it in all"* (`src/font/SharedGrid.zig:13-18`)
    /// — so a configuration change is a *move*: acquire the new entry, then release the old. Doing
    /// it in that order is what lets a grid re-select the same key without the entry being freed in
    /// between, and it is also the failure order: a build that fails leaves the grid exactly where
    /// it was, with nothing half-applied to roll back.
    ///
    /// The grid must then re-pack. Its packed instances address slots in the *old* entry's cache, and
    /// the new entry's are its own — ghostty ends `setFontGrid` with the same call for the same
    /// reason, *"cached rows may still reference an outdated atlas from the old grid and this can
    /// cause garbage to be rendered"* (`src/renderer/generic.zig:1112-1114`).
    ///
    /// **The instance count is dropped unconditionally, and the unconditional part is the point.**
    /// The retained-grid path (`apply_damage`, which is what `justerm-web` drives) re-packs inside
    /// the same `render`, so dropping it there is invisible — until the re-pack *fails*, which
    /// `render` deliberately survives rather than blanking the frame. Without this, that survival
    /// would draw the old entry's slot ids through the new entry's atlas: a wrong glyph rather than
    /// a stale one, which is the failure class this repo treats as sacred. The direct `apply_frame`
    /// path has no columns to re-pack from at all, so for it this is the whole repair. A grid that
    /// draws only its background until the consumer's next frame is honest; one that draws another
    /// configuration's glyphs is not.
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
    /// The new grid is **registered but not drawn** — it holds its own per-grid state from this
    /// moment, and draws only once `setViewport` says where. That order is
    /// the consumer's, not a convenience: a widget's rect is a DOM measurement, and it has none
    /// until it is laid out.
    ///
    /// It costs **one** of the per-grid tier and nothing of the other two: one GPU instance buffer
    /// **and the VAO that points at it** ([ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md) D2 — no selector, not shareable, cheap to create;
    /// a VAO's whole content is *which* buffer feeds the draw, so it cannot be shared byte-for-byte
    /// and follows the buffer). No atlas, rasteriser, glyph cache, program or shared quad
    /// buffer — those stay one per context / per configuration.
    ///
    /// **The seven selectors are this grid's font, and they are optional and trailing**:
    /// `addGrid(palette, fg, bg)` takes the defaults (`"monospace"`, 16 CSS px, no letter spacing,
    /// line height 1, weights `"normal"` / `"bold"`, grayscale text), and any of the seven may be
    /// given instead. A weight takes what `setFontWeight` takes, and the last argument is what
    /// `setSubpixelAntialiasing` takes. They are what the
    /// grid's atlas is keyed by, so a grid whose selectors match a sibling's **joins that
    /// sibling's atlas and bakes nothing** — the whole economy of the middle tier, and the reason
    /// they belong here rather than in a setter called a line later: a grid born at the defaults and
    /// moved immediately would bake an atlas nobody asked for, once per registration.
    ///
    /// A non-finite selector, or a weight outside those values, is ignored in favour of the default,
    /// and size / line height are floored exactly as their setters floor them, so a grid cannot be
    /// born on a configuration `setFontSize` could not have produced.
    ///
    /// It is **not drawn** until `setViewport` places it, and until then it is
    /// not packed either: `render` skips a grid with no viewport before the pack, so feeding a hidden
    /// grid costs the scatter and nothing after it.
    // Three palette columns plus the seven font/metric selectors; the selectors are optional and
    // TRAILING, the `apply_frame` precedent, so `addGrid(palette, fg, bg)` still reads as a call.
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
        // Safety: live GL context — and "live" is not checked here, deliberately.
        //
        // **Measured 2026-08-19, because the obvious assumption is false**: Chromium's
        // `createBuffer()` hands back a NON-null object on a lost context, both in the synchronous
        // window before `webglcontextlost` dispatches and after it, so `create_buffer` returns `Ok`
        // and a registration during a loss succeeds with a buffer that died with the context. (The
        // `Err` arm below is glow's `null` path, which this browser does not take.)
        //
        // **`createTexture` answers the same way — measured 2026-08-20 (#774)**, and that half was
        // load-bearing rather than symmetric. The `acquire_config` call below **bakes** on a cache
        // miss, with no liveness guard, so a grid asking mid-loss for a configuration nobody holds
        // creates a texture and rasterises the ASCII prebake into it. Had `createTexture` answered
        // `null`, `bake_config` would map it to `Err` and this function would **refuse** — exactly
        // the contract the paragraph below says would be wrong. It does not, so the contract holds.
        // What it costs instead is one thrown-away bake per such registration (measured: `bakes()`
        // +1 during the loss, and `restore` bakes that configuration again a moment later). The
        // grid recovers with correct pixels and nothing is left corrupt. Tracked rather than fixed
        // here, because the question it really raises is which liveness predicate a mid-life entry
        // point that performs GL work should ask, and ADR-0027's conformance map has no row for
        // this one.
        //
        // That is left alone rather than guarded, because refusing would be the wrong contract: a
        // consumer registering a terminal while the context happens to be dead wants the grid, and
        // `restore` gives **every** registered grid a fresh VAO and buffer and refills it — drawn
        // and not-drawn alike (#771 had to, since a stale per-grid VAO draws the *wrong* grid once
        // there is a draw loop). **Watched rather than reasoned since #774**:
        // `demo/context-loss-grids.html` registers *and feeds* a grid inside the loss window with
        // three siblings already on the registry, then places it after the restore and asserts it
        // draws its own ink rather than a neighbour's. See the map territory.
        // **A grid says which font it is born into, and that is what keeps the middle tier's
        // economy real** (#772 AC 4, #773). It joins rather than bakes whenever a sibling already
        // stands on the same configuration: six terminals in one font hold one atlas between them.
        //
        // Until S5 a new grid was born onto whichever configuration the implicit *default* grid
        // stood on, because it had no way to ask for its own. Taking the selectors here rather than
        // hardcoding the defaults is not a convenience on top of that: a grid born at the defaults
        // and moved a line later would **bake an atlas nobody asked for**, once per registration,
        // and free it again the moment the move released it — a bake per terminal, in the slice
        // whose whole point is one atlas per font.
        //
        // It also retires the mid-loss edge the inheritance carried: a grid registered while a
        // `setFontSize` was deferred used to be born at the configuration in force rather than the
        // one asked for, with no way back. It names its own font now, and `restore` reconciles it.
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
            // Hand back the reference just taken, or a failed registration would hold an atlas open
            // for the renderer's whole life.
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
            // No cells until this grid is sized. `cols`/`rows` answer 0 honestly rather than
            // inheriting a sibling's dimensions, which would be a size nobody asked for.
            (0, 0),
        ));
        Ok(id.raw())
    }

    /// Unregister a grid and release the GPU buffer it owned.
    ///
    /// This is the *session-close* operation, not the hide one: hiding is
    /// `clearViewport`, which keeps every byte resident so coming back is
    /// a placement rather than a rebuild. Removing and re-adding a grid to hide it would
    /// reintroduce exactly the re-attach cost the shared surface exists to remove.
    ///
    /// Errors on an unknown id. **Every** grid is removable, the first one included:
    /// there is no longer a grid whose lifetime someone other than the consumer owns.
    #[wasm_bindgen(js_name = removeGrid)]
    pub fn remove_grid(&mut self, grid: u32) -> Result<(), JsValue> {
        let removed = self
            .grids
            .remove(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))?;
        // Safety: live GL context. Deleting an object that died with a lost context raises
        // `INVALID_OPERATION` and changes nothing (measured, #770) — an error flag, not a no-op.
        unsafe {
            self.global.gl.delete_vertex_array(removed.vao);
            self.global.gl.delete_buffer(removed.instance_vbo);
        }
        // …and give up its share of the configuration. The atlas goes only if this was the last
        // grid standing on it (#772) — closing one of six terminals in one font frees a buffer and
        // a VAO, not the font machinery the other five are still drawing through.
        self.release_config(removed.config);
        Ok(())
    }

    /// Place a grid on the shared drawing buffer, in **device pixels**, top-left origin.
    ///
    /// A placed grid is a drawn grid — the state the draw loop reads. The GL flip to a
    /// bottom-origin y belongs to the site that issues `gl.viewport`, not here: this is the rect
    /// the consumer measured, stored as measured.
    ///
    /// Errors on an unknown id and on a rect with no area. It errors on **no grid in particular**:
    /// every rect has one producer — the consumer's measured box — so there is no grid
    /// whose placement someone else owns.
    #[wasm_bindgen(js_name = setViewport)]
    pub fn set_viewport(
        &mut self,
        grid: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), JsValue> {
        // A viewport with no area draws no pixels, so accepting one and then answering `true` to
        // `isGridDrawn` would be a lie about what this renderer does. The state for "this grid has
        // no rect yet" already exists and is called `clearViewport` — which is the honest answer
        // for the case that produces a zero rect in the first place, a consumer measuring a DOM box
        // that is still `display:none`.
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
    /// was released; the consumer re-supplies the rect, which it has to anyway — a hidden widget's
    /// DOM box reads back as zero, so a rect retained across the hide would be a copy that can be
    /// wrong on the way back.
    ///
    /// Every grid is hideable, the first one included — a rect has one producer now, the
    /// consumer's measured box, so there is no grid that would go on painting after being hidden.
    #[wasm_bindgen(js_name = clearViewport)]
    pub fn clear_viewport(&mut self, grid: u32) -> Result<(), JsValue> {
        self.grids
            .clear_viewport(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// How many grids are registered, drawn or not. Zero on a fresh renderer.
    ///
    /// Registry *state*, not a diagnostic counter: it answers what this renderer holds, which the
    /// consumer put there. ([ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md) D5 leaves where diagnostics like `packs` live to whoever adds
    /// the second one; this is not that question.)
    #[wasm_bindgen(js_name = gridCount)]
    pub fn grid_count(&self) -> usize {
        self.grids.len()
    }

    /// How many distinct font configurations this renderer holds resources for — i.e. how many
    /// glyph atlases exist.
    ///
    /// This is what makes sharing **observable** rather than asserted: six terminals in one font
    /// answer `1`, and a seventh that changes its font answers `2`. Ghostty exposes the same number
    /// for the same reason (`SharedGridSet.count`). Registry *state*, like
    /// `gridCount` — not a diagnostic counter.
    #[wasm_bindgen(js_name = atlasCount)]
    pub fn atlas_count(&self) -> usize {
        self.configs.len()
    }

    /// Number of atlas bakes run so far (a diagnostic) — every configuration built from nothing,
    /// plus every in-place rebuild of one (a DPR change, a context restore).
    ///
    /// The consumer/proofs read the **delta** across an operation, as they do with
    /// `packs`: a grid *joining* an existing configuration must move this by zero,
    /// which is the claim the middle tier exists to make and the one a memory figure cannot settle.
    ///
    /// It counts **committed** bakes. A rebuild that fails part-way discards every replacement it
    /// built and leaves this where it was, so the number tracks configurations this renderer is
    /// drawing through rather than rasterising work it performed — which is what a delta is read
    /// for, and what keeps the delta deterministic. Not a stable API surface; a counter for
    /// verification. Wraps harmlessly.
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
