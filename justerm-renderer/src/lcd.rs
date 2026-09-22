//! Per-channel (LCD / subpixel) glyph coverage (#961) — the pure half, host-testable.
//!
//! A configuration that opts in bakes each text glyph twice: the grayscale coverage the atlas has
//! always held (alpha, from a transparent canvas), plus a **light mask** — the same glyph drawn white
//! over opaque black, whose R/G/B are the browser's per-channel coverage. The slot carries the mask
//! in RGB and the grayscale coverage in A ([`with_lcd`]).
//!
//! One mask serves every ink colour. Dark ink is drawn with a different coverage curve, so the
//! fragment shader raises the mask to [`fit_dark_gamma`]'s exponent for ink whose luminance is
//! below the light-ink threshold it holds. The exponent is measured per configuration from a
//! calibration draw rather than fixed, because the curve is the platform's text gamma and not a
//! property of a face.

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
///
/// One pass over the pixels, then the search runs over the 254 partial light levels rather than
/// the pixels: the error splits per level into `n·l^2g − 2·l^g·Σd` plus a term no exponent moves,
/// so its cost does not grow with the draw.
pub fn fit_dark_gamma(light: &[u8], dark: &[u8]) -> f32 {
    // Per light level: how many channels, and the sum of their dark coverage.
    let mut count = [0u64; 256];
    let mut dark_sum = [0f64; 256];
    for (l, d) in light.chunks_exact(4).zip(dark.chunks_exact(4)) {
        for k in 0..3 {
            count[l[k] as usize] += 1;
            dark_sum[l[k] as usize] += 1.0 - d[k] as f64 / 255.0;
        }
    }
    let steps = ((GAMMA_MAX - GAMMA_MIN) / GAMMA_STEP).round() as u32;
    let mut best = (f64::INFINITY, 1.0);
    for i in 0..=steps {
        let g = GAMMA_MIN + i as f32 * GAMMA_STEP;
        let err: f64 = (1..255)
            .filter(|&v| count[v] != 0)
            .map(|v| {
                let p = (v as f64 / 255.0).powf(g as f64);
                count[v] as f64 * p * p - 2.0 * p * dark_sum[v]
            })
            .sum();
        if err < best.0 {
            best = (err, g);
        }
    }
    best.1
}

/// A glyph bitmap in the subpixel layout: `rgba`'s alpha (grayscale coverage) kept, and its RGB
/// replaced by `mask`'s RGB (the light mask).
pub fn with_lcd(rgba: &[u8], mask: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for (px, m) in out.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
        px[..3].copy_from_slice(&m[..3]);
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
        // Red is fully covered and says nothing about the curve (every exponent ties on it); only
        // green and blue carry g = 2.5, so a fit reading one channel per pixel lands on 1.0.
        let g = 2.5f32;
        let l = [255u8, 128, 200];
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

    /// The fit as a direct sum over every channel of every pixel — what the per-level form must equal.
    fn fit_directly(light: &[u8], dark: &[u8]) -> f32 {
        let steps = ((GAMMA_MAX - GAMMA_MIN) / GAMMA_STEP).round() as u32;
        let mut best = (f64::INFINITY, 1.0);
        for i in 0..=steps {
            let g = GAMMA_MIN + i as f32 * GAMMA_STEP;
            let err: f64 = light
                .chunks_exact(4)
                .zip(dark.chunks_exact(4))
                .flat_map(|(l, d)| (0..3).map(move |k| (l[k], d[k])))
                .map(|(l, d)| {
                    let (l, d) = (l as f64 / 255.0, 1.0 - d as f64 / 255.0);
                    (l.powf(g as f64) - d).powi(2)
                })
                .sum();
            if err < best.0 - 1e-9 {
                best = (err, g);
            }
        }
        best.1
    }

    #[test]
    fn the_per_level_fit_equals_the_direct_one() {
        // Noisy draws, so the answer is not one of the exponents a fixture was built from: a
        // linear-congruential stream of light levels, and dark levels scattered around a curve.
        let mut seed = 0x2545_f491u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        };
        for g in [1.3f32, 2.2, 2.65, 3.7] {
            let (mut light, mut dark) = (Vec::new(), Vec::new());
            for _ in 0..400 {
                let l = next();
                let noise = next() as f32 / 255.0 * 0.2 - 0.1;
                let d = (1.0 - ((l as f32 / 255.0).powf(g) + noise).clamp(0.0, 1.0)) * 255.0;
                light.extend([l, l, l, 255]);
                dark.extend([d.round() as u8; 3]);
                dark.push(255);
            }
            assert_eq!(fit_dark_gamma(&light, &dark), fit_directly(&light, &dark), "g = {g}");
        }
    }

    #[test]
    fn a_mask_replaces_rgb_and_keeps_the_grayscale_alpha() {
        let rgba = vec![255, 255, 255, 90, 255, 255, 255, 0];
        let mask = vec![10, 120, 200, 255, 0, 0, 0, 255];
        assert_eq!(with_lcd(&rgba, &mask), vec![10, 120, 200, 90, 0, 0, 0, 0]);
    }
}
