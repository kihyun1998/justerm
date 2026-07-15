//! Selection / search overlay compositing — pure, host-testable (#271).
//!
//! The engine owns the selection *model* (scrollback-aware, ADR-0002) and projects it onto the
//! viewport as stride-3 `(row, left, right)` spans; the wasm decoder already exposes them
//! (`justerm-wasm-decode` `selectionSpans` / `matchSpans`, `OVERLAY_STRIDE`, #108). This module is
//! the renderer's half: it turns those spans + the consumer's injected blend colours (#115) into a
//! composited per-cell background, choosing **blend** (alpha-blend over a non-default / inverse cell
//! so its own colour shows through) vs **solid** (crisp fill over the default terminal background).
//!
//! Ports the shipped, tested justerm-web logic (`overlay.ts` `highlightAt`, `render-policy.ts`
//! `blendOver`, `decoration-render.ts`'s highlight branch) to Rust — the *background* half only. The
//! foreground long-tail (selectionForeground #227, minimumContrastRatio #225, DIM un-dim #224/#232,
//! inverse-default tile #241, excludeFromContrast #226) is deliberately left to #272.
//!
//! It does **not** port the web `overlay-compose.ts` delta machinery (`prevOverlay` /
//! `overlayCellKeys` / `overlayRepaintKeys`, #140): that is a beamterm-specific workaround for its
//! incremental per-cell model. The renderer re-packs the whole viewport every frame and the #263
//! upload diff re-sends only the cells whose packed bytes changed, so a cell that gains or loses a
//! highlight re-uploads for free — as long as compositing happens at pack time (which it does).
//!
//! A **search match** paints a **solid** background — the match colour opaque over any cell, matching
//! xterm.js (`CellColorResolver` overwrites the bg from the match decoration, alpha dropped) and
//! alacritty (`compute_cell_rgb` forces `bg_alpha = 1.0`): a match's job is to be *found*, so on a
//! coloured cell it must read crisp, not as a muddy 0x80 tint (#400 item ①, [`should_blend_kind`]). A
//! **selection** still blends over a non-default / inverse cell (its own colour shows through) and
//! paints solid only over the default background. Two xterm-parity items remain deferred (#400): the
//! *focused/active* match layered above the selection (needs an active-match wire signal — and the
//! two references even disagree on it), and a selection blending over a bottom-decoration bg vs the
//! cell's own bg.

use crate::attrs::is_inverse;

/// `u32`s per overlay span in the `(row, left, right)` directory — mirrors `justerm-wasm-decode`
/// `OVERLAY_STRIDE`. `left`/`right` are inclusive viewport columns; `row` is a viewport row.
pub const OVERLAY_STRIDE: usize = 3;

/// Alpha of the selection / search highlight blend (0..=255), matching xterm's `CellColorResolver`
/// (it forces the selection colour to `0x80` before blending). Web sibling: `render-policy.ts`
/// `HIGHLIGHT_BLEND_ALPHA`.
///
/// `0x80 / 0xFF = 128/255` is not a half of any integer channel delta (`256` and `255` are coprime,
/// so `(over-base) * 128/255` is a whole number for every `over-base` in `-255..=255`), so the
/// per-channel `round` never lands on a `.5` tie — Rust's round-half-away and JS's `Math.round`
/// round-half-up can only disagree there, and here they cannot. Valid as long as the alpha stays
/// `0x80`; a different alpha would reopen the tie question.
pub const HIGHLIGHT_BLEND_ALPHA: u8 = 0x80;

/// Which overlay covers a cell — the live selection or a search match. They are separate wire
/// groups with separate blend colours; on a cell both cover, the selection wins (`overlay.ts`
/// `highlightAt`: a selection blend is visually primary over a match).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighlightKind {
    Selection,
    Match,
}

/// The two injected blend colours (consumer policy #115), packed `0xRRGGBB`. The renderer is
/// theme-agnostic: the consumer resolves its palette to these before handing them over.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HighlightColors {
    pub selection_bg: u32,
    pub match_bg: u32,
}

