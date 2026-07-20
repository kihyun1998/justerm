//! Marker-anchored decoration rects (#120/#393) — pure, host-testable.
//!
//! A decoration is a per-cell **background / foreground override** on a two-layer stack: a `Bottom`
//! decoration paints *under* the selection/search highlight (the glyph stays legible above it), a
//! `Top` decoration paints *over* it. The consumer owns the model (justerm-web `DecorationRegistry`,
//! #197) and projects the on-viewport rects each frame from core's markers; the renderer only
//! composites the rects it is handed — same consumer-push seam as the selection [`overlay`], so the
//! renderer stays policy-agnostic. The wire shape mirrors justerm-web `decorations.ts`
//! (`DecorationRect`); the *resolution* is the renderer's alone — web's `decoration-render.ts`
//! (`decorationAt`) was the pre-#273 consumer-side compositor and no longer exists, so
//! [`decoration_override_at`] has no sibling to stay byte-neutral with and follows xterm directly.
//!
//! Colours are **absolute** packed `0xRRGGBB` — the consumer owns its theme and resolves a decoration
//! to a concrete colour before pushing it, so the renderer uses it verbatim (unlike a *cell* colour,
//! which arrives as a theme-agnostic ref for the renderer to resolve). This matches justerm-web, whose
//! `composeCellColors` writes a decoration's colour straight to the cell without any palette resolve.
//! Either override may be **absent** (a decoration can tint only the bg, only the fg, or both), encoded
//! on the wire by the [`NO_REF`] sentinel — which cannot collide with a 24-bit `0xRRGGBB` (its top byte
//! is always `0`).
//!
//! [`overlay`]: crate::overlay

/// Which layer a decoration paints on (justerm-web `DecorationLayer`): `Bottom` overrides the cell
/// background *beneath* the glyph and under the highlight; `Top` paints *over* the highlight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecorationLayer {
    Bottom,
    Top,
}

/// `u32`s per rect in the flat wire directory: `row, left, right, layer, bg, fg`. `layer` is `0` =
/// Bottom / `1` = Top; `bg`/`fg` are absolute `0xRRGGBB` colours or [`NO_REF`] for "no override".
pub const DECORATION_STRIDE: usize = 6;

/// The wire sentinel for an absent bg/fg override. A decoration colour is a 24-bit `0xRRGGBB` (top byte
/// always `0`), so `u32::MAX` (top byte `0xFF`) can never collide with a real colour — and `0` is *not*
/// free (it is a valid colour, black).
pub const NO_REF: u32 = u32::MAX;

/// One decoration rect projected onto the viewport: an inclusive column span `left..=right` on `row`,
/// its layer, and its optional **absolute** `0xRRGGBB` bg/fg overrides (used verbatim; not resolved).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecorationRect {
    pub row: u32,
    pub left: u32,
    pub right: u32,
    pub layer: DecorationLayer,
    /// Background override (`0xRRGGBB`), or `None` (a fg-only decoration).
    pub bg: Option<u32>,
    /// Foreground override (`0xRRGGBB`), or `None` (a bg-only decoration).
    pub fg: Option<u32>,
}

/// Parse a flat [`DECORATION_STRIDE`] wire directory into rects. A tail shorter than one stride is
/// ignored (the decoder never emits one); an unknown `layer` value folds to `Bottom` (defensive — the
/// consumer only ever sends `0`/`1`). [`NO_REF`] decodes to `None`.
pub fn parse_decorations(flat: &[u32]) -> Vec<DecorationRect> {
    let mut out = Vec::with_capacity(flat.len() / DECORATION_STRIDE);
    let mut i = 0;
    while i + DECORATION_STRIDE <= flat.len() {
        let layer = if flat[i + 3] == 1 {
            DecorationLayer::Top
        } else {
            DecorationLayer::Bottom
        };
        let opt = |v: u32| if v == NO_REF { None } else { Some(v) };
        out.push(DecorationRect {
            row: flat[i],
            left: flat[i + 1],
            right: flat[i + 2],
            layer,
            bg: opt(flat[i + 4]),
            fg: opt(flat[i + 5]),
        });
        i += DECORATION_STRIDE;
    }
    out
}

