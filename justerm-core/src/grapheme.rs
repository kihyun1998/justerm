//! Grapheme-cluster segmentation for the streaming parser (#295, DECSET mode 2027).
//!
//! justerm-core processes input one `char` at a time (`Term::print`). Under mode 2027 the parser
//! must decide, per incoming scalar, whether it **extends** the previous cell's grapheme cluster
//! (ride the side-table, no new cell) or **breaks** (start a new cell). This is the incremental
//! form of UAX #29 extended grapheme-cluster segmentation.
//!
//! The break decision is delegated to `unicode-segmentation` (the full UAX #29 rule set:
//! GB9 Extend/ZWJ, GB9a SpacingMark, GB9b Prepend, GB11 emoji-ZWJ, GB12/GB13 regional-indicator
//! pairing) rather than hand-rolled — the rules need large Unicode property tables that would rot.
//!
//! No break-state is persisted across `print` calls (cursor moves / CR-LF would corrupt it, cf.
//! ghostty `Terminal.print`). Instead the caller reconstructs the previous cluster's text from the
//! cell (`base scalar + side-table Vec<char>`) and asks [`grapheme_extends`] fresh each time.

use unicode_segmentation::UnicodeSegmentation;

/// Whether appending `c` to `prev_cluster` keeps it a **single** grapheme cluster — i.e. `c`
/// extends the cluster rather than starting a new one. `prev_cluster` is the full text of the
/// preceding cell's cluster (its base scalar plus any already-joined scalars); it is one grapheme
/// by construction. Returns `false` for an empty `prev_cluster` (nothing to extend).
///
/// Uses extended grapheme-cluster rules (UAX #29). Appending a scalar that does not add a new
/// grapheme boundary (a combining mark, ZWJ, an emoji joined via ZWJ, a skin-tone modifier, a
/// VS16/VS15 selector, the second of a regional-indicator pair) extends; anything that starts a
/// fresh cluster (a new base letter, a CJK ideograph, the *third* regional indicator by GB12/13
/// parity) breaks.
pub(crate) fn grapheme_extends(prev_cluster: &str, c: char) -> bool {
    if prev_cluster.is_empty() {
        return false;
    }
    // `c` extends iff appending it adds no new grapheme boundary: the segment count is unchanged.
    // Comparing the delta (not asserting `== 1`) stays correct even if a caller ever passes a
    // multi-grapheme prefix.
    let before = prev_cluster.graphemes(true).count();
    let mut extended = String::with_capacity(prev_cluster.len() + c.len_utf8());
    extended.push_str(prev_cluster);
    extended.push(c);
    extended.graphemes(true).count() == before
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent UAX #29 truths — each case is a known grapheme-break fact, not a re-derivation of
    // the implementation.
    #[test]
    fn combining_mark_extends() {
        // é = 'e' + U+0301 combining acute (GB9 × Extend) → one grapheme.
        assert!(grapheme_extends("e", '\u{0301}'));
    }

    #[test]
    fn zwj_extends_then_joined_emoji_extends() {
        // GB9 (× ZWJ): a ZWJ joins the preceding emoji…
        assert!(grapheme_extends("\u{1F468}", '\u{200D}'), "👨 + ZWJ");
        // …and GB11 (ExtPict ZWJ × ExtPict): the emoji after the ZWJ joins too.
        assert!(
            grapheme_extends("\u{1F468}\u{200D}", '\u{1F469}'),
            "👨‍ + 👩"
        );
    }

    #[test]
    fn skin_tone_modifier_extends() {
        // 👍 + 🏽 (U+1F3FB, Emoji_Modifier = Extend) → one grapheme (GB9).
        assert!(grapheme_extends("\u{1F44D}", '\u{1F3FB}'));
    }

    #[test]
    fn regional_indicator_pair_extends_but_the_third_breaks() {
        // GB12/GB13: RIs pair 2-by-2. The second RI joins the first (one flag)…
        assert!(grapheme_extends("\u{1F1F0}", '\u{1F1F7}'), "🇰 + 🇷 = 🇰🇷");
        // …but a third RI starts a NEW flag (parity break).
        assert!(
            !grapheme_extends("\u{1F1F0}\u{1F1F7}", '\u{1F1FA}'),
            "🇰🇷 + 🇺 breaks"
        );
    }

    #[test]
    fn vs16_and_vs15_selectors_extend() {
        // A variation selector joins its base (GB9 × Extend), emoji (FE0F) or text (FE0E) alike.
        assert!(grapheme_extends("\u{25B6}", '\u{FE0F}'), "▶ + VS16");
        assert!(grapheme_extends("\u{25B6}", '\u{FE0E}'), "▶ + VS15");
    }

    #[test]
    fn a_new_base_scalar_breaks() {
        assert!(!grapheme_extends("\u{1F468}", 'A'), "👨 + A breaks");
        assert!(!grapheme_extends("A", 'B'), "A + B breaks");
        assert!(
            !grapheme_extends("\u{4E2D}", '\u{6587}'),
            "中 + 文 breaks (CJK)"
        );
    }

    #[test]
    fn empty_prefix_never_extends() {
        assert!(!grapheme_extends("", 'A'));
        assert!(!grapheme_extends("", '\u{1F468}'));
    }
}
