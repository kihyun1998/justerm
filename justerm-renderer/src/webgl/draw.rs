//! The frame path — a frame in, packed, uploaded and drawn.

use crate::bitmap::{is_color_bitmap, split_wide_bitmap};
use crate::color::gl_rgb;
use crate::context_loss::{ContextLiveness, FrameAction};
use crate::cursor::{cursor_thickness, guarded_cursor_colors, shape_id};
use crate::decoration::parse_decorations;
use crate::emoji::is_emoji_text;
use crate::frame::{Frame, INSTANCE_FLOATS, pack_instances};
use crate::frame_grid::cell_count;
use crate::glyph_resolve::{Cells, ResolveError, resolve_frame};
use crate::mat4::Mat4;
use crate::overlay::Overlay;
use crate::preedit::{Patch as PreeditPatch, Span as PreeditSpan};
use crate::registry::Viewport;
use crate::render_policy::ColorPolicy;
use crate::upload::{UploadPlan, plan_upload};
use glow::HasContext;
use wasm_bindgen::prelude::*;

use super::{ConfigTier, JustermRenderer, f32_bytes, upload_glyph};

#[wasm_bindgen]
impl JustermRenderer {
    /// Apply a `cols`×`rows` frame (dense row-major, length `cols*rows` — see `applyDamage` for the
    /// Partial-frame adapter): `bg`/`fg` are tagged-u32 colour refs, `codepoints` the glyph
    /// per cell, `flags` the `CellFlags`. A `WIDE_CHAR` lead cell rasterises a double-width
    /// glyph and splits it into two atlas slots; its `WIDE_CHAR_SPACER` cell reuses the
    /// right-half slot. New glyphs are rasterised + uploaded on demand.
    ///
    /// This direct path carries no grapheme clusters — `applyDamage` does. A frame is refused with
    /// an error, and the grid keeps its last frame, when it holds more distinct glyphs than the
    /// atlas can, when a glyph fails to rasterise, or when a column is shorter than `cols*rows`.
    // The frame's scalars and columns as separate arguments at the wasm-bindgen boundary
    // (`docs/map/territory/frame-adapter.md` § The damage entry point's arguments).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_frame(
        &mut self,
        grid: u32,
        cols: u32,
        rows: u32,
        bg: &[u32],
        fg: &[u32],
        codepoints: &[u32],
        flags: &[u16],
        blink_on: bool,
        // #520: the underline colour (SGR 58) column, tagged-u32 like `fg`/`bg`. Optional and
        // trailing; omitted ⇒ every underline follows the fg (Default).
        underline_colors: Option<Vec<u32>>,
    ) -> Result<(), JsValue> {
        // The direct (dense, cluster-free) path: one base codepoint per cell.
        let cells = Cells {
            cols,
            rows,
            codepoints,
            flags,
            clusters: &[],
        };
        // The direct path packs immediately — it retains no grid for `render` to re-pack from, so
        // it cannot defer (#421). Clear the dirty flag: this pack IS the current state.
        let at = self.slot(grid)?;
        let underline_colors = underline_colors.unwrap_or_default();
        self.pins.clear(); // this pack's scope is itself — see the field's doc
        let result = self.resolve_and_pack(at, &cells, bg, fg, &underline_colors, blink_on);
        self.grid_at_mut(at).needs_repack = false;
        result
    }

    /// Re-resolve [`GridTier::cursor_cells`](super::GridTier::cursor_cells) against the last frame's flags. Called when a frame arrives
    /// (its flags may have changed under a still cursor) *and* when the cursor moves (onto either
    /// half of a wide char, with no new frame).
    fn resolve_cursor_cells(&mut self, at: usize) {
        self.grid_at_mut(at).resolve_cursor_cells()
    }

    /// The inclusive span the open composition covers, or `None` when nothing is composing or the
    /// anchor is off the grid. The packer takes it so the layers below glyph resolution can stand
    /// down over those cells; `preedit_patch` writes the same cells, and both derive from
    /// [`preedit::writes`](crate::preedit::writes) so they cannot disagree.
    fn preedit_span(&self, at: usize, cols: u32, rows: u32) -> Option<PreeditSpan> {
        self.grid_at(at).preedit_span(cols, rows)
    }

    /// The composed cells, re-supplied with background, foreground and glyph together (#249,
    /// ADR-0028 D2) — a pass that takes the cells out of the layer stack. Why:
    /// `docs/map/territory/cell-compositing.md` § An IME preedit is not a layer in this stack.
    ///
    /// Returns owned columns, and only while a composition is open: a page that never composes
    /// allocates nothing here. `0` is the `Default` colour tag (see [`palette`](crate::palette)),
    /// so the run draws in the terminal's own default fg over its default bg.
    fn preedit_patch(
        &self,
        at: usize,
        cells: &Cells,
        bg: &[u32],
        fg: &[u32],
    ) -> Option<PreeditPatch<'static>> {
        self.grid_at(at).preedit_patch(cells, bg, fg)
    }

    /// Resolve each cell's glyph slot then pack the instance buffer. Shared by [`apply_frame`]
    /// (no clusters) and [`apply_damage`] (grapheme clusters from the persistent grid, #285).
    ///
    /// [`apply_frame`]: Self::apply_frame
    /// [`apply_damage`]: Self::apply_damage
    fn resolve_and_pack(
        &mut self,
        at: usize,
        cells: &Cells,
        bg: &[u32],
        fg: &[u32],
        underline_colors: &[u32],
        blink_on: bool,
    ) -> Result<(), JsValue> {
        self.pack_count = self.pack_count.wrapping_add(1); // #421 diagnostic — see `packs()`
        // The first arithmetic a JS-supplied `cols`/`rows` touches, checked here as `resolve_frame`
        // checks it again (#355; `docs/map/territory/frame-adapter.md` § Every index is bounded).
        let count = cell_count(cells.cols, cells.rows).ok_or_else(|| {
            JsValue::from_str(&format!(
                "justerm-renderer: grid {}x{} has more cells than a u32 can count",
                cells.cols, cells.rows
            ))
        })?;
        // ADR-0028 D2: the preedit's cells replace the frame's before anything resolves them, so
        // every later stage sees the composed cell. The suggestion (#972) is the second pass and
        // yields to the first: while a composition is open it draws nothing.
        let patch = self
            .preedit_patch(at, cells, bg, fg)
            .or_else(|| self.grid_at(at).suggestion_patch(cells, bg, fg));
        let patched = patch.as_ref().map(|p| Cells {
            cols: cells.cols,
            rows: cells.rows,
            codepoints: &p.codepoints,
            flags: &p.flags,
            clusters: &p.clusters[..],
        });
        let (cells, bg, fg) = match (patched.as_ref(), patch.as_ref()) {
            (Some(c), Some(p)) => (c, &p.bg[..], &p.fg[..]),
            _ => (cells, bg, fg),
        };

        // Resolve the per-cell glyph slots via the pure host-tested resolver (#280): it
        // rasterises before committing (a failure strands nothing), pins this frame's
        // working set (an over-capacity frame is surfaced, not silently corrupted), and
        // sanitises control codepoints to space. The cache is the one this grid's configuration
        // owns (#772), borrowed field by field so `&mut configs` stays disjoint from the `&global`
        // GL the upload closure needs.
        let gl = &self.global.gl;
        let pins = &mut self.pins;
        let config = self.configs.get_mut(self.grids.grid_at(at).config);
        let ConfigTier {
            cache,
            rasterizer,
            atlas,
            atlas_cell,
            ..
        } = config;
        let (atlas, atlas_cell) = (*atlas, *atlas_cell);
        let rasterizer = &*rasterizer;
        let (pad_w, pad_h) = atlas_cell;
        let slots = resolve_frame(
            cells,
            cache,
            pins,
            |text, style, wide| {
                // Rasterise, then classify with the hybrid signal (#297): either the bitmap or the
                // text says emoji, and either routes the glyph to a colour-sampled slot
                // (`docs/map/territory/emoji-classification.md`).
                let rgba = rasterizer.rasterize(text, style, wide)?;
                let is_emoji = is_emoji_text(text, wide) || is_color_bitmap(&rgba);
                let rgba = rasterizer.finish(rgba, text, style, wide, is_emoji)?;
                Ok((rgba, is_emoji))
            },
            |base, wide, rgba: Vec<u8>| {
                if wide {
                    // The wide source is two content cells plus one outer margin (guard band and
                    // horizontal band, #966) each side; split into two padded cells.
                    let m = rasterizer.margin_x();
                    let (left, right) =
                        split_wide_bitmap(&rgba, 2 * pad_w - 2 * m, pad_w, pad_h, m);
                    upload_glyph(gl, atlas, atlas_cell, base, &left);
                    upload_glyph(gl, atlas, atlas_cell, base + 1, &right);
                } else {
                    upload_glyph(gl, atlas, atlas_cell, base, &rgba);
                }
            },
        )
        .map_err(|e| match e {
            ResolveError::Rasterize(js) => js,
            ResolveError::FrameExceedsCapacity => JsValue::from_str(
                // Two causes since #772, and a consumer cannot tell them apart from the outside, so
                // the message names both: this frame alone, or this frame together with the other
                // grids drawn beside it through the same font configuration. Either way the pack is
                // refused rather than drawn wrong — the grid keeps its last frame and this reaches
                // the consumer as a thrown error.
                "justerm-renderer: more distinct glyphs than the atlas can hold — this frame, or \
                 this frame together with the other grids sharing its font configuration",
            ),
            ResolveError::GridOverflows { cols, rows } => JsValue::from_str(&format!(
                "justerm-renderer: grid {cols}x{rows} has more cells than a u32 can count"
            )),
            ResolveError::FrameShorterThanGrid { cells, got } => JsValue::from_str(&format!(
                "justerm-renderer: grid claims {cells} cells but the frame carries {got}"
            )),
        })?;

        // `pack_instances` would read a short `bg`/`fg` as Default rather than refuse it, so they are
        // bounded here (#355) — after `resolve_frame`, which bounds the other two columns and
        // allocates no more cells than `codepoints` holds, so a frame short in every column reports
        // `FrameShorterThanGrid`, the more useful diagnosis.
        if bg.len() < count || fg.len() < count {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: grid claims {count} cells but bg/fg carry {}/{}",
                bg.len(),
                fg.len()
            )));
        }

        // Keep the flags: a cursor may move onto a wide char before the next frame arrives.
        self.grid_at_mut(at).last_flags.clear();
        self.grid_at_mut(at)
            .last_flags
            .extend_from_slice(cells.flags);
        self.grid_at_mut(at).last_cols = cells.cols;
        self.grid_at_mut(at).last_blink_on = blink_on;
        self.resolve_cursor_cells(at);
        let frame = Frame {
            cols: cells.cols,
            rows: cells.rows,
            // The same span the patch above took over — handed on so every stage after glyph
            // resolution stands down inside it (ADR-0028 D2). Derived here rather than carried out
            // of `preedit_patch` so that the two cannot disagree about which cells are composed.
            preedit: self.preedit_span(at, cells.cols, cells.rows),
            bg,
            fg,
            slots: &slots,
            flags: cells.flags,
            codepoints: cells.codepoints,
            underline_colors,
        };
        // #271: composite the current selection / search overlay into each cell's packed bg. The
        // spans are owned by the renderer so they outlive the borrow; empty ⇒ no highlight.
        let overlay = Overlay {
            active: &self.grid_at(at).active_match_spans,
            selection: &self.grid_at(at).selection_spans,
            matches: &self.grid_at(at).match_spans,
            link_hover: &self.grid_at(at).link_hover_spans,
            colors: self.grid_at(at).highlight_colors,
        };
        // #272: the RGB-space colour policy (bold→bright, dim, minimum-contrast, …), assembled from
        // the renderer's fields.
        let policy = ColorPolicy {
            bold_to_bright: self.grid_at(at).bold_to_bright,
            min_contrast: self.grid_at(at).min_contrast,
            selection_fg: self.grid_at(at).selection_fg,
        };
        // #393: the consumer-projected marker decorations for this frame (parsed from the flat wire).
        let decorations = parse_decorations(&self.grid_at(at).decoration_spans);
        self.grid_at_mut(at).instances = pack_instances(
            &frame,
            &self.grid_at(at).palette,
            blink_on,
            &overlay,
            &policy,
            &decorations,
        );
        self.grid_at_mut(at).instance_count = count as i32;
        self.upload_instances(at);
        // Record the atlas state these instances were packed against, AFTER the resolve that may
        // itself have evicted (#772). Last, so a frame that failed above records nothing.
        let evictions = self.config_at(at).cache.evictions();
        self.grids.grid_at_mut(at).packed_at_evictions = evictions;
        Ok(())
    }

    /// Reconcile grid `at`'s GPU instance buffer with its freshly packed `instances`, uploading
    /// only the cells that changed since the last upload (#263). A size change (first frame /
    /// resize) reallocates the whole buffer; otherwise each changed contiguous range goes up via
    /// `buffer_sub_data` and an unchanged frame does no GL work at all. The grid's `uploaded`
    /// mirrors what the GPU holds so the next frame can diff against it.
    pub(super) fn upload_instances(&mut self, at: usize) {
        // The global GL and the grid are bound once, as separate fields of `self`, so
        // `uploaded.clone_from(&instances)` — two fields of one grid — borrows as a place
        // expression rather than re-borrowing all of `self`.
        let gl = &self.global.gl;
        let grid = self.grids.grid_at_mut(at);
        match plan_upload(&grid.uploaded, &grid.instances, INSTANCE_FLOATS) {
            UploadPlan::Full => unsafe {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(grid.instance_vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    f32_bytes(&grid.instances),
                    glow::DYNAMIC_DRAW,
                );
                grid.uploaded.clone_from(&grid.instances);
            },
            UploadPlan::Ranges(ranges) => {
                if ranges.is_empty() {
                    return; // nothing changed — skip the bind + upload entirely
                }
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(grid.instance_vbo));
                    for (start, end) in ranges {
                        let (lo, hi) = (start * INSTANCE_FLOATS, end * INSTANCE_FLOATS);
                        gl.buffer_sub_data_u8_slice(
                            glow::ARRAY_BUFFER,
                            (lo * std::mem::size_of::<f32>()) as i32,
                            f32_bytes(&grid.instances[lo..hi]),
                        );
                        grid.uploaded[lo..hi].copy_from_slice(&grid.instances[lo..hi]);
                    }
                }
            }
        }
    }

    /// Re-pack the instance buffer from the retained dense grid — the single pack [`render`] runs when
    /// a mutation dirtied the buffer (#421; #271 was the original overlay-only re-pack). A no-op until
    /// the first `apply_damage` (the direct `apply_frame` path keeps no columns to re-pack from). Takes
    /// the grid out so the `&mut self` pack does not borrow-conflict, then puts it back.
    ///
    /// [`render`]: Self::render
    fn repack_from_grid(&mut self, at: usize) -> Result<(), JsValue> {
        let Some(grid) = self.grid_at_mut(at).grid.take() else {
            return Ok(());
        };
        let cells = Cells {
            cols: grid.cols(),
            rows: grid.rows(),
            codepoints: grid.codepoints(),
            flags: grid.flags(),
            clusters: grid.clusters(),
        };
        let result = self.resolve_and_pack(
            at,
            &cells,
            grid.bg(),
            grid.fg(),
            grid.underline_colors(),
            self.grid_at(at).last_blink_on,
        );
        self.grid_at_mut(at).grid = Some(grid);
        result
    }

    /// Draw every placed grid: clear the whole drawing buffer to transparent, then clear each
    /// grid's rect to its own default background and draw its cells with one instanced draw call.
    /// A grid whose content changed since the last `render` is re-packed first; a grid with no
    /// viewport is neither packed nor drawn.
    ///
    /// Context loss is handled here, before any GL work: while the context is lost — whether the
    /// browser has reported it yet or not — this is a silent no-op, and on the frame after
    /// `webglcontextrestored` it first rebuilds the destroyed resources. Recovery therefore needs
    /// no consumer cooperation beyond continuing to call `render`. A failed rebuild propagates and
    /// is retried on the next frame.
    ///
    /// One grid's refused pack does not blank the others: they still draw, and the error is
    /// returned after the draw.
    pub fn render(&mut self) -> Result<(), JsValue> {
        // Ask the context, then let the state machine compose that with its flag (#695,
        // ADR-0027 D3). The liveness read happens before the `Ref` below is taken, and the `Ref`
        // is released before the `&mut self` calls in the match.
        let live = if self.global.raw_gl.is_context_lost() {
            ContextLiveness::Dead
        } else {
            ContextLiveness::Usable
        };
        let action = self.global.ctx_loss.state.borrow().action(live);
        match action {
            FrameAction::Skip => return Ok(()),
            FrameAction::Rebuild => {
                self.restore()?;
                // Only now that the rebuild is committed does the retry latch clear.
                self.global.ctx_loss.state.borrow_mut().rebuilt();
            }
            FrameAction::Draw => {}
        }
        // Pack once per drawn grid, here, if a mutation since the last render dirtied its buffer
        // (#421) — the context is live past the match above. On a pack error the flag stays set,
        // so the next render retries; the error is held and the frame still draws. A grid with no
        // viewport is skipped before the pack (#771), and one pin set spans the whole loop so a
        // grid cannot evict a slot a sibling packed earlier in the same frame (#772). Why each:
        // `docs/map/territory/multi-viewport.md` § The draw loop.
        self.pins.clear();
        let mut pack_error = None;
        for at in 0..self.grids.len() {
            if self.grids.viewport_at(at).is_none() {
                continue;
            }
            // …and a grid whose atlas moved under it re-packs too, even though nothing it owns
            // changed (#772): its configuration's eviction count moved since it last packed.
            let stale =
                self.grid_at(at).packed_at_evictions != self.config_at(at).cache.evictions();
            if self.grid_at(at).needs_repack || stale {
                match self.repack_from_grid(at) {
                    Ok(()) => self.grid_at_mut(at).needs_repack = false,
                    Err(e) => {
                        // A refused pack dirties every drawn grid on the same configuration, so
                        // the refusal is a fixed point rather than an alternation: the grid
                        // registered first wins each frame (`docs/map/territory/multi-viewport.md`
                        // § The draw loop).
                        let config = self.grid_at(at).config;
                        for other in 0..self.grids.len() {
                            if self.grids.viewport_at(other).is_some()
                                && self.grid_at(other).config == config
                            {
                                self.grid_at_mut(other).needs_repack = true;
                            }
                        }
                        pack_error = pack_error.or(Some(e));
                    }
                }
            }
        }
        self.draw();
        match pack_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Number of instance-buffer packs run so far (a diagnostic). The consumer/proofs read the
    /// **delta** across an operation to assert `render` packs once per *dirty drawn grid* per frame
    /// — not once per setter, and not at all for a grid with no viewport.
    /// Not a stable API surface — a counter for verification, not a rendering control.
    #[wasm_bindgen(js_name = packs)]
    pub fn packs(&self) -> u32 {
        self.pack_count
    }

    /// Issue the frame's GL commands: one clear of the whole drawing buffer, then one pass per
    /// **drawn** grid (#771). The caller has established that the context is live and its resources
    /// are intact.
    ///
    /// The full-buffer clear happens once, before the loop, to **transparent** rather than to any
    /// grid's background. Grids are painted in registration order and are not composited with each
    /// other: each opens with a `clear` of its own rect, so a later grid's rect replaces what is
    /// under it. Why: `docs/map/territory/multi-viewport.md` § The draw loop.
    fn draw(&self) {
        unsafe {
            self.global.gl.disable(glow::SCISSOR_TEST);
            self.global
                .gl
                .viewport(0, 0, self.global.size.0, self.global.size.1);
            self.global.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            self.global.gl.clear(glow::COLOR_BUFFER_BIT);

            // Every grid's clear and every grid's cells are confined to its own rect from here —
            // the scissor, because `clear` ignores the viewport.
            self.global.gl.enable(glow::SCISSOR_TEST);
            for at in 0..self.grids.len() {
                if let Some(viewport) = self.grids.viewport_at(at) {
                    self.draw_grid(at, viewport);
                }
            }
            self.global.gl.disable(glow::SCISSOR_TEST);
        }
    }

    /// Draw one grid into its rect. Assumes `SCISSOR_TEST` is enabled by the caller.
    ///
    /// # Safety
    ///
    /// Live context with intact resources, as [`draw`](Self::draw) establishes.
    unsafe fn draw_grid(&self, at: usize, viewport: Viewport) {
        let grid = self.grid_at(at);
        // The configuration this grid selects into (#772); every uniform below is set per draw,
        // because two grids in two fonts draw through two atlases and two cell geometries.
        let config = self.config_at(at);
        let (vx, vy, vw, vh) = viewport.gl_rect(self.global.size.1);
        let [dr, dg, db] = gl_rgb(grid.palette.default_bg);
        unsafe {
            self.global.gl.viewport(vx, vy, vw, vh);
            self.global.gl.scissor(vx, vy, vw, vh);
            // Clear with the injected background opacity so any area of this grid's rect not
            // covered by a cell is see-through too; cells then write their own per-pixel alpha
            // (#298). A rect of `cols * cell_width(grid)` device px leaves no such area (#331).
            self.global.gl.clear_color(dr, dg, db, grid.bg_alpha);
            self.global.gl.clear(glow::COLOR_BUFFER_BIT);

            if grid.instance_count == 0 {
                return;
            }

            self.global.gl.use_program(Some(self.global.program));
            self.global.gl.active_texture(glow::TEXTURE0);
            self.global
                .gl
                .bind_texture(glow::TEXTURE_2D_ARRAY, Some(config.atlas));
            // The instance buffer already holds the current frame (`upload_instances`, #263), so
            // render just binds + draws. This grid's VAO points the attributes at this grid's
            // buffer (#771).
            self.global.gl.bind_vertex_array(Some(grid.vao));

            // The projection is sized to the rect, not to the buffer: `gl.viewport` above already
            // maps clip space onto the rect.
            let proj = Mat4::orthographic_from_size(vw as f32, vh as f32);
            self.global.gl.uniform_matrix_4_f32_slice(
                Some(&self.global.u_projection),
                false,
                &proj.data,
            );
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_cell_size),
                config.cell_size.0 as f32,
                config.cell_size.1 as f32,
            );
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_char_size),
                config.char_size.0 as f32,
                config.char_size.1 as f32,
            );
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_char_offset),
                config.char_offset.0 as f32,
                config.char_offset.1 as f32,
            );
            // How much of each padded atlas cell is guard band, so the shader insets the texcoord
            // to the content region (see FRAG_SRC). The band is a fixed pixel count, so its
            // fraction is the configuration's.
            let ((ox, oy), (sx, sy)) = crate::metrics::cell_uv(
                crate::metrics::SlotGeometry {
                    padded: config.atlas_cell,
                    draw_origin: (0, 0), // unused by `cell_uv`
                },
                config.cell_size,
            );
            self.global
                .gl
                .uniform_4_f32(Some(&self.global.u_cell_uv), ox, oy, sx, sy);
            let line_thickness = crate::metrics::line_thickness(grid.font_size * self.global.dpr);
            self.global
                .gl
                .uniform_1_f32(Some(&self.global.u_line_thickness), line_thickness as f32);
            // Per grid, like the thickness it is derived from — both depend on the font size, and
            // the dot count also on the cell width, so neither is a per-config constant (#830).
            self.global.gl.uniform_1_f32(
                Some(&self.global.u_dots_per_cell),
                crate::metrics::dots_per_cell(config.cell_size.0, line_thickness) as f32,
            );
            self.global
                .gl
                .uniform_1_f32(Some(&self.global.u_bg_alpha), grid.bg_alpha);
            // Per configuration, not per grid: the bands are a property of the face (#791, #966).
            let (bleed_x, bleed_y) = config.rasterizer.bleed();
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_bleed_px),
                bleed_x as f32,
                bleed_y as f32,
            );
            // Per configuration too: whether its atlas carries per-channel coverage, and the
            // dark-ink curve it was measured with (#961).
            self.global.gl.uniform_1_f32(
                Some(&self.global.u_lcd_gamma),
                config.rasterizer.lcd_gamma(),
            );
            // `u_cursor.w == 0` means NO cursor; a shape is `shape_id + 1`. Every shape — block
            // included — reaches the shader this way, so a move or a blink is a uniform, not an
            // upload (#270).
            let (cx, cy, span, shape) = match grid.cursor {
                Some(c) => (
                    grid.cursor_cells.0 as f32,
                    c.row as f32,
                    grid.cursor_cells.1 as f32,
                    shape_id(c.shape) as f32 + 1.0,
                ),
                None => (0.0, 0.0, 1.0, 0.0),
            };
            self.global
                .gl
                .uniform_4_f32(Some(&self.global.u_cursor), cx, cy, span, shape);
            // The visibility guard (#368): look up the cursor cell's resolved bg in the packed
            // instances (row-major, `bg` at float offset 2 of each `INSTANCE_FLOATS` cell) and invert
            // the cursor to the default fg/bg if its contrast is below the injected threshold. A
            // cursor off the current grid (no packed cell) keeps the consumer's colours. The index is
            // bounded only by `get()`; why that is enough: `docs/map/territory/caret-drawing.md`
            // § The contrast guard reads the packed background.
            let (color, text_color) = match grid.cursor {
                Some(c) => {
                    let cell_bg = (c.row as usize)
                        .checked_mul(grid.last_cols as usize)
                        .and_then(|i| i.checked_add(grid.cursor_cells.0 as usize))
                        .and_then(|i| i.checked_mul(INSTANCE_FLOATS))
                        .and_then(|base| grid.instances.get(base + 2..base + 5));
                    match cell_bg {
                        Some(bg) => guarded_cursor_colors(
                            c.color,
                            c.text_color,
                            [bg[0], bg[1], bg[2]],
                            grid.palette.default_fg,
                            grid.palette.default_bg,
                            grid.cursor_contrast,
                        ),
                        None => (c.color, c.text_color),
                    }
                }
                None => (0, 0),
            };
            let [cr, cg, cb] = gl_rgb(color);
            self.global
                .gl
                .uniform_3_f32(Some(&self.global.u_cursor_color), cr, cg, cb);
            let [tr, tg, tb] = gl_rgb(text_color);
            self.global
                .gl
                .uniform_3_f32(Some(&self.global.u_cursor_text_color), tr, tg, tb);
            self.global.gl.uniform_1_f32(
                Some(&self.global.u_cursor_thickness),
                cursor_thickness(grid.cursor_thickness_frac, config.cell_size.0) as f32,
            );

            self.global
                .gl
                .draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, grid.instance_count);
        }
    }
}
