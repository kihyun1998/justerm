//! Minimum-contrast-ratio foreground adjustment (#225/#272) — pure, host-testable.
//!
//! A faithful port of xterm's `rgba.ensureContrastRatio` via justerm-web `contrast.ts`: given a
//! background, a foreground, and a target WCAG contrast ratio, nudge the foreground's luminance
//! (away from the background, in 10% steps) until it meets the ratio, so low-contrast text stays
//! legible. justerm has no alpha, so this works on packed `0xRRGGBB`.
//!
//! Kept **in `f64`** and separate from [`color`](crate::color)'s `f32` `contrast` (the cursor
//! visibility guard, #368, which mirrors alacritty's `vte` metric) — the two mirror *different*
//! references, and the 10%-step loop here must land on the SAME byte as justerm-web so the #273
//! switch is visually neutral. Byte-exactness is pinned by reference vectors computed from the web
//! algorithm (see the tests).

/// WCAG relative luminance of a packed `0xRRGGBB`, channels as `0..=255` (contrast.ts
/// `relativeLuminance2`). `f64` throughout to match the reference to the last step.
fn luminance(rgb: u32) -> f64 {
    let channel = |c: u32| -> f64 {
        let s = c as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel((rgb >> 16) & 0xFF)
        + 0.7152 * channel((rgb >> 8) & 0xFF)
        + 0.0722 * channel(rgb & 0xFF)
}

/// A luminance-adjust step (`reduce_luminance` / `increase_luminance`): `(fg, bg_luminance, ratio) → fg`.
type LumAdjust = fn(u32, f64, f64) -> u32;

/// WCAG contrast ratio between two relative luminances, order-independent (contrast.ts `contrastRatio`).
fn contrast_ratio(l1: f64, l2: f64) -> f64 {
    let (lo, hi) = if l1 < l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Adjust `fg` (packed `0xRRGGBB`) so it meets `ratio` contrast against `bg`, or `None` if the pair
/// already does — `None` lets the caller keep the original fg (and its dim). Faithful to xterm: try
/// the luminance direction away from `bg` first, fall back to the other, keep whichever reaches a
/// higher ratio if neither fully meets it.
pub fn ensure_contrast_ratio(bg: u32, fg: u32, ratio: f64) -> Option<u32> {
    let bg_l = luminance(bg);
    let fg_l = luminance(fg);
    if contrast_ratio(bg_l, fg_l) >= ratio {
        return None;
    }
    // Move away from the background's luminance first; if that direction can't reach the ratio, try
    // the other and keep whichever got closer.
    let (first, second): (LumAdjust, LumAdjust) = if fg_l < bg_l {
        (reduce_luminance, increase_luminance)
    } else {
        (increase_luminance, reduce_luminance)
    };
    let a = first(fg, bg_l, ratio);
    let a_r = contrast_ratio(bg_l, luminance(a));
    if a_r < ratio {
        let b = second(fg, bg_l, ratio);
        let b_r = contrast_ratio(bg_l, luminance(b));
        Some(if a_r > b_r { a } else { b })
    } else {
        Some(a)
    }
}

/// Darken `fg` in 10% steps until it meets `ratio` against a background of luminance `bg_l` (or hits
/// black). `saturating_sub` stands in for the JS `Math.max(0, …)` floor.
fn reduce_luminance(fg: u32, bg_l: f64, ratio: f64) -> u32 {
    let (mut r, mut g, mut b) = ((fg >> 16) & 0xFF, (fg >> 8) & 0xFF, fg & 0xFF);
    while contrast_ratio(luminance((r << 16) | (g << 8) | b), bg_l) < ratio
        && (r > 0 || g > 0 || b > 0)
    {
        r = r.saturating_sub((r as f64 * 0.1).ceil() as u32);
        g = g.saturating_sub((g as f64 * 0.1).ceil() as u32);
        b = b.saturating_sub((b as f64 * 0.1).ceil() as u32);
    }
    (r << 16) | (g << 8) | b
}

/// Lighten `fg` in 10% steps until it meets `ratio` against a background of luminance `bg_l` (or hits
/// white).
fn increase_luminance(fg: u32, bg_l: f64, ratio: f64) -> u32 {
    let (mut r, mut g, mut b) = ((fg >> 16) & 0xFF, (fg >> 8) & 0xFF, fg & 0xFF);
    while contrast_ratio(luminance((r << 16) | (g << 8) | b), bg_l) < ratio
        && (r < 0xFF || g < 0xFF || b < 0xFF)
    {
        r = (r + (((0xFF - r) as f64) * 0.1).ceil() as u32).min(0xFF);
        g = (g + (((0xFF - g) as f64) * 0.1).ceil() as u32).min(0xFF);
        b = (b + (((0xFF - b) as f64) * 0.1).ceil() as u32).min(0xFF);
    }
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference vectors computed by running justerm-web `contrast.ts`'s algorithm in node — the
    // byte-exact outputs the #273 switch must reproduce. NOT recomputed the way this code computes
    // them; a drift in the step loop shows up as a one-byte mismatch here.

    #[test]
    fn returns_none_when_the_pair_already_meets_the_ratio() {
        assert_eq!(
            ensure_contrast_ratio(0x00_00_00, 0xFF_FF_FF, 4.5),
            None,
            "white/black = 21"
        );
        // Mid grey on black already clears 4.5 (this is the dim-white @ mcr/2 case).
        assert_eq!(ensure_contrast_ratio(0x00_00_00, 0x80_80_80, 4.5), None);
    }

    #[test]
    fn lightens_a_too_dark_foreground_on_a_dark_background() {
        assert_eq!(
            ensure_contrast_ratio(0x00_00_00, 0x55_55_55, 4.5),
            Some(0x76_76_76)
        );
        assert_eq!(
            ensure_contrast_ratio(0x00_00_00, 0x00_00_FF, 4.5),
            Some(0x6A_6A_FF)
        );
    }

    #[test]
    fn darkens_a_too_light_foreground_on_a_light_background() {
        assert_eq!(
            ensure_contrast_ratio(0xFF_FF_FF, 0x80_80_80, 4.5),
            Some(0x73_73_73)
        );
    }

    #[test]
    fn adjusts_toward_a_high_ratio_on_a_coloured_background() {
        assert_eq!(
            ensure_contrast_ratio(0x40_00_00, 0xCC_00_00, 7.0),
            Some(0xEC_93_93)
        );
    }

    #[test]
    fn the_result_actually_reaches_the_ratio() {
        // The property, independent of the pinned bytes: a returned colour clears the ratio.
        let bg = 0x1E_1E_2E;
        let fg = 0x2A_2A_3A; // very low contrast
        let adj = ensure_contrast_ratio(bg, fg, 4.5).expect("a low-contrast pair is adjusted");
        assert!(contrast_ratio(luminance(bg), luminance(adj)) >= 4.5);
    }
}
