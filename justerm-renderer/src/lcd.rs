//! Per-channel (LCD / subpixel) glyph coverage (#961) — the pure half, host-testable.
//!
//! A configuration that opts in bakes each text glyph twice: the grayscale coverage the atlas has
//! always held (alpha, from a transparent canvas), plus a **light mask** — the same glyph drawn white
//! over opaque black, whose R/G/B are the browser's per-channel coverage. The slot carries the mask
//! in RGB and the grayscale coverage in A ([`with_lcd`]).
//!
//! One mask serves every ink colour. Dark ink is drawn with a different coverage curve, so the
//! shader raises the mask to [`fit_dark_gamma`]'s exponent for ink darker than
//! [`LIGHT_INK_LUMINANCE`]. The exponent is measured per configuration from a calibration draw
//! rather than fixed, because the curve is the platform's text gamma and not a property of a face.

/// The ink luminance (Rec. 709 weights over the 0..1 sRGB channels) at and above which the light
/// mask is used as-is; darker ink raises it to the fitted exponent. Mirrored as a literal in the
/// fragment shader's subpixel branch.
pub const LIGHT_INK_LUMINANCE: f32 = 0.75;

/// The exponents [`fit_dark_gamma`] searches, lowest first, in steps of `GAMMA_STEP`.
const GAMMA_MIN: f32 = 1.0;
const GAMMA_MAX: f32 = 4.0;
const GAMMA_STEP: f32 = 0.05;

/// The exponent that best maps the light mask onto dark-ink coverage.
///
/// `light` is a calibration string drawn white over opaque black, `dark` the same string drawn black
/// over opaque white, both RGBA of equal size. Each channel of each pixel is a pair: light coverage
/// `l / 255` against dark coverage `1 - d / 255`. Returns the exponent in `[1, 4]` minimising the
/// squared error of `l^g` against it, the lowest on a tie. A fully covered or empty channel scores
/// the same under every exponent, so a draw with no partial coverage fits `1.0`.
pub fn fit_dark_gamma(light: &[u8], dark: &[u8]) -> f32 {
    let pairs: Vec<(f32, f32)> = light
        .chunks_exact(4)
        .zip(dark.chunks_exact(4))
        .flat_map(|(l, d)| (0..3).map(move |k| (l[k], d[k])))
        .map(|(l, d)| (l as f32 / 255.0, 1.0 - d as f32 / 255.0))
        .collect();
    let steps = ((GAMMA_MAX - GAMMA_MIN) / GAMMA_STEP).round() as u32;
    let mut best = (f32::INFINITY, 1.0);
    for i in 0..=steps {
        let g = GAMMA_MIN + i as f32 * GAMMA_STEP;
        let err: f32 = pairs.iter().map(|&(l, d)| (l.powf(g) - d).powi(2)).sum();
        if err < best.0 {
            best = (err, g);
        }
    }
    best.1
}

/// A glyph bitmap in the subpixel layout: `rgba`'s alpha (grayscale coverage) kept, and its RGB
/// replaced by `lcd`'s RGB (the light mask). With no mask — a glyph the font never drew, such as a
/// builtin block element — RGB is the alpha repeated, so the per-channel coverage the shader reads
/// equals the grayscale one.
pub fn with_lcd(rgba: &[u8], lcd: Option<&[u8]>) -> Vec<u8> {
    let mut out = rgba.to_vec();
    match lcd {
        Some(mask) => {
            for (px, m) in out.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                px[..3].copy_from_slice(&m[..3]);
            }
        }
        None => {
            for px in out.chunks_exact_mut(4) {
                let a = px[3];
                px[..3].fill(a);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An RGBA buffer from per-pixel RGB triples, alpha 255 (an opaque canvas).
    fn opaque(px: &[[u8; 3]]) -> Vec<u8> {
        px.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    }

    /// The light/dark pair a platform with dark-ink curve `g` would produce for `levels`.
    fn pair_for(g: f32, levels: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let light: Vec<[u8; 3]> = levels.iter().map(|&l| [l, l, l]).collect();
        let dark: Vec<[u8; 3]> = levels
            .iter()
            .map(|&l| {
                let d = 255.0 - (l as f32 / 255.0).powf(g) * 255.0;
                let d = d.round() as u8;
                [d, d, d]
            })
            .collect();
        (opaque(&light), opaque(&dark))
    }

    const LEVELS: [u8; 9] = [0, 16, 48, 96, 128, 160, 200, 240, 255];

    #[test]
    fn the_fit_recovers_the_curve_the_platform_drew_with() {
        for g in [1.0, 1.8, 2.65, 3.4] {
            let (light, dark) = pair_for(g, &LEVELS);
            let fit = fit_dark_gamma(&light, &dark);
            assert!((fit - g).abs() <= GAMMA_STEP, "drew with {g}, fitted {fit}");
        }
    }

    #[test]
    fn the_fit_reads_each_channel_on_its_own() {
        // A pixel whose three channels differ, as an LCD edge does: each channel is its own sample.
        // The light mask is (60, 128, 200) and the dark draw follows g = 2.5 per channel.
        let g = 2.5f32;
        let l = [60u8, 128, 200];
        let d = l.map(|v| (255.0 - (v as f32 / 255.0).powf(g) * 255.0).round() as u8);
        let fit = fit_dark_gamma(&opaque(&[l]), &opaque(&[d]));
        assert!((fit - g).abs() <= GAMMA_STEP, "fitted {fit}");
    }

    #[test]
    fn full_and_empty_pixels_carry_no_curve() {
        // Only 0 and 255 in the light draw: `0^g` and `1^g` do not depend on `g`, so however the dark
        // draw disagrees, every exponent ties and the lowest wins.
        let light = opaque(&[[0, 0, 0], [255, 255, 255]]);
        let dark = opaque(&[[3, 3, 3], [250, 250, 250]]);
        assert_eq!(fit_dark_gamma(&light, &dark), 1.0);
    }

    #[test]
    fn the_fit_stays_inside_its_range() {
        // A dark draw far darker than any curve in range pins to the top, not past it.
        let light = opaque(&[[128, 128, 128]]);
        let dark = opaque(&[[255, 255, 255]]);
        assert_eq!(fit_dark_gamma(&light, &dark), GAMMA_MAX);
    }

    #[test]
    fn a_mask_replaces_rgb_and_keeps_the_grayscale_alpha() {
        let rgba = vec![255, 255, 255, 90, 255, 255, 255, 0];
        let mask = vec![10, 120, 200, 255, 0, 0, 0, 255];
        assert_eq!(
            with_lcd(&rgba, Some(&mask)),
            vec![10, 120, 200, 90, 0, 0, 0, 0]
        );
    }

    #[test]
    fn no_mask_repeats_the_alpha_so_the_channels_equal_the_grayscale_coverage() {
        // White RGB outside the ink would read as full coverage in the subpixel branch.
        let rgba = vec![255, 255, 255, 0, 255, 255, 255, 140];
        assert_eq!(with_lcd(&rgba, None), vec![0, 0, 0, 0, 140, 140, 140, 140]);
    }
}
