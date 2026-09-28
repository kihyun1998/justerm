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

use crate::attrs::{
    BLANK_SLOT, STRIKETHROUGH, UNDERLINE, USTYLE_SHIFT, glyph_field, is_concealed, is_dim,
    is_inverse,
};

/// The underline style field's value for a single straight line — what core writes for `SGR 4`.
const SINGLE_UNDERLINE: u16 = 1 << USTYLE_SHIFT;
use crate::color::gl_rgb;
use crate::contrast::ensure_contrast_ratio;
use crate::decoration::{
    DecorationLayer, DecorationOverride, DecorationRect, decoration_override_at,
};
use crate::glyph_class::treat_glyph_as_background_color;
use crate::overlay::{
    HIGHLIGHT_BLEND_ALPHA, HighlightKind, Overlay, blend_over, composite_bg, should_blend_kind,
};
use crate::palette::{Palette, resolve_indexed_or_rgb};
use crate::render_policy::{ColorPolicy, dim_foreground, resolve_cell};

/// Floats per cell instance: `col, row, bg(3), fg(3), glyph_field, underline_fg, strike_fg,
/// bg_default`, then the four neighbours' glyph fields and their four inks. The two line inks
/// (#513, #525) are packed `0xRRGGBB` one per float; `bg_default` is `1.0` iff the cell's bg is
/// the pristine default backdrop (#455). Why: `docs/map/territory/cell-compositing.md` § The
/// packer.
pub const INSTANCE_FLOATS: usize = 20;

/// The cell's resolved background, 3 floats. The offsets below are named (#791).
pub const BG_RGB: usize = 2;
/// The cell's own resolved foreground, 3 floats.
pub const FG_RGB: usize = 5;
/// The glyph field: slot in bits 0..12, underline/strike/emoji/ink-class in the high bits.
pub const GLYPH_FIELD: usize = 8;
/// 1.0 iff this cell's bg is the pristine default backdrop (#455).
pub const BACKDROP: usize = 11;
/// `I_neighbour` handles (ADR-0019 R1.2): the glyph field of the cell above, below (#791), to the
/// left and to the right (#966), or [`BLANK_SLOT`] where that neighbour's ink is withdrawn. Four
/// consecutive floats, read by the shader as one `vec4` attribute.
pub const NEIGHBOUR_UP: usize = 12;
pub const NEIGHBOUR_DN: usize = 13;
pub const NEIGHBOUR_LT: usize = 14;
pub const NEIGHBOUR_RT: usize = 15;
/// The ink each of those neighbours draws in — its owner's (ADR-0019 R1.2) — packed `0xRRGGBB`
/// one per float, in the same order; one `vec4` attribute.
pub const NEIGHBOUR_UP_FG: usize = 16;
pub const NEIGHBOUR_DN_FG: usize = 17;
pub const NEIGHBOUR_LT_FG: usize = 18;
pub const NEIGHBOUR_RT_FG: usize = 19;

/// A decoded frame's per-cell grid: dimensions + the four parallel column arrays the packer
/// reads (all row-major, ideally length `cols*rows`). `bg`/`fg` are tagged-u32 colour refs,
/// `slots` the resolved atlas slot per cell, `flags` the `CellFlags` bits. A short/missing
/// entry resolves as `Default` / slot `0` / no flags — every cell is still emitted (#255).
pub struct Frame<'a> {
    pub cols: u32,
    pub rows: u32,
    /// The cells an open IME composition covers (#249, ADR-0028 D2), or `None`. Every stage below
    /// glyph resolution stands down inside it.
    pub preedit: Option<crate::preedit::Span>,
    pub bg: &'a [u32],
    pub fg: &'a [u32],
    pub slots: &'a [u16],
    pub flags: &'a [u16],
    /// Per-cell base codepoint — the first scalar of the resolved glyph — read only to classify
    /// the glyph ([`treat_glyph_as_background_color`]). A short/missing entry resolves as `0`.
    pub codepoints: &'a [u32],
    /// Per-cell underline colour reference (SGR 58, #520), tagged-u32 like `fg`/`bg`; `0` =
    /// `Default`, the line following the glyph ink. It reaches the underline band only (#525) and
    /// is not read inside an open composition (#711).
    pub underline_colors: &'a [u32],
}

