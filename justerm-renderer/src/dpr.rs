//! Device-pixel arithmetic (#265, #331).
//!
//! **Device pixels are the source of truth.** The cell is measured in them (the rasteriser ink-scans
//! `█` at `FONT_SIZE * dpr`), the shader lays the grid out in them (`u_cell_size`), and the drawing
//! buffer is an exact multiple of them ([`grid_px`]). The CSS view ([`css_px`]) is *derived*, and is
//! a float precisely so that the derivation can be undone — a consumer's `cols * cssCellWidth()`
//! box scales back to `cols * cell` device px.
//!
//! "Scales back" is arithmetic, not physics (#337). A CSS length snaps to the browser's layout grid
//! before it reaches the compositor — 1/64 px in Blink (`layout_unit.h`, `FixedPoint<6, int32_t>`);
//! other engines differ and we have not read their source — so at a fractional DPR the used box
//! misses the buffer by up to `dpr/128` device px, measured 0.0016 to 0.0156 in headed Chromium at
//! dpr 1.1. **No CSS length can do better**: `L * 1.1` is a whole device pixel only when `10 | L`,
//! and `cols * cell` is not generally a multiple of 11. (Worse: browsers report the ratio as
//! 1.100000023841858, so nothing lands exactly.) There is no exact answer here, only a nearest one.
//!
//! The bug this closes (#331) was not "rounding". It was computing the grid and the buffer from
//! *different quantities*: the buffer from `round(cssBox * dpr)`, the layout from `cols * cell`.
//! Two sound cures exist — derive the buffer from the grid (xterm.js:
//! `device.canvas.width = cols * device.cell.width`) or derive the grid from the buffer and letterbox
//! the remainder (beamterm: `cols = canvas_width / cell_width`, leftover painted with
//! `canvas_padding_color`). We take xterm's, which makes the overhang unrepresentable rather than
//! merely absorbed.
//!
//! The browser wiring (reading `devicePixelRatio`, canvas sizing) lives in `webgl` (wasm32).

/// The CSS-pixel view of a device-pixel length at `dpr`. **Not rounded**: the device length is the
/// measured quantity, and a whole-CSS-pixel view of it cannot be converted back (#331). xterm.js
/// keeps its `dimensions.css.cell` a float for the same reason, and never sizes anything from it.
///
/// #337 asked whether the *canvas box* (as opposed to the cell) should round, as xterm.js's
/// `dimensions.css.canvas` does. It should not, and the tests below say why: rounding's error is
/// absolute (`<= dpr/2` device px) where the layout grid's is not, so it dominates on a small canvas
/// and can make the box *larger* than the buffer it displays.
///
/// Both references leave a *derived* CSS length fractional, and neither contradicts this:
/// xterm's `css.cell` is `device.cell / dpr` (`WebglRenderer.ts:694`) and beamterm's
/// `css_cell_size()` is `cell / pixel_ratio` (`terminal_grid.rs:405`). xterm's rounded `css.canvas`
/// is not a derived-length exception so much as a value it *also* feeds to DOM layers
/// (`screenElement`, mouse coords, selection, a11y, the overview ruler), where an integer costs it
/// nothing — the reason its own comment gives is avoiding `ceil`'s overshoot, which we dodge by not
/// rounding at all. beamterm's integer CSS box is an *input* (`resize(width, height)` in logical px)
/// from which it derives the device buffer — a route #331 closed by making the grid the truth.
pub fn css_px(device: u32, dpr: f32) -> f32 {
    device as f32 / dpr
}

/// The device-pixel extent of a CSS length at `dpr` — the inverse of [`css_px`], and what re-derives
/// the shared drawing buffer at a new density since the surface stopped being one grid's cells (#773).
///
/// **Rounded, where the buffer used to be exact.** While the buffer was `cols * cell` it was an
/// integral multiple of the cell by construction (#331); a surface holding N grids in M cells has no
/// such multiple to be, so it is simply the consumer's box in device pixels and the rounding error
/// is the same sub-pixel one every CSS length carries. What #331 was protecting — a column falling
/// outside the buffer that holds it — is not reachable this way: a grid draws inside its own rect
/// under `gl.scissor`, not inside the buffer.
///
/// Floored to 1 and saturated at `i32::MAX` for the same reason [`grid_px`] is: a zero or negative
/// `canvas.width` is not a size, and a browser clamps a too-large one anyway.
pub fn device_px(css: f32, dpr: f32) -> i32 {
    let px = css * dpr;
    if !px.is_finite() {
        return 1;
    }
    (px.round() as i64).clamp(1, i32::MAX as i64) as i32
}

