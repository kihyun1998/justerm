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

/// W3C relative luminance of a normalised `[r, g, b]` (`0.0..=1.0`) colour — the WCAG 2.0
/// [relative-luminance] algorithm: linearise each sRGB channel, then weight `0.2126 R + 0.7152 G
/// + 0.0722 B`. Computed in `f64` to match the reference to the last digit even though the atlas
/// works in `f32`. Used only by [`contrast`] (the cursor-visibility guard, #368).
///
/// [relative-luminance]: https://www.w3.org/TR/WCAG20/#relativeluminancedef
fn relative_luminance(rgb: [f32; 3]) -> f64 {
    let channel = |c: f32| -> f64 {
        let c = c as f64;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

/// WCAG 2.0 [contrast ratio] between two normalised colours, `(lighter + 0.05) / (darker + 0.05)`
/// — in `[1.0, 21.0]` (equal colours → 1, black vs white → 21). This is alacritty's cursor-visibility
/// metric (`vte` `Rgb::contrast`), used by the `MIN_CURSOR_CONTRAST` guard (#368), NOT the WCAG AA
/// `4.5` readability bar — alacritty's threshold is `1.5`, a "distinguishable" cursor, not readable
/// text.
///
/// [contrast ratio]: https://www.w3.org/TR/WCAG20/#contrast-ratiodef
pub fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (darker, lighter) = if la > lb { (lb, la) } else { (la, lb) };
    ((lighter + 0.05) / (darker + 0.05)) as f32
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

    #[test]
    fn contrast_matches_the_wcag_reference_table() {
        // The known-good table is alacritty's OWN `vte` `Rgb::contrast` test (`vte/src/ansi.rs`) —
        // independent WCAG values, not recomputed the way `contrast` computes them. `f32` (vs the
        // reference `f64`) costs a few ulps, well under this tolerance.
        let c = |a: u32, b: u32| contrast(gl_rgb(a), gl_rgb(b));
        assert!((c(0xFFFFFF, 0x000000) - 21.0).abs() < 1e-3, "white/black");
        assert!(
            (c(0xFFFFFF, 0xFFFFFF) - 1.0).abs() < 1e-4,
            "equal colours = 1"
        );
        assert!(
            (c(0xFF00FF, 0x00FF00) - 2.285_543_6).abs() < 1e-3,
            "magenta/green"
        );
        assert!(
            (c(0x123456, 0xFEDCBA) - 9.786_559).abs() < 1e-2,
            "arbitrary pair"
        );
        // Symmetry: contrast(a,b) == contrast(b,a).
        assert_eq!(c(0x123456, 0xFEDCBA), c(0xFEDCBA, 0x123456));
    }
}