impl HighlightColors {
    /// The blend colour for a kind.
    pub fn of(&self, kind: HighlightKind) -> u32 {
        match kind {
            HighlightKind::Selection => self.selection_bg,
            HighlightKind::Match => self.match_bg,
        }
    }
}

/// One frame's overlay directories + the resolved blend colours. Borrowed — the caller owns the
/// span buffers (the decoder's views). Empty directories mean "no highlight".
#[derive(Clone, Copy, Default)]
pub struct Overlay<'a> {
    /// Live-selection spans, [`OVERLAY_STRIDE`] `u32`s each.
    pub selection: &'a [u32],
    /// Search-match spans, same stride.
    pub matches: &'a [u32],
    pub colors: HighlightColors,
}

impl Overlay<'_> {
    /// The highlight kind covering viewport cell `(row, col)`, or `None`. Columns are inclusive
    /// (`left..=right`). A selection covering the cell wins over a match on the same cell, whatever
    /// order the spans sit in the directories (`overlay.ts` `highlightAt`).
    pub fn highlight_at(&self, row: u32, col: u32) -> Option<HighlightKind> {
        if covers(self.selection, row, col) {
            Some(HighlightKind::Selection)
        } else if covers(self.matches, row, col) {
            Some(HighlightKind::Match)
        } else {
            None
        }
    }

    /// The blend colour covering `(row, col)`, or `None` — [`highlight_at`](Self::highlight_at)
    /// resolved through [`HighlightColors`].
    pub fn color_at(&self, row: u32, col: u32) -> Option<u32> {
        self.highlight_at(row, col).map(|k| self.colors.of(k))
    }
}

/// Does any `(row, left, right)` span in `flat` cover cell `(row, col)`? A malformed tail shorter
/// than [`OVERLAY_STRIDE`] is ignored (the decoder never emits one).
fn covers(flat: &[u32], row: u32, col: u32) -> bool {
    let mut i = 0;
    while i + OVERLAY_STRIDE <= flat.len() {
        let (r, left, right) = (flat[i], flat[i + 1], flat[i + 2]);
        if r == row && col >= left && col <= right {
            return true;
        }
        i += OVERLAY_STRIDE;
    }
    false
}

/// Composite `over` onto `base` at `alpha` (0..=255), per channel:
/// `out = base + round((over - base) * alpha/255)`, on packed `0xRRGGBB`. This is xterm's
/// `rgba.blend` channel math (`common/Color.ts`), mirrored by justerm-web `render-policy.ts`
/// `blendOver` — the integer form, so the result matches the reference to the byte (an `f32`
/// intermediate would drift). The alpha lives in the call, not the colour.
pub fn blend_over(base: u32, over: u32, alpha: u8) -> u32 {
    let a = alpha as f32 / 255.0;
    let chan = |shift: u32| -> u32 {
        let b = ((base >> shift) & 0xFF) as f32;
        let o = ((over >> shift) & 0xFF) as f32;
        (b + ((o - b) * a).round()) as u32
    };
    (chan(16) << 16) | (chan(8) << 8) | chan(0)
}

/// Whether a cell's highlight must **blend** (vs paint solid): true iff the cell is inverse, or its
/// background reference is not `Default` (a non-zero tag byte — Indexed or Rgb). Mirrors justerm-web
/// `render-core.ts:156` (`inverse || bgRef >>> 24 !== 0`). A solid fill (crisp highlight) is only
/// right over the default terminal background; anywhere else the cell's own colour must show through.
///
/// `bg_ref` is the **pre-inverse** colour reference (the tagged `u32` the decoder ships), and
/// `flags` the cell's `CellFlags` — both read before the pack applies inverse.
pub fn should_blend(bg_ref: u32, flags: u16) -> bool {
    is_inverse(flags) || (bg_ref >> 24) != 0
}

