//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour references with
//! the injected palette + colour policy (inverse, bold→bright, dim — [`render_policy`]), composite
//! the selection/search highlight ([`overlay`]), fold the underline/strikethrough decoration into
//! the glyph field, and pack per-cell instance data in Rust — one flat buffer for a single
//! instanced draw call. Glyph-slot resolution (the stateful atlas cache) and rasterisation
//! happen in the browser layer; this packer takes the already-resolved slots + cell flags.
//!
//! [`render_policy`]: crate::render_policy
//! [`overlay`]: crate::overlay

use crate::attrs::{BLANK_SLOT, glyph_field, is_concealed, is_dim};
use crate::color::gl_rgb;
use crate::overlay::{Overlay, composite_bg, should_blend};
use crate::palette::Palette;
use crate::render_policy::{ColorPolicy, dim_foreground, resolve_cell};

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
/// `[col, row, bg_r, bg_g, bg_b, fg_r, fg_g, fg_b, glyph_field]`. Colours resolve through the
/// injected `policy` ([`resolve_cell`]): inverse swaps the fg/bg, a bold ANSI 0–7 fg brightens to
/// 8–15 (#223), and a DIM cell's fg fades toward its bg (#232, [`dim_foreground`]).
/// Underline/strikethrough fold into the glyph field's high bits. A *concealed* cell —
/// hidden (`ESC[8m`), or blink with `blink_on == false` — collapses to the blank slot
/// ([`BLANK_SLOT`]) so only its (inverse-aware) background shows; `blink_on` is the render
/// loop's phase, driven by the consumer (timing is policy, #282). **Every cell is emitted** —
/// no cell is left un-drawn (#255).
///
/// A cell covered by the selection / search `overlay` (#271) has its resolved background composited
/// with the injected highlight colour — blended over a non-default / inverse cell, painted solid over
/// the default background ([`composite_bg`]). Compositing happens in packed colour space, before the
/// `gl_rgb` unpack, so the blend matches the web reference to the byte; and because the whole viewport
/// re-packs each frame and the #263 upload diff re-sends only changed cells, a cell that gains or loses
/// a highlight re-uploads with no extra bookkeeping (unlike beamterm's overlay delta).
pub fn pack_instances(
    frame: &Frame,
    palette: &Palette,
    blink_on: bool,
    overlay: &Overlay,
    policy: &ColorPolicy,
) -> Vec<f32> {
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

            // Resolve refs → packed 0xRRGGBB, applying inverse + bold→bright (#223), and keep the
            // values packed through dim and the #271 highlight composite, unpacking to gl floats only
            // at the end — the blends are integer math (match the web reference to the byte).
            let bg_ref = bg.get(idx).copied().unwrap_or(0);
            let fg_ref = fg.get(idx).copied().unwrap_or(0);
            let (fg_packed, bg_packed) =
                resolve_cell(fg_ref, bg_ref, cell_flags, palette, policy.bold_to_bright);
            // #232 DIM: fade the fg toward its (pre-highlight) bg. A selection undims the fg (#224) —
            // a later #272 slice; until then a selected dim cell stays dimmed (cumulative tail).
            let fg_packed = if is_dim(cell_flags) {
                dim_foreground(fg_packed, bg_packed)
            } else {
                fg_packed
            };
            // #271: composite the selection / search highlight onto the (post-swap) background.
            // `should_blend` reads the PRE-inverse ref + flags: an inverse or non-default cell blends
            // so its own colour shows through, a plain default-bg cell paints solid.
            let bg_packed = composite_bg(
                bg_packed,
                should_blend(bg_ref, cell_flags),
                overlay.color_at(row, col),
            );
            let bg_rgb = gl_rgb(bg_packed);
            let fg_rgb = gl_rgb(fg_packed);

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
    use crate::overlay::{HIGHLIGHT_BLEND_ALPHA, HighlightColors, blend_over};
    use crate::render_policy::ColorPolicy;

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
            &Overlay::default(),
            &ColorPolicy::default(),
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
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[INVERSE]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        assert_eq!(&got[2..5], &[1.0, 1.0, 1.0], "bg is the (swapped-in) fg"); // white
        assert_eq!(got[5], 30.0 / 255.0, "fg is the (swapped-in) default bg"); // 0x1E
    }

    #[test]
    fn underline_flag_folds_into_the_glyph_field() {
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[33], &[UNDERLINE]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
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
            &Overlay::default(),
            &ColorPolicy::default(),
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
        let got = pack_instances(
            &frame(&[0], &[0], &[33], &[HIDDEN | INVERSE]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
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
        let on = pack_instances(
            &frame(&[0], &[0], &[33], &[BLINK]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        assert_eq!(on[8], 33.0, "blink-on draws the real glyph");
        // Blink phase OFF: the glyph is concealed to the blank slot.
        let off = pack_instances(
            &frame(&[0], &[0], &[33], &[BLINK]),
            &p,
            false,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
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
        let got = pack_instances(&f, &p, true, &Overlay::default(), &ColorPolicy::default());
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
        let on = pack_instances(
            &frame(&[0], &[0], &[33], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        let off = pack_instances(
            &frame(&[0], &[0], &[33], &[0]),
            &p,
            false,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        assert_eq!(on, off);
        assert_eq!(on[8], 33.0);
    }

    // --- #271: selection / search overlay compositing into the packed background ---

    const SEL_BG: u32 = 0x3A_3D_5C;
    const MATCH_BG: u32 = 0x5C_3A_3A;

    /// One-cell overlay covering (row 0, col 0) as the given group.
    fn selected<'a>(selection: &'a [u32], matches: &'a [u32]) -> Overlay<'a> {
        Overlay {
            selection,
            matches,
            colors: HighlightColors {
                selection_bg: SEL_BG,
                match_bg: MATCH_BG,
            },
        }
    }

    #[test]
    fn a_selected_default_bg_cell_is_painted_the_solid_selection_colour() {
        // bg Default (ref 0), no inverse → solid: the highlight replaces the bg outright.
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &selected(&[0, 0, 0], &[]),
            &ColorPolicy::default(),
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(SEL_BG),
            "solid selection over the default bg"
        );
    }

    #[test]
    fn a_selected_coloured_cell_blends_so_its_own_colour_shows_through() {
        // bg Rgb(0xE06C75): a non-default cell blends the selection over its own colour (#115).
        let p = palette();
        let cell_bg = 0xE0_6C_75;
        let got = pack_instances(
            &frame(&[(2 << 24) | cell_bg], &[0], &[0], &[0]),
            &p,
            true,
            &selected(&[0, 0, 0], &[]),
            &ColorPolicy::default(),
        );
        let expect = gl_rgb(blend_over(cell_bg, SEL_BG, HIGHLIGHT_BLEND_ALPHA));
        assert_eq!(
            &got[2..5],
            &expect,
            "selection blended over the cell colour"
        );
        // ...and it is NOT the solid colour — proves the blend branch really ran.
        assert_ne!(
            &got[2..5],
            &gl_rgb(SEL_BG),
            "a coloured cell must not paint solid"
        );
    }

    #[test]
    fn an_inverse_default_cell_blends_over_its_swapped_in_background() {
        // Inverse makes the shown bg the swapped-in fg (white here). should_blend is true for an
        // inverse cell even with a Default bg ref, so the selection blends over that white — not solid.
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[INVERSE]),
            &p,
            true,
            &selected(&[0, 0, 0], &[]),
            &ColorPolicy::default(),
        );
        // Post-swap bg is the default fg (white, 0xFFFFFF); the selection blends over it.
        let expect = gl_rgb(blend_over(0xFF_FF_FF, SEL_BG, HIGHLIGHT_BLEND_ALPHA));
        assert_eq!(&got[2..5], &expect, "inverse cell blends over its shown bg");
        assert_ne!(&got[2..5], &gl_rgb(SEL_BG), "inverse never paints solid");
    }

    #[test]
    fn a_search_match_uses_the_match_colour_not_the_selection_colour() {
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &selected(&[], &[0, 0, 0]),
            &ColorPolicy::default(),
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(MATCH_BG),
            "a match paints its own colour"
        );
    }

    #[test]
    fn a_cell_the_overlay_does_not_cover_keeps_its_background() {
        // The control: with the span one column to the right, the cell is untouched — so the tests
        // above are asserting the highlight, not just any repaint.
        let p = palette();
        let cell_bg = 0xE0_6C_75;
        let got = pack_instances(
            &frame(&[(2 << 24) | cell_bg], &[0], &[0], &[0]),
            &p,
            true,
            &selected(&[0, 1, 1], &[]), // covers (0,1), not (0,0)
            &ColorPolicy::default(),
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(cell_bg),
            "an uncovered cell is unchanged"
        );
    }

    // --- #272 slice 1: bold→bright (#223) + DIM (#232) wired through pack_instances ---

    use crate::attrs::{BOLD, DIM};

    /// A palette with a dim/bright ANSI pair so bold→bright is observable end-to-end.
    fn bright_palette() -> Palette {
        let mut colors = [0u32; 256];
        colors[1] = 0xCC_00_00; // ANSI 1, red
        colors[9] = 0xFF_55_55; // ANSI 9, bright red
        Palette {
            colors,
            default_fg: 0xFF_FF_FF,
            default_bg: 0x00_00_00,
        }
    }

    const IDX: u32 = 1 << 24;

    #[test]
    fn a_bold_indexed_cell_packs_the_bright_foreground_when_the_policy_is_on() {
        let p = bright_palette();
        // fg Indexed(1), BOLD, default policy (bold→bright ON) → the packed fg is ANSI 9.
        let got = pack_instances(
            &frame(&[0], &[IDX | 1], &[0], &[BOLD]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(0xFF_55_55),
            "bold ANSI 1 packs as bright red"
        );
        // ...and it is NOT the base colour — proves the remap actually ran.
        assert_ne!(&got[5..8], &gl_rgb(0xCC_00_00));
    }

    #[test]
    fn the_bold_to_bright_policy_off_packs_the_base_foreground() {
        let p = bright_palette();
        let got = pack_instances(
            &frame(&[0], &[IDX | 1], &[0], &[BOLD]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy {
                bold_to_bright: false,
            },
        );
        assert_eq!(&got[5..8], &gl_rgb(0xCC_00_00), "policy off keeps ANSI 1");
    }

    #[test]
    fn a_dim_cell_packs_a_foreground_faded_toward_its_background() {
        let p = bright_palette();
        // fg white (default), DIM, over black default bg → the packed fg is the dim blend, not white.
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[DIM]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        let expect = gl_rgb(crate::render_policy::dim_foreground(0xFF_FF_FF, 0x00_00_00));
        assert_eq!(&got[5..8], &expect, "dim fades the fg toward the bg");
        assert_ne!(
            &got[5..8],
            &gl_rgb(0xFF_FF_FF),
            "a dim cell is not full-brightness"
        );
    }

    #[test]
    fn bold_and_dim_compose_brighten_then_fade() {
        // BOLD + DIM together: bold→bright first (ANSI 1 → 9), THEN dim fades that bright colour
        // toward the bg — xterm's order (resolveCell brightens, the RGB policy dims). Pins the
        // composition, not each transform alone.
        let p = bright_palette();
        let got = pack_instances(
            &frame(&[0], &[IDX | 1], &[0], &[BOLD | DIM]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        let expect = gl_rgb(crate::render_policy::dim_foreground(0xFF_55_55, 0x00_00_00));
        assert_eq!(
            &got[5..8],
            &expect,
            "bold brightens, then dim fades the bright colour"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(0xFF_55_55),
            "the bright colour is dimmed, not drawn raw"
        );
    }
}
