//! RGB-space colour policy — pure, host-testable (#272). Mirrors justerm-web `render-policy.ts`.
//!
//! The renderer resolves a cell's colour *references* against the injected palette ([`palette`]),
//! then applies the policies that turn them into the colours actually drawn: **inverse** (fg/bg
//! swap), **bold→bright** (#223, a bold ANSI 0–7 fg brightens to 8–15), and **DIM** (#232, the fg
//! fades toward the bg). These are the ref-space + RGB-space transforms `resolve_rgb` deliberately
//! omits; the selection/search highlight ([`overlay`]) composites on top.
//!
//! The **fg long-tail** grows slice by slice (#272 is cumulative). Shipped: bold→bright + dim (here)
//! and `minimumContrastRatio` (#225, the WCAG step-adjust lives in [`contrast`](crate::contrast); the
//! `min_contrast` policy + orchestration are in [`ColorPolicy`] / `pack_instances`). Still to come:
//! the selection-side fg overrides (undim #224, `selectionForeground` #227) and the tile glyph rules
//! (#226/#239/#241).
//!
//! [`palette`]: crate::palette
//! [`overlay`]: crate::overlay

use crate::attrs::{BOLD, is_inverse};
use crate::overlay::blend_over;
use crate::palette::{Palette, Role, resolve_rgb};

/// The consumer-injected RGB-space colour policy (ADR-0017), assembled per pack from the renderer's
/// fields — the wasm-side analog of justerm-web's `Theme` render options. Grows with #272's cumulative
/// slices (a new policy is a new field, not a new `pack_instances` argument).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPolicy {
    /// Draw bold text in the bright (8–15) ANSI colour (#223, xterm's `drawBoldTextInBrightColors`).
    pub bold_to_bright: bool,
    /// Minimum WCAG fg/bg contrast ratio in `[1, 21]` (#225, xterm's `minimumContrastRatio`). Below
    /// it the fg is nudged lighter/darker to stay legible ([`contrast::ensure_contrast_ratio`]).
    /// `1.0` = off (the default, xterm's).
    ///
    /// [`contrast::ensure_contrast_ratio`]: crate::contrast::ensure_contrast_ratio
    pub min_contrast: f32,
}

impl Default for ColorPolicy {
    /// xterm's defaults: bold→bright ON, minimum contrast OFF (`1.0`).
    fn default() -> Self {
        Self {
            bold_to_bright: true,
            min_contrast: 1.0,
        }
    }
}

/// Alpha of the DIM blend (0..=255). xterm's `multiplyOpacity(fg, DIM_OPACITY = 0.5)` sets the fg's
/// alpha byte to `round(0.5 * 255) = 128`, then source-over-composites it over the bg — so the
/// effective fraction is `128/255`, NOT an exact 0.5 (justerm-web `render-policy.ts` `DIM_BLEND_ALPHA`).
/// Numerically equal to the highlight alpha but a distinct policy, so it carries its own constant.
pub const DIM_BLEND_ALPHA: u8 = 0x80;

/// Resolve one reference to packed `0xRRGGBB`, flipping a `Default` ref's fg/bg meaning under inverse
/// (xterm: an inverse `Default` fg draws as the theme bg and vice versa). Mirrors justerm-web
/// `render-policy.ts` `resolveSlot`: a `Default` resolves against `default_fg` iff the slot is a
/// foreground XOR inverse; `Indexed`/`Rgb` are role- and inverse-independent.
fn resolve_slot(reference: u32, palette: &Palette, is_foreground: bool, inverse: bool) -> u32 {
    if reference >> 24 == 0 {
        if is_foreground != inverse {
            palette.default_fg
        } else {
            palette.default_bg
        }
    } else {
        // Role is ignored for Indexed/Rgb; pass Fg arbitrarily.
        resolve_rgb(reference, palette, Role::Fg)
    }
}

/// Resolve a cell's `(fg_ref, bg_ref)` to the packed `0xRRGGBB` colours actually drawn, applying the
/// **ref-space** transforms — inverse (fg/bg swap, with `Default` meaning flipped) and **bold→bright**
/// (#223). Mirrors justerm-web `render-policy.ts` `resolveCell`. Returns `(fg, bg)` already
/// inverse-swapped; the RGB-space transforms (dim, contrast) and the highlight run after.
///
/// bold→bright is applied to the **post-swap** fg reference so that under bold+inverse it brightens
/// the original background index — xterm couples the (mode, index) swap with the `+8`. A bold ANSI
/// `Indexed(0..=7)` foreground becomes its `8..=15` bright variant; `boldToBright` is consumer policy
/// (xterm's `drawBoldTextInBrightColors`, default on).
pub fn resolve_cell(
    fg_ref: u32,
    bg_ref: u32,
    flags: u16,
    palette: &Palette,
    bold_to_bright: bool,
) -> (u32, u32) {
    let inverse = is_inverse(flags);
    // Inverse swaps the slots in REF space (so bright below sees the drawn fg's ref).
    let mut drawn_fg = if inverse { bg_ref } else { fg_ref };
    let drawn_bg = if inverse { fg_ref } else { bg_ref };
    if bold_to_bright && (flags & BOLD) != 0 && (drawn_fg >> 24) == 1 && (drawn_fg & 0xFF) < 8 {
        drawn_fg += 8;
    }
    (
        resolve_slot(drawn_fg, palette, true, inverse),
        resolve_slot(drawn_bg, palette, false, inverse),
    )
}

