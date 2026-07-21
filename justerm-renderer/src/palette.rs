//! Colour palette + reference resolution — pure, host-testable.
//!
//! Kept in lockstep with Rust `justerm_core::encode_color` (and justerm-wasm-decode
//! `js/colors.js`): a colour reference is a tagged u32 — high byte is the tag (`0` Default,
//! `1` Indexed, `2` Rgb), low 24 bits the payload. The consumer owns the scheme and injects
//! it (ADR-0002); the renderer only resolves.
//!
//! This module is the *pure lookup* half: reference → RGB for `Indexed`/`Rgb`. The `Default`
//! tag is NOT resolved here — it needs the inverse flag, so it belongs to render policy
//! ([`render_policy::resolve_slot`](crate::render_policy)), which owns it alone (#470).

/// The consumer's frozen colour scheme: 256 indexed colours plus the two defaults,
/// each packed `0xRRGGBB`. Build `colors` once per scheme (the 16 ANSI + 6×6×6 cube +
/// grayscale ramp) — the same layout `justerm_core`/`buildPalette` produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub colors: [u32; 256],
    pub default_fg: u32,
    pub default_bg: u32,
}

/// The injected `colors` slice was not exactly 256 entries.
#[derive(Debug, PartialEq, Eq)]
pub struct PaletteLenError {
    pub got: usize,
}

impl Palette {
    /// Build a palette from the consumer's injected scheme. `colors` MUST be exactly
    /// 256 — the pre-built xterm table (16 ANSI + 6×6×6 cube + grayscale ramp), e.g. the
    /// output of justerm-wasm-decode `buildPalette`. The renderer does NOT synthesise the
    /// cube/grayscale (the family centralises that in the decoder, so a consumer never
    /// re-implements the standard); a wrong length is rejected rather than silently padded
    /// with black — which would resolve `Indexed(16..=255)` to `0x000000`.
    pub fn from_colors(
        colors: &[u32],
        default_fg: u32,
        default_bg: u32,
    ) -> Result<Palette, PaletteLenError> {
        if colors.len() != 256 {
            return Err(PaletteLenError { got: colors.len() });
        }
        let mut arr = [0u32; 256];
        arr.copy_from_slice(colors);
        Ok(Palette {
            colors: arr,
            default_fg,
            default_bg,
        })
    }
}

/// Resolve an `Indexed`/`Rgb` reference to a packed `0xRRGGBB`. Alloc-free — call it per cell.
/// Role- and inverse-independent, and deliberately **not** total over the tag space: a `Default`
/// reference does not belong here (#470).
///
/// Which default a `Default` resolves to depends on inverse — an inverse `Default` fg draws as the
/// theme *bg* — so that decision needs render policy, and its single home is
/// [`render_policy::resolve_slot`](crate::render_policy), which intercepts tag 0 before calling
/// this. Answering it here too would put the same knowledge in two places with the compiler
/// enforcing neither; the `role` parameter that used to do so was dead on every production path.
///
/// The published JS sibling `colors.js` `resolveRgb` keeps its `role` parameter and its `Default`
/// arm — correctly: it is a package **API** any npm consumer may call with an arbitrary reference,
/// whereas this is a crate-private helper (`pub(crate)` since #465, npm-only crate) with one caller.
/// justerm-web's `render-policy.ts` `resolveSlot` splits the same way, for the same reason.
///
/// # Panics (debug only)
/// Debug-asserts the reference is not `Default`. In release a tag-0 reference would fall through to
/// the `Rgb` arm and read as `0x000000`, so the assert is what keeps that silent-black case a test
/// failure rather than a rendering mystery.
pub fn resolve_indexed_or_rgb(reference: u32, palette: &Palette) -> u32 {
    debug_assert_ne!(
        reference >> 24,
        0,
        "a Default reference must be resolved by render_policy::resolve_slot (#470)"
    );
    match reference >> 24 {
        1 => palette.colors[(reference & 0xFF) as usize],
        _ => reference & 0xFF_FFFF,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let mut colors = [0u32; 256];
        colors[1] = 0x00_FF_00; // a distinctive indexed slot
        colors[200] = 0x0A_0B_0C;
        Palette {
            colors,
            default_fg: 0xFF_FF_FF,
            default_bg: 0x1E_1E_2E,
        }
    }

    // The `Default` tag has no test here on purpose: this function does not answer it (#470).
    // Its behaviour is asserted where it is decided — `render_policy::resolve_slot`, which is the
    // only path production takes a tag-0 reference through.

    #[test]
    fn indexed_reads_the_palette_slot() {
        let p = palette();
        // tag 1 (encode_color(Indexed(i)) == (1<<24)|i)
        assert_eq!(resolve_indexed_or_rgb((1 << 24) | 1, &p), 0x00_FF_00);
        assert_eq!(resolve_indexed_or_rgb((1 << 24) | 200, &p), 0x0A_0B_0C);
    }

    #[test]
    fn rgb_returns_the_low_24_bits() {
        let p = palette();
        // tag 2 (encode_color(Rgb) == (2<<24)|rgb)
        assert_eq!(
            resolve_indexed_or_rgb((2 << 24) | 0xE0_6C_75, &p),
            0xE0_6C_75
        );
    }

    #[test]
    #[should_panic(expected = "resolved by render_policy::resolve_slot")]
    fn a_default_reference_is_rejected_rather_than_silently_black() {
        // Guards the release-mode fall-through: without the debug assert, tag 0 would take the Rgb
        // arm and resolve to 0x000000 — a plausible-looking colour, so the bug would surface as a
        // rendering mystery instead of a test failure.
        let _ = resolve_indexed_or_rgb(0, &palette());
    }

    #[test]
    fn from_colors_requires_exactly_256() {
        // Passing the 16 ANSI colours (the buildPalette *input* shape) must be rejected,
        // not silently padded with black for slots 16..255 (the footgun the 2-lens caught).
        let sixteen = vec![0u32; 16];
        assert_eq!(
            Palette::from_colors(&sixteen, 0xFFFFFF, 0x1E1E2E),
            Err(PaletteLenError { got: 16 })
        );
        // A pre-built 256-entry table is accepted and copied verbatim.
        let mut full = vec![0u32; 256];
        full[200] = 0x8A_8A_8A;
        let p = Palette::from_colors(&full, 0xFFFFFF, 0x1E1E2E).expect("256 is valid");
        assert_eq!(p.colors[200], 0x8A_8A_8A);
        assert_eq!(resolve_indexed_or_rgb((1 << 24) | 200, &p), 0x8A_8A_8A);
    }
}
