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
    /// This is *the* cell, the exact measured one. Anything that addresses the drawing buffer —
    /// `readPixels`, GL interop, a picking rect — belongs here; `cssCellWidth` is the derived view
    /// for CSS layout.
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
    /// to decide how many columns fit, and maps mouse coordinates through it.
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
    /// exactly that, and nothing else reads it. **It is an echo**: a consumer that asked for more
    /// than fits learns it from `cssWidth`, never from this. Why:
    /// [`docs/map/territory/cell-geometry.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-geometry.md)
    /// § The grid dimensions were outputs.
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

    /// Size **one grid** to `cols`×`rows` cells — the per-grid half of what was `resize(cols, rows)`;
    /// the drawing buffer's half is `resizeSurface`.
    ///
    /// This is what `cols` / `rows` report back, and what the frames
    /// fed to this grid are expected to carry. At least one cell each way.
    ///
    /// **Nothing clamps it to the grid's rect.** Keeping every column inside the rect is the
    /// consumer's: place a rect a whole number of cells wide, which it can, having divided its box
    /// by `cssCellWidth` to get `cols` in the first place. What this renderer guarantees is that an
    /// overhang cannot reach a **neighbour** — every grid draws under its own `gl.scissor`, so a
    /// cell past the rect is clipped rather than painted over the terminal next door. Why:
    /// [`docs/map/territory/cell-geometry.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-geometry.md)
    /// § The surface's size is the consumer's device-px request.
    #[wasm_bindgen(js_name = resizeGrid)]
    pub fn resize_grid(&mut self, grid: u32, cols: u32, rows: u32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        // At least one cell each way.
        self.grid_at_mut(at).grid_size = (cols.max(1), rows.max(1));
        Ok(())
    }

    /// Size the **surface** — the one canvas every grid draws into — to a drawing buffer of
    /// `width`×`height` **device pixels**.
    ///
    /// The consumer sets the canvas's CSS display box itself from `cssWidth` / `cssHeight`. Forget
    /// it and the device-px buffer is displayed at device px — twice its intended size on a Retina
    /// display.
    ///
    /// **Device pixels, in the same space as `setViewport`.** A single-grid consumer wanting an
    /// exact fit asks for `cols * cellWidth(grid)`, both integers this crate handed it.
    ///
    /// A non-finite or non-positive size is refused rather than clamped: it is a caller error, and
    /// the honest zero case — a container that is still `display:none` — has an answer already
    /// (leave the grid unplaced, or `clearViewport` it).
    ///
    /// **WebGL is not obliged to grant the buffer**, so this asks and then adopts what it
    /// got; `cssWidth` reports the *granted* box, which is what the consumer
    /// should size its display box to. **A clamp moves every placed rect**, because a rect's GL y
    /// is measured from the buffer's bottom edge — a consumer that shrinks the surface must
    /// re-place the grids on it.
    ///
    /// **A density change does not resize it.** `setDevicePixelRatio` re-bakes every atlas and
    /// leaves the buffer exactly as asked, so `cssWidth` reports a different CSS box for the same
    /// buffer until the consumer re-issues this call — as it re-issues every viewport rect.
    ///
    /// **When the drawing buffer cannot be read, the box is adopted but not verified**: during a
    /// context loss this commits the box and defers only the read-back to the restore, where the
    /// clamp settles instead. In that window `cssWidth` describes a buffer that does not exist yet,
    /// so a consumer sizing its canvas from it overshoots, and the overshoot outlives the restore —
    /// repeat the fit once the context is back. Why each: [`docs/map/territory/cell-geometry.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-geometry.md)
    /// § The surface's size is the consumer's device-px request.
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
    /// Three callers — construction, a consumer's `resizeSurface`, and a context restore; the
    /// restore is why the request is *stored* rather than passed. A density change does **not**
    /// call this. At most two passes; `canvas.width` / `height` are re-set down to the grant rather
    /// than left at the request. Why:
    /// `docs/map/territory/cell-geometry.md` § The surface's size is the consumer's device-px
    /// request.
    pub(super) fn apply_surface_size(&mut self) {
        let (mut dw, mut dh) = self.global.requested;
        for _ in 0..2 {
            self.global.canvas.set_width(dw as u32);
            self.global.canvas.set_height(dh as u32);

            let (bw, bh) = (
                self.global.raw_gl.drawing_buffer_width(),
                self.global.raw_gl.drawing_buffer_height(),
            );
            // A buffer of no size is not a grant, it is the absence of an answer (#639): the
            // request stays committed and only the
            // verification defers. It guards on the read-back, not on the context's state
            // (`docs/map/territory/gl-context-lifecycle.md` § "Is the context lost" has two answers).
            if bw <= 0 || bh <= 0 {
                break;
            }
            if bw >= dw && bh >= dh {
                break; // granted in full; a larger grant is ignored — the request leads
            }
            (dw, dh) = (dw.min(bw), dh.min(bh));
        }
        self.global.size = (dw, dh);
        // Safety: live GL context, or a lost one, where this is a silent no-op
        // (`docs/map/territory/gl-context-lifecycle.md` § Deleting a dead object mid-life).
        unsafe {
            self.global.gl.viewport(0, 0, dw, dh);
        }
    }

    /// The drawing buffer's width in **CSS pixels** — what the consumer should set the canvas's CSS
    /// display box to, so the device-px buffer is shown at as close to the right size as a CSS
    /// length can get. Unrounded, for the same reason as `cssCellWidth`, and because a rounded box
    /// misses the buffer by up to `dpr/2` device px where this one misses by at most the browser's
    /// layout grain.
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
