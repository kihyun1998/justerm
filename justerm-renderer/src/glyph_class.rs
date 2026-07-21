//! Glyph classes that tile seamlessly with the adjacent background (#226/#272) — pure, host-testable.
//!
//! Powerline separators and box-drawing / block elements butt cleanly against the neighbouring cell.
//! A `minimumContrastRatio` nudge on one would shift its colour and open a visible seam, so xterm
//! excludes them from the contrast demand (`excludeFromContrastRatioDemands`), and re-tints them
//! toward the selection colour rather than standing them out as a hard stroke (#239). Ported from
//! xterm's `RendererUtils`, and since #507 **unioned with what this crate draws itself** — see
//! [`treat_glyph_as_background_color`]. (justerm-web carried a `glyph-class.ts` twin until #504
//! deleted it with the widget's compositing half — this is now the family's only copy.)

/// Powerline symbols, `U+E0A4..=U+E0D6` — the full range xterm treats as a background colour (not the
/// narrower restricted set it uses for glyph rescaling).
fn is_powerline_glyph(codepoint: u32) -> bool {
    (0xE0A4..=0xE0D6).contains(&codepoint)
}

/// Box-drawing (`U+2500..=U+257F`) + block elements (`U+2580..=U+259F`).
fn is_box_or_block_glyph(codepoint: u32) -> bool {
    (0x2500..=0x259F).contains(&codepoint)
}

