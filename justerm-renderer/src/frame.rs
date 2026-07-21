//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour references with
//! the injected palette + colour policy (inverse, bold→bright, dim — [`render_policy`]), composite
//! the marker [`decoration`] overrides and the selection/search highlight ([`overlay`]) back-to-front
//! (base < bottom-decoration < highlight < top-decoration), fold the underline/strikethrough into the
//! glyph field, and pack per-cell instance data in Rust — one flat buffer for a single instanced draw
//! call. Glyph-slot resolution (the stateful atlas cache) and rasterisation happen in the browser
//! layer; this packer takes the already-resolved slots + cell flags.
//!
//! [`render_policy`]: crate::render_policy
//! [`overlay`]: crate::overlay
//! [`decoration`]: crate::decoration

use crate::attrs::{BLANK_SLOT, glyph_field, is_concealed, is_dim, is_inverse};
use crate::color::gl_rgb;
use crate::contrast::ensure_contrast_ratio;
use crate::decoration::{DecorationLayer, DecorationRect, decoration_override_at};
use crate::glyph_class::treat_glyph_as_background_color;
use crate::overlay::{
    HIGHLIGHT_BLEND_ALPHA, HighlightKind, Overlay, blend_over, composite_bg, should_blend_kind,
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
    ///
    /// The packer sees **only** the base — a cell's grapheme cluster (`extra` + `side_table`) stops
    /// at [`glyph_resolve`](crate::glyph_resolve), which rasterises it. That is the declared rule,
    /// not an omission: coverage is set by the base and a combining mark can only add ink to it
    /// (#495, reasoned in [`glyph_class`](crate::glyph_class)).
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
/// A cell covered by the selection / search `overlay` (#271/#400) has its resolved background
/// composited with the injected highlight colour: a *selection* blends over a non-default / inverse
/// cell (painting solid over the default background), while a search *match* always paints solid
/// ([`should_blend_kind`], [`composite_bg`]). Compositing happens in packed colour space, before the
/// `gl_rgb` unpack, so the blend matches the web reference to the byte; and because the whole viewport
/// re-packs each frame and the #263 upload diff re-sends only changed cells, a cell that gains or loses
/// a highlight re-uploads with no extra bookkeeping (unlike beamterm's overlay delta).
pub fn pack_instances(
    frame: &Frame,
    palette: &Palette,
    blink_on: bool,
    overlay: &Overlay,
    policy: &ColorPolicy,
    decorations: &[DecorationRect],
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
            // The highlight covering this cell (if any) — the *bg* channel's winner. The fg channel
            // is INDEPENDENT (#430, xterm's model): the selection-only fg rules (#224/#227/#239) key
            // on selection *coverage*, not on the winning kind, so they survive on a cell whose bg
            // the ACTIVE match outranks (CellColorResolver keys its selection stage on `$isSelected`
            // while the active match is a bg-only top decoration).
            let kind = overlay.highlight_at(row, col);
            let is_selection = overlay.is_selected(row, col);
            // #226: a Powerline / box-drawing / block glyph tiles with the bg — excluded from the
            // contrast demand and re-tinted under selection (classify the base codepoint).
            let exclude =
                treat_glyph_as_background_color(codepoints.get(idx).copied().unwrap_or(0));
            // #120/#393 decorations compose back-to-front around the highlight (justerm-web
            // `composeCellColors`): base < BOTTOM decoration < highlight < TOP decoration. A decoration
            // overrides the bg and/or fg with an **absolute** `0xRRGGBB` (the consumer owns its theme
            // and resolves it before pushing — a decoration is NOT a core colour ref, so it is used
            // verbatim, no palette/inverse/bold→bright). A fg override sets `fg_overridden`, which the
            // #230 re-dim below keys off. `fg` starts from the cell's own fg (undimmed).
            let mut fg = cell_fg;
            let mut fg_overridden = false;
            let mut bg_running = cell_bg;
            // #444: whether a real colour from a bottom decoration now sits beneath the highlight —
            // the selection blend decision reads this too, so the decoration is not erased.
            let mut deco_bg = false;
            // #452: bg and fg merge INDEPENDENTLY across every decoration covering the cell, so a
            // bg-only and an fg-only decoration both apply (xterm's per-property last-wins).
            let bottom = decoration_override_at(decorations, row, col, DecorationLayer::Bottom);
            if let Some(c) = bottom.bg {
                bg_running = c;
                deco_bg = true;
            }
            if let Some(c) = bottom.fg {
                fg = c;
                fg_overridden = true;
            }
            // #227 selectionForeground: force a selected cell's fg to the injected colour (never a
            // match), overriding the cell's own / bottom-decoration fg. A tile glyph discards it (#239
            // below). It is selection-only, so it never triggers the #230 re-dim (`!is_selection`).
            if is_selection && let Some(sfg) = policy.selection_fg {
                fg = sfg;
            }
            // #271/#400/#444: composite the selection / search highlight onto the (bottom-decorated)
            // background — the fg policy then sees the EFFECTIVE bg the glyph is drawn over.
            // `should_blend_kind` reads the highlight kind + everything with a real colour beneath it:
            // the PRE-inverse *cell* ref/flags and (#444) whether a bottom decoration painted a bg. A
            // SELECTION blends so what is underneath shows through, and paints solid only over a bare
            // default bg; a search MATCH always paints solid, whatever is beneath (xterm/alacritty
            // parity — a match must read crisp, not a tint).
            let mut eff_bg = composite_bg(
                bg_running,
                kind.is_some_and(|k| should_blend_kind(k, bg_ref, cell_flags, deco_bg)),
                kind.map(|k| overlay.colors.of(k)),
            );
            // #239/#241: a tile glyph under a SELECTION fuses into the band — xterm re-tints it toward
            // the RAW selection colour (not the effective post-blend bg), starting from the cell's own
            // undimmed fg and discarding selectionForeground. #241: an inverse cell with a DEFAULT bg
            // is "treated as transparent" — it contributes no colour of its own, so its fg becomes the
            // band over whatever IS beneath: the raw selection colour with no blend, or (#453) the
            // selection over a bottom decoration's bg when one painted there.
            //
            // The re-tint starts from `cell_fg`, which has bold→bright (#223) applied — byte-identical
            // to justerm-web (its `fgUndimmed` is likewise brightened), so the #273 switch stays
            // neutral. This diverges from xterm ONLY for a BOLD + ANSI-0..7 + tile + selection cell,
            // where xterm re-tints from the *base* ANSI colour (its `CellColorResolver` bypasses the
            // `+8`): a corner-of-corner, sub-perceptible under the 0x80 blend. 100 % xterm parity here
            // is a *family* change (web + renderer together) — tracked as #398, not smuggled in.
            if is_selection && exclude {
                let raw_sel = overlay.colors.of(HighlightKind::Selection);
                let inverse_default_bg = is_inverse(cell_flags) && (bg_ref >> 24) == 0;
                fg = if inverse_default_bg {
                    // #453: the cell contributes nothing, so the tile shows the band as it falls on
                    // whatever IS beneath — since #444 that includes a bottom decoration's bg. Recompute
                    // the band with the cell taken out of the stack: `bg_running` here is the
                    // decoration's colour when one painted (`deco_bg`), and `composite_bg` then blends;
                    // with no decoration it returns the RAW selection colour, byte-identical to before
                    // and to xterm (`CellColorResolver.ts:139` sets the selection colour flat). NOT
                    // `eff_bg` — that is the band over THIS cell, and for an inverse cell it carries the
                    // cell's own colour (probe: 0x97AFDF vs raw 0x3060C0), which is exactly the colour
                    // "transparent" says to drop.
                    //
                    // The decoration folds in only when the SELECTION is the layer painted over it: a
                    // match paints solid (#400) and erases the decoration from the bg channel, so
                    // blending over it would compose a stack no pixel shows. The fg is selection-keyed
                    // either way (#430) — under an active match it stays the raw selection colour, as
                    // the undecorated sibling already pinned.
                    // The kind literal is the collapsed form of "did the bg channel blend?"
                    // (`deco_bg && kind.is_some_and(|k| should_blend_kind(k, bg_ref, cell_flags,
                    // deco_bg))`): this arm requires `is_inverse`, which makes `should_blend`
                    // unconditionally true, so the two are provably equal here. Valid as long as no
                    // future `HighlightKind` both outranks `Selection` AND blends — such a kind would
                    // blend on the bg channel while this literal kept the fg flat.
                    let beneath_the_band =
                        deco_bg && matches!(kind, Some(HighlightKind::Selection));
                    composite_bg(bg_running, beneath_the_band, Some(raw_sel))
                } else {
                    blend_over(cell_fg, raw_sel, HIGHLIGHT_BLEND_ALPHA)
                };
            }
            // #120/#393 TOP decoration: paints OVER the highlight (foreground-most), overriding the
            // effective bg and/or the fg (`composeCellColors` applies `top` last, after selection).
            // Its bg/fg also merge per-property across the top layer (#452), independently of the
            // bottom layer's merge — xterm runs one accumulating pass per layer.
            let top = decoration_override_at(decorations, row, col, DecorationLayer::Top);
            if let Some(c) = top.bg {
                eff_bg = c;
            }
            if let Some(c) = top.fg {
                fg = c;
                fg_overridden = true;
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
            // #230: a decoration fg override on a dim non-selected cell KEEPS the cell's DIM — xterm
            // leaves `BgFlags.DIM` set, so the resolved override is dimmed too. It is re-dimmed here
            // (before contrast); the base fg's own dim is the `!fg_overridden` arm of the policy below,
            // so exactly one path dims the fg. (composeCellColors #230 → then the contrast pass.)
            if dim && fg_overridden {
                fg = dim_foreground(fg, eff_bg);
            }
            let fg_packed = if mcr > 1.0 && !exclude {
                let ratio = if dim { mcr / 2.0 } else { mcr };
                match ensure_contrast_ratio(eff_bg, fg, ratio) {
                    Some(adjusted) => adjusted,
                    None if dim && !fg_overridden => dim_foreground(fg, eff_bg),
                    None => fg,
                }
            } else if dim && !fg_overridden {
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
    use crate::decoration::{DecorationLayer, DecorationRect};
    use crate::overlay::{HIGHLIGHT_BLEND_ALPHA, HighlightColors, blend_over};
    use crate::render_policy::ColorPolicy;

    /// A single Bottom/Top decoration rect covering (row 0, col 0).
    fn deco(layer: DecorationLayer, bg: Option<u32>, fg: Option<u32>) -> DecorationRect {
        DecorationRect {
            row: 0,
            left: 0,
            right: 0,
            layer,
            bg,
            fg,
        }
    }

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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
        );
        assert_eq!(on[8], 33.0, "blink-on draws the real glyph");
        // Blink phase OFF: the glyph is concealed to the blank slot.
        let off = pack_instances(
            &frame(&[0], &[0], &[33], &[BLINK]),
            &p,
            false,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[],
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
        let got = pack_instances(
            &f,
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[],
        );
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
            &[],
        );
        let off = pack_instances(
            &frame(&[0], &[0], &[33], &[0]),
            &p,
            false,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(on, off);
        assert_eq!(on[8], 33.0);
    }

    // --- #271: selection / search overlay compositing into the packed background ---

    const SEL_BG: u32 = 0x3A_3D_5C;
    const MATCH_BG: u32 = 0x5C_3A_3A;
    const ACTIVE_BG: u32 = 0x2E_5C_3A; // #427: the active/focused match colour, distinct from both

    /// One-cell overlay covering (row 0, col 0) as the given group.
    fn selected<'a>(selection: &'a [u32], matches: &'a [u32]) -> Overlay<'a> {
        Overlay {
            active: &[],
            selection,
            matches,
            colors: HighlightColors {
                selection_bg: SEL_BG,
                match_bg: MATCH_BG,
                active_match_bg: ACTIVE_BG,
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
            &[],
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
            &[],
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
            &[],
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
            &[],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(MATCH_BG),
            "a match paints its own colour"
        );
    }

    #[test]
    fn a_search_match_on_a_coloured_cell_paints_solid_not_a_blend() {
        // #400: xterm (`CellColorResolver` drops the decoration alpha) and alacritty
        // (`compute_cell_rgb` forces `bg_alpha = 1.0`) both paint a search match's bg SOLID, whatever
        // the cell colour — so a match on a coloured cell reads crisp, not a muddy 50% tint. A SELECTION
        // on the same cell still blends (see `a_selected_coloured_cell_blends...`), so this pins the
        // match/selection divergence, not just "match paints something".
        let p = palette();
        let cell_bg = 0xE0_6C_75; // Rgb (non-default) → a selection would blend here (should_blend=true)
        let got = pack_instances(
            &frame(&[(2 << 24) | cell_bg], &[0], &[0], &[0]),
            &p,
            true,
            &selected(&[], &[0, 0, 0]), // a MATCH covers (0,0)
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(MATCH_BG),
            "a match paints its own colour SOLID over a coloured cell"
        );
        // ...and it is NOT the blend a selection would produce — proves the solid branch really ran.
        assert_ne!(
            &got[2..5],
            &gl_rgb(blend_over(cell_bg, MATCH_BG, HIGHLIGHT_BLEND_ALPHA)),
            "a match must not blend over the cell colour"
        );
    }

    /// One-cell overlay where the active/focused match, the selection, and the match groups are set
    /// independently — for the #427 ranking tests.
    fn with_active<'a>(active: &'a [u32], selection: &'a [u32], matches: &'a [u32]) -> Overlay<'a> {
        Overlay {
            active,
            selection,
            matches,
            colors: HighlightColors {
                selection_bg: SEL_BG,
                match_bg: MATCH_BG,
                active_match_bg: ACTIVE_BG,
            },
        }
    }

    #[test]
    fn an_active_match_paints_its_own_colour_over_a_selected_cell() {
        // #427: the active (focused/current) match ranks ABOVE the selection — a cell covered by BOTH
        // the active group and the selection shows the ACTIVE colour, solid. Pins the
        // ActiveMatch > Selection ranking end-to-end through pack_instances.
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[0, 0, 0]), // active + selection + match all cover (0,0)
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(ACTIVE_BG),
            "the active match colour wins over the selection"
        );
        // ...and it is NOT the selection colour that cell would otherwise show — proves the ranking ran.
        assert_ne!(&got[2..5], &gl_rgb(SEL_BG), "not the selection colour");
    }

    #[test]
    fn an_active_matched_selected_cell_keeps_selection_fg_semantics() {
        // #430: fg and bg resolve on INDEPENDENT channels — xterm's model, adopted deliberately
        // (this FLIPS the former #427 pin `…keeps_match_fg_semantics_not_selection`). In
        // `CellColorResolver` the selection fg stage keys on `$isSelected` (L84/127) while the
        // active match is a bg-only `top` decoration (`DecorationManager.ts:139`) overriding only
        // `$bg` (L177–187) — so on a cell that is BOTH the active match AND selected, the solid
        // active bg wins the bg channel and the selection fg treatments survive on the fg channel.
        let p = bright_palette(); // fg white, bg black
        let sfg = 0x00_FF_00; // green selectionForeground — survives under the active bg
        let policy = ColorPolicy {
            selection_fg: Some(sfg),
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[DIM]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]), // active + selection both cover (0,0)
            &policy,
            &[],
        );
        // bg channel: the solid active colour still wins (#427 ranking, unchanged).
        assert_eq!(
            &got[2..5],
            &gl_rgb(ACTIVE_BG),
            "bg stays the solid active colour"
        );
        // fg channel: selectionForeground VERBATIM — selected ⇒ un-dim, so it is not dimmed either.
        assert_eq!(
            &got[5..8],
            &gl_rgb(sfg),
            "selectionForeground survives under the active bg"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(dim_foreground(0xFF_FF_FF, ACTIVE_BG)),
            "not the dimmed own fg (the retired match semantics)"
        );
    }

    #[test]
    fn an_active_matched_selected_dim_cell_is_undimmed_without_selection_fg() {
        // #430: the un-dim half (#224) is channel-independent too — with NO selectionForeground
        // injected, the overlap cell keeps its OWN fg but selected ⇒ DIM cleared (xterm strips
        // `BgFlags.DIM` for any selected cell with a bg override, keyed on `$isSelected` regardless
        // of which bg won, CellColorResolver L191–198). Without this, the selected current match
        // would be the least legible cell on screen: dim text under a bright solid bg.
        let p = bright_palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[DIM]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(0xFF_FF_FF),
            "own fg, UNdimmed (selection un-dim survives the active bg)"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(dim_foreground(0xFF_FF_FF, ACTIVE_BG)),
            "not dimmed toward the active bg"
        );
    }

    #[test]
    fn an_active_match_outside_the_selection_keeps_match_fg_semantics() {
        // #430 overshoot guard: channel independence keys on selection COVERAGE, not on the active
        // kind — an active match the user has NOT selected keeps plain match fg semantics (own fg,
        // DIM kept, selectionForeground ignored), exactly as before #430.
        let p = bright_palette();
        let policy = ColorPolicy {
            selection_fg: Some(0x00_FF_00),
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[DIM]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[], &[]), // active only — no selection anywhere
            &policy,
            &[],
        );
        assert_eq!(&got[2..5], &gl_rgb(ACTIVE_BG), "bg is the active colour");
        assert_eq!(
            &got[5..8],
            &gl_rgb(dim_foreground(0xFF_FF_FF, ACTIVE_BG)),
            "own fg, dimmed — match semantics unchanged off-selection"
        );
    }

    #[test]
    fn minimum_contrast_corrects_selection_fg_against_the_active_bg() {
        // #430 mitigation pin: when a theme's selectionForeground clashes with the active bg, the
        // contrast pass corrects against the FINAL composited bg — the ACTIVE colour, not the cell's
        // own bg — so `minimumContrastRatio` genuinely backstops the overlap cell. (The generic
        // effective-bg principle is pinned by
        // `the_contrast_pass_runs_against_the_effective_highlight_bg`; this pins the active∩selected
        // cell specifically, the case the #430 decision leans on.)
        let p = bright_palette();
        let sfg = 0x2A_5A_38; // nearly the ACTIVE_BG green — illegible on it
        let policy = ColorPolicy {
            selection_fg: Some(sfg),
            min_contrast: 4.5,
            ..ColorPolicy::default()
        };
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &policy,
            &[],
        );
        let expect =
            ensure_contrast_ratio(ACTIVE_BG, sfg, 4.5).expect("sfg fails 4.5:1 on ACTIVE_BG");
        assert_eq!(
            &got[5..8],
            &gl_rgb(expect),
            "sfg corrected against the ACTIVE bg"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(sfg),
            "the clashing sfg does not pass through"
        );
    }

    #[test]
    fn an_active_matched_selected_dim_cell_does_not_re_dim_a_decoration_fg_override() {
        // #430 × #230: the selected-cell DIM strip applies to a decoration fg override too — on the
        // overlap cell the override draws FULL-brightness (xterm strips `BgFlags.DIM` keyed on
        // `$isSelected`, so the resolved override is no longer dimmed), where a NON-selected dim
        // cell re-dims it (`a_dim_cell_re_dims_a_decoration_foreground_override`).
        let p = bright_palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[33], &[DIM]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, None, Some(DECO_FG))],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(DECO_FG),
            "the override draws full-brightness on the overlap cell"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(dim_foreground(DECO_FG, ACTIVE_BG)),
            "not re-dimmed (#230 is a non-selected rule)"
        );
    }

    #[test]
    fn an_active_matched_selected_dim_cell_demands_the_full_contrast_ratio() {
        // #430: the un-dim removes the DIM half-ratio concession too — the overlap cell demands the
        // FULL minimumContrastRatio (pre-#430, the halved `mcr/2` let a mid-contrast fg through
        // untouched and then dimmed it). The chosen fg sits BETWEEN `mcr/2` and `mcr`, so only the
        // full demand corrects it — the preconditions assert that split, keeping the pin
        // self-diagnosing if the contrast math ever moves.
        let p = bright_palette();
        let fg = 0xC0_C0_C0;
        let policy = ColorPolicy {
            min_contrast: 7.0,
            ..ColorPolicy::default()
        };
        assert!(
            ensure_contrast_ratio(ACTIVE_BG, fg, 7.0).is_some(),
            "precondition: the fg fails the full ratio"
        );
        assert!(
            ensure_contrast_ratio(ACTIVE_BG, fg, 3.5).is_none(),
            "precondition: the fg clears the halved ratio"
        );
        let got = pack_instances(
            &frame(&[0], &[RGB | fg], &[0], &[DIM]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &policy,
            &[],
        );
        let expect = ensure_contrast_ratio(ACTIVE_BG, fg, 7.0).unwrap();
        assert_eq!(&got[5..8], &gl_rgb(expect), "corrected at the FULL ratio");
        assert_ne!(
            &got[5..8],
            &gl_rgb(fg),
            "not passed through (the old halved demand would)"
        );
    }

    #[test]
    fn an_inverse_default_bg_tile_on_an_active_matched_selected_cell_uses_the_raw_selection_colour()
    {
        // #430 × #241: the inverse-default-bg tile arm (here fg = the RAW selection colour, unblended
        // — nothing is beneath the transparent cell; with a bottom decoration under a *selection* it
        // is the band over that, #453) is a selection fg treatment too, so it survives under the
        // active bg. Here the glyph does
        // NOT dissolve — the band under it is the ACTIVE colour, not the selection's — and that is
        // xterm's literal output as well: its selected-tile stage sets the fg from the selection
        // colour while the active match overrides only the bg.
        let p = bright_palette();
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(SEL_BG),
            "fg = the raw selection colour, unblended (#241)"
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(ACTIVE_BG),
            "bg = the solid active colour (a match never blends)"
        );
    }

    #[test]
    fn an_active_match_over_a_decorated_transparent_tile_still_uses_the_raw_selection_colour() {
        // #453 × #430: the transparent-tile fg folds in a bottom decoration only when the SELECTION is
        // what is painted over it. An active match paints SOLID (#400), which erases the decoration
        // from the bg channel — so blending the selection over that decoration would compose a stack
        // no pixel shows. The fg stays the raw selection colour, exactly as the undecorated sibling
        // above pins it, which is xterm's literal output (its active match is a bg-only top
        // decoration applied after the selection stage).
        let p = bright_palette();
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(0x80_40_00), None)],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(SEL_BG),
            "fg = the raw selection colour — the decoration is not beneath a SOLID layer"
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(ACTIVE_BG),
            "bg = the solid active colour, decoration erased (#400)"
        );
        // Control: the expected value above equals the NO-decoration value, so on its own it would
        // also pass with a dead fixture (wrong layer, wrong row, a broken `decoration_override_at`).
        // Drop only the active match — same decoration, same cell — and the fg must move, proving the
        // decoration is live and the highlight KIND is the single variable under test.
        let selection_only = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &with_active(&[], &[0, 0, 0], &[]),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(0x80_40_00), None)],
        );
        assert_eq!(
            &selection_only[5..8],
            &gl_rgb(blend_over(0x80_40_00, SEL_BG, HIGHLIGHT_BLEND_ALPHA)),
            "same decoration, selection only: it DOES fold in — the fixture is live"
        );
    }

    #[test]
    fn a_tile_glyph_on_an_active_matched_selected_cell_retints_toward_the_selection() {
        // #430 AC-③, xterm verbatim: the selection-stage tile re-tint (#239 — toward the RAW
        // selection colour, inside `if ($isSelected)` in CellColorResolver L133–174) survives under
        // the active bg: fg = blend(own fg, selection colour), bg = the solid active colour.
        let p = bright_palette();
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[0], &[BLOCK]),
            &p,
            true,
            &with_active(&[0, 0, 0], &[0, 0, 0], &[]),
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(&got[2..5], &gl_rgb(ACTIVE_BG), "bg is the active colour");
        let expect = blend_over(0xFF_FF_FF, SEL_BG, HIGHLIGHT_BLEND_ALPHA);
        assert_eq!(
            &got[5..8],
            &gl_rgb(expect),
            "tile re-tinted toward the selection colour"
        );
        assert_ne!(&got[5..8], &gl_rgb(0xFF_FF_FF), "not the raw cell fg");
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
                ..Default::default()
            },
            ..Default::default()
        };
        // fg Default (white); bg Default → the highlight paints SOLID grey (default-bg cell).
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &overlay,
            &policy,
            &[],
        );
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
            &[],
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
            &[],
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
            active: &[],
            selection,
            matches,
            colors: HighlightColors {
                selection_bg: bg,
                match_bg: bg,
                active_match_bg: bg,
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
        let cell = |ov: &Overlay| {
            pack_instances(&frame(&[0], &[0], &[0], &[0]), &p, true, ov, &policy, &[])
        };
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
                &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
        );
        assert_eq!(
            &text[5..8],
            &gl_rgb(0xFF_FF_FF),
            "a non-tile glyph keeps its fg"
        );
    }

    /// #495: a cell whose painted grapheme is `█` + `U+0301` (combining acute) still takes the tile
    /// branch, because the packer classifies the cell's BASE scalar — the declared rule
    /// (`glyph_class`): coverage is set by the base, and a mark can only add ink to it.
    ///
    /// The second half is what makes this discriminating. `0x0301` is precisely the scalar xterm's
    /// `CellColorResolver` would classify by (`cell.getCode()` = the cluster's last UTF-16 unit), and
    /// on its own it does NOT tile — so implementing that rule flips the first assertion. The axes
    /// otherwise never cross: every other tile test passes a bare codepoint with no cluster, and the
    /// cluster tests (`glyph_resolve`) never look at the tile branch.
    #[test]
    fn a_combined_tile_cell_classifies_from_its_base_not_its_combining_mark() {
        let p = bright_palette(); // default_fg white
        let raw_sel = 0x80_00_00;
        let ov = overlay_kind(&[0, 0, 0], &[], raw_sel);
        const COMBINING_ACUTE: u32 = 0x0301;
        // The cell's cluster is "█\u{0301}"; `glyph_resolve` rasterises the whole grapheme, and the
        // packer sees only the base column — which is the rule, not an omission.
        let combined = pack_instances(
            &frame_cp(&[0], &[0], &[0], &[BLOCK]),
            &p,
            true,
            &ov,
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &combined[5..8],
            &gl_rgb(blend_over(0xFF_FF_FF, raw_sel, HIGHLIGHT_BLEND_ALPHA)),
            "a combining mark on a block does not un-tile the cell"
        );
        // Classifying the cluster's LAST scalar (xterm's resolver rule) would land here instead.
        let by_last_scalar = pack_instances(
            &frame_cp(&[0], &[0], &[0], &[COMBINING_ACUTE]),
            &p,
            true,
            &ov,
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &by_last_scalar[5..8],
            &gl_rgb(0xFF_FF_FF),
            "the combining mark alone is not a tile glyph — so the two rules differ here"
        );
    }

    #[test]
    fn an_inverse_default_bg_tile_under_selection_is_transparent() {
        // #241: an inverse cell with a DEFAULT bg renders its tile glyph transparent — with nothing
        // beneath it, fg becomes the RAW selection colour with NO blend, so the glyph dissolves into
        // the band. (What "beneath" adds when a bottom decoration paints there is #453, below.)
        let p = bright_palette();
        let raw_sel = 0x80_00_00;
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &ColorPolicy::default(),
            &[],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(raw_sel),
            "inverse+default tile fg = raw selection colour"
        );
        // The bg channel does NOT go transparent with it: `should_blend`'s `is_inverse` term keeps the
        // cell's swapped-in colour in the composite, so bg = blend(default_fg, sel) — the two channels
        // disagree. Pinned because it is xterm's behaviour verbatim (its `$bg` blends the cell colour
        // at CellColorResolver.ts:100-120 while `$fg` is the flat selection colour at :139), not an
        // accident of this port; on a 100 %-coverage █ it is invisible, on a partial tile (─, ▀) it is
        // a visible seam. Tracked as a decision in #496.
        assert_eq!(
            &got[2..5],
            &gl_rgb(blend_over(0xFF_FF_FF, raw_sel, HIGHLIGHT_BLEND_ALPHA)),
            "bg keeps the cell's swapped-in colour (xterm parity, #496)"
        );
    }

    #[test]
    fn an_inverse_default_bg_tile_under_selection_keeps_a_bottom_decoration_beneath_it() {
        // #453: "transparent" (#241) means *show what is actually beneath* — and since #444 a bottom
        // decoration's bg IS beneath. The tile fg must therefore be the band as it falls on the
        // decoration, not the bare selection colour, or a 100 %-coverage glyph repaints the whole cell
        // in solid selection and erases the decoration through the fg channel (the bg channel already
        // keeps it, making #444's guarantee half-true).
        //
        // Measured before the fix (throwaway probe, this exact cell): bg = 0x585060 (decoration
        // survives) but fg = 0x3060C0 = the raw selection — the erasure.
        let p = bright_palette();
        let raw_sel = 0x30_60_C0;
        let deco_bg = 0x80_40_00;
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(deco_bg), None)],
        );
        let band_over_deco = blend_over(deco_bg, raw_sel, HIGHLIGHT_BLEND_ALPHA);
        assert_eq!(
            &got[5..8],
            &gl_rgb(band_over_deco),
            "the tile fg is the selection over the DECORATION, not the bare selection"
        );
        // Right reason, not just the right value: the fg must agree with the bg channel, which is
        // what makes the cell read as one uniform band colour rather than two layers disagreeing.
        assert_eq!(&got[2..5], &got[5..8], "fg and bg channels agree");
        // And it must NOT be `eff_bg` computed for this cell — the expression the issue named. Here
        // they coincide (both 0x585060) BECAUSE the cell is transparent; the no-decoration sibling
        // test above is what separates them (there eff_bg = 0x97AFDF but the fg must stay raw_sel).
    }

    #[test]
    fn an_fg_only_bottom_decoration_leaves_a_transparent_tile_on_the_raw_selection_colour() {
        // The #453 fold-in keys on the decoration's BACKGROUND (`deco_bg`), the only half that is
        // "beneath" anything. An fg-only bottom decoration paints no colour under the band — and its
        // fg is discarded by the re-tint regardless (xterm parity) — so the transparent tile stays on
        // the raw selection colour. Without this, `bottom.fg.is_some()` would look like an equally
        // plausible trigger.
        let p = bright_palette();
        let raw_sel = 0x30_60_C0;
        let got = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, None, Some(0x00_FF_00))],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(raw_sel),
            "an fg-only decoration is not 'beneath' the band"
        );
        // Control, same reason as the active-match test: give the SAME decoration a bg as well and the
        // fg must move, so this cannot pass on a fixture that never covered the cell.
        let with_bg = pack_instances(
            &frame_cp(&[0], &[0], &[INVERSE], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &ColorPolicy::default(),
            &[deco(
                DecorationLayer::Bottom,
                Some(0x80_40_00),
                Some(0x00_FF_00),
            )],
        );
        assert_eq!(
            &with_bg[5..8],
            &gl_rgb(blend_over(0x80_40_00, raw_sel, HIGHLIGHT_BLEND_ALPHA)),
            "the same decoration WITH a bg does fold in — the fixture is live"
        );
    }

    #[test]
    fn a_selected_dim_tile_glyph_is_re_tinted_undimmed() {
        // Pins the #224 dependency the re-tint leans on (#453 acceptance ③): a SELECTED cell's DIM is
        // cleared, so the re-tinted tile fg is never faded afterwards. The re-tint does not set
        // `fg_overridden` (xterm sets `$hasFg`), and this test is what would catch that mattering.
        //
        // Measured on THIS cell, not assumed: re-running it with #224 narrowed (`dim` no longer forced
        // off under selection) gives fg = 0x8B70A0 with `fg_overridden = true` added to the re-tint and
        // 0x8B70A0 without it — byte-identical. The flag cannot change a tile's output, because
        // `exclude` is true
        // here, which skips the contrast branch and leaves the two arms as the same
        // `dim_foreground(fg, eff_bg)`. So the issue's "if #224 is narrowed this becomes a live bug"
        // is false; valid as long as tile cells stay contrast-excluded (#226).
        let p = bright_palette();
        let raw_sel = 0x30_60_C0;
        let got = pack_instances(
            &frame_cp(&[IDX | 1], &[0], &[DIM], &[BLOCK]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], raw_sel),
            &ColorPolicy::default(),
            &[],
        );
        // The undimmed re-tint: the cell's own fg (white) blended halfway to the selection.
        assert_eq!(
            &got[5..8],
            &gl_rgb(blend_over(0xFF_FF_FF, raw_sel, HIGHLIGHT_BLEND_ALPHA)),
            "a selected tile is re-tinted from its UNDIMMED fg (#224)"
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
            &[],
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

    // --- #393 marker decorations: 2-layer bg/fg override, z-ordered around the highlight ---

    const DECO_BG: u32 = 0x80_00_00; // absolute decoration background (consumer-resolved theme colour)
    const DECO_FG: u32 = 0x00_FF_00; // absolute decoration foreground

    #[test]
    fn a_bottom_decoration_overrides_the_cell_background() {
        let p = palette(); // default_bg 0x1E1E2E
        let decos = [deco(DecorationLayer::Bottom, Some(DECO_BG), None)];
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &decos,
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(0x80_00_00),
            "bottom decoration paints the bg"
        );
    }

    #[test]
    fn a_decoration_colour_is_absolute_not_a_resolved_ref() {
        // #393 D2 (2-lens): a decoration colour is an ABSOLUTE 0xRRGGBB used verbatim, matching
        // justerm-web. A black decoration (0x000000) must render BLACK — if it were (wrongly) resolved
        // as a tagged ref, its top byte 0 would read as `Default` and it would become `default_bg`.
        let p = palette(); // default_bg 0x1E1E2E (distinct from black)
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(0x00_00_00), None)],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(0x00_00_00),
            "a black decoration is black, not default_bg"
        );
        assert_ne!(
            &got[2..5],
            &gl_rgb(0x1E_1E_2E),
            "not resolved as a Default ref"
        );
    }

    #[test]
    fn a_top_decoration_paints_over_the_highlight_but_a_bottom_paints_under_it() {
        let p = palette();
        let sel_bg = 0x30_60_C0;
        let ov = overlay_kind(&[0, 0, 0], &[], sel_bg);
        // BOTTOM under the highlight: the selection paints over it but does NOT erase it — #444 makes
        // the blend decision decoration-aware, so the decoration shows through the translucent
        // selection even though the cell's own bg is Default. (Before #444 this arm painted the
        // selection solid, discarding the decoration.)
        let bottom = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &ov,
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(DECO_BG), None)],
        );
        assert_eq!(
            &bottom[2..5],
            &gl_rgb(blend_over(DECO_BG, sel_bg, HIGHLIGHT_BLEND_ALPHA)),
            "the highlight blends over a bottom decoration, which stays visible"
        );
        // TOP over the highlight: it wins over the selection.
        let top = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &ov,
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Top, Some(DECO_BG), None)],
        );
        assert_eq!(
            &top[2..5],
            &gl_rgb(0x80_00_00),
            "a top decoration paints over the highlight"
        );
    }

    #[test]
    fn a_decoration_overrides_the_foreground() {
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[33], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, None, Some(DECO_FG))],
        );
        assert_eq!(&got[5..8], &gl_rgb(0x00_FF_00), "decoration fg override");
    }

    #[test]
    fn a_dim_cell_re_dims_a_decoration_foreground_override() {
        // #230: a decoration fg override on a DIM non-selected cell keeps the cell's DIM — the
        // resolved override colour is faded toward the (effective) bg, not drawn full-brightness.
        let p = palette(); // default_bg 0x1E1E2E
        let got = pack_instances(
            &frame(&[0], &[0], &[33], &[DIM]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, None, Some(DECO_FG))],
        );
        let expect = gl_rgb(dim_foreground(0x00_FF_00, 0x1E_1E_2E));
        assert_eq!(&got[5..8], &expect, "the override fg is re-dimmed (#230)");
        assert_ne!(&got[5..8], &gl_rgb(0x00_FF_00), "not drawn full-brightness");
    }

    #[test]
    fn a_dim_base_fg_dims_against_the_effective_bg_including_a_bottom_decoration() {
        // #393 D1 (2-lens, decided to keep): a DIM non-selected cell whose fg is NOT overridden dims
        // toward the EFFECTIVE bg — here the bottom-decoration bg the glyph is actually drawn over —
        // NOT the cell's own bg. This is the slice-2 single-pass model (xterm dims against the drawn
        // bg, `TextureAtlas`); it diverges from justerm-web (which pre-dims against the cell bg), a
        // known/tracked #273 deviation extended to decorations, pinned here so it is intentional.
        let p = palette(); // default_bg 0x1E1E2E; default_fg white
        let got = pack_instances(
            &frame(&[0], &[0], &[33], &[DIM]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(DECO_BG), None)], // bg-only bottom decoration
        );
        // eff_bg = the decoration bg (0x800000, no highlight); the base fg dims toward it.
        let expect = gl_rgb(dim_foreground(0xFF_FF_FF, 0x80_00_00));
        assert_eq!(
            &got[5..8],
            &expect,
            "dims toward the decoration bg (the drawn bg)"
        );
        assert_ne!(
            &got[5..8],
            &gl_rgb(dim_foreground(0xFF_FF_FF, 0x1E_1E_2E)),
            "NOT the web's cell-bg dim (the tracked #273 deviation)"
        );
    }

    #[test]
    fn a_cell_outside_a_decoration_span_is_unchanged() {
        let p = palette();
        // The decoration covers (0,1); the packed cell (0,0) keeps its own colours.
        let deco_right = DecorationRect {
            row: 0,
            left: 1,
            right: 1,
            layer: DecorationLayer::Bottom,
            bg: Some(DECO_BG),
            fg: Some(DECO_FG),
        };
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[deco_right],
        );
        assert_eq!(&got[2..5], &gl_rgb(0x1E_1E_2E), "bg unchanged");
        assert_eq!(&got[5..8], &gl_rgb(0xFF_FF_FF), "fg unchanged");
    }

    #[test]
    fn a_selection_blends_over_a_bottom_decoration_bg_not_the_cell_bg() {
        // #393 Row 5, resolved by #444: on a SELECTED, non-default-bg cell the highlight blends over
        // the BOTTOM-decoration bg — what is actually beneath it — NOT the cell's own bg. xterm
        // re-derives its blend base from the cell (`CellColorResolver`), discarding the decoration;
        // justerm deliberately keeps it (see `overlay::should_blend_kind` for the full rationale:
        // alpha compositing shows what is underneath, and xterm's own API doc calls `'bottom'` a layer
        // rendered *under* the selection). No longer a deferred divergence — a decided contract.
        let p = palette();
        let cell_bg = 0x20_40_60; // Rgb (non-default) → the highlight BLENDS (should_blend = true)
        let sel_bg = 0x30_60_C0;
        let got = pack_instances(
            &frame(&[(2 << 24) | cell_bg], &[0], &[0], &[0]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], sel_bg),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(DECO_BG), None)],
        );
        // Renderer/web: blend the selection over the DECORATION bg (0x800000).
        let over_deco = gl_rgb(blend_over(0x80_00_00, sel_bg, HIGHLIGHT_BLEND_ALPHA));
        assert_eq!(
            &got[2..5],
            &over_deco,
            "selection blends over the bottom-decoration bg (== web)"
        );
        // xterm would blend over the CELL bg (0x204060) — the divergence, deferred to #400.
        let over_cell = gl_rgb(blend_over(cell_bg, sel_bg, HIGHLIGHT_BLEND_ALPHA));
        assert_ne!(
            &got[2..5],
            &over_cell,
            "not the cell-bg blend xterm would use"
        );
    }

    #[test]
    fn a_selection_blends_over_a_bottom_decoration_bg_on_a_default_bg_cell_too() {
        // #444: the blend DECISION is decoration-aware, not just the blend base. A bottom-decoration
        // bg is a real colour beneath the selection, so it must show through even when the CELL's own
        // bg is Default. Before #444 the decision read the cell alone, so this cell painted the
        // selection SOLID and erased the decoration outright — while a coloured cell blended over it.
        let p = palette();
        let sel_bg = 0x30_60_C0;
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]), // Default bg ref, non-inverse
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], sel_bg),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(DECO_BG), None)],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(blend_over(DECO_BG, sel_bg, HIGHLIGHT_BLEND_ALPHA)),
            "the selection blends over the decoration bg"
        );
        assert_ne!(
            &got[2..5],
            &gl_rgb(sel_bg),
            "not the solid selection that used to erase the decoration"
        );
    }

    #[test]
    fn a_bottom_decoration_bg_survives_a_sibling_fg_only_decoration() {
        // #452: xterm resolves bg and fg INDEPENDENTLY across every decoration on a layer —
        // `DecorationService.forEachDecorationAtCell` calls back for *all* matches and
        // `CellColorResolver` accumulates `$bg` / `$fg` in two separate `if`s. So a later fg-only
        // decoration must not discard an earlier decoration's bg. Post-#444 that bg also decides
        // blend-vs-solid, so losing it flips the compositing mode, not just a colour.
        let p = palette();
        let sel_bg = 0x30_60_C0;
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]), // Default bg cell
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], sel_bg),
            &ColorPolicy::default(),
            &[
                deco(DecorationLayer::Bottom, Some(DECO_BG), None),
                deco(DecorationLayer::Bottom, None, Some(DECO_FG)),
            ],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(blend_over(DECO_BG, sel_bg, HIGHLIGHT_BLEND_ALPHA)),
            "the earlier bg survives and still drives the #444 blend"
        );
        assert_ne!(
            &got[2..5],
            &gl_rgb(sel_bg),
            "not the solid selection a lost bg would produce"
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(DECO_FG),
            "the later fg-only decoration still applies"
        );
    }

    #[test]
    fn a_bottom_decoration_fg_survives_a_later_bg_only_decoration() {
        // #452 mirror direction: registration order must not matter to the *other* property. An
        // fg-only decoration registered first (e.g. an error-token tint) keeps its fg when a bg-only
        // decoration (e.g. a modified-line band) is registered over it afterwards. Guards the fg half
        // of the per-property merge at the pack call site, which the bg-direction test cannot reach.
        let p = palette();
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &Overlay::default(),
            &ColorPolicy::default(),
            &[
                deco(DecorationLayer::Bottom, None, Some(DECO_FG)),
                deco(DecorationLayer::Bottom, Some(DECO_BG), None),
            ],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(DECO_BG),
            "the later bg-only decoration paints the background"
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(DECO_FG),
            "…and the earlier fg-only decoration keeps the foreground"
        );
    }

    #[test]
    fn a_top_fg_only_decoration_overrides_selection_foreground() {
        // The Top layer's fg branch had NO pack-site coverage (a 2-lens mutation left it a no-op with
        // every test still green), yet #452 rewrote it. Pin the ordering it encodes: `top` is applied
        // LAST, so a Top fg override beats #227 selectionForeground on a selected cell — xterm's
        // `CellColorResolver` runs its top decoration pass after the selection stage.
        let p = palette();
        let sfg = 0x00_00_FF;
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &overlay_kind(&[0, 0, 0], &[], 0x30_60_C0),
            &ColorPolicy {
                selection_fg: Some(sfg),
                ..Default::default()
            },
            &[deco(DecorationLayer::Top, None, Some(DECO_FG))],
        );
        assert_eq!(
            &got[5..8],
            &gl_rgb(DECO_FG),
            "the top decoration's fg wins over selectionForeground"
        );
        assert_ne!(&got[5..8], &gl_rgb(sfg), "not the selection foreground");
    }

    #[test]
    fn a_search_match_still_paints_solid_over_a_bottom_decoration_bg() {
        // #444 must not leak into the MATCH kind. A match is solid over ANY cell (#400 item ①, where
        // xterm and alacritty converge), so a decoration bg beneath must NOT make it blend — that
        // would reintroduce exactly the muddy tint #400 removed. Guards the decoration term staying
        // INSIDE `should_blend_kind`'s `Selection` arm.
        let p = palette();
        let m_bg = 0x30_60_C0;
        let got = pack_instances(
            &frame(&[0], &[0], &[0], &[0]),
            &p,
            true,
            &overlay_kind(&[], &[0, 0, 0], m_bg),
            &ColorPolicy::default(),
            &[deco(DecorationLayer::Bottom, Some(DECO_BG), None)],
        );
        assert_eq!(
            &got[2..5],
            &gl_rgb(m_bg),
            "a match stays solid over a decoration bg"
        );
    }
}
