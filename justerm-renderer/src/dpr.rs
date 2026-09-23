//! Device-pixel arithmetic (#265, #331).
//!
//! **Device pixels are the source of truth.** The cell is measured in them (the rasteriser reads the
//! face's advance and ink-scans its `█` at `FONT_SIZE * dpr`), the shader lays the grid out in them (`u_cell_size`), and the drawing
//! buffer is sized in them (`resize_surface` takes them as given). The CSS view ([`css_px`]) is
//! *derived*, and is a float precisely so that the derivation can be undone — a consumer's
//! `cols * cssCellWidth()` box scales back to `cols * cell` device px, which is how it sizes a
//! surface and places a rect since the buffer stopped being any one grid's cells (#773).
//!
//! "Scales back" is arithmetic, not physics (#337): at a fractional DPR a CSS box lands within a
//! fraction of a device pixel, never exactly. Why, and why the grid rather than the buffer is the
//! truth (#331): ADR-0018 and `docs/map/territory/cell-geometry.md`.
//!
//! The browser wiring (reading `devicePixelRatio`, canvas sizing) lives in `webgl` (wasm32).

/// The CSS-pixel view of a device-pixel length at `dpr`. **Not rounded**: the device length is the
/// measured quantity, and a whole-CSS-pixel view of it cannot be converted back (#331). Deliberately
/// unrounded for the canvas box too (#337), unlike xterm.js's `css.canvas` — ADR-0018, and the
/// tests below.
pub fn css_px(device: u32, dpr: f32) -> f32 {
    device as f32 / dpr
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
    fn a_css_box_built_from_the_float_cell_scales_back_to_the_device_grid() {
        // **The property #331 broke, and whose arithmetic it is moved twice.** A consumer lays out
        // in CSS: it sizes a box as `cols * cssCellWidth()` and the browser scales that by the DPR.
        // That must land exactly on the grid the shader draws — `cols * cell` device px — or the
        // last column is clipped.
        //
        // It only holds because the CSS cell is a **float**, which is the whole reason `css_px`
        // returns one. Rounding it to a whole CSS pixel first is what used to make `10 -> 7 -> 11`
        // out of a 10-device-px cell.
        //
        // Until #331 this was the renderer's to keep, and it kept it by sizing the buffer from the
        // grid. Until #773 it was `grid_px`'s. It is now the *consumer's*: it asks
        // `resizeSurface` for `cols * cellWidth(grid)` device px directly, so the scaling below is
        // what a browser does to the style box the consumer sets from `cssWidth()`. This module's
        // remaining share of it is the one thing asserted here — that `css_px`'s float survives the
        // round trip.
        //
        // Two cases the old code got wrong. `cell = 33 @ dpr 2` is the real measured cell (#328);
        // `dpr 1.1` is browser zoom at 110 %, where every demo's grid overhung its buffer.
        for (cols, cell, dpr) in [(3u32, 33u32, 2.0f32), (8, 9, 1.1), (4, 12, 1.5)] {
            let css_box = css_px(cell, dpr) * cols as f32;
            assert_eq!(
                (css_box * dpr).round() as u32,
                cols * cell,
                "cols={cols} cell={cell} dpr={dpr}"
            );
        }
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
