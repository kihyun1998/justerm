//! Cell and surface geometry — the cell a grid draws, resizing a grid, and the drawing buffer's size.

use crate::dpr::css_px;
use glow::HasContext;
use wasm_bindgen::prelude::*;

use super::JustermRenderer;

#[wasm_bindgen]
impl JustermRenderer {
    /// The cell width in **device pixels** — exactly the `u_cell_size.x` the shader lays the grid
    /// out with: the face's advance at `font_size * dpr`, floored as xterm.js floors it, **plus the
    /// consumer's `letterSpacing`**. It is the *grid* cell, as xterm's `device.cell.width` is; the glyph
    /// box inside it is smaller whenever the spacing policy is not the identity.
    ///
    /// This is *the* cell. The bare name carries it because it is the exact, measured
    /// one, as in xterm.js's `dimensions.device.cell` and beamterm's `cell_size()`. Anything that
    /// addresses the drawing buffer — `readPixels`, GL interop, a picking rect — belongs here;
    /// `cssCellWidth` is the derived view for CSS layout.
    pub fn cell_width(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.config_at(at).cell_size.0)
    }

    /// The cell height in **device pixels** (see `cell_width`).
    pub fn cell_height(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.config_at(at).cell_size.1)
    }

    /// The cell width in **CSS pixels**, unrounded. The consumer divides its available box by this
    /// to decide how many columns fit, exactly as xterm.js's `FitAddon` divides by
    /// `dimensions.css.cell.width`, and maps mouse coordinates through it (beamterm's
    /// `css_cell_size` doc says the same).
    ///
    /// It is a **float on purpose**. Rounding it to a whole CSS pixel loses the device cell for
    /// good — 33 device px at dpr 2 is 16.5, and 17 does not scale back to 33.
    #[wasm_bindgen(js_name = cssCellWidth)]
    pub fn css_cell_width(&self, grid: u32) -> Result<f32, JsValue> {
        let at = self.slot(grid)?;
        Ok(css_px(self.config_at(at).cell_size.0, self.global.dpr))
    }

    /// The cell height in **CSS pixels**, unrounded (see `cssCellWidth`).
    #[wasm_bindgen(js_name = cssCellHeight)]
    pub fn css_cell_height(&self, grid: u32) -> Result<f32, JsValue> {
        let at = self.slot(grid)?;
        Ok(css_px(self.config_at(at).cell_size.1, self.global.dpr))
    }

    /// The number of columns this grid was last sized to by `resizeGrid` —
    /// exactly that, and nothing else reads it.
    ///
    /// **It is an echo, and it has not always been one.** While the renderer sized the drawing buffer
    /// from the grid it could refuse one it could not draw, so this reported the grid actually
    /// adopted and a clamp was visible here. The buffer belongs to the *surface* now — N
    /// grids in M cell sizes share it — so `resizeSurface` adopts what the
    /// browser granted and a consumer that asked for more than fits learns it from
    /// `cssWidth`, never from this.
    ///
    /// A consumer that keeps sending frames of a grid larger than its rect does not corrupt
    /// anything — every per-cell read is bounds-checked and the surplus cells are clipped by the
    /// grid's own scissor — but its mouse mapping and reflow will be wrong. The grid is the
    /// consumer's to compute from its own box ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)), as xterm's `FitAddon` computes it.
    #[wasm_bindgen(js_name = cols)]
    pub fn cols(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.grid_at(at).cols())
    }

    /// The number of rows this grid was last sized to by `resizeGrid` — see
    /// `cols`.
    #[wasm_bindgen(js_name = rows)]
    pub fn rows(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.grid_at(at).rows())
    }

    /// Size **one grid** to `cols`×`rows` cells.
    ///
    /// Until S5 this was `resize(cols, rows)` and it wrote two tiers at once: the implicit grid's
    /// dimensions *and* the drawing buffer, which it snapped to `cols * cell` device px. [ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md)
    /// D3 is the rule that separates them — tier the **fields**, and describe a setter by which
    /// fields it writes — and multi-viewport is what makes the separation load-bearing: with two
    /// grids in two cells on one canvas there is no cell the *buffer* can be a multiple of. The
    /// buffer half became `resizeSurface`; this is the per-grid half.
    ///
    /// This is what `cols` / `rows` report back, and what the frames
    /// fed to this grid are expected to carry. At least one cell each way.
    ///
    /// **Nothing clamps it to the grid's rect, and that is a decision.** The cell-geometry guarantee — a
    /// column cannot fall outside the buffer holding it — was the renderer's to keep while the
    /// buffer was derived from the grid. The rect is now the consumer's own measured box, so
    /// keeping it is the consumer's: place a rect a whole number of cells wide, which it can,
    /// having divided that box by `cssCellWidth` to get `cols` in the
    /// first place. What this renderer still guarantees is that an overhang cannot reach a
    /// **neighbour** — every grid draws under its own `gl.scissor`, so a cell past the rect is
    /// clipped rather than painted over the terminal next door.
    #[wasm_bindgen(js_name = resizeGrid)]
    pub fn resize_grid(&mut self, grid: u32, cols: u32, rows: u32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        // A grid must have at least one cell: a zero would make `cols`/`rows` describe something
        // that cannot be fed a frame.
        self.grid_at_mut(at).grid_size = (cols.max(1), rows.max(1));
        Ok(())
    }

    /// Size the **surface** — the one canvas every grid draws into — to a drawing buffer of
    /// `width`×`height` **device pixels**.
    ///
    /// The consumer sets the canvas's CSS display box itself from `cssWidth` /
    /// `cssHeight`, exactly as it did when the buffer came from a grid (the measured cell geometry
    /// couples the two; beamterm's `auto_resize_canvas_css = false` is the same split). Forget it
    /// and the device-px buffer is displayed at device px — twice its intended size on a Retina
    /// display.
    ///
    /// **Device pixels, in the same space as `setViewport`.** The cell count
    /// this replaced is gone because the surface no longer belongs to a grid (see
    /// `resizeGrid`), and the obvious substitute — a CSS box, as three.js's
    /// `setSize` takes — would make this the one canvas-addressing export in CSS px while every
    /// rect placed on that canvas is in device px. One canvas, one space. It also keeps that guarantee
    /// *reachable*: a single-grid consumer wanting the exact guarantee that made the old
    /// `resize(cols, rows)` safe asks for `cols * cellWidth(grid)`, and both numbers are integers
    /// this crate handed it. A CSS box would put a rounding step between them.
    ///
    /// A non-finite or non-positive size is refused rather than clamped: it is a caller error, and
    /// the honest zero case — a container that is still `display:none` — has an answer already
    /// (leave the grid unplaced, or `clearViewport` it).
    ///
    /// **A clamp moves every placed rect**, because a rect's GL y is measured from the buffer's
    /// bottom edge (`Viewport::gl_rect`). A consumer that shrinks the surface must re-place the
    /// grids on it — which it is doing anyway, since its own layout is what shrank.
    ///
    /// **A density change does not move it either.** `set_device_pixel_ratio` re-bakes every atlas
    /// and leaves the buffer exactly as asked, so `css_width` reports a different CSS box for the
    /// same buffer and the canvas is displayed at a different size until the consumer re-issues
    /// this call. That is the same rule a viewport rect already follows, applied to the surface: a
    /// device-px quantity the consumer measured is re-issued by the consumer, never adjusted here.
    /// A consumer has to re-fit after a density change regardless — the cell moved, so its column
    /// count and every rect moved with it — so this costs it nothing and removes the one thing that
    /// could go wrong silently.
    ///
    /// **WebGL is not obliged to grant the buffer**, so this asks and then adopts what it
    /// got; `cssWidth` reports the *granted* box, which is what the consumer
    /// should size its display box to.
    ///
    /// **When the drawing buffer cannot be read, the box is adopted but not verified**. A
    /// resize can land at any moment in a context-loss window and a consumer has no obligation to
    /// notice, so this commits the box and defers only the read-back;
    /// the restore path re-derives the buffer from it on a live context, which is where
    /// the clamp settles instead. During that window `cssWidth` describes a buffer that does not
    /// exist yet, so a consumer sizing its canvas from it overshoots — and the overshoot outlives
    /// the restore, because the display box is the consumer's and nothing here can rewrite it
    /// (measured through `justerm-web`). Its remedy is to repeat its fit once the
    /// context is back.
    #[wasm_bindgen(js_name = resizeSurface)]
    pub fn resize_surface(&mut self, width: i32, height: i32) -> Result<(), JsValue> {
        if width <= 0 || height <= 0 {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: a surface needs a positive drawing buffer, got {width}x{height}"
            )));
        }
        self.global.requested = (width, height);
        self.apply_surface_size();
        Ok(())
    }

    /// Re-ask for the stored drawing buffer and adopt whatever the browser actually grants.
    ///
    /// Two callers, and the second is why the request is *stored* rather than passed: a consumer's
    /// `resizeSurface`, and a context restore (the loss reset the buffer, and nobody is going to
    /// re-ask). A density change does **not** call this — the buffer is the consumer's device-px
    /// measurement and only the consumer re-measures it.
    ///
    /// Two passes suffice, and the bound is a backstop against a browser that clamps
    /// non-monotonically: pass 2 asks for a buffer the browser has already granted, so it cannot be
    /// clamped again. `canvas.width` is re-set *down* to the grant rather than left oversized as
    /// xterm, beamterm and three.js all leave it, because #337 couples the CSS display box to it —
    /// a lying attribute would make `cssWidth()` describe a buffer that does not exist.
    pub(super) fn apply_surface_size(&mut self) {
        let (mut dw, mut dh) = self.global.requested;
        for _ in 0..2 {
            self.global.canvas.set_width(dw as u32);
            self.global.canvas.set_height(dh as u32);

            let (bw, bh) = (
                self.global.raw_gl.drawing_buffer_width(),
                self.global.raw_gl.drawing_buffer_height(),
            );
            // **A buffer of no size is not a grant, it is the absence of an answer** (#639). A lost
            // context reports 0x0; adopting it would commit a 1x1 surface that `restore` then
            // rebuilds at, leaving the canvas one pixel wide permanently and silently. The
            // requested box stays committed and the verification is what defers.
            //
            // This guards on the READ-BACK rather than on the context's state, and that is the
            // load-bearing part: a browser kills a context synchronously and only queues
            // `webglcontextlost`, so in that window the state machine's flag is still clear while
            // `drawingBufferWidth` already reads 0 (measured in Chromium, same task as
            // `loseContext()`). The other entry points cannot phrase the question this way because
            // they read nothing back; this one has the answer in its hand.
            if bw <= 0 || bh <= 0 {
                break;
            }
            if bw >= dw && bh >= dh {
                break; // granted in full; a larger grant is ignored — the request leads
            }
            (dw, dh) = (dw.min(bw), dh.min(bh));
        }
        self.global.size = (dw, dh);
        // Safety: live GL context (or a dead one, where this is a no-op with an error flag).
        unsafe {
            self.global.gl.viewport(0, 0, dw, dh);
        }
    }

    /// The drawing buffer's width in **CSS pixels** — what the consumer should set the canvas's CSS
    /// display box to, so the device-px buffer is shown at as close to the right size as a CSS
    /// length can get. Unrounded, for the same reason as `cssCellWidth`,
    /// and for one more: a rounded box misses the buffer by up to `dpr/2` device px — an
    /// absolute error, so it is ruinous on a small canvas — where this one misses by at most the
    /// browser's layout grain (`dpr/128`; measured 0.0016..0.0156 at dpr 1.1). It can also round
    /// *up*, stretching the image over a box wider than the buffer feeding it.
    ///
    /// Round it yourself if your layout needs a whole CSS pixel; the reverse is not available.
    #[wasm_bindgen(js_name = cssWidth)]
    pub fn css_width(&self) -> f32 {
        css_px(self.global.size.0 as u32, self.global.dpr)
    }

    /// The drawing buffer's height in **CSS pixels** (see `cssWidth`).
    #[wasm_bindgen(js_name = cssHeight)]
    pub fn css_height(&self) -> f32 {
        css_px(self.global.size.1 as u32, self.global.dpr)
    }
}
