//! Pure colour helpers — host-testable, no wasm/GL runtime (family idiom: the
//! pure core stays `cargo test`-able while the WebGL glue is browser-only).

/// Unpack a `0xRRGGBB` colour into WebGL's normalised `[r, g, b]` floats (`0.0..=1.0`).
///
/// The renderer clears the canvas to the injected default background with this. Full
/// palette resolution (`Default`/`Indexed`/`Rgb` references → RGB) lands in #261 — this
/// is only the final byte-unpack once a colour is already a concrete `0xRRGGBB`.
pub fn gl_rgb(packed: u32) -> [f32; 3] {
    let r = ((packed >> 16) & 0xFF) as f32 / 255.0;
    let g = ((packed >> 8) & 0xFF) as f32 / 255.0;
    let b = (packed & 0xFF) as f32 / 255.0;
    [r, g, b]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_channels_to_normalised_floats() {
        // 0xFF8040: R=255→1.0, G=128→0.5019608, B=64→0.2509804 (worked by hand from
        // n/255, NOT recomputed the way the code computes it).
        let [r, g, b] = gl_rgb(0xFF_80_40);
        assert!((r - 1.0).abs() < 1e-6, "r={r}");
        assert!((g - 0.501_960_8).abs() < 1e-4, "g={g}");
        assert!((b - 0.250_980_4).abs() < 1e-4, "b={b}");
    }

    #[test]
    fn black_and_white_extremes() {
        assert_eq!(gl_rgb(0x00_00_00), [0.0, 0.0, 0.0]);
        assert_eq!(gl_rgb(0xFF_FF_FF), [1.0, 1.0, 1.0]);
    }
}
