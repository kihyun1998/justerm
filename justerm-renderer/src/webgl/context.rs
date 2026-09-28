//! The GL context lifecycle: the canvas `webglcontextlost` / `webglcontextrestored` listeners, the
//! restore deadline, and the `JustermRenderer` exports and rebuild that act on a lost context.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::config_registry::ConfigKey;
use crate::context_loss::{ContextLiveness, ContextState, DEFAULT_RESTORE_TIMEOUT_MS};
use crate::upload::invalidate_baseline;

use super::{BakedConfig, GridBuffers, JustermRenderer, Pipeline};

/// Canvas `webglcontextlost` / `webglcontextrestored` listeners feeding a shared [`ContextState`]
/// (#269). The closures capture ONLY the `Rc`'d state — never the renderer — so they can fire while
/// a `&mut JustermRenderer` method is on the stack without a `RefCell` double-borrow.
pub(super) struct ContextLossHandler {
    canvas: HtmlCanvasElement,
    pub(super) state: Rc<RefCell<ContextState>>,
    /// Consumer callback for "the context did not come back within the deadline" (#327). `None`
    /// until injected, and cleared on `Drop` so a deadline that outlives the renderer finds nobody
    /// to call — the reason no `clearTimeout` is needed (see [`arm_restore_deadline`]).
    notify: Rc<RefCell<Option<js_sys::Function>>>,
    /// Consumer-injected grace period, in ms (ADR-0017: the renderer times, the consumer decides
    /// how long). Read when a loss arms its deadline.
    timeout_ms: Rc<Cell<i32>>,
    // Kept alive for as long as the listeners are attached; `Drop` detaches them.
    on_lost: Closure<dyn FnMut(web_sys::Event)>,
    on_restored: Closure<dyn FnMut(web_sys::Event)>,
}