/// Fade a foreground toward its background for a DIM cell (#232): the same integer blend xterm's
/// `multiplyOpacity` performs — the fg at alpha [`DIM_BLEND_ALPHA`] over the bg. beamterm/justerm
/// bake it into the fg RGB (no per-glyph alpha). Mirrors justerm-web `render-policy.ts` `dimForeground`.
pub fn dim_foreground(fg: u32, bg: u32) -> u32 {
    blend_over(bg, fg, DIM_BLEND_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::INVERSE;

    fn palette() -> Palette {
        let mut colors = [0u32; 256];
        // ANSI 1 = red (dim base), 9 = bright red (the bold→bright target).
        colors[1] = 0xCC_00_00;
        colors[9] = 0xFF_55_55;
        colors[4] = 0x00_00_CC;
        colors[12] = 0x55_55_FF;
        Palette {
            colors,
            default_fg: 0xFF_FF_FF,
            default_bg: 0x1E_1E_2E,
        }
    }

    const IDX: u32 = 1 << 24; // Indexed tag
    const RGB: u32 = 2 << 24; // Rgb tag

    // --- resolve_cell: inverse + bold→bright ---

    #[test]
    fn a_plain_cell_resolves_fg_and_bg_by_role() {
        let p = palette();
        // fg Indexed(1)=red, bg Default → default_bg.
        let (fg, bg) = resolve_cell(IDX | 1, 0, 0, &p, true);
        assert_eq!(fg, 0xCC_00_00);
        assert_eq!(bg, 0x1E_1E_2E);
    }

    #[test]
    fn inverse_swaps_fg_and_bg_and_flips_default_meaning() {
        let p = palette();
        // fg Default, bg Default, INVERSE: shown fg = default_bg, shown bg = default_fg.
        let (fg, bg) = resolve_cell(0, 0, INVERSE, &p, true);
        assert_eq!(fg, 0x1E_1E_2E, "inverse Default fg draws as theme bg");
        assert_eq!(bg, 0xFF_FF_FF, "inverse Default bg draws as theme fg");
    }

    #[test]
    fn bold_brightens_an_ansi_0_7_foreground() {
        let p = palette();
        // Bold + Indexed(1) fg → Indexed(9) = bright red.
        let (fg, _) = resolve_cell(IDX | 1, 0, BOLD, &p, true);
        assert_eq!(fg, 0xFF_55_55, "bold ANSI 1 brightens to ANSI 9");
        // Without bold, no brighten.
        let (fg_plain, _) = resolve_cell(IDX | 1, 0, 0, &p, true);
        assert_eq!(fg_plain, 0xCC_00_00);
        // With the policy off, bold does NOT brighten (xterm's drawBoldTextInBrightColors=false).
        let (fg_off, _) = resolve_cell(IDX | 1, 0, BOLD, &p, false);
        assert_eq!(fg_off, 0xCC_00_00, "policy off keeps the base colour");
    }

    #[test]
    fn bold_bright_only_touches_ansi_0_7_indexed_not_8_15_or_rgb() {
        let p = palette();
        // Indexed(9) is already bright — must not wrap past 15.
        let (fg9, _) = resolve_cell(IDX | 9, 0, BOLD, &p, true);
        assert_eq!(fg9, 0xFF_55_55, "an already-bright index is untouched");
        // An Rgb fg is never remapped.
        let (fgrgb, _) = resolve_cell(RGB | 0x12_34_56, 0, BOLD, &p, true);
        assert_eq!(fgrgb, 0x12_34_56);
    }

    #[test]
    fn bold_bright_under_inverse_brightens_the_swapped_in_background_index() {
        let p = palette();
        // bg Indexed(4)=blue, fg anything; INVERSE makes bg the drawn fg, and bold brightens it to 12.
        let (fg, _) = resolve_cell(0, IDX | 4, BOLD | INVERSE, &p, true);
        assert_eq!(
            fg, 0x55_55_FF,
            "bold+inverse brightens the original bg index (4→12)"
        );
    }

    // --- dim_foreground ---

    #[test]
    fn dim_fades_the_foreground_halfway_to_the_background() {
        // fg white over black bg at alpha 0x80 → mid grey (128 each), the xterm multiplyOpacity result.
        assert_eq!(dim_foreground(0xFF_FF_FF, 0x00_00_00), 0x80_80_80);
        // Dimming toward its own colour is a no-op.
        assert_eq!(dim_foreground(0x40_50_60, 0x40_50_60), 0x40_50_60);
    }
}
