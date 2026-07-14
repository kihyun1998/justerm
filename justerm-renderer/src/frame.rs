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

use crate::attrs::{BLANK_SLOT, glyph_field, is_concealed, is_dim, is_inverse};
use crate::color::gl_rgb;
use crate::contrast::ensure_contrast_ratio;
use crate::glyph_class::treat_glyph_as_background_color;
use crate::overlay::{
    HIGHLIGHT_BLEND_ALPHA, HighlightKind, Overlay, blend_over, composite_bg, should_blend,
};
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
    /// Per-cell **base** codepoint — the first scalar of the resolved glyph. Only the tile-glyph
    /// classifier ([`treat_glyph_as_background_color`], #226/#239) reads it; a short/missing entry
    /// resolves as `0` (not a tile). The atlas *slot* is already resolved in `slots`; this is kept
    /// solely to classify the glyph's contrast/selection behaviour.
    pub codepoints: &'a [u32],
}

/// Pack a [`Frame`] (row-major) into per-cell instance data
/// `[col, row, bg_r, bg_g, bg_b, fg_r, fg_g, fg_b, glyph_field]`. Colours resolve through the injected
/// `policy`: inverse swaps the fg/bg and a bold ANSI 0–7 fg brightens to 8–15 ([`resolve_cell`], #223);
/// a DIM cell's fg fades toward its bg (#232, [`dim_foreground`]); `minimumContrastRatio` nudges an
/// illegible fg (#225); and a *selected* cell takes `selectionForeground` (#227) and has its DIM
/// cleared (#224) — the selection-side rules key off the [`overlay`](crate::overlay).
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
        codepoints,
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

            // Resolve refs → packed 0xRRGGBB, applying inverse + bold→bright (#223); keep the values
            // packed through the highlight composite + the fg policy, unpacking to gl floats only at the
            // end — the blends are integer math (match the reference to the byte).
            let bg_ref = bg.get(idx).copied().unwrap_or(0);
            let fg_ref = fg.get(idx).copied().unwrap_or(0);
            // The cell's OWN resolved fg (inverse + bold→bright) — kept undimmed for the tile-glyph
            // re-tint below, which starts from it (discarding any selectionForeground).
            let (cell_fg, cell_bg) =
                resolve_cell(fg_ref, bg_ref, cell_flags, palette, policy.bold_to_bright);
            // The highlight covering this cell (if any) and whether it is a *selection* (vs a search
            // match) — the selection-only fg rules (#224/#227/#239) key off this.
            let kind = overlay.highlight_at(row, col);
            let is_selection = kind == Some(HighlightKind::Selection);
            // #226: a Powerline / box-drawing / block glyph tiles with the bg — excluded from the
            // contrast demand and re-tinted under selection (classify the base codepoint).
            let exclude =
                treat_glyph_as_background_color(codepoints.get(idx).copied().unwrap_or(0));
            // #227 selectionForeground: force a selected cell's fg to the injected colour (never a
            // match), overriding the cell's own fg. A tile glyph discards it (#239 below).
            let mut fg = match (is_selection, policy.selection_fg) {
                (true, Some(sfg)) => sfg,
                _ => cell_fg,
            };
            // #271: composite the selection / search highlight onto the (post-swap) background FIRST,
            // so the fg policy sees the EFFECTIVE bg the glyph is drawn over. `should_blend` reads the
            // PRE-inverse ref + flags: an inverse or non-default cell blends so its own colour shows
            // through, a plain default-bg cell paints solid.
            let eff_bg = composite_bg(
                cell_bg,
                should_blend(bg_ref, cell_flags),
                kind.map(|k| overlay.colors.of(k)),
            );
            // #239/#241: a tile glyph under a SELECTION fuses into the band — xterm re-tints it toward
            // the RAW selection colour (not the effective post-blend bg), starting from the cell's own
            // undimmed fg and discarding selectionForeground. #241: an inverse cell with a DEFAULT bg
            // is "treated as transparent" — its fg becomes the raw selection colour with NO blend.
            //
            // The re-tint starts from `cell_fg`, which has bold→bright (#223) applied — byte-identical
            // to justerm-web (its `fgUndimmed` is likewise brightened), so the #273 switch stays
            // neutral. This diverges from xterm ONLY for a BOLD + ANSI-0..7 + tile + selection cell,
            // where xterm re-tints from the *base* ANSI colour (its `CellColorResolver` bypasses the
            // `+8`): a corner-of-corner, sub-perceptible under the 0x80 blend. 100 % xterm parity here
            // is a *family* change (web + renderer together) — tracked, not smuggled into this slice.
            if is_selection && exclude {
                let raw_sel = overlay.colors.of(HighlightKind::Selection);
                let inverse_default_bg = is_inverse(cell_flags) && (bg_ref >> 24) == 0;
                fg = if inverse_default_bg {
                    raw_sel
                } else {
                    blend_over(cell_fg, raw_sel, HIGHLIGHT_BLEND_ALPHA)
                };
            }
            // The fg colour policy, applied ONCE against the effective bg on the UNDIMMED fg — xterm's
            // model (`TextureAtlas._getMinimumContrastColor`), which the renderer can follow because the
            // highlight is already folded into `eff_bg` (beamterm couldn't, so justerm-web double-passes
            // — a compromise the renderer sheds; the #272 2-lens pinned this). minimumContrastRatio
            // (#225) is checked FIRST: if it fires, the corrected fg wins and DIM is skipped (mutually
            // exclusive, xterm `TextureAtlas.ts:329`); a dim cell that already clears the halved ratio is
            // dimmed instead (#232). #226: a tile glyph is EXCLUDED from the contrast demand.
            // #224 selection un-dim: a *selected* cell's DIM is cleared (xterm `& ~BgFlags.DIM`), so its
            // text stays legible over the highlight and the contrast ratio is NOT halved. `dim` folding
            // in `!is_selection` handles both.
            let dim = is_dim(cell_flags) && !is_selection;
            let mcr = policy.min_contrast as f64;
            let fg_packed = if mcr > 1.0 && !exclude {
                let ratio = if dim { mcr / 2.0 } else { mcr };
                match ensure_contrast_ratio(eff_bg, fg, ratio) {
                    Some(adjusted) => adjusted,
                    None if dim => dim_foreground(fg, eff_bg),
                    None => fg,
                }
            } else if dim {
                dim_foreground(fg, eff_bg)
            } else {
                fg
            };
            let bg_rgb = gl_rgb(eff_bg);
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

    /// A single-cell frame for the packer tests. Codepoints default to empty (→ classified as
    /// non-tile); the tile tests build a [`Frame`] with an explicit `codepoints` column instead.
    fn frame<'a>(bg: &'a [u32], fg: &'a [u32], slots: &'a [u16], flags: &'a [u16]) -> Frame<'a> {
        Frame {
            cols: 1,
            rows: 1,
            bg,
            fg,
            slots,
            flags,
            codepoints: &[],
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
            codepoints: &[0x4E2D, 0],
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
                ..ColorPolicy::default()
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

    // --- #272 slice 2: minimumContrastRatio (#225) wired through pack_instances ---

    const RGB: u32 = 2 << 24;

    #[test]
    fn minimum_contrast_corrects_a_low_contrast_foreground() {
        let p = palette(); // default_bg 0x1E1E2E
        // fg Rgb 0x2A2A3A — barely above the bg, well under any real ratio. mcr 7 nudges it legible.
        let policy = ColorPolicy {
            min_contrast: 7.0,
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame(&[0], &[RGB | 0x2A_2A_3A], &[0], &[0]),
            &p,
            true,
            &Overlay::default(),
            &policy,
        );
        let expect = gl_rgb(ensure_contrast_ratio(0x1E_1E_2E, 0x2A_2A_3A, 7.0).unwrap());
        assert_eq!(
            &got[5..8],
            &expect,
            "the fg is corrected against the cell bg"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(0x2A_2A_3A),
            "a low-contrast fg does not pass through"
        );
    }

    #[test]
    fn minimum_contrast_off_leaves_even_an_illegible_foreground() {
        let p = palette();
        // The control: default policy (mcr = 1.0, off) draws the low-contrast fg verbatim.
        let got = pack_instances(
            &frame(&[0], &[RGB | 0x2A_2A_3A], &[0], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(0x2A_2A_3A),
            "mcr off passes the fg through"
        );
    }

    #[test]
    fn the_contrast_pass_runs_against_the_effective_highlight_bg() {
        // A white fg is fine on the dark cell bg (stage-2 no-op), but a light SELECTION highlight
        // makes it illegible — the overlay contrast pass must re-correct against the *effective* bg.
        let p = bright_palette(); // default_bg black, default_fg white
        let policy = ColorPolicy {
            min_contrast: 4.5,
            ..ColorPolicy::default()
        };
        let sel_bg = 0xCC_CC_CC; // light grey selection → white-on-grey is low contrast
        let overlay = Overlay {
            selection: &[0, 0, 0],
            matches: &[],
            colors: crate::overlay::HighlightColors {
                selection_bg: sel_bg,
                match_bg: 0,
            },
        };
        // fg Default (white); bg Default → the highlight paints SOLID grey (default-bg cell).
        let got = pack_instances(&frame(&[0], &[0], &[0], &[0]), &p, true, &overlay, &policy);
        let expect = gl_rgb(ensure_contrast_ratio(sel_bg, 0xFF_FF_FF, 4.5).unwrap());
        assert_eq!(
            &got[5..8],
            &expect,
            "fg corrected against the selection bg, not the cell bg"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(0xFF_FF_FF),
            "white is darkened on the light highlight"
        );
    }

    #[test]
    fn a_dim_cell_that_fails_contrast_is_corrected_not_dimmed_mutual_exclusion() {
        // The stage-2 rule (render-policy.ts makeRenderPolicy): if minimumContrastRatio fires, its
        // corrected fg wins and DIM is SKIPPED. Only this case makes stage-2 distinct from the overlay
        // pass — without it a dim cell would be dimmed FIRST and then corrected from the dimmer colour,
        // a different result. A dim low-contrast fg (0x2A2A3A on 0x1E1E2E) fails mcr/2, so it must be
        // corrected from its UNDIMMED value.
        let p = palette();
        let policy = ColorPolicy {
            min_contrast: 7.0, // dim halves it to 3.5
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame(&[0], &[RGB | 0x2A_2A_3A], &[0], &[DIM]),
            &p,
            true,
            &Overlay::default(),
            &policy,
        );
        let corrected_undimmed = ensure_contrast_ratio(0x1E_1E_2E, 0x2A_2A_3A, 3.5).unwrap();
        assert_eq!(
            &got[5..8],
            &gl_rgb(corrected_undimmed),
            "corrected from the undimmed fg (dim skipped)"
        );
        // The dim-first path (what dropping stage-2 would produce) is a different colour.
        let dim_first = ensure_contrast_ratio(
            0x1E_1E_2E,
            crate::render_policy::dim_foreground(0x2A_2A_3A, 0x1E_1E_2E),
            3.5,
        )
        .unwrap();
        assert_ne!(
            corrected_undimmed, dim_first,
            "the two paths must differ, or this test proves nothing"
        );
    }

    #[test]
    fn a_dim_cell_that_meets_contrast_undimmed_stays_dimmed_not_re_corrected() {
        // xterm applies minimumContrastRatio ONCE, against the effective bg, on the UNDIMMED fg
        // (TextureAtlas._getMinimumContrastColor). A dim fg whose UNDIMMED value already clears mcr/2
        // is dimmed and left alone — it is NOT re-corrected after dimming. (justerm-web keeps it
        // dimmed too, via overlayTint's early return.) A double-pass that re-runs contrast on the
        // dimmed fg would wrongly brighten it back — the 2-lens (#272) caught exactly this.
        let p = bright_palette(); // default_bg black
        let policy = ColorPolicy {
            min_contrast: 4.5, // mcr/2 = 2.25
            ..ColorPolicy::default()
        };
        // 0x555555 on black = contrast 2.81 >= 2.25 → mcr does NOT fire → dims to 0x2B2B2B, and
        // 0x2B2B2B (contrast 1.48 < 2.25) must NOT be lightened back.
        let got = pack_instances(
            &frame(&[0], &[RGB | 0x55_55_55], &[0], &[DIM]),
            &p,
            true,
            &Overlay::default(),
            &policy,
        );
        let dimmed = crate::render_policy::dim_foreground(0x55_55_55, 0x00_00_00);
        assert_eq!(
            &got[5..8],
            &gl_rgb(dimmed),
            "the dimmed fg is kept, not re-corrected"
        );
    }

    // --- #272 slice 3: selection un-dim (#224) + selectionForeground (#227) ---

    /// A single-cell overlay of the given kind, both blend colours `bg`.
    fn overlay_kind(
        selection: &'static [u32],
        matches: &'static [u32],
        bg: u32,
    ) -> Overlay<'static> {
        Overlay {
            selection,
            matches,
            colors: HighlightColors {
                selection_bg: bg,
                match_bg: bg,
            },
        }
    }

    #[test]
    fn selection_foreground_overrides_a_selected_cells_fg_but_not_a_match() {
        let p = bright_palette(); // fg default = white
        let sfg = 0x00_FF_00; // green selectionForeground
        let policy = ColorPolicy {
            selection_fg: Some(sfg),
            ..ColorPolicy::default()
        };
        let cell =
            |ov: &Overlay| pack_instances(&frame(&[0], &[0], &[0], &[0]), &p, true, ov, &policy);
        // A selected cell: fg forced to green.
        let sel = cell(&overlay_kind(&[0, 0, 0], &[], 0x3A_3A_3A));
        assert_eq!(
            &sel[5..8],
            &gl_rgb(sfg),
            "selectionForeground overrides a selected cell"
        );
        // A search match is NOT a selection → the cell keeps its own fg (white).
        let mat = cell(&overlay_kind(&[], &[0, 0, 0], 0x3A_3A_3A));
        assert_eq!(&mat[5..8], &gl_rgb(0xFF_FF_FF), "a match keeps the cell fg");
        // No highlight → not overridden.
        let none = cell(&Overlay::default());
        assert_eq!(
            &none[5..8],
            &gl_rgb(0xFF_FF_FF),
            "an unselected cell keeps its fg"
        );
    }

    #[test]
    fn a_selection_undims_the_foreground_but_a_match_keeps_it_dim() {
        let p = bright_palette(); // fg white, bg black
        // Both highlights paint SOLID black (= the default bg), so `eff_bg` is black either way and
        // only the un-dim differs.
        let dim_cell = |ov: &Overlay| {
            pack_instances(
                &frame(&[0], &[0], &[0], &[DIM]),
                &p,
                true,
                ov,
                &ColorPolicy::default(),
            )
        };
        let sel = dim_cell(&overlay_kind(&[0, 0, 0], &[], 0x00_00_00));
        assert_eq!(
            &sel[5..8],
            &gl_rgb(0xFF_FF_FF),
            "a selection clears DIM (full-brightness fg)"
        );
        let mat = dim_cell(&overlay_kind(&[], &[0, 0, 0], 0x00_00_00));
        let dimmed = dim_foreground(0xFF_FF_FF, 0x00_00_00);
        assert_eq!(
            &mat[5..8],
            &gl_rgb(dimmed),
            "a search match keeps the cell dimmed"
        );
        assert_ne!(
            &sel[5..8],
            &mat[5..8],
            "selection and match must differ on a dim cell"
        );
    }

    #[test]
    fn a_selection_foreground_still_flows_through_the_contrast_pass() {
        // selectionForeground is applied BEFORE minimumContrastRatio (xterm resolves it pre-atlas),
        // so an illegible selectionForeground on the selection bg is still corrected. Pins the
        // composition (#227 override → #225 correct), not either alone.
        let p = bright_palette();
        let sel_bg = 0xCC_CC_CC; // light selection → white selectionForeground is illegible on it
        let policy = ColorPolicy {
            selection_fg: Some(0xFF_FF_FF),
            min_contrast: 4.5,
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], sel_bg),
            &policy,
        );
        let expect = gl_rgb(ensure_contrast_ratio(sel_bg, 0xFF_FF_FF, 4.5).unwrap());
        assert_eq!(
            &got[5..8],
            &expect,
            "selectionForeground is contrast-corrected"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(0xFF_FF_FF),
            "the raw selectionForeground was illegible"
        );
    }

    // --- #272 slice 4: tile glyphs (#226 excludeFromContrast, #239 recolor, #241 transparent) ---

    const BLOCK: u32 = 0x2588; // █ — a tile glyph (block element)

    /// A single-cell frame carrying a base `codepoint` so the tile classifier can see it.
    fn frame_cp<'a>(
        bg: &'a [u32],
        fg: &'a [u32],
        flags: &'a [u16],
        codepoints: &'a [u32],
    ) -> Frame<'a> {
        Frame {
            cols: 1,
            rows: 1,
            bg,
            fg,
            slots: &[0],
            flags,
            codepoints,
        }
    }

    #[test]
    fn a_tile_glyph_is_excluded_from_the_contrast_correction() {
        let p = palette(); // default_bg 0x1E1E2E
        let policy = ColorPolicy {
            min_contrast: 7.0,
            ..ColorPolicy::default()
        };
        // A █ (tile) with a low-contrast fg is NOT corrected — a nudge would seam it (#226).
        let tile = pack_instances(
            &frame_cp(&[0], &[RGB | 0x2A_2A_3A], &[0], &[BLOCK]),
            &p,
            true,
            &Overlay::default(),
            &policy,
        );
        assert_eq!(
            &tile[5..8],
            &gl_rgb(0x2A_2A_3A),
            "a tile glyph keeps its illegible fg"
        );
        // The SAME low-contrast fg on a non-tile glyph ('A') IS corrected — proves exclusion is real.
        let text = pack_instances(
            &frame_cp(&[0], &[RGB | 0x2A_2A_3A], &[0], &[0x41]),
            &p,
            true,
            &Overlay::default(),
            &policy,
        );
        assert_ne!(
            &text[5..8],
            &gl_rgb(0x2A_2A_3A),
            "a non-tile low-contrast fg is corrected"
        );
    }

    #[test]
    fn a_tile_glyph_under_selection_is_retinted_toward_the_raw_selection_colour() {
        let p = bright_palette(); // default_fg white
        let raw_sel = 0x80_00_00;
        let ov = overlay_kind(&[0, 0, 0], &[], raw_sel);
        // A selected █: fg re-tinted = blend(cell fg, raw selection colour) — NOT the cell fg, NOT the
        // effective (solid) bg.
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[0], &[BLOCK]),
            &p,
            true,
            &ov,
            &ColorPolicy::default(),
        );
        let expect = blend_over(0xFF_FF_FF, raw_sel, HIGHLIGHT_BLEND_ALPHA);
        assert_eq!(
            &got[5..8],
            &gl_rgb(expect),
            "tile re-tinted toward the raw selection colour"
        );
        assert_ne!(&got[5..8], &gl_rgb(0xFF_FF_FF), "not the cell fg");
        // A non-tile selected cell is NOT re-tinted (keeps its own white fg).
        let text = pack_instances(
            &frame_cp(&[0], &[0], &[0], &[0x41]),
            &p,
            true,
            &ov,
            &ColorPolicy::default(),
        );
        assert_eq!(
            &text[5..8],
            &gl_rgb(0xFF_FF_FF),
            "a non-tile glyph keeps its fg"
        );
    }

    #[test]
    fn an_inverse_default_bg_tile_under_selection_is_transparent() {
        // #241: an inverse cell with a DEFAULT bg renders its tile glyph transparent — fg becomes the
        // RAW selection colour with NO blend, so the glyph dissolves into the band.
        let p = bright_palette();
        let raw_sel = 0x80_00_00;
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &ColorPolicy::default(),
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(raw_sel),
            "inverse+default tile fg = raw selection colour"
        );
    }

    #[test]
    fn a_tile_glyph_under_selection_discards_selection_foreground() {
        // The re-tint starts from the cell's OWN fg and ignores selectionForeground (xterm re-resolves
        // the model fg for these glyphs).
        let p = bright_palette();
        let raw_sel = 0x80_00_00;
        let policy = ColorPolicy {
            selection_fg: Some(0x00_FF_00), // green — must be discarded for a tile glyph
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[0], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &policy,
        );
        let expect = blend_over(0xFF_FF_FF, raw_sel, HIGHLIGHT_BLEND_ALPHA);
        assert_eq!(
            &got[5..8],
            &gl_rgb(expect),
            "re-tint from the cell fg, not selectionForeground"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(0x00_FF_00),
            "selectionForeground is discarded for a tile"
        );
    }
}
