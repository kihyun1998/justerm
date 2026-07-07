//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour reference with
//! the injected palette and pack per-cell instance data in Rust, then hand one flat
//! buffer to a single instanced draw call.

use crate::color::gl_rgb;
use crate::palette::{Palette, Role, resolve_rgb};

/// Floats per cell instance: `col, row, r, g, b`.
pub const INSTANCE_FLOATS: usize = 5;

/// Pack a grid of background colour references (`cols`×`rows`, row-major) into per-cell
/// instance data `[col, row, r, g, b]`. **Every cell is emitted**, so no cell is left
/// un-drawn — this is what makes #255's GL-default-colour bleed structurally impossible.
///
/// A reference missing from `bg` (short slice) resolves as `Default` (`0`), i.e. the
/// palette's default background.
pub fn pack_bg_instances(cols: u32, rows: u32, bg: &[u32], palette: &Palette) -> Vec<f32> {
    let mut out = Vec::with_capacity(cols as usize * rows as usize * INSTANCE_FLOATS);
    for row in 0..rows {
        for col in 0..cols {
            let idx = (row * cols + col) as usize;
            let reference = bg.get(idx).copied().unwrap_or(0);
            let [r, g, b] = gl_rgb(resolve_rgb(reference, palette, Role::Bg));
            out.extend_from_slice(&[col as f32, row as f32, r, g, b]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn packs_every_cell_with_resolved_bg() {
        // 2×1 grid: cell0 = Rgb(0xE06C75), cell1 = Indexed(1) → colors[1] = 0x00FF00.
        let p = palette();
        let bg = [(2 << 24) | 0xE0_6C_75, (1 << 24) | 1];

        let got = pack_bg_instances(2, 1, &bg, &p);

        // [col,row, r,g,b] per cell, worked by hand from n/255:
        //   cell0 (0,0) 0xE06C75 -> 224,108,117 /255
        //   cell1 (1,0) 0x00FF00 ->   0,255,  0 /255
        let expect = [
            0.0,
            0.0,
            224.0 / 255.0,
            108.0 / 255.0,
            117.0 / 255.0, //
            1.0,
            0.0,
            0.0,
            1.0,
            0.0,
        ];
        assert_eq!(got.len(), expect.len());
        for (i, (a, b)) in got.iter().zip(expect.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "float {i}: got {a}, want {b}");
        }
    }

    #[test]
    fn short_bg_slice_fills_missing_cells_with_default_bg() {
        // A 2×1 grid but only one ref supplied → cell1 falls back to Default (bg).
        let p = palette();
        let got = pack_bg_instances(2, 1, &[(2 << 24) | 0x11_22_33], &p);

        // cell1 = default_bg 0x1E1E2E -> 30,30,46 /255
        assert!((got[7] - 30.0 / 255.0).abs() < 1e-6, "cell1 r={}", got[7]);
        assert!((got[8] - 30.0 / 255.0).abs() < 1e-6);
        assert!((got[9] - 46.0 / 255.0).abs() < 1e-6);
    }
}
