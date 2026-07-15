//! Marker-anchored decoration rects (#120/#393) — pure, host-testable.
//!
//! A decoration is a per-cell **background / foreground override** on a two-layer stack: a `Bottom`
//! decoration paints *under* the selection/search highlight (the glyph stays legible above it), a
//! `Top` decoration paints *over* it. The consumer owns the model (justerm-web `DecorationRegistry`,
//! #197) and projects the on-viewport rects each frame from core's markers; the renderer only
//! composites the rects it is handed — same consumer-push seam as the selection [`overlay`], so the
//! renderer stays policy-agnostic. Ports justerm-web `decorations.ts` (`DecorationRect`) +
//! `decoration-render.ts` (`decorationAt`, the layer query + the compose order).
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

/// The decoration covering viewport cell `(row, col)` on `layer`, or `None`. Columns are inclusive
/// (`left..=right`). When several decorations on the same layer overlap the cell, the **last** wins —
/// later registration paints on top (justerm-web `decorationAt` / xterm's draw order).
pub fn decoration_at(
    rects: &[DecorationRect],
    row: u32,
    col: u32,
    layer: DecorationLayer,
) -> Option<&DecorationRect> {
    let mut hit = None;
    for r in rects {
        if r.layer == layer && r.row == row && col >= r.left && col <= r.right {
            hit = Some(r);
        }
    }
    hit
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

    #[test]
    fn decoration_at_matches_the_inclusive_span_on_its_layer() {
        let rects = parse_decorations(&[1, 2, 4, 0, 5, NO_REF]);
        let at = |col| decoration_at(&rects, 1, col, DecorationLayer::Bottom).map(|r| r.bg);
        assert_eq!(at(1), None, "left of the span");
        assert_eq!(at(2), Some(Some(5)), "left edge inclusive");
        assert_eq!(at(4), Some(Some(5)), "right edge inclusive");
        assert_eq!(at(5), None, "right of the span");
        assert_eq!(
            decoration_at(&rects, 0, 3, DecorationLayer::Bottom),
            None,
            "another row"
        );
        assert_eq!(
            decoration_at(&rects, 1, 3, DecorationLayer::Top),
            None,
            "another layer"
        );
    }

    #[test]
    fn the_last_overlapping_decoration_on_a_layer_wins() {
        // Two Bottom decorations both cover (0, 0..=2); the later one (bg 7) paints on top.
        let rects = parse_decorations(&[0, 0, 2, 0, 6, NO_REF, 0, 0, 2, 0, 7, NO_REF]);
        assert_eq!(
            decoration_at(&rects, 0, 1, DecorationLayer::Bottom)
                .unwrap()
                .bg,
            Some(7)
        );
        // A Top decoration on the same cell is independent (queried per layer).
        let mixed = parse_decorations(&[0, 0, 2, 0, 6, NO_REF, 0, 0, 2, 1, 8, NO_REF]);
        assert_eq!(
            decoration_at(&mixed, 0, 1, DecorationLayer::Bottom)
                .unwrap()
                .bg,
            Some(6)
        );
        assert_eq!(
            decoration_at(&mixed, 0, 1, DecorationLayer::Top)
                .unwrap()
                .bg,
            Some(8)
        );
    }
}
