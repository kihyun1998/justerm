//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour references with
//! the injected palette, apply inverse, fold the underline/strikethrough decoration into
//! the glyph field, and pack per-cell instance data in Rust — one flat buffer for a single
//! instanced draw call. Glyph-slot resolution (the stateful atlas cache) and rasterisation
//! happen in the browser layer; this packer takes the already-resolved slots + cell flags.

use crate::attrs::{BLANK_SLOT, glyph_field, is_concealed, is_inverse};
use crate::color::gl_rgb;
use crate::palette::{Palette, Role, resolve_rgb};

/// Floats per cell instance: `col, row, bg(3), fg(3), glyph_field`.
pub const INSTANCE_FLOATS: usize = 9;

/// A decoded frame's per-cell grid: dimensions + the four parallel column arrays the packer
/// reads (all row-major, ideally length `cols*rows`). `bg`/`fg` are tagged-u32 colour refs,
/// `slots` the resolved atlas slot per cell, `flags` the `CellFlags` bits. A short/missing
/// entry resolves as `Default` / slot `0` / no flags — every cell is still emitted (#255).
pub struct Frame<'a> {
    pub cols: u32,
    pub rows: u32,
    pub bg: &'a [u32],
    pub fg: &'a [u32],
    pub slots: &'a [u16],
    pub flags: &'a [u16],
}