/// Whether a glyph is meant to tile with the background — xterm's `treatGlyphAsBackgroundColor`.
/// Such a cell's fg is excluded from the `minimumContrastRatio` correction (so the glyph keeps butting
/// cleanly against its neighbour), re-tinted under a selection (#239), and taken over by a bg-only TOP
/// decoration (#494) — three consequences of one premise: the glyph is background, not text. Classify a cell's **base**
/// codepoint (the first scalar of the resolved symbol).
///
/// # A combined cell classifies from its base — declared divergence from xterm (#495)
///
/// A tile glyph carrying a combining mark (`█` + `U+0301`, `U+E0B0` + `U+0301`) is still a tile here.
/// The rule is **classify what is painted**: [`glyph_resolve`](crate::glyph_resolve) rasterises the
/// whole grapheme when the cell carries a cluster (`Cells::clusters`, #285 — pinned by
/// `a_cluster_override_rasterises_the_whole_grapheme_not_the_base_codepoint`), and a combining mark
/// only *adds* ink to that bitmap. Coverage never drops, so a cell whose base tiles still butts
/// against its neighbour, and a contrast nudge on it would still open the seam #226 exists to avoid.
///
/// xterm offers no single rule to match — **its two call sites disagree for exactly this input**:
/// - `addons/addon-webgl/src/CellColorResolver.ts:133` classifies `cell.getCode()`, which for a
///   combined cell is `combinedData.charCodeAt(combinedData.length - 1)` — the **last** UTF-16 unit
///   (`src/common/buffer/CellData.ts:52-56`). A block + combining acute therefore does *not* tile.
/// - `addons/addon-webgl/src/TextureAtlas.ts:538` classifies `chars.charCodeAt(0)` — the **first**
///   unit — and so does tile. The same file guards its two sibling classifiers (`isPowerlineGlyph`,
///   `isRestrictedPowerlineGlyph`, `:536-537`) with `chars.length === 1`, leaving `:538` unguarded:
///   the inconsistency is inside one file, not just across two.
///
/// Both read a UTF-16 **code unit**, so on an astral base (or an astral trailing scalar) they
/// classify a lone surrogate, which lands in neither range by construction. This function takes a
/// `u32` **scalar** instead.
///
/// # Why the sibling classifier answers the opposite way
///
/// [`emoji`](crate::emoji) deliberately does *not* classify a cluster from its base — it uses a
/// structural signal (ZWJ joiner / emoji-presentation lead) and rejects `width() >= 2` because that
/// falsely matches a wide text base plus a combining mark. That is not a contradiction: the two
/// classifiers answer different questions. Emoji asks **what the cluster is** (a ZWJ family is one
/// emoji, not a base with attachments), so the whole cluster decides. Tiling asks **how much of the
/// cell the ink covers**, and coverage is set by the base — a mark can only add to it.
///
/// # The range is xterm's list UNIONED with what this crate draws (#507)
///
/// xterm's two ranges alone left a contradiction: [`builtin`](crate::builtin) draws all of
/// `U+1FB00..=U+1FB9F` (bar the reserved `U+1FB93`) **to the cell** — 159 codepoints across sextants,
/// wedges, one-eighth blocks, extra eighths, shades, checkers, hatches and triangular halves — while
/// this classifier called them text. Measured, such a cell was byte-identical to `'A'` on all three
/// behaviours above, yet painted as a solid part-cell tile.
///
/// The union is asked of the *drawer* ([`builtin::owns`](crate::builtin::owns)) rather than copied
/// into a second list here, so a codepoint added to `builtin` is classified correctly the day it is
/// added; `builtin`'s own test pins the predicate against what it actually draws. A curated subset
/// was considered and rejected: every codepoint `builtin` draws abuts a cell edge, and xterm's
/// already-classified range spans a 3 %-lit single-edge stub (`U+2574 ╴`) to a flat 25 %-alpha wash
/// (`U+2591 ░`), so neither coverage nor opacity offers a defensible cut line.
///
/// **This diverges from xterm's list**, deliberately. xterm draws `U+1FB00..=U+1FBFA`, Braille and
/// Nerd Font ranges in its webgl addon while `RendererUtils.treatGlyphAsBackgroundColor` stays at
/// `U+2500..=U+259F` + Powerline — with no comment, test or doc acknowledging the gap, and its
/// classifier is not "what xterm draws" either (it classifies `U+E0A4..=U+E0AF`, which it does not
/// draw). There is no xterm *principle* to violate here, only an ad-hoc list; xterm cannot union the
/// two anyway, because its classifier is shared with a DOM renderer that draws no custom glyphs.
///
/// The alternative (match `CellColorResolver`, classifying the cluster's last scalar) is *available*
/// — clusters already reach the renderer via the `extra` + `side_table` columns — but it would need
/// a clusters column threaded into [`frame::Frame`](crate::frame::Frame) for the packer, and would
/// un-tile a cell that visibly still tiles. Rejected on coherence, not on cost.
///
/// (#495 originally listed a third cost: parity with a justerm-web twin of this classifier. **That
/// argument is withdrawn** — the twin was reachable only from the widget's dead compositing path and
/// was deleted with it in #504. The rule above never depended on it.)
pub fn treat_glyph_as_background_color(codepoint: u32) -> bool {
    is_powerline_glyph(codepoint)
        || is_box_or_block_glyph(codepoint)
        || crate::builtin::owns(codepoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_drawing_and_block_elements_tile() {
        assert!(treat_glyph_as_background_color(0x2500)); // ─ box-drawing start
        assert!(treat_glyph_as_background_color(0x257F)); // box-drawing end
        assert!(treat_glyph_as_background_color(0x2580)); // ▀ block start
        assert!(treat_glyph_as_background_color(0x2588)); // █ full block
        assert!(treat_glyph_as_background_color(0x259F)); // block end
    }

    #[test]
    fn powerline_symbols_tile() {
        assert!(treat_glyph_as_background_color(0xE0A4)); // powerline start
        assert!(treat_glyph_as_background_color(0xE0B0)); //  a common separator
        assert!(treat_glyph_as_background_color(0xE0D6)); // powerline end
    }

    /// #507: Symbols for Legacy Computing tile because **this crate draws them to the cell**
    /// ([`crate::builtin`]) — the same premise that makes `U+2588` tile. One per sub-family, since
    /// `builtin` reaches them through four different drawing paths.
    #[test]
    fn legacy_computing_tiles_because_the_crate_draws_it_to_the_cell() {
        assert!(treat_glyph_as_background_color(0x1FB00)); // sextant
        assert!(treat_glyph_as_background_color(0x1FB3C)); // wedge
        assert!(treat_glyph_as_background_color(0x1FB70)); // one-eighth block
        assert!(treat_glyph_as_background_color(0x1FB8B)); // extra eighth block
        assert!(treat_glyph_as_background_color(0x1FB8C)); // medium shade
        assert!(treat_glyph_as_background_color(0x1FB98)); // diagonal hatch
        assert!(treat_glyph_as_background_color(0x1FB9F)); // triangular shade, last of the block
    }

    /// The two negatives that keep the union honest: the reserved hole inside the block, and the
    /// tail beyond it (legacy box drawing / segmented digits) which the FONT draws, not this crate.
    #[test]
    fn the_reserved_hole_and_the_font_drawn_tail_do_not_tile() {
        assert!(!treat_glyph_as_background_color(0x1FB93)); // reserved — builtin draws nothing
        assert!(!treat_glyph_as_background_color(0x1FBA0)); // font-drawn legacy box drawing
        assert!(!treat_glyph_as_background_color(0x1FBF0)); // font-drawn segmented digit
    }

    #[test]
    fn ordinary_text_and_the_range_edges_do_not_tile() {
        assert!(!treat_glyph_as_background_color(0x41)); // 'A'
        assert!(!treat_glyph_as_background_color(0x24FF)); // just below box-drawing
        assert!(!treat_glyph_as_background_color(0x25A0)); // just above block elements (■)
        assert!(!treat_glyph_as_background_color(0xE0A3)); // just below powerline
        assert!(!treat_glyph_as_background_color(0xE0D7)); // just above powerline
        assert!(!treat_glyph_as_background_color(0x1F680)); // 🚀 emoji
        assert!(!treat_glyph_as_background_color(0x0301)); // #495: a combining mark is not a tile —
        // it is what xterm's CellColorResolver would classify a `█ + U+0301` cell by
    }
}
