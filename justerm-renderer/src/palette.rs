//! Colour palette + reference resolution — pure, host-testable.
//!
//! Mirrors justerm-wasm-decode `js/colors.js` `resolveRgb`, kept in lockstep with
//! Rust `justerm_core::encode_color`: a colour reference is a tagged u32 — high byte
//! is the tag (`0` Default, `1` Indexed, `2` Rgb), low 24 bits the payload. The
//! consumer owns the scheme and injects it (ADR-0002); the renderer only resolves.

/// Which default a `Default` reference resolves to.
///
/// `Bg` is currently constructed **only by this module's tests** (#465 surfaced it once the module
/// stopped being `pub` and dead-code analysis switched back on). That is not an oversight to delete:
/// production never reaches [`resolve_rgb`]'s `Default` arm, because [`render_policy::resolve_slot`]
/// intercepts every `Default` reference first — it has to, since inverse flips which default a
/// `Default` reference means. So the same knowledge lives in two places and this parameter is dead on
/// the production path. Untangling that is a design decision tracked as **#470**; kept as-is until
/// then, deliberately, rather than silently reshaping an API a sibling mirrors (`colors.js`
/// `resolveRgb`).
///
/// [`render_policy::resolve_slot`]: crate::render_policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Role {
    Fg,
    Bg,
}

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

/// Resolve a tagged-u32 colour reference to a packed `0xRRGGBB`. Alloc-free — call it
/// per cell. Mirror of `colors.js` `resolveRgb`; does NOT apply inverse/dim/bold→bright
/// (those are render policy applied later).
pub fn resolve_rgb(reference: u32, palette: &Palette, role: Role) -> u32 {
    match reference >> 24 {
        0 => {
            if role == Role::Fg {
                palette.default_fg
            } else {
                palette.default_bg
            }
        }
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

    #[test]
    fn default_resolves_by_role() {
        let p = palette();
        // tag 0 (encode_color(Default) == 0)
        assert_eq!(resolve_rgb(0, &p, Role::Bg), 0x1E_1E_2E);
        assert_eq!(resolve_rgb(0, &p, Role::Fg), 0xFF_FF_FF);
    }

    #[test]
    fn indexed_reads_the_palette_slot() {
        let p = palette();
        // tag 1 (encode_color(Indexed(i)) == (1<<24)|i)
        assert_eq!(resolve_rgb((1 << 24) | 1, &p, Role::Bg), 0x00_FF_00);
        assert_eq!(resolve_rgb((1 << 24) | 200, &p, Role::Fg), 0x0A_0B_0C);
    }

    #[test]
    fn rgb_returns_the_low_24_bits() {
        let p = palette();
        // tag 2 (encode_color(Rgb) == (2<<24)|rgb)
        assert_eq!(
            resolve_rgb((2 << 24) | 0xE0_6C_75, &p, Role::Bg),
            0xE0_6C_75
        );
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
        assert_eq!(resolve_rgb((1 << 24) | 200, &p, Role::Bg), 0x8A_8A_8A);
    }
}
