//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour references with
//! the injected palette and pack per-cell instance data in Rust, then hand one flat buffer
//! to a single instanced draw call. Glyph-slot resolution (the stateful atlas cache) and
//! rasterisation happen in the browser layer; this packer takes the already-resolved slots.

use crate::color::gl_rgb;
use crate::palette::{Palette, Role, resolve_rgb};

/// Floats per cell instance: `col, row, bg(3), fg(3), glyph_slot`.
pub const INSTANCE_FLOATS: usize = 9;

/// Pack a `cols`×`rows` frame (row-major) into per-cell instance data
/// `[col, row, bg_r, bg_g, bg_b, fg_r, fg_g, fg_b, glyph_slot]`. `bg`/`fg` are tagged-u32
/// colour references (resolved with the injected palette); `slots` is the atlas slot per
/// cell (the caller resolved it via the glyph cache). **Every cell is emitted** — no cell
/// is left un-drawn (#255). A missing entry resolves as `Default` / slot `0` (blank).
pub fn pack_instances(
    cols: u32,
    rows: u32,
    bg: &[u32],
    fg: &[u32],
    slots: &[u16],
    palette: &Palette,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(cols as usize * rows as usize * INSTANCE_FLOATS);
    for row in 0..rows {
        for col in 0..cols {
            let idx = (row * cols + col) as usize;
            let [br, bg_g, bb] = gl_rgb(resolve_rgb(
                bg.get(idx).copied().unwrap_or(0),
                palette,
                Role::Bg,
            ));
            let [fr, fg_g, fb] = gl_rgb(resolve_rgb(
                fg.get(idx).copied().unwrap_or(0),
                palette,
                Role::Fg,
            ));
            let slot = slots.get(idx).copied().unwrap_or(0);
            out.extend_from_slice(&[
                col as f32,
                row as f32,
                br,
                bg_g,
                bb,
                fr,
                fg_g,
                fb,
                slot as f32,
            ]);
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
    fn packs_bg_fg_and_glyph_slot_per_cell() {
        // 2×1 grid:
        //   cell0: bg Rgb(0xE06C75)=224,108,117 ; fg Default->white 255 ; slot 33
        //   cell1: bg Default->1E1E2E=30,30,46  ; fg Indexed(1)->0,255,0 ; slot 2048
        let p = palette();
        let bg = [(2 << 24) | 0xE0_6C_75, 0];
        let fg = [0, (1 << 24) | 1];
        let slots = [33u16, 2048];

        let got = pack_instances(2, 1, &bg, &fg, &slots, &p);

        let expect = [
            // col row  bg r,g,b               fg r,g,b        slot
            0.0,
            0.0,
            224.0 / 255.0,
            108.0 / 255.0,
            117.0 / 255.0,
            1.0,
            1.0,
            1.0,
            33.0, //
            1.0,
            0.0,
            30.0 / 255.0,
            30.0 / 255.0,
            46.0 / 255.0,
            0.0,
            1.0,
            0.0,
            2048.0,
        ];
        assert_eq!(got.len(), expect.len());
        for (i, (a, b)) in got.iter().zip(expect.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "float {i}: got {a}, want {b}");
        }
    }

    #[test]
    fn missing_cells_fall_back_to_default_bg_default_fg_and_slot_zero() {
        // 2×1 grid, only cell0 supplied → cell1 = default bg/fg + blank slot 0.
        let p = palette();
        let got = pack_instances(2, 1, &[(2 << 24) | 0x11_22_33], &[], &[7], &p);

        // cell1 starts at INSTANCE_FLOATS (9).
        assert_eq!(&got[9..11], &[1.0, 0.0]); // col,row
        assert_eq!(got[11], 30.0 / 255.0); // bg default 0x1E
        assert_eq!(got[14], 1.0); // fg default white r
        assert_eq!(got[17], 0.0); // slot 0 (blank)
    }
}