/// Schedule the restore deadline for the loss episode `epoch` (#327).
///
/// The timer is **never cancelled**. `clearTimeout` would work — a merely-queued timer task aborts
/// when it finds its id gone from the map (HTML spec, timer initialization steps), which is how
/// xterm.js does it — but cancelling means *owning* the `Closure`, and the consumer's notification
/// handler is exactly the place that destroys the renderer (VSCode's `onContextLoss` calls
/// `_disposeOfWebglRenderer()`). Dropping the handler would free the very closure whose body is
/// running. JS gets away with this because its closures are garbage-collected; we cannot.
///
/// So the closure is handed to JS instead (`Closure::once_into_js` keeps it alive through an
/// internal `Rc` cycle that the single invocation breaks, freeing it *after* the body returns), and
/// every deadline that has nothing to say identifies itself: `on_restore_deadline` rejects it if the
/// context came back, if we already notified, or if it belongs to an earlier loss. A stale deadline
/// costs one no-op task.
///
/// The `epoch` is what makes this safe, and what it is safe *against* is the
/// **lost → restored → lost** order: the first loss's timer is still pending when the second loss
/// arms its own, and without the stamp it would land inside the second loss's grace period and cut
/// it short. That sequence is reachable — every transition into "lost" dispatches — and
/// `context_loss.rs`'s `a_deadline_left_over_from_a_previous_loss_never_notifies` is written on it.
///
/// **This used to claim more, and the extra claim was wrong** (measured 2026-08-04, #579). It said
/// the epoch made us stricter than xterm.js, whose single `_contextRestorationTimeout` is
/// overwritten without being cleared when a second `webglcontextlost` arrives *with no restore
/// between* (`WebglRenderer.ts:131`) — "both timers then fire and its `onContextLoss` is delivered
/// twice". The overwrite is real in its source; the antecedent is not reachable. A second
/// `WEBGL_lose_context.loseContext()` on an already-lost context delivers **no** second event
/// (headless Chromium: two `loseContext()` calls with no restore between produce exactly **1**
/// `webglcontextlost`), because the event fires on the transition into lost and an already-lost
/// context has none to make. So that comparison described a state neither implementation can be
/// put into. The epoch still earns its place on the order above — a favourable comparison is just
/// the kind nobody re-checks.
fn arm_restore_deadline(
    state: &Rc<RefCell<ContextState>>,
    notify: &Rc<RefCell<Option<js_sys::Function>>>,
    epoch: u32,
    timeout_ms: i32,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let (state, notify) = (Rc::clone(state), Rc::clone(notify));
    let deadline = Closure::once_into_js(move || {
        // Release the borrow before calling out to JS: the consumer's handler runs re-entrantly and
        // may touch the renderer (dispose it, poll `isRestoreOverdue`).
        let should_notify = state.borrow_mut().on_restore_deadline(epoch);
        if !should_notify {
            return;
        }
        // Clone the callback out for the same reason — the handler is free to replace it.
        let callback = notify.borrow().clone();
        if let Some(callback) = callback {
            let _ = callback.call0(&JsValue::NULL);
        }
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        deadline.unchecked_ref(),
        timeout_ms,
    );
}

impl ContextLossHandler {
    pub(super) fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsValue> {
        let state = Rc::new(RefCell::new(ContextState::default()));
        let notify: Rc<RefCell<Option<js_sys::Function>>> = Rc::new(RefCell::new(None));
        let timeout_ms = Rc::new(Cell::new(DEFAULT_RESTORE_TIMEOUT_MS));

        let lost_state = Rc::clone(&state);
        let lost_notify = Rc::clone(&notify);
        let lost_timeout = Rc::clone(&timeout_ms);
        let on_lost = Self::listen(canvas, "webglcontextlost", move |event: web_sys::Event| {
            // Without `preventDefault()` the browser never fires `webglcontextrestored` — the
            // context stays dead forever. Every reference implementation does this first
            // (beamterm context_loss.rs, xterm.js WebglRenderer.ts).
            event.prevent_default();
            let epoch = {
                let mut state = lost_state.borrow_mut();
                state.on_lost();
                state.loss_epoch()
            };
            arm_restore_deadline(&lost_state, &lost_notify, epoch, lost_timeout.get());
        })?;

        let restored_state = Rc::clone(&state);
        let on_restored = Self::listen(canvas, "webglcontextrestored", move |_event| {
            restored_state.borrow_mut().on_restored();
        })?;

        Ok(Self {
            canvas: canvas.clone(),
            state,
            notify,
            timeout_ms,
            on_lost,
            on_restored,
        })
    }

    fn listen(
        canvas: &HtmlCanvasElement,
        event: &str,
        f: impl 'static + FnMut(web_sys::Event),
    ) -> Result<Closure<dyn FnMut(web_sys::Event)>, JsValue> {
        let closure = Closure::wrap(Box::new(f) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
        Ok(closure)
    }
}

impl Drop for ContextLossHandler {
    fn drop(&mut self) {
        // A restore deadline may still be pending in the browser and we do not cancel it (see
        // `arm_restore_deadline`), so disarm it at the other end: with no callback there is nobody
        // to notify, and the `Rc`s it captured keep its state alive until it runs once and frees
        // itself. Same observable contract as xterm.js's `clearTimeout` on dispose
        // (WebglRenderer.ts:161-163).
        *self.notify.borrow_mut() = None;
        for (event, closure) in [
            ("webglcontextlost", self.on_lost.as_ref()),
            ("webglcontextrestored", self.on_restored.as_ref()),
        ] {
            let _ = self
                .canvas
                .remove_event_listener_with_callback(event, closure.unchecked_ref());
        }
    }
}

#[wasm_bindgen]
impl JustermRenderer {
    /// Whether the WebGL context is currently lost. While lost the renderer draws nothing;
    /// it recovers by itself when the browser fires `webglcontextrestored`. Exposed so the consumer
    /// can surface the state (e.g. dim the terminal); no consumer action is required.
    ///
    /// This is the **event-driven** view — what the browser has told us — which is the honest thing
    /// to report to a consumer, and deliberately *not* what the crate's own internals guard on
    /// (`gpu_work_must_wait`, private): a context dies synchronously while its event is merely
    /// queued, so this answers `false` for a window in which every GL call is already dead. Read it
    /// as *"has a loss been reported"*, not *"is the GPU usable right now"*.
    #[wasm_bindgen(js_name = isContextLost)]
    pub fn is_context_lost(&self) -> bool {
        self.global.ctx_loss.state.borrow().is_lost()
    }

    /// Whether GPU work must be deferred *right now* — the internal counterpart of
    /// [`is_context_lost`](Self::is_context_lost), and deliberately a different question (#639).
    ///
    /// It asks **two** sources because each covers a window the other misses, and both are
    /// event-vs-state races around the same pair of DOM events:
    ///
    /// - `raw_gl.is_context_lost()` — the context itself. The browser kills a context
    ///   **synchronously** and only *queues* `webglcontextlost`, so between those two moments the
    ///   state machine still says "live" while every GL call is already dead and
    ///   `drawingBufferWidth` already reads 0. Measured in Chromium: immediately after
    ///   `WEBGL_lose_context.loseContext()`, `gl.isContextLost()` is `true` and the flag below is
    ///   still `false`. Guarding on the flag alone is what let #639 survive its own first fix.
    /// - the state machine's flags — our own bookkeeping. The mirror window: a context can come back
    ///   before we have processed `webglcontextrestored`, so the GL answers "live" while the
    ///   program, VAO and atlas it owned are still the destroyed ones and `restore` has not run.
    ///   Baking into those would be as wasted as baking into a dead context.
    ///
    /// **The second bullet described a window this function did not actually cover, until #772.**
    /// It asked `is_lost()`, and `on_restored` clears exactly that flag while setting
    /// `pending_rebuild` — so in the post-`webglcontextrestored`, pre-rebuild window both sources
    /// answered "fine" and a setter went ahead and baked into resources `restore` replaced on the
    /// next frame. The composition now lives on the state machine (`must_defer`), beside `action`,
    /// which is where ADR-0027 D1 puts it: the source that owns the flags answers the question about
    /// them. Note that `apply_surface_size` does not use this — it holds the actual answer, having just read the
    /// drawing buffer back, and guards on that instead.
    pub(super) fn gpu_work_must_wait(&self) -> bool {
        let live = if self.global.raw_gl.is_context_lost() {
            ContextLiveness::Dead
        } else {
            ContextLiveness::Usable
        };
        self.global.ctx_loss.state.borrow().must_defer(live)
    }

    /// Register a callback invoked when a lost context has not been restored within the deadline
    /// — xterm.js's `onContextLoss`. It fires **at most once per loss**, and only if the
    /// context is still lost when the deadline lands.
    ///
    /// This is a *warning*, not a verdict: Chromium keeps re-attempting a real context restore once
    /// a second indefinitely, so a `webglcontextrestored` may still arrive afterwards, and the
    /// renderer will rebuild and repaint as usual. What to do in the meantime is consumer policy
    /// ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) — VSCode tears its WebGL renderer down and falls back to a DOM one. The callback
    /// may safely destroy this renderer.
    #[wasm_bindgen(js_name = setOnContextLoss)]
    pub fn set_on_context_loss(&mut self, callback: js_sys::Function) {
        *self.global.ctx_loss.notify.borrow_mut() = Some(callback);
    }

    /// Override how long a lost context is given to come back before
    /// `setOnContextLoss` fires. Defaults to
    /// `DEFAULT_RESTORE_TIMEOUT_MS` (3000 ms, xterm.js parity). Applies to the *next* loss; a
    /// deadline already armed keeps the duration it was armed with. Negative values clamp to 0.
    #[wasm_bindgen(js_name = setContextRestoreTimeoutMs)]
    pub fn set_context_restore_timeout_ms(&mut self, ms: i32) {
        self.global.ctx_loss.timeout_ms.set(ms.max(0));
    }

    /// Whether a lost context has missed its restore deadline. The poll counterpart of
    /// `setOnContextLoss`, for a consumer that attaches late. Cleared
    /// by a late `webglcontextrestored`, which also heals the renderer.
    #[wasm_bindgen(js_name = isRestoreOverdue)]
    pub fn is_restore_overdue(&self) -> bool {
        self.global.ctx_loss.state.borrow().restore_overdue()
    }

    /// Recreate every GPU resource the lost context destroyed (#269), then refill the instance
    /// buffer so the very next `render` paints the pre-loss frame. Called by `render`
    /// when the state machine reports [`FrameAction::Rebuild`] — never on a lost context, which is a
    /// property of the predicate that reports it rather than of this function: `Rebuild` requires
    /// the *context's own* answer as well as the flag, because the flag alone said "live" for the
    /// slice before a re-loss was dispatched and this ran there anyway (#695, ADR-0027 D3).
    ///
    /// The context *object* survives a loss (the browser reuses it; xterm.js keeps its `_gl` and
    /// beamterm's re-`getContext` hands back the same object), so only the objects it owned —
    /// program, VAO, buffers, atlas texture, and the uniform locations bound to that program — are
    /// rebuilt. CPU state (glyph cache, `instances`, `grid`, palette) survives untouched, which is
    /// what preserves the terminal's content across the loss.
    ///
    /// The DPR is re-read *first*, because the display may have changed density while the context
    /// was dead (#322 is the same re-bake driven by a `matchMedia` notification) — so the fresh
    /// atlas is baked once at the live density instead of baked at the stale one and immediately
    /// re-baked, as beamterm's `restore_context` → `handle_pixel_ratio_change` does.
    ///
    /// On any failure the old resources are left in place and `pending_rebuild` stays set, so the
    /// next frame retries (self-healing, mirroring [`set_device_pixel_ratio`](Self::set_device_pixel_ratio)).
    pub(super) fn restore(&mut self) -> Result<(), JsValue> {
        let dpr = web_sys::window().map_or(self.global.dpr, |w| w.device_pixel_ratio() as f32);

        // 1. Build every replacement without touching a live field.
        //
        //    Every grid gets its own buffers — the VAO and the instance buffer are per-grid (#771),
        //    and both died with the context. Rebuilding only the default's would leave a registered
        //    grid binding a VAO that belongs to a dead context: the bind raises `INVALID_OPERATION`
        //    and leaves the *previous* grid's VAO in place, so grid B would silently draw grid A's
        //    cells. The refill comes with it — step 4 uploads every slot against a baseline
        //    invalidated for every grid — and #774 is where that stopped resting on reasoning:
        //    `demo/context-loss-grids.html` loses one context with four grids in four states
        //    (drawn / hidden / never drawn / registered mid-loss) and reads each grid's OWN rect
        //    back. Measured there: one bake per *live* configuration, including the configuration
        //    whose only holder is hidden, and `packs()` unmoved across the whole restore.
        //
        //    And every **configuration** gets its own atlas, at the live DPR, keeping its own glyph
        //    slots (#772). Baking one atlas here would have restored one grid's font and left the
        //    others sampling a dead texture. Each `?`/`Err` arm below deletes what it built, so the
        //    order of the three is free rather than load-bearing.
        let pipeline = Self::build_pipeline(&self.global.gl)?;
        let mut grid_buffers = Vec::with_capacity(self.grids.len());
        let mut baked: Vec<BakedConfig> = Vec::new();
        // Safety: live GL context; everything deleted here is this function's own and unpublished.
        let discard = |gl: &glow::Context, bufs: &[GridBuffers], baked: &[BakedConfig]| unsafe {
            for b in bufs {
                gl.delete_vertex_array(b.vao);
                gl.delete_buffer(b.instance_vbo);
            }
            for b in baked {
                gl.delete_texture(b.atlas);
            }
            gl.delete_program(pipeline.program);
            gl.delete_buffer(pipeline.quad_vbo);
        };
        for _ in 0..self.grids.len() {
            match Self::build_grid_buffers(&self.global.gl, pipeline.quad_vbo) {
                Ok(b) => grid_buffers.push(b),
                Err(e) => {
                    discard(&self.global.gl, &grid_buffers, &baked);
                    return Err(e);
                }
            }
        }
        // Only the configurations that will still have a holder once step 3 has run — asked of the
        // registry, by KEY, because that is the question (#788). An entry whose every grid has
        // drifted off its key is released by the reconcile, so baking it here would rasterise a
        // whole glyph set into a texture deleted a few lines later: one full re-bake thrown away on
        // every restore that follows a mid-loss font change, which is the operation this epic
        // exists to stop paying for.
        //
        // **This used to also require a *current* holder (`grid.config == id`), and that is the set
        // as of now rather than as of after.** Step 3 places a grid on the entry its key matches, so
        // a grid can join an entry no grid holds at this instant — two grids swapping
        // configurations mid-loss re-baked neither, and one of them came back drawing through a
        // texture that died with the context. Measured: `bakes()` 1 against `atlasCount()` 2, and
        // that grid's ink 168 where a correctly baked atlas gives 183. Silent, and not self-healing
        // until the next loss.
        let wanted: Vec<ConfigKey> = (0..self.grids.len()).map(|at| self.key_of(at)).collect();
        let config_ids = self.configs.ids_wanted_by(&wanted);
        for &id in &config_ids {
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
                    discard(&self.global.gl, &grid_buffers, &baked);
                    return Err(e);
                }
            }
        }

        // 2. Commit. Deleting the outgoing GL objects frees glow's handle slots, which is the whole
        //    reason to do it — the objects themselves died with the context.
        //
        //    **It is not a no-op on the GL side, which this comment claimed until it was measured**
        //    (#770). An object belongs to the context that created it, and after a restore that is
        //    the *previous* context, so each delete below raises `INVALID_OPERATION`. Measured two
        //    ways and in two environments, both agreeing: in raw WebGL with no wasm involved
        //    (delete a pre-loss buffer → `0x0502`; delete one created after the restore → `0`), and
        //    through this function (the restoring `render` leaves `0x0502`, a renderer that never
        //    lost its context leaves `0`) — on headless SwiftShader and on a real NVIDIA/D3D11
        //    browser alike.
        //
        //    Harmless, and stated so nobody re-derives it: it is an error *flag*, with no state
        //    effect, and the next frame reads clean. What it costs is a consumer polling `getError`
        //    around a restore, which would see a failure that is not one.
        let (old_program, old_quad_vbo) = (self.global.program, self.global.quad_vbo);
        let old_grid_buffers: Vec<GridBuffers> = grid_buffers
            .into_iter()
            .enumerate()
            .map(|(at, new)| {
                let grid = self.grids.grid_at_mut(at);
                let old = GridBuffers {
                    vao: grid.vao,
                    instance_vbo: grid.instance_vbo,
                };
                grid.vao = new.vao;
                grid.instance_vbo = new.instance_vbo;
                // The fresh buffer is empty and the baseline still describes the dead one — drop it
                // so the refill below plans a `Full` upload even when the frame is byte-identical
                // (#263). Every grid, not just the default: the trap #774 is named for, and
                // narrowing *either* this or step 4's refill to the grids that draw is what that
                // page measures — both mutations leave a hidden grid's rect blank on the far side
                // of the restore, with every other check on the page still green.
                invalidate_baseline(&mut grid.uploaded);
                old
            })
            .collect();
        let old_atlases: Vec<glow::Texture> = config_ids
            .into_iter()
            .zip(baked)
            .map(|(id, b)| {
                self.bake_count = self.bake_count.wrapping_add(1);
                self.configs.get_mut(id).adopt(b)
            })
            .collect();
        // DESTRUCTURED, not field-by-field: a uniform location that survives a restore by being
        // copied here is one an author has to remember, and `demo/context-loss.html` says in as many
        // words that forgetting one leaves a location belonging to the dead program. #791 forgot
        // `u_bleed_px` exactly that way — every frame after a restore raised `INVALID_OPERATION` and
        // the band silently stopped drawing. Binding every field by name makes the next omission a
        // compile error instead of a proof nobody wrote.
        let Pipeline {
            program,
            quad_vbo,
            u_projection,
            u_cell_size,
            u_char_size,
            u_char_offset,
            u_line_thickness,
            u_dots_per_cell,
            u_cell_uv,
            u_bg_alpha,
            u_bleed_px,
            u_lcd_gamma,
            u_cursor,
            u_cursor_color,
            u_cursor_text_color,
            u_cursor_thickness,
        } = pipeline;
        self.global.program = program;
        self.global.quad_vbo = quad_vbo;
        self.global.u_projection = u_projection;
        self.global.u_cell_size = u_cell_size;
        self.global.u_char_size = u_char_size;
        self.global.u_line_thickness = u_line_thickness;
        self.global.u_dots_per_cell = u_dots_per_cell;
        self.global.u_char_offset = u_char_offset;
        self.global.u_cell_uv = u_cell_uv;
        self.global.u_bg_alpha = u_bg_alpha;
        self.global.u_bleed_px = u_bleed_px;
        self.global.u_lcd_gamma = u_lcd_gamma;
        self.global.u_cursor = u_cursor;
        self.global.u_cursor_color = u_cursor_color;
        self.global.u_cursor_text_color = u_cursor_text_color;
        self.global.u_cursor_thickness = u_cursor_thickness;
        self.global.dpr = dpr;
        // **The old objects are NOT deleted, and that is the fix rather than the omission** (#793).
        // Every handle displaced above belonged to the context that was destroyed — `restore` has
        // exactly one call site, `FrameAction::Rebuild`, and every path into it is a rebuild after a
        // loss — so the objects are already gone and deleting them is a no-op that raises
        // `INVALID_OPERATION`. Measured on master before this change: the first frame after every
        // restore raised it **five** times (one atlas, the program, the quad VBO, one VAO, one
        // instance VBO) with the pixels perfectly correct, so nothing in the corpus could see it.
        //
        // It is not cosmetic, and the reason is what #793 is for. A uniform location that survives a
        // restore by pointing at the dead program raises the *same* `INVALID_OPERATION` — that is
        // exactly how #791's `u_bleed_px` failed — so a renderer that leaves the error flag set on
        // every restore has no channel left for the guard to listen to. Deleting nothing keeps the
        // channel clean, which is what makes `demo/context-loss-neighbour.html`'s error check a
        // guard rather than a permanent red.
        //
        // The `discard` closure above still deletes, and must: what it frees was built by *this*
        // function on the **live** context and never published.
        drop((old_atlases, old_program, old_quad_vbo, old_grid_buffers));

        // 3. Reconcile any grid whose **selectors** moved while the context was dead (#772). A
        //    `setFontSize` / `setLetterSpacing` arriving mid-loss writes the selector and defers the
        //    rest, so that grid now names a configuration whose key it no longer matches. Step 2
        //    rebuilt the entries that exist; this is what moves a grid between them, and it runs
        //    after the commit because the context has to be live for it — which it is, since this
        //    whole function is only reached on a `Rebuild`. A failure here leaves a committed,
        //    self-consistent restore and returns `Err`, so the retry latch stays set and the next
        //    frame runs the whole thing again (idempotent, self-healing).
        for at in 0..self.grids.len() {
            let key = self.key_of(at);
            self.select_config(at, key)?;
        }

        // 4. The loss reset the drawing-buffer size and the viewport; re-ask for the buffer the
        //    consumer last requested — in device px, as given — then refill every grid's buffer so
        //    `render` draws the pre-loss frame. (This said "from the CSS box" until 2026-08-19: the
        //    surface stored a CSS box for part of #773 and stopped, and the comment outlived it.) This is also where a `resizeSurface` that arrived *during* the loss
        //    gets its adopt-what-fits pass: it committed the request and skipped the read-back,
        //    leaving that to this call (#639). Nothing extra is stored for it — the buffer it asked
        //    for IS `requested`.
        self.apply_surface_size();
        for at in 0..self.grids.len() {
            self.upload_instances(at);
        }
        Ok(())
    }
}