/// Pack a [`Frame`] (row-major) into per-cell instance data
/// `[col, row, bg_r, bg_g, bg_b, fg_r, fg_g, fg_b, glyph_field]`. Inverse swaps the resolved
/// fg/bg; underline/strikethrough fold into the glyph field's high bits. A *concealed* cell —
/// hidden (`ESC[8m`), or blink with `blink_on == false` — collapses to the blank slot
/// ([`BLANK_SLOT`]) so only its (inverse-aware) background shows; `blink_on` is the render
/// loop's phase, driven by the consumer (timing is policy, #282). **Every cell is emitted** —
/// no cell is left un-drawn (#255).
pub fn pack_instances(frame: &Frame, palette: &Palette, blink_on: bool) -> Vec<f32> {
    let Frame {
        cols,
        rows,
        bg,
        fg,
        slots,
        flags,
    } = *frame;
    // `usize` is 32 bits on wasm32, so `cells * 9` overflows it for a grid `resolve_frame` would
    // still accept (it only bounds `cells` by the slice length). A failed reservation is not worth
    // an abort: fall back to growing on demand — the loop below is bounded by `rows * cols` either
    // way, and `resolve_frame` has already refused a grid its slices cannot back (#355).
    let cells = (cols as usize).saturating_mul(rows as usize);
    let mut out = Vec::with_capacity(cells.checked_mul(INSTANCE_FLOATS).unwrap_or(0));
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
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

            // A concealed cell points at the blank slot: zero coverage, no decoration bits,
            // so only the (already inverse-swapped) background shows.
            let field = if is_concealed(cell_flags, blink_on) {
                BLANK_SLOT
            } else {
                glyph_field(slots.get(idx).copied().unwrap_or(0), cell_flags)
            };

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
    use crate::attrs::{
        BLANK_SLOT, BLINK, GLYPH_UNDERLINE, HIDDEN, INVERSE, STRIKETHROUGH, UNDERLINE,
    };

    fn palette() -> Palette {
        let mut colors = [0u32; 256];
        colors[1] = 0x00_FF_00;
        Palette {
            colors,
            default_fg: 0xFF_FF_FF,
            default_bg: 0x1E_1E_2E,
        }
    }

    /// A single-cell frame for the packer tests.
    fn frame<'a>(bg: &'a [u32], fg: &'a [u32], slots: &'a [u16], flags: &'a [u16]) -> Frame<'a> {
        Frame {
            cols: 1,
            rows: 1,
            bg,
            fg,
            slots,
            flags,
        }
    }

    #[test]
    fn packs_bg_fg_and_glyph_field_per_cell() {
        // cell0: bg Rgb(0xE06C75)=224,108,117 ; fg Default->white ; slot 33, no flags
        let p = palette();
        let got = pack_instances(
            &frame(&[(2 << 24) | 0xE0_6C_75], &[0], &[33], &[0]),
            &p,
            true,
        );
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
        let got = pack_instances(&frame(&[0], &[0], &[0], &[INVERSE]), &p, true);
        assert_eq!(&got[2..5], &[1.0, 1.0, 1.0], "bg is the (swapped-in) fg"); // white
        assert_eq!(got[5], 30.0 / 255.0, "fg is the (swapped-in) default bg"); // 0x1E
    }

    #[test]
    fn underline_flag_folds_into_the_glyph_field() {
        let p = palette();
        let got = pack_instances(&frame(&[0], &[0], &[33], &[UNDERLINE]), &p, true);
        assert_eq!(got[8], (33 | GLYPH_UNDERLINE) as f32);
    }

    #[test]
    fn hidden_cell_renders_background_only() {
        // A hidden cell with a real slot + underline: the glyph field collapses to the blank
        // slot (0) — no coverage, no decoration — so only the background shows.
        let p = palette();
        let got = pack_instances(
            &frame(
                &[(2 << 24) | 0xE0_6C_75], // bg Rgb(224,108,117)
                &[0],                      // fg default (white)
                &[33],                     // a real glyph slot
                &[HIDDEN | UNDERLINE | STRIKETHROUGH],
            ),
            &p,
            true,
        );
        assert_eq!(got[8], BLANK_SLOT as f32, "glyph field is the blank slot");
        // Background is untouched (only the glyph is concealed).
        assert_eq!(&got[2..5], &[224.0 / 255.0, 108.0 / 255.0, 117.0 / 255.0]);
    }

    #[test]
    fn hidden_with_inverse_shows_the_swapped_background() {
        // INVERSE swaps fg/bg first, then HIDDEN conceals the glyph — the cell is a solid
        // block of the (swapped-in) foreground, matching alacritty's fg==bg conceal model.
        let p = palette();
        let got = pack_instances(&frame(&[0], &[0], &[33], &[HIDDEN | INVERSE]), &p, true);
        assert_eq!(got[8], BLANK_SLOT as f32);
        assert_eq!(
            &got[2..5],
            &[1.0, 1.0, 1.0],
            "bg is the swapped-in fg (white)"
        );
    }

    #[test]
    fn blink_cell_is_concealed_only_on_the_off_phase() {
        let p = palette();
        // Blink phase ON: the glyph draws normally.
        let on = pack_instances(&frame(&[0], &[0], &[33], &[BLINK]), &p, true);
        assert_eq!(on[8], 33.0, "blink-on draws the real glyph");
        // Blink phase OFF: the glyph is concealed to the blank slot.
        let off = pack_instances(&frame(&[0], &[0], &[33], &[BLINK]), &p, false);
        assert_eq!(off[8], BLANK_SLOT as f32, "blink-off conceals the glyph");
    }

    #[test]
    fn hidden_wide_char_conceals_both_halves() {
        // A concealed wide glyph must hide BOTH cells — the lead and its spacer. This is only
        // correct because justerm-core stamps the same SGR flags onto a wide char's lead and
        // spacer (one pen, `term.rs::write_glyph`); pin that cross-crate contract at the
        // renderer boundary so a core regression that stopped propagating HIDDEN to the spacer
        // would surface here instead of leaking the glyph's right half.
        use crate::attrs::{WIDE_CHAR, WIDE_CHAR_SPACER};
        let p = palette();
        let f = Frame {
            cols: 2,
            rows: 1,
            bg: &[0, 0],
            fg: &[0, 0],
            slots: &[2048, 2049], // wide lead + its right-half slot
            flags: &[HIDDEN | WIDE_CHAR, HIDDEN | WIDE_CHAR_SPACER],
        };
        let got = pack_instances(&f, &p, true);
        assert_eq!(got[8], BLANK_SLOT as f32, "lead half concealed");
        assert_eq!(
            got[8 + INSTANCE_FLOATS],
            BLANK_SLOT as f32,
            "spacer (right) half concealed too — no leak"
        );
    }

    #[test]
    fn non_blink_cell_is_unaffected_by_the_blink_phase() {
        // A plain cell renders identically regardless of the blink phase.
        let p = palette();
        let on = pack_instances(&frame(&[0], &[0], &[33], &[0]), &p, true);
        let off = pack_instances(&frame(&[0], &[0], &[33], &[0]), &p, false);
        assert_eq!(on, off);
        assert_eq!(on[8], 33.0);
    }
}
