//! Device-pixel-ratio arithmetic (#265). The renderer works in **device pixels** internally
//! (the GL drawing buffer + the glyph atlas are device-sized so HiDPI stays sharp), while the
//! consumer speaks **CSS pixels** (#252) and the renderer applies the DPR. This is the pure
//! conversion; the browser wiring (reading `devicePixelRatio`, canvas sizing) lives in `webgl`
//! (wasm32). The DPR is fixed at construction; autonomous mid-session DPR-change re-bake is a
//! tracked follow-up.

/// Convert a CSS-pixel length to the device-pixel drawing-buffer length at `dpr`: `round(css*dpr)`,
/// floored to 1 so a degenerate size never yields a zero-dimension buffer/viewport.
pub fn device_px(css: i32, dpr: f32) -> i32 {
    ((css as f32 * dpr).round() as i32).max(1)
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
    fn buffer_scales_by_dpr() {
        // The falsifiable DPR behaviour: a 100-CSS-px canvas backs a 200-px buffer at dpr 2.
        assert_eq!(device_px(100, 2.0), 200);
    }

    #[test]
    fn dpr_one_is_identity() {
        assert_eq!(device_px(100, 1.0), 100); // non-HiDPI: buffer == CSS
    }

    #[test]
    fn fractional_dpr_rounds_to_the_nearest_pixel() {
        assert_eq!(device_px(100, 1.5), 150);
        assert_eq!(device_px(101, 1.5), 152); // 151.5 rounds up
        assert_eq!(device_px(100, 1.25), 125);
    }

    #[test]
    fn a_degenerate_size_floors_to_one() {
        // A zero/near-zero CSS size must never yield a 0-dimension buffer/viewport.
        assert_eq!(device_px(0, 2.0), 1);
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