/// The bg/fg overrides in force on a cell after merging every decoration that covers it on one layer
/// — the *resolved pair*, not any one decoration. Either half is `None` when no covering decoration
/// set it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DecorationOverride {
    /// The winning background override, or `None` if no covering decoration set one.
    pub bg: Option<u32>,
    /// The winning foreground override, or `None` if no covering decoration set one.
    pub fg: Option<u32>,
}

/// The decoration overrides in force at viewport cell `(row, col)` on `layer`. Columns are inclusive
/// (`left..=right`).
///
/// **`bg` and `fg` resolve INDEPENDENTLY** (#452), each last-wins across *all* covering decorations
/// on the layer — so a bg-only decoration and an fg-only one on the same cell both apply, and a later
/// decoration that sets only one property leaves the other's winner intact.
///
/// The reference is xterm's **webgl** path specifically (`addon-webgl/CellColorResolver`): its
/// `DecorationService.forEachDecorationAtCell` invokes the callback for **every** matching decoration
/// and `CellColorResolver` accumulates `$bg` / `$fg` in two **separate** `if`s, matching the public
/// contract that `backgroundColor` and `foregroundColor` are independent options each resolved to
/// "the last registered decoration" (`xterm.d.ts`). xterm's **DOM** renderer accumulates per-property
/// too, but handles *layers* differently — one pass over all layers with an `isTop` latch, so a bottom
/// decoration ordered after a top one is dropped and a top decoration suppresses the selection bg
/// entirely (`DomRendererRowFactory`). A WebGL renderer follows the webgl path; the divergence is
/// xterm-internal, not a choice justerm is making.
///
/// Note this is the *merge* rule only. What the merged `bg` is then **used for** deliberately departs
/// from xterm: #444 feeds it into the selection's blend-vs-solid decision, whereas xterm decides that
/// from the cell's own bg alone and discards the decoration. Do not "restore parity" here by reading
/// this paragraph as an endorsement — see `overlay::should_blend_kind`.
///
/// **Precedence is wire order.** "Last wins" means last in the `rects` slice, so the consumer must
/// push in the order it wants resolved. (xterm's own ordering is its line-cache bucket, which starts
/// as registration order but is perturbed by buffer trims/inserts re-appending decorations — so
/// "last registered" is an approximation upstream too.)
///
/// Returning the merged pair rather than a borrowed [`DecorationRect`] is the point: a *rect* is the
/// wrong unit of resolution. Picking one whole rect made an fg-only decoration discard an earlier
/// decoration's background — and since #444 promoted "a bottom decoration supplied a bg" to a
/// blend-vs-solid input, that discard flipped the *compositing mode*, not merely a colour.
pub fn decoration_override_at(
    rects: &[DecorationRect],
    row: u32,
    col: u32,
    layer: DecorationLayer,
) -> DecorationOverride {
    let mut out = DecorationOverride::default();
    for r in rects {
        if r.layer == layer && r.row == row && col >= r.left && col <= r.right {
            // Per-property last-wins: only a rect that *sets* the property overwrites the winner.
            if r.bg.is_some() {
                out.bg = r.bg;
            }
            if r.fg.is_some() {
                out.fg = r.fg;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_stride_6_and_the_no_ref_sentinel() {
        // one Bottom rect (row 1, cols 2..=4, bg Rgb, no fg) + one Top rect (row 0, col 0, fg only).
        let flat = [
            1,
            2,
            4,
            0,
            (2 << 24) | 0xE0_6C_75,
            NO_REF,
            0,
            0,
            0,
            1,
            NO_REF,
            (1 << 24) | 3,
        ];
        let rects = parse_decorations(&flat);
        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            DecorationRect {
                row: 1,
                left: 2,
                right: 4,
                layer: DecorationLayer::Bottom,
                bg: Some((2 << 24) | 0xE0_6C_75),
                fg: None,
            }
        );
        assert_eq!(
            rects[1],
            DecorationRect {
                row: 0,
                left: 0,
                right: 0,
                layer: DecorationLayer::Top,
                bg: None,
                fg: Some((1 << 24) | 3),
            }
        );
    }

    #[test]
    fn a_short_tail_is_ignored() {
        // 6 valid u32s + a 3-u32 tail: only the first rect parses.
        let flat = [0, 0, 0, 0, NO_REF, NO_REF, 9, 9, 9];
        assert_eq!(parse_decorations(&flat).len(), 1);
    }

    /// No override in force — what an uncovered cell resolves to.
    const NONE: DecorationOverride = DecorationOverride { bg: None, fg: None };

    #[test]
    fn decoration_override_at_matches_the_inclusive_span_on_its_layer() {
        let rects = parse_decorations(&[1, 2, 4, 0, 5, NO_REF]);
        let at = |col| decoration_override_at(&rects, 1, col, DecorationLayer::Bottom).bg;
        assert_eq!(at(1), None, "left of the span");
        assert_eq!(at(2), Some(5), "left edge inclusive");
        assert_eq!(at(4), Some(5), "right edge inclusive");
        assert_eq!(at(5), None, "right of the span");
        assert_eq!(
            decoration_override_at(&rects, 0, 3, DecorationLayer::Bottom),
            NONE,
            "another row"
        );
        assert_eq!(
            decoration_override_at(&rects, 1, 3, DecorationLayer::Top),
            NONE,
            "another layer"
        );
    }

    #[test]
    fn the_last_overlapping_decoration_on_a_layer_wins_per_property() {
        // Two Bottom decorations both cover (0, 0..=2); the later one (bg 7) paints on top.
        let rects = parse_decorations(&[0, 0, 2, 0, 6, NO_REF, 0, 0, 2, 0, 7, NO_REF]);
        assert_eq!(
            decoration_override_at(&rects, 0, 1, DecorationLayer::Bottom).bg,
            Some(7)
        );
        // A Top decoration on the same cell is independent (resolved per layer).
        let mixed = parse_decorations(&[0, 0, 2, 0, 6, NO_REF, 0, 0, 2, 1, 8, NO_REF]);
        assert_eq!(
            decoration_override_at(&mixed, 0, 1, DecorationLayer::Bottom).bg,
            Some(6)
        );
        assert_eq!(
            decoration_override_at(&mixed, 0, 1, DecorationLayer::Top).bg,
            Some(8)
        );
    }

    #[test]
    fn a_bg_only_and_an_fg_only_decoration_on_one_cell_both_apply() {
        // #452: the two properties resolve INDEPENDENTLY, so the pair survives together. Picking one
        // whole rect (the old `decoration_at`) discarded the bg-only decoration entirely — and since
        // #444 the bg also decides blend-vs-solid, so the loss flipped the compositing mode too.
        let rects = parse_decorations(&[0, 0, 2, 0, 6, NO_REF, 0, 0, 2, 0, NO_REF, 9]);
        assert_eq!(
            decoration_override_at(&rects, 0, 1, DecorationLayer::Bottom),
            DecorationOverride {
                bg: Some(6),
                fg: Some(9)
            },
        );
    }

    #[test]
    fn a_later_decoration_setting_one_property_leaves_the_others_winner_intact() {
        // The ordering half of per-property last-wins, in BOTH directions: a later bg-only rect must
        // not clear an earlier fg, and a later fg-only rect must not clear an earlier bg. Guards
        // against a merge written as "overwrite both from the last rect that set either".
        let bg_last = parse_decorations(&[0, 0, 2, 0, 6, 9, 0, 0, 2, 0, 7, NO_REF]);
        assert_eq!(
            decoration_override_at(&bg_last, 0, 1, DecorationLayer::Bottom),
            DecorationOverride {
                bg: Some(7),
                fg: Some(9)
            },
            "later bg wins; the earlier fg survives"
        );
        let fg_last = parse_decorations(&[0, 0, 2, 0, 6, 9, 0, 0, 2, 0, NO_REF, 4]);
        assert_eq!(
            decoration_override_at(&fg_last, 0, 1, DecorationLayer::Bottom),
            DecorationOverride {
                bg: Some(6),
                fg: Some(4)
            },
            "later fg wins; the earlier bg survives"
        );
    }

    #[test]
    fn a_non_covering_decoration_does_not_contribute_either_property() {
        // The merge must respect the span, not just the layer — a rect on the same layer that does
        // not cover the cell contributes nothing, even though it sets both properties.
        let rects = parse_decorations(&[0, 0, 1, 0, 6, NO_REF, 0, 5, 7, 0, 7, 9]);
        assert_eq!(
            decoration_override_at(&rects, 0, 0, DecorationLayer::Bottom),
            DecorationOverride {
                bg: Some(6),
                fg: None
            },
        );
    }
}
