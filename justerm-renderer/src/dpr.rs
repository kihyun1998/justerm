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

/// The device-pixel extent of `count` cells of `cell` device px each — the drawing-buffer size, by
/// definition an exact multiple of the cell. Floored to 1 so a degenerate grid never yields a
/// zero-dimension buffer/viewport. xterm.js sizes its canvas the same way
/// (`device.canvas.width = cols * device.cell.width`).
///
/// Saturating, in `u64`, at `i32::MAX` (#339): `cols * cell` overflows a `u32` well below the grid
/// sizes a caller can ask for, and `as i32` would then hand a *negative* width to `canvas.width`.
/// Saturating is the honest answer because no buffer that large can be allocated anyway — the
/// browser clamps, [`cells_that_fit`] reads back what it actually gave, and the caller learns the
/// grid it really got.
pub fn grid_px(count: u32, cell: u32) -> i32 {
    (count as u64 * cell as u64).clamp(1, i32::MAX as u64) as i32
}

/// How many whole `cell`-wide cells fit in `buffer_px` device pixels — the inverse of [`grid_px`],
/// used only when the browser did not give us the buffer we asked for (#339). Floored to 1: a
/// renderer with a zero-column grid has no state a caller could describe.
///
/// This is the `grid <- buffer` direction beamterm takes by default (`cols = screen_size.0 /
/// cell_size.width`, `terminal_grid.rs:240`). Here it is the *fallback*: the grid leads (#331), and
/// this recovers when `drawingBufferWidth` comes back smaller than `canvas.width`.
pub fn cells_that_fit(buffer_px: i32, cell: u32) -> u32 {
    (buffer_px.max(0) as u32 / cell.max(1)).max(1)
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
    fn the_buffer_is_an_exact_multiple_of_the_device_cell() {
        // The drawing buffer is sized from the grid, not from a pixel box the consumer guessed at
        // (#331). 4 columns of a 12-device-px cell is 48 device px, exactly — never 47, never 49.
        assert_eq!(grid_px(4, 12), 48);
    }

    #[test]
    fn a_degenerate_grid_still_yields_a_drawable_buffer() {
        // A zero-column grid must never produce a zero-dimension buffer/viewport.
        assert_eq!(grid_px(0, 12), 1);
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
        for (cols, cell, dpr) in [(3u32, 33u32, 2.0f32), (8, 9, 1.1), (4, 12, 1.5)] {
            let css_box = css_px(cell, dpr) * cols as f32;
            let device_box = (css_box * dpr).round() as i32;
            assert_eq!(
                device_box,
                grid_px(cols, cell),
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
    fn a_grid_too_large_to_multiply_does_not_wrap_or_go_negative() {
        // #339. `1_000_000 * 10_000` is 10^10: it overflows a u32 (panic in debug, wrap in release),
        // and the wrapped value `as i32` is negative — a negative `canvas.width`. Saturate instead.
        assert_eq!(grid_px(1_000_000, 10_000), i32::MAX);
        // The last product that still fits, and the first that does not.
        assert_eq!(grid_px(u32::MAX, 1), i32::MAX);
        assert_eq!(grid_px(i32::MAX as u32, 1), i32::MAX);
        assert_eq!(grid_px(i32::MAX as u32 - 1, 1), i32::MAX - 1);
    }

    #[test]
    fn the_grid_that_fits_a_buffer_is_the_inverse_of_the_buffer_a_grid_needs() {
        // #339: when the browser hands back a smaller drawing buffer than we asked for, the grid we
        // actually drew is whatever fits it. Measured: Chromium clamps a 16385-px request to
        // MAX_TEXTURE_SIZE (16384 on a real GPU, 8192 under SwiftShader) and leaves `canvas.width`
        // at the request, so this is the only way to learn the truth.
        assert_eq!(cells_that_fit(grid_px(40, 9), 9), 40);
        assert_eq!(cells_that_fit(8192, 9), 910); // 910 * 9 = 8190 <= 8192
        // A buffer narrower than one cell still leaves a describable grid.
        assert_eq!(cells_that_fit(5, 9), 1);
        assert_eq!(cells_that_fit(0, 9), 1);
        // And a degenerate cell cannot divide by zero.
        assert_eq!(cells_that_fit(100, 0), 100);
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
