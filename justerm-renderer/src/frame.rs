//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour references with
//! the injected palette, apply inverse, fold the underline/strikethrough decoration into
//! the glyph field, and pack per-cell instance data in Rust — one flat buffer for a single
//! instanced draw call. Glyph-slot resolution (the stateful atlas cache) and rasterisation
//! happen in the browser layer; this packer takes the already-resolved slots + cell flags.

use crate::attrs::{glyph_field, is_inverse};
use crate::color::gl_rgb;
use crate::palette::{Palette, Role, resolve_rgb};

/// Floats per cell instance: `col, row, bg(3), fg(3), glyph_field`.
pub const INSTANCE_FLOATS: usize = 9;

/// Pack a `cols`×`rows` frame (row-major) into per-cell instance data
/// `[col, row, bg_r, bg_g, bg_b, fg_r, fg_g, fg_b, glyph_field]`. `bg`/`fg` are tagged-u32
/// colour references, `slots` the atlas slot per cell, `flags` the `CellFlags` bits.
/// Inverse swaps the resolved fg/bg; underline/strikethrough fold into the glyph field's
/// high bits. **Every cell is emitted** — no cell is left un-drawn (#255). Missing entries
/// resolve as `Default` / slot `0` / no flags.
pub fn pack_instances(
    cols: u32,
    rows: u32,
    bg: &[u32],
    fg: &[u32],
    slots: &[u16],
    flags: &[u16],
    palette: &Palette,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(cols as usize * rows as usize * INSTANCE_FLOATS);
    for row in 0..rows {
        for col in 0..cols {
            let idx = (row * cols + col) as usize;
            let cell_flags = flags.get(idx).copied().unwrap_or(0);

            let bg_rgb = gl_rgb(resolve_rgb(
                bg.get(idx).copied().unwrap_or(0),
                palette,
                Role::Bg,
            ));
            let fg_rgb = gl_rgb(resolve_rgb(
                fg.get(idx).copied().unwrap_or(0),
                palette,
                Role::Fg,
            ));
            // Inverse swaps foreground and background.
            let (bg_rgb, fg_rgb) = if is_inverse(cell_flags) {
                (fg_rgb, bg_rgb)
            } else {
                (bg_rgb, fg_rgb)
            };

            let field = glyph_field(slots.get(idx).copied().unwrap_or(0), cell_flags);

            out.extend_from_slice(&[
                col as f32,
                row as f32,
                bg_rgb[0],
                bg_rgb[1],
                bg_rgb[2],
                fg_rgb[0],
                fg_rgb[1],
                fg_rgb[2],
                field as f32,
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{GLYPH_UNDERLINE, INVERSE, UNDERLINE};

    fn palette() -> Palette {
        let mut colors = [0u32; 256];
        colors[1] = 0x00_FF_00;
        Palette {
            colors,
            default_fg: 0xFF_FF_FF,
            default_bg: 0x1E_1E_2E,
        }
    }

    #[test]
    fn packs_bg_fg_and_glyph_field_per_cell() {
        // cell0: bg Rgb(0xE06C75)=224,108,117 ; fg Default->white ; slot 33, no flags
        let p = palette();
        let got = pack_instances(1, 1, &[(2 << 24) | 0xE0_6C_75], &[0], &[33], &[0], &p);
        let expect = [
            0.0,
            0.0,
            224.0 / 255.0,
            108.0 / 255.0,
            117.0 / 255.0,
            1.0,
            1.0,
            1.0,
            33.0,
        ];
        assert_eq!(got.len(), expect.len());
        for (i, (a, b)) in got.iter().zip(expect.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "float {i}: got {a}, want {b}");
        }
    }

    #[test]
    fn inverse_swaps_the_resolved_fg_and_bg() {
        // bg default (1E1E2E), fg white; INVERSE → bg becomes white, fg becomes 1E1E2E.
        let p = palette();
        let got = pack_instances(1, 1, &[0], &[0], &[0], &[INVERSE], &p);
        assert_eq!(&got[2..5], &[1.0, 1.0, 1.0], "bg is the (swapped-in) fg"); // white
        assert_eq!(got[5], 30.0 / 255.0, "fg is the (swapped-in) default bg"); // 0x1E
    }

    #[test]
    fn underline_flag_folds_into_the_glyph_field() {
        let p = palette();
        let got = pack_instances(1, 1, &[0], &[0], &[33], &[UNDERLINE], &p);
        assert_eq!(got[8], (33 | GLYPH_UNDERLINE) as f32);
    }
}
