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
    /// Tracked limits (surfaced by adversarial passes, not silent): colour emoji and
    /// ZWJ/grapheme clusters are separate slices; a frame with more distinct glyphs
    /// than a region's capacity, or a rasterise failure, can strand a slot.
    // Seven typed-array / scalar columns at the wasm-bindgen boundary; each is a distinct JS view
    // that cannot be grouped without an AoS rewrite breaking the zero-copy SoA (as on `apply_damage`).
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
        // TRAILING so a caller (or demo) that predates it keeps working — omitted / `undefined`
        // ⇒ every underline follows the fg (Default). Not grouped with the colour columns because
        // that would shift every existing call; the packer reads it tolerantly regardless.
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
        // The same multiply `resolve_frame` guards, evaluated one frame earlier — so guarding only
        // the pure layer left the panic exactly where it was (#355). This is the first arithmetic a
        // JS-supplied `cols`/`rows` touches; `resolve_frame` re-checks it because it is a public,
        // separately-tested surface, not because this line can be trusted to have run.
        let count = cell_count(cells.cols, cells.rows).ok_or_else(|| {
            JsValue::from_str(&format!(
                "justerm-renderer: grid {}x{} has more cells than a u32 can count",
                cells.cols, cells.rows
            ))
        })?;
        // ADR-0028 D2: the preedit takes its cells out of the stack before anything resolves them,
        // so the glyph it supplies is rasterised like any other and every later stage — contrast,
        // overlay compositing, the cursor span — sees the composed cell rather than the one the
        // application last wrote there.
        // The suggestion (#972) is the second pass and yields to the first: while a composition is
        // open it draws nothing.
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
        // sanitises control codepoints to space. Field-level borrows keep `&mut cache`
        // disjoint from the GL fields the upload closure needs.
        //
        // Which cache is the one this GRID selects into (#772) — not "the" cache, of which there is
        // no longer one. The field-level split is what keeps `&mut configs` (the cache) disjoint
        // from `&global` (the GL the upload closure needs); they are separate fields of the facade,
        // so this borrows neither through the other.
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
                // Rasterise, then classify with the hybrid signal (#297): a colour emoji comes
                // back in its own palette (COLR/CBDT/SVG) → is_color_bitmap; an emoji the font
                // draws in pure grayscale (`⬛ ⬜ ⚫ ⚪`) has R=G=B so the bitmap misses it → the
                // unicode `is_emoji_text` (keyed off core's `wide`) recovers it. Either signal
                // routes the glyph to a colour-sampled slot; a text glyph satisfies neither.
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

        // `resolve_frame` bounds `codepoints`/`flags`, the two columns it reads, and allocates only
        // `count <= codepoints.len()` — so this can wait until after it. `bg`/`fg` are read by
        // `pack_instances`, which `.get(idx).unwrap_or(0)`s them: no panic, but a short colour column
        // renders silently in Default rather than being refused. Same rule for every column — a frame
        // that does not carry its cells is not a frame (#355).
        //
        // It runs *after* so that a frame short in every column reports the cells it is missing, not
        // just its colours; `FrameShorterThanGrid` is the more useful diagnosis.
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

    /// Reconcile the GPU instance buffer with the freshly packed `self.grid().instances`, uploading
    /// only the cells that changed since the last upload (#263). A size change (first frame /
    /// resize) reallocates the whole buffer; otherwise each changed contiguous range goes up via
    /// `buffer_sub_data` and an unchanged frame does no GL work at all. `self.grid().uploaded` mirrors
    /// what the GPU holds so the next frame can diff against it.
    pub(super) fn upload_instances(&mut self, at: usize) {
        // Bind the two tiers this touches once, as separate fields of `self`: the baseline lives
        // beside the buffer it mirrors (both per-grid), and the context that uploads it is global.
        // Going through `grid_mut()` at each site instead would re-borrow all of `self` per call —
        // and `uploaded.clone_from(&instances)` is two fields of ONE grid, which only splits when
        // the grid is a place expression.
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

    /// Clear to the palette's default background, then draw every cell of the current frame
    /// (glyph composited over background) with one instanced draw call.
    ///
    /// Context loss is handled here, before any GL work: while the context is lost this is a
    /// silent no-op (a draw call on a dead context accomplishes nothing), and on the frame after
    /// `webglcontextrestored` it first rebuilds the destroyed resources. Recovery therefore needs
    /// no consumer cooperation beyond continuing to call `render`. A failed rebuild propagates and
    /// is retried on the next frame.
    ///
    /// **"While the context is lost" means either sense of lost, and it did not always**.
    /// The decision consults the context itself *and* the state machine's flag, because a browser
    /// destroys a context synchronously and only queues the event: asking the flag alone, this
    /// promise was false for that slice — a pending rebuild ran on a dead context and threw.
    pub fn render(&mut self) -> Result<(), JsValue> {
        // Ask the CONTEXT, then let the state machine compose that with the flag it owns
        // (#695, ADR-0027 D3). Bound to locals in this order deliberately: the liveness read
        // must not happen while the `Ref` below is alive, and the `Ref` must be released
        // before the `&mut self` calls in the match.
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
        // (#421) — the context is live past the match above (Skip returned, Rebuild restored). A
        // frame that set overlay + decorations + `apply_damage` marked dirty three times but
        // re-packs once. On a pack error the flag stays set, so the next render retries
        // (self-healing).
        //
        // **A grid with no viewport is not packed either, and that is a decision** (#771). The
        // registry's `Option<Viewport>` says whether a grid *draws*; nothing said whether a hidden
        // grid still pays for the frames it is fed, and the consumer's adoption design keeps hidden
        // terminals mounted **and feeding**. Measured on a release build at 120x40, an ungated
        // hidden grid costs about 0.4 ms/frame — pack + upload about 0.33 of it — so ten of them
        // would spend a quarter of a 60 fps budget on pixels nobody sees. Gating it here is free
        // rather than clever: the dirty flag stays set while the grid is hidden, so the first
        // render after it is placed packs it, once. Ghostty gates the same two things on the same
        // state (`renderer/Thread.zig:526-531` the draw, `:644-650` the CPU rebuild); alacritty
        // gates only the paint.
        //
        // One grid's bad frame must not blank its neighbours, so a pack error is held and the
        // frame still draws. It surfaces after the draw, and the flag it left set means the next
        // render retries exactly as the single-grid path did.
        // One pin set for the whole loop, so a grid cannot evict a slot a sibling packed earlier in
        // the SAME frame (#772). Without it the second grid's pack repoints the first's committed
        // slots, the first is not re-diffed because its instance floats did not change, and it draws
        // **stably wrong** — measured, and invisible to a pixel check without a control: a grid drew
        // 911 lit subpixels beside a sibling and 891 alone, every frame, with no error anywhere.
        // With the pin the second pack is refused instead, which is exactly what an over-capacity
        // *single* frame has always got (`FrameExceedsCapacity`), extended to the union.
        self.pins.clear();
        let mut pack_error = None;
        for at in 0..self.grids.len() {
            if self.grids.viewport_at(at).is_none() {
                continue;
            }
            // …and a grid whose atlas moved under it re-packs too, even though nothing it owns
            // changed (#772). A sibling on the same configuration can evict a slot this grid's
            // instances still address; the upload diff cannot notice, because the floats are the
            // same and only the atlas behind them moved. Comparing counters costs a `u32` per grid
            // per frame and is the whole of the guarantee ADR-0021 asked this tier for.
            let stale =
                self.grid_at(at).packed_at_evictions != self.config_at(at).cache.evictions();
            if self.grid_at(at).needs_repack || stale {
                match self.repack_from_grid(at) {
                    Ok(()) => self.grid_at_mut(at).needs_repack = false,
                    Err(e) => {
                        // A refused pack has to leave a **fixed point**, or the frames alternate.
                        //
                        // The pin only covers grids that actually packed this frame, and a clean
                        // grid does not pack — so without this the cycle is: this grid is refused
                        // and its sibling stays correct; next frame the sibling is clean, packs
                        // nothing, leaves the pins empty, and *this* grid succeeds by repointing
                        // the sibling's slots. Measured: `a` alternating 891 (right) / 911 (wrong)
                        // with the error appearing only on alternate frames.
                        //
                        // Dirtying every grid that shares this configuration makes them all pack,
                        // every frame, for as long as the overflow lasts — so the earlier ones pin
                        // their glyphs first and stay correct, and the same grid is refused each
                        // time. Registration order decides who wins, which is the order everything
                        // else in this loop already uses (#771): the grid registered first wins. The extra packing costs what a re-pack costs, in a state that is
                        // already reporting an error every frame; it is bounded by the overflow.
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
    /// The full-buffer clear happens once and before the loop, and it clears to **transparent**
    /// rather than to any grid's background. The buffer is one shared plane that the grids do not
    /// have to tile (ADR-0021's z-order constraint: every terminal is an overlay on it), so the
    /// area between two rects belongs to the page behind the canvas — painting a terminal's colour
    /// there would be this renderer deciding a background it was never given. A single-grid
    /// consumer sees no difference *if it places its grid over the whole buffer*, which is what the
    /// single-grid arrangement is since #773 — its own clear then covers every pixel this one
    /// touched. It is the consumer's arrangement now rather than something `resize` guaranteed, so
    /// a consumer that places a smaller rect gets a transparent margin, honestly.
    ///
    /// three.js's multiple-views example does no full clear at all
    /// (`examples/webgl_multiple_views.html:252-278`) — its views tile the canvas, so it has no
    /// uncovered area to answer for. That is a silence rather than a divergence.
    ///
    /// **Grids are painted in registration order and are not composited with each other.** A later
    /// grid's rect *replaces* what is under it rather than blending with it, because each grid opens
    /// with a `clear` and a clear writes. So a translucent grid (`setBgAlpha`) shows the page behind
    /// the canvas, never the grid it overlaps. Overlapping rects are the consumer's business —
    /// tiling panes do not produce one — and the same is true of the reference's per-view clear.
    fn draw(&self) {
        unsafe {
            self.global.gl.disable(glow::SCISSOR_TEST);
            self.global
                .gl
                .viewport(0, 0, self.global.size.0, self.global.size.1);
            self.global.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            self.global.gl.clear(glow::COLOR_BUFFER_BIT);

            // Every grid's clear and every grid's cells are confined to its own rect from here.
            // The viewport alone would already clip the *cells* (they are drawn in clip space and
            // the transform maps NDC onto the rect), but `clear` ignores the viewport entirely —
            // so without the scissor each grid's background clear would wipe the whole buffer,
            // leaving only the last grid visible.
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
        // The configuration THIS grid selects into (#772). Two grids in two fonts draw through two
        // atlases with two cell geometries in the same frame, which is why every one of these is a
        // per-draw uniform rather than a per-program one — `u_cell_uv` included, since the
        // guard band is a fixed pixel count and its *fraction* differs with the padded cell.
        let config = self.config_at(at);
        let (vx, vy, vw, vh) = viewport.gl_rect(self.global.size.1);
        let [dr, dg, db] = gl_rgb(grid.palette.default_bg);
        unsafe {
            self.global.gl.viewport(vx, vy, vw, vh);
            self.global.gl.scissor(vx, vy, vw, vh);
            // Clear with the injected background opacity so any area of this grid's rect not
            // covered by a cell is see-through too; cells then write their own per-pixel alpha
            // (#298). A rect is the consumer's box and may be any size, so the uncovered area is
            // whatever its cells do not reach — asking for `cols * cell_width(grid)` device px is
            // what makes it none, and since #773 that is the consumer's arithmetic (#331).
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
            // The instance buffer already holds the current frame — `upload_instances` (in the
            // pack path) uploaded only the changed cells (#263), so render just binds + draws.
            // Binding THIS grid's VAO is what points the attributes at THIS grid's buffer: the
            // pointer is VAO state, which is why the VAO is per-grid (#771 resolving #768).
            self.global.gl.bind_vertex_array(Some(grid.vao));

            // The projection is sized to the RECT, not to the buffer: `gl.viewport` above already
            // maps clip space onto the rect, so a buffer-sized projection would scale every grid
            // by `buffer / rect`. Identical for a grid placed over the whole buffer, which is
            // what the single-grid arrangement is.
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
            // to the content region (see FRAG_SRC). Set once per program until #772; per draw now,
            // because the padded cell belongs to the configuration rather than to the context.
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
            // The visibility guard (#368): look up the cursor cell's RESOLVED bg in the packed
            // instances (row-major, `bg` at float offset 2 of each `INSTANCE_FLOATS` cell) and invert
            // the cursor to the default fg/bg if its contrast is below the injected threshold. Only the
            // renderer has this resolved RGB, which is why the mechanism lives here (ADR-0017). If the
            // cursor sits off the current grid (no packed cell), honour the consumer's colours as-is.
            //
            // The index is bounded only by `get()`, not by `col < last_cols`: a cursor with
            // `col >= last_cols` but a small row would read a DIFFERENT row's cell here. That is
            // harmless because the shader's `covers()` paints the cursor only where a real cell has
            // `col ∈ [cursor.col, cursor.col + span)`, i.e. only when `col < cols` — so a mis-read
            // guarded colour is never sampled by any fragment. Valid as long as `covers()` keeps that
            // gate.
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