/// Whether a highlight of `kind` must **blend** over the cell (vs paint solid). A **selection**
/// defers to [`should_blend`] — it blends over an inverse / non-default-bg cell so the cell's own
/// colour shows through, and paints solid only over the default terminal background. A **search
/// match** is **always solid**, whatever the cell: xterm.js (`CellColorResolver` overwrites the bg
/// from the match decoration, `$bg = rgba >> 8 & RGB_MASK`, alpha dropped) and alacritty
/// (`compute_cell_rgb` forces `bg_alpha = 1.0` for the search colour) both override a match's
/// background opaquely — a match's job is to be *found*, so on a coloured cell it must read as a
/// crisp colour, not a muddy 50% tint of the cell it landed on (#400).
pub fn should_blend_kind(kind: HighlightKind, bg_ref: u32, flags: u16) -> bool {
    matches!(kind, HighlightKind::Selection) && should_blend(bg_ref, flags)
}

/// A cell's background after compositing its highlight, all in packed `0xRRGGBB`. `bg` is the cell's
/// already-resolved, already-inverse-swapped background (what it shows on screen). With no highlight
/// the background is returned unchanged; a `blend` cell alpha-blends the highlight over it, the rest
/// paint it solid.
pub fn composite_bg(bg: u32, blend: bool, highlight: Option<u32>) -> u32 {
    match highlight {
        None => bg,
        Some(hl) if blend => blend_over(bg, hl, HIGHLIGHT_BLEND_ALPHA),
        Some(hl) => hl,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::INVERSE;

    // --- highlight_at: span coverage + selection-over-match precedence ---

    #[test]
    fn a_span_covers_its_inclusive_column_range_on_its_row() {
        let ov = Overlay {
            selection: &[1, 2, 4], // row 1, cols 2..=4
            ..Default::default()
        };
        assert_eq!(ov.highlight_at(1, 1), None, "left of the span");
        assert_eq!(
            ov.highlight_at(1, 2),
            Some(HighlightKind::Selection),
            "left edge inclusive"
        );
        assert_eq!(
            ov.highlight_at(1, 4),
            Some(HighlightKind::Selection),
            "right edge inclusive"
        );
        assert_eq!(ov.highlight_at(1, 5), None, "right of the span");
        assert_eq!(ov.highlight_at(0, 3), None, "another row");
    }

    #[test]
    fn a_match_span_is_its_own_kind() {
        let ov = Overlay {
            matches: &[0, 0, 3, 2, 1, 1], // two triples: (0,0..=3) and (2,1..=1)
            ..Default::default()
        };
        assert_eq!(ov.highlight_at(0, 2), Some(HighlightKind::Match));
        assert_eq!(ov.highlight_at(2, 1), Some(HighlightKind::Match));
        assert_eq!(ov.highlight_at(2, 0), None);
    }

    #[test]
    fn a_selection_wins_over_a_match_on_the_same_cell_regardless_of_order() {
        // Both cover (0,0..=2). Selection must win even though `matches` would be walked second —
        // the precedence is by kind, not directory order (overlay.ts highlightAt).
        let ov = Overlay {
            selection: &[0, 0, 2],
            matches: &[0, 0, 2],
            ..Default::default()
        };
        assert_eq!(ov.highlight_at(0, 1), Some(HighlightKind::Selection));
    }

    // --- blend_over: xterm's integer channel math, matched to the byte ---

    #[test]
    fn blend_over_matches_xterm_rgba_blend_at_alpha_128() {
        // alpha 0x80 → a = 128/255 = 0.50196. Worked by hand, NOT the way the code computes it:
        //   black under white:  0 + round(255 * 0.50196) = round(128.0) = 128 → 0x80 each channel.
        assert_eq!(blend_over(0x00_00_00, 0xFF_FF_FF, 0x80), 0x80_80_80);
        //   0x204060 under 0x800000:
        //     r: 32 + round((128-32)*a)=32+round(48.19)=80=0x50
        //     g: 64 + round((0  -64)*a)=64+round(-32.13)=32=0x20
        //     b: 96 + round((0  -96)*a)=96+round(-48.19)=48=0x30
        assert_eq!(blend_over(0x20_40_60, 0x80_00_00, 0x80), 0x50_20_30);
    }

    #[test]
    fn blend_over_is_identity_at_the_alpha_extremes() {
        assert_eq!(
            blend_over(0x12_34_56, 0xAB_CD_EF, 0x00),
            0x12_34_56,
            "alpha 0 keeps base"
        );
        assert_eq!(
            blend_over(0x12_34_56, 0xAB_CD_EF, 0xFF),
            0xAB_CD_EF,
            "alpha 255 is over"
        );
    }

    // --- should_blend: inverse OR non-default bg ref ---

    #[test]
    fn a_default_bg_non_inverse_cell_paints_solid() {
        // bg ref Default = tag byte 0; no inverse → solid (blend = false).
        assert!(!should_blend(0x00_00_00_00, 0));
    }

    #[test]
    fn an_indexed_or_rgb_bg_blends() {
        assert!(should_blend(0x01_00_00_05, 0), "Indexed(5): tag 1");
        assert!(should_blend(0x02_E0_6C_75, 0), "Rgb: tag 2");
    }

    #[test]
    fn an_inverse_cell_blends_even_with_a_default_bg() {
        // An inverse cell's shown bg is the swapped-in fg — a real colour, so it must blend.
        assert!(should_blend(0x00_00_00_00, INVERSE));
    }

    // --- should_blend_kind: a selection defers to should_blend, a match is ALWAYS solid (#400) ---

    #[test]
    fn a_selection_blends_exactly_when_should_blend_says_so() {
        // A selection defers to should_blend: solid on a default-bg cell, blend on a coloured/inverse one.
        assert!(
            !should_blend_kind(HighlightKind::Selection, 0x00_00_00_00, 0),
            "default bg → solid"
        );
        assert!(
            should_blend_kind(HighlightKind::Selection, 0x02_E0_6C_75, 0),
            "Rgb bg → blend"
        );
        assert!(
            should_blend_kind(HighlightKind::Selection, 0x00_00_00_00, INVERSE),
            "inverse → blend"
        );
    }

    #[test]
    fn a_search_match_never_blends_even_on_a_coloured_or_inverse_cell() {
        // xterm/alacritty override a match's bg SOLID regardless of the cell (#400) — the very cell
        // properties that make a SELECTION blend must NOT make a match blend, or a match on coloured
        // TUI output reads as a muddy tint instead of a crisp, findable colour.
        assert!(
            !should_blend_kind(HighlightKind::Match, 0x00_00_00_00, 0),
            "default bg → solid"
        );
        assert!(
            !should_blend_kind(HighlightKind::Match, 0x02_E0_6C_75, 0),
            "Rgb bg → still solid"
        );
        assert!(
            !should_blend_kind(HighlightKind::Match, 0x01_00_00_05, 0),
            "Indexed bg → still solid"
        );
        assert!(
            !should_blend_kind(HighlightKind::Match, 0x00_00_00_00, INVERSE),
            "inverse → still solid"
        );
    }

    // --- composite_bg: the whole rule wired together ---

    #[test]
    fn composite_leaves_an_unhighlighted_cell_untouched() {
        assert_eq!(composite_bg(0x12_34_56, false, None), 0x12_34_56);
        assert_eq!(composite_bg(0x12_34_56, true, None), 0x12_34_56);
    }

    #[test]
    fn composite_paints_solid_or_blends_by_the_flag() {
        let hl = 0x80_00_00;
        assert_eq!(
            composite_bg(0x20_40_60, false, Some(hl)),
            hl,
            "solid replaces the bg"
        );
        assert_eq!(
            composite_bg(0x20_40_60, true, Some(hl)),
            blend_over(0x20_40_60, hl, HIGHLIGHT_BLEND_ALPHA),
            "blend composites over the bg",
        );
    }
}
