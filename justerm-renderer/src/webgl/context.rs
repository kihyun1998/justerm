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
/// (#269). The closures capture only the `Rc`'d state, never the renderer. Why:
/// `docs/map/territory/gl-context-lifecycle.md` § The listeners hold only the shared state.
pub(super) struct ContextLossHandler {
    canvas: HtmlCanvasElement,
    pub(super) state: Rc<RefCell<ContextState>>,
    /// Consumer callback for "the context did not come back within the deadline" (#327). `None`
    /// until injected; cleared on `Drop`, which disarms any deadline still pending.
    notify: Rc<RefCell<Option<js_sys::Function>>>,
    /// Consumer-injected grace period, in ms. Read when a loss arms its deadline.
    timeout_ms: Rc<Cell<i32>>,
    // Kept alive for as long as the listeners are attached; `Drop` detaches them.
    on_lost: Closure<dyn FnMut(web_sys::Event)>,
    on_restored: Closure<dyn FnMut(web_sys::Event)>,
}

/// Schedule the restore deadline for the loss episode `epoch` (#327). The timer is never
/// cancelled; a deadline for a restored context, an already-notified loss or an earlier epoch is
/// rejected by `on_restore_deadline` when it lands. Why: `docs/map/territory/gl-context-lifecycle.md`
/// § The restore deadline is never cancelled.
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
        // The borrow is released, and the callback cloned out, before calling into JS: the handler
        // is re-entrant.
        let should_notify = state.borrow_mut().on_restore_deadline(epoch);
        if !should_notify {
            return;
        }
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
            // First, or the browser never fires `webglcontextrestored`.
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
        // Disarms a restore deadline still pending: with no callback it has nobody to notify.
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
    /// It reports what the browser has *told* the renderer: a context dies synchronously while its
    /// event is only queued, so this answers `false` for a window in which every GL call is already
    /// dead. Read it as *"has a loss been reported"*, not *"is the GPU usable right now"*.
    #[wasm_bindgen(js_name = isContextLost)]
    pub fn is_context_lost(&self) -> bool {
        self.global.ctx_loss.state.borrow().is_lost()
    }

    /// Whether GPU work must be deferred *right now* — the internal counterpart of
    /// [`is_context_lost`](Self::is_context_lost), and a different question (#639). It asks the
    /// context itself and the state machine's flags (`must_defer`, #772), since each covers a
    /// window the other misses. Why: `docs/map/territory/gl-context-lifecycle.md` § "Is the context
    /// lost" has two answers, and § `gpu_work_must_wait` covered only one of its two windows.
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

    /// Recreate every GPU object the lost context owned (#269) at the live DPR, then refill every
    /// grid's instance buffer so the next `render` paints the pre-loss frame. Called by `render` on
    /// [`FrameAction::Rebuild`](crate::context_loss::FrameAction::Rebuild) only, which requires the
    /// context's own answer as well as the flag (#695, ADR-0027 D3). On any failure the old
    /// resources stay in place and `pending_rebuild` stays set, so the next frame retries. Why:
    /// `docs/map/territory/gl-context-lifecycle.md` § `restore` runs in four steps.
    pub(super) fn restore(&mut self) -> Result<(), JsValue> {
        let dpr = web_sys::window().map_or(self.global.dpr, |w| w.device_pixel_ratio() as f32);

        // 1. Build every replacement without touching a live field: the pipeline, a VAO and
        //    instance buffer per grid (#771), and an atlas per configuration at the live DPR (#772).
        //    Each `Err` arm deletes what this step built.
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
        // Only the configurations that will still have a holder once step 3 has run, asked of the
        // registry by key (#788).
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

        // 2. Commit: swap every replacement in.
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
                // The fresh buffer is empty and the baseline still describes the dead one, so the
                // refill below plans a `Full` upload (#263) — for every grid (#774).
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
        // Destructured, so a uniform location left uncopied is a compile error (#791).
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
        // The displaced objects died with the context and are not deleted (#793); `discard` above
        // deletes only what this function built on the live context.
        drop((old_atlases, old_program, old_quad_vbo, old_grid_buffers));

        // 3. Reconcile every grid whose selectors moved while the context was dead (#772).
        for at in 0..self.grids.len() {
            let key = self.key_of(at);
            self.select_config(at, key)?;
        }

        // 4. Refill: re-ask for the drawing buffer the consumer last requested, then re-upload
        //    every grid.
        self.apply_surface_size();
        for at in 0..self.grids.len() {
            self.upload_instances(at);
        }
        Ok(())
    }
}
