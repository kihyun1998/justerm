//! Device-pixel arithmetic (#265, #331).
//!
//! **Device pixels are the source of truth.** The cell is measured in them (the rasteriser ink-scans
//! `█` at `FONT_SIZE * dpr`), the shader lays the grid out in them (`u_cell_size`), and the drawing
//! buffer is an exact multiple of them ([`grid_px`]). The CSS view ([`css_px`]) is *derived*, and is
//! a float precisely so that the derivation can be undone — a consumer's `cols * cssCellWidth()`
//! box scales back to `cols * cell` device px exactly.
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
pub fn css_px(device: u32, dpr: f32) -> f32 {
    device as f32 / dpr
}

/// The device-pixel extent of `count` cells of `cell` device px each — the drawing-buffer size, by
/// definition an exact multiple of the cell. Floored to 1 so a degenerate grid never yields a
/// zero-dimension buffer/viewport. xterm.js sizes its canvas the same way
/// (`device.canvas.width = cols * device.cell.width`).
pub fn grid_px(count: u32, cell: u32) -> i32 {
    (count * cell).max(1) as i32
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
    fn dpr_change_is_detected_only_when_it_actually_changes() {
        // #322: a real DPR step re-bakes; a same-ratio re-notification / float noise is a no-op.
        assert!(dpr_changed(1.0, 2.0));
        assert!(dpr_changed(1.0, 1.5));
        assert!(!dpr_changed(2.0, 2.0));
        assert!(!dpr_changed(2.0, 2.0 + 1e-6));
    }
}