/// Whether the DPR changed enough to re-bake the atlas at the new device size (#322). A tiny
/// float delta is not a change — a re-notification at the same ratio is a no-op.
pub fn dpr_changed(old: f32, new: f32) -> bool {
    (old - new).abs() > 1e-3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_css_box_becomes_the_device_buffer_it_is_displayed_at() {
        assert_eq!(device_px(640.0, 1.0), 640);
        assert_eq!(device_px(640.0, 2.0), 1280);
        // Fractional ratios round rather than truncate: 704.0 would lose most of a device pixel.
        assert_eq!(device_px(640.0, 1.1), 704);
        assert_eq!(device_px(100.5, 1.5), 151);
    }

    #[test]
    fn a_device_size_stored_as_a_css_box_comes_back_unchanged() {
        // **`resize_surface` takes device pixels and stores the CSS box they are displayed at**
        // (#773), because a density change must hold the physical size still while the buffer moves.
        // That is only free if the conversion round-trips at the density it was asked at — otherwise
        // every context restore and every no-op DPR notification would nudge the canvas by a pixel,
        // silently and cumulatively.
        //
        // The awkward ratios are the ones that matter: 1.1 is browser zoom at 110 %, the density at
        // which #331's grid used to overhang its buffer. The widths span a one-cell canvas to the
        // largest buffer any implementation grants (MAX_TEXTURE_SIZE, 16384 on a real GPU).
        for dpr in [1.0f32, 1.1, 1.25, 1.5, 2.0, 3.0] {
            for w in [1u32, 9, 33, 360, 1281, 3840, 8192, 16384] {
                assert_eq!(device_px(css_px(w, dpr), dpr), w as i32, "w={w} dpr={dpr}");
            }
        }
    }

    #[test]
    fn a_density_change_moves_the_buffer_and_holds_the_css_box() {
        // The other half of the same rule: the stored box is what a DPR change re-derives from, so
        // the buffer must scale with the ratio and the box must not. A 640x384 canvas at dpr 1
        // dragged onto a Retina display is 1280x768 device px behind the same 640x384 CSS box.
        let (w, h) = (640u32, 384u32);
        let (css_w, css_h) = (css_px(w, 1.0), css_px(h, 1.0));
        assert_eq!((device_px(css_w, 2.0), device_px(css_h, 2.0)), (1280, 768));
        // …and back, with nothing accumulated.
        assert_eq!((device_px(css_w, 1.0), device_px(css_h, 1.0)), (640, 384));
    }

    #[test]
    fn a_degenerate_or_unrepresentable_box_still_yields_a_usable_buffer() {
        // A zero box is what a `display:none` container measures as; a buffer of no size is not a
        // size at all, and `canvas.width = 0` would take the context down with it.
        assert_eq!(device_px(0.0, 2.0), 1);
        assert_eq!(device_px(-10.0, 2.0), 1);
        assert_eq!(device_px(f32::NAN, 2.0), 1);
        assert_eq!(device_px(f32::INFINITY, 2.0), 1);
        // Saturating, not wrapping (#339): `as i32` on a huge float would hand `canvas.width` a
        // NEGATIVE width. This is where that guard lives now — it used to sit on `grid_px`, whose
        // `cols * cell` overflowed a u32 well below the grids a caller can ask for. Saturating is
        // the honest answer either way, because no buffer that large can be allocated: Chromium
        // clamps a 16385-px request to MAX_TEXTURE_SIZE (16384 on a real GPU, 8192 under
        // SwiftShader) and leaves `canvas.width` at the request, so `apply_surface_size` reads the
        // grant back rather than predicting it.
        assert_eq!(device_px(1e30, 2.0), i32::MAX);
        assert_eq!(device_px(i32::MAX as f32, 2.0), i32::MAX);
    }

    #[test]
    fn the_css_view_of_a_device_length_is_not_rounded() {
        // #331/#335: the cell is measured in device px and handed to the shader as `u_cell_size`;
        // the CSS view is derived from it. Rounding that view to a whole CSS pixel destroys the
        // cell. 33 device px at dpr 2 is 16.5 CSS px — reporting 17 loses half a device pixel per
        // cell, which is how a grid ends up wider than the buffer holding it.
        // (33 is measured, not invented: the ink-scan of `█` at FONT_SIZE * 2 in Chromium.)
        assert_eq!(css_px(33, 2.0), 16.5);
    }

    #[test]
    fn a_device_length_converts_back_to_css() {
        // A 200-device-px cell on a dpr-2 display is 100 CSS px.
        assert_eq!(css_px(200, 2.0), 100.0);
    }

    #[test]
    fn a_css_box_built_from_the_float_cell_recovers_the_device_grid() {
        // The property #331 broke and this restores. A consumer lays out in CSS: it sizes its box as
        // `cols * cssCellWidth()` and the browser scales that by the DPR. That must land exactly on
        // the grid the shader draws — `cols * cell` device px — or the last column is clipped.
        //
        // It only holds because the CSS cell is a float. Rounding it to a whole CSS pixel first is
        // what used to make `10 -> 7 -> 11` out of a 10-device-px cell.
        //
        // Two cases the old code got wrong. `cell = 33 @ dpr 2` is the real measured cell (#328);
        // `dpr 1.1` is browser zoom at 110 %, where every demo's grid overhung its buffer.
        //
        // **Whose property this is moved at #773, and the numbers did not.** While the renderer
        // sized the buffer from the grid, this held between `css_px` and the (now retired)
        // `grid_px`. The surface is no longer any grid's cells, so the consumer keeps it: it places
        // a rect of `cols * cellWidth(grid)` device px, and it is `device_px` that has to land on
        // the same integer when the browser scales the CSS box it laid out in.
        for (cols, cell, dpr) in [(3u32, 33u32, 2.0f32), (8, 9, 1.1), (4, 12, 1.5)] {
            let css_box = css_px(cell, dpr) * cols as f32;
            assert_eq!(
                device_px(css_box, dpr),
                (cols * cell) as i32,
                "cols={cols} cell={cell} dpr={dpr}"
            );
        }
    }

    #[test]
    fn rounding_the_css_box_moves_it_further_off_the_device_grid_than_leaving_it_alone() {
        // #337: should `cssWidth()`/`cssHeight()` round, as xterm.js's `dimensions.css.canvas` does?
        //
        // Measured in headed Chromium at dpr 1.1 against a 36-device-px buffer (4 cols x 9 px):
        //   unrounded  style=32.727px  ->  used 35.9906 device px   (err 0.009)
        //   rounded    style=33px      ->  used 36.3000 device px   (err 0.300, and LARGER than the
        //                                  buffer it holds — the image is stretched)
        //
        // The rounded box's error is absolute (<= dpr/2 device px), so it grows relative to a
        // shrinking canvas. The unrounded box's error is whatever the browser's 1/64-px layout grid
        // imposes, and nothing we choose here can beat that. Rounding is never better; on a small
        // canvas it is much worse, in the exact way xterm's own comment blames for blurriness
        // ("the backing canvas image is 1 pixel too large for the canvas element size" — it blames
        // `ceil`, but `round` overshoots half the time too).
        let err = |css: f32, dpr: f32, device: u32| (css * dpr - device as f32).abs();
        // (device buffer, dpr): 36/72/360 @ 1.1 is browser zoom at 110 %; 33 @ 2 is the measured
        // cell height on a retina display (#328), whose CSS view is 16.5.
        for (device, dpr) in [(36u32, 1.1f32), (72, 1.1), (360, 1.1), (33, 2.0)] {
            let exact = css_px(device, dpr);
            assert!(
                err(exact, dpr, device) < err(exact.round(), dpr, device),
                "device={device} dpr={dpr}: exact box off by {}, rounded box off by {}",
                err(exact, dpr, device),
                err(exact.round(), dpr, device),
            );
        }
    }

    #[test]
    fn dpr_change_is_detected_only_when_it_actually_changes() {
        // #322: a real DPR step re-bakes; a same-ratio re-notification / float noise is a no-op.
        assert!(dpr_changed(1.0, 2.0));
        assert!(dpr_changed(1.0, 1.5));
        assert!(!dpr_changed(2.0, 2.0));
        assert!(!dpr_changed(2.0, 2.0 + 1e-6));
    }
}