/// Pack a [`Frame`] (row-major) into [`INSTANCE_FLOATS`] per cell: resolve colours through the
/// palette and `policy`, compose the decorations and the selection/search highlight, fold the
/// marks into the glyph field, and grant each cell its neighbours' ink. A concealed cell (hidden,
/// or blink while `blink_on` is false) shows only its background. Every cell is emitted (#255). Why each stage is as it is: `docs/map/territory/cell-compositing.md` § The packer,
/// `decoration.md` § The packer's half, `colour-policy.md` § Where the policies meet the
/// highlight.
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
        preedit,
        bg,
        fg,
        slots,
        flags,
        codepoints,
        underline_colors,
    } = *frame;
    // Reserve up front where the product fits `usize`, else grow on demand (#355).
    let cells = (cols as usize).saturating_mul(rows as usize);
    let mut out = Vec::with_capacity(cells.checked_mul(INSTANCE_FLOATS).unwrap_or(0));
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
            let cell_flags = flags.get(idx).copied().unwrap_or(0);

            // Resolve refs to packed 0xRRGGBB; unpack to GL floats only at the end.
            let bg_ref = bg.get(idx).copied().unwrap_or(0);
            let fg_ref = fg.get(idx).copied().unwrap_or(0);
            // The cell's OWN resolved fg (inverse + bold→bright) — kept undimmed for the tile-glyph
            // re-tint below, which starts from it (discarding any selectionForeground).
            let (cell_fg, cell_bg) =
                resolve_cell(fg_ref, bg_ref, cell_flags, palette, policy.bold_to_bright);
            // The highlight covering this cell — the bg channel's winner; the fg channel keys on
            // selection coverage (#430). Inside an open composition every stage below stands down
            // (ADR-0028 D2).
            let composed = preedit.is_some_and(|p| p.covers(row, col));
            // Every span lookup asks about this cell and its wide-pair partner (#454).
            let partner = crate::pair::partner_at(flags, cols, row, col);
            let kind = if composed {
                None
            } else {
                overlay.highlight_at(row, col, partner)
            };
            let is_selection = !composed && overlay.is_selected(row, col, partner);
            // #934: a hovered link draws a single underline where the cell has none; the colour stages
            // keep reading `cell_flags`.
            let line_flags =
                if overlay.is_link_hovered(row, col, partner) && cell_flags & UNDERLINE == 0 {
                    cell_flags | UNDERLINE | SINGLE_UNDERLINE
                } else {
                    cell_flags
                };
            // #226: a Powerline / box-drawing / block glyph tiles with the bg — excluded from the
            // contrast demand and re-tinted under selection (classify the base codepoint).
            let exclude =
                treat_glyph_as_background_color(codepoints.get(idx).copied().unwrap_or(0));
            // Decorations compose back-to-front around the highlight (#120/#393): base < bottom <
            // highlight < top, as absolute colours.
            let mut fg = cell_fg;
            let mut fg_overridden = false;
            let mut bg_running = cell_bg;
            // #444: whether a bottom decoration painted a bg beneath the highlight.
            let mut deco_bg = false;
            // #452: bg and fg merge independently across decorations.
            let bottom = if composed {
                DecorationOverride::default()
            } else {
                decoration_override_at(decorations, row, col, partner, DecorationLayer::Bottom)
            };
            if let Some(c) = bottom.bg {
                bg_running = c;
                deco_bg = true;
            }
            if let Some(c) = bottom.fg {
                fg = c;
                fg_overridden = true;
            }
            // The top layer, read before the ink rules because whether it takes the glyph decides them.
            let top = if composed {
                DecorationOverride::default()
            } else {
                decoration_override_at(decorations, row, col, partner, DecorationLayer::Top)
            };
            // #508: a bg-only top decoration over a background-class glyph takes the glyph, and the R1
            // ink rules stand down.
            let glyph_taken_by_decoration = exclude && top.bg.is_some() && top.fg.is_none();
            // #227 selectionForeground, selection only.
            if is_selection && let Some(sfg) = policy.selection_fg {
                fg = sfg;
            }
            // #271/#400/#444: composite the selection/search highlight onto the bg — a selection
            // blends over a real colour and paints solid over a bare default bg; a match always paints
            // solid.
            let mut eff_bg = composite_bg(
                bg_running,
                kind.is_some_and(|k| should_blend_kind(k, bg_ref, cell_flags, deco_bg)),
                kind.map(|k| overlay.colors.of(k)),
            );
            // #239/#241: a tile glyph under a selection is re-tinted toward the raw selection colour
            // (below). The line's ink forks from the glyph's here (#513): an explicit SGR 58 colour is
            // authoritative (#520) and reaches the underline only (#525); inside a composition the
            // reference is zeroed, so the line follows the pass's fg (#711).
            let ucolor_ref = if composed {
                0
            } else {
                underline_colors.get(idx).copied().unwrap_or(0)
            };
            let explicit_line =
                (ucolor_ref != 0).then(|| resolve_indexed_or_rgb(ucolor_ref, palette));
            let mut line_fg = fg;
            if is_selection && exclude && !glyph_taken_by_decoration {
                let raw_sel = overlay.colors.of(HighlightKind::Selection);
                let inverse_default_bg = is_inverse(cell_flags) && (bg_ref >> 24) == 0;
                fg = if inverse_default_bg {
                    // #453: the band with the cell taken out of the stack — over a bottom decoration's bg when
                    // one painted and the selection is the layer over it.
                    let beneath_the_band =
                        deco_bg && matches!(kind, Some(HighlightKind::Selection));
                    composite_bg(bg_running, beneath_the_band, Some(raw_sel))
                } else {
                    blend_over(cell_fg, raw_sel, HIGHLIGHT_BLEND_ALPHA)
                };
            }
            // #120/#393 top decoration: paints over the highlight, bg and fg merged per property
            // (#452).
            if let Some(c) = top.bg {
                eff_bg = c;
                // #494: a tile glyph follows a bg-only top decoration (a deliberate divergence from xterm).
            }
            if let Some(c) = top.fg {
                fg = c;
                // #513 rule 5: a decoration's fg reaches a follow-fg line; an explicit line ignores it
                // (#520).
                line_fg = c;
                fg_overridden = true;
            } else if exclude && top.bg.is_some() {
                // #494/#508: a bg-only top decoration does not write the ink channel; the glyph is dropped
                // by slot.
            }
            // The fg policy, once, against the effective bg on the undimmed fg: minimum contrast (#225)
            // first, else DIM (#232); a selected cell is not dim (#224); a tile is excluded (#226).
            let dim = is_dim(cell_flags) && !is_selection;
            let mcr = policy.min_contrast as f64;
            // #230: a decoration fg on a dim, unselected cell keeps the DIM.
            if dim && fg_overridden {
                fg = dim_foreground(fg, eff_bg);
                // #513 rule 6: a dim cell's underline is dim.
                line_fg = dim_foreground(line_fg, eff_bg);
            }
            // #226's exclusion does not reach a tile whose glyph a decoration took (#508).
            let fg_packed = if mcr > 1.0 && (!exclude || glyph_taken_by_decoration) {
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
            // #513 rules 6-7 over the line's ink, with the glyph's contrast gate; computed only when a
            // follow-fg band draws (#520, #525).
            let needs_follow_fg = (line_flags & UNDERLINE != 0 && explicit_line.is_none())
                || cell_flags & STRIKETHROUGH != 0;
            let follow_fg_line = if !needs_follow_fg {
                line_fg
            } else if mcr > 1.0 && (!exclude || glyph_taken_by_decoration) {
                let ratio = if dim { mcr / 2.0 } else { mcr };
                match ensure_contrast_ratio(eff_bg, line_fg, ratio) {
                    Some(adjusted) => adjusted,
                    None if dim && !fg_overridden => dim_foreground(line_fg, eff_bg),
                    None => line_fg,
                }
            } else if dim && !fg_overridden {
                dim_foreground(line_fg, eff_bg)
            } else {
                line_fg
            };
            // #520: an explicit SGR 58 colour is drawn raw.
            let underline_packed = explicit_line.unwrap_or(follow_fg_line);
            // The strike has no declared colour; it is the follow-fg value.
            let strike_packed = follow_fg_line;
            let bg_rgb = gl_rgb(eff_bg);
            let fg_rgb = gl_rgb(fg_packed);

            // A concealed cell points at the blank slot: zero coverage, no decoration bits,
            // so only the (already inverse-swapped) background shows.
            let field = if is_concealed(cell_flags, blink_on) {
                u32::from(BLANK_SLOT)
            } else {
                // #508: a taken glyph blanks the slot and keeps the attribute bits; its ink class (#712)
                // goes with it.
                let (slot, ink_class) = if glyph_taken_by_decoration {
                    (BLANK_SLOT, false)
                } else {
                    (slots.get(idx).copied().unwrap_or(0), exclude)
                };
                glyph_field(slot, line_flags, ink_class)
            };

            // #455: the pristine default backdrop — nothing wrote the bg channel.
            let bg_is_default_backdrop = (bg_ref >> 24) == 0
                && !is_inverse(cell_flags)
                && !deco_bg
                && top.bg.is_none()
                && kind.is_none();

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
                underline_packed as f32,
                strike_packed as f32,
                if bg_is_default_backdrop { 1.0 } else { 0.0 },
                // `I_neighbour` starts WITHDRAWN and is granted below — the safe default, since a
                // cell with no neighbour and a cell whose neighbour is across a background edge
                // must both end up blank.
                f32::from(BLANK_SLOT),
                f32::from(BLANK_SLOT),
                f32::from(BLANK_SLOT),
                f32::from(BLANK_SLOT),
                0.0,
                0.0,
                0.0,
                0.0,
            ]);
        }
    }

    // #791/#966, ADR-0019 rule 5: grant each cell its neighbours' ink where the resolved
    // backgrounds match, in a second walk over what was packed.
    let bg_at = |out: &[f32], i: usize| {
        let b = i * INSTANCE_FLOATS + BG_RGB;
        [out[b], out[b + 1], out[b + 2]]
    };
    // Re-pack the neighbour's resolved foreground into the `0xRRGGBB` float the line inks already
    // use. Exact: these floats were divided by 255 from integers a moment ago.
    let fg_packed_at = |out: &[f32], i: usize| {
        let b = i * INSTANCE_FLOATS + FG_RGB;
        let ch = |v: f32| ((v * 255.0).round() as u32) & 0xFF;
        ((ch(out[b]) << 16) | (ch(out[b + 1]) << 8) | ch(out[b + 2])) as f32
    };
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
            let mine = bg_at(&out, idx);
            if row > 0 {
                let up = idx - cols as usize;
                if bg_at(&out, up) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_UP] =
                        out[up * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_UP_FG] = fg_packed_at(&out, up);
                }
            }
            if row + 1 < rows {
                let dn = idx + cols as usize;
                if bg_at(&out, dn) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_DN] =
                        out[dn * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_DN_FG] = fg_packed_at(&out, dn);
                }
            }
            // The same grant across (#966).
            if col > 0 {
                let lt = idx - 1;
                if bg_at(&out, lt) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_LT] =
                        out[lt * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_LT_FG] = fg_packed_at(&out, lt);
                }
            }
            if col + 1 < cols {
                let rt = idx + 1;
                if bg_at(&out, rt) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_RT] =
                        out[rt * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_RT_FG] = fg_packed_at(&out, rt);
                }
            }
        }
    }
    // A wide pair's receivers must agree (`docs/map/invariant/a-span-covers-a-wide-pair-whole.md`).
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
            for (field, fg_field, src_row) in [
                (NEIGHBOUR_UP, NEIGHBOUR_UP_FG, row.checked_sub(1)),
                (
                    NEIGHBOUR_DN,
                    NEIGHBOUR_DN_FG,
                    (row + 1 < rows).then_some(row + 1),
                ),
            ] {
                let Some(src) = src_row else { continue };
                let Some(partner_col) = crate::pair::partner_at(flags, cols, src, col) else {
                    continue;
                };
                let mate = row as usize * cols as usize + partner_col as usize;
                let (a, b) = (
                    idx * INSTANCE_FLOATS + field,
                    mate * INSTANCE_FLOATS + field,
                );
                // Blank both receivers, not just this one.
                if out[a] != out[b] {
                    out[a] = f32::from(BLANK_SLOT);
                    out[b] = f32::from(BLANK_SLOT);
                    out[idx * INSTANCE_FLOATS + fg_field] = 0.0;
                    out[mate * INSTANCE_FLOATS + fg_field] = 0.0;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
