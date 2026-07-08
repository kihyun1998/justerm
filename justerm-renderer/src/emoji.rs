//! Unicode emoji-presentation classification — the *text* half of the hybrid colour-emoji
//! detector (#297). Pure, host `cargo test`-able (no GL, no rasteriser).
//!
//! ## Why a text-side check at all (the #284 → #297 story)
//! #284 classifies emoji by the **bitmap** the browser drew ([`is_color_bitmap`]): a colour
//! glyph comes back in the font's own palette, a text glyph in grayscale white. That is font
//! ground-truth and correct for the common case, but misses one class — an emoji the font draws
//! in pure grayscale (`⬛ ⬜ ⚫ ⚪`, monochrome chess/card emoji): every pixel is `R=G=B`, so the
//! bitmap check reads it as text and it renders tinted to the cell fg instead of its own gray.
//! [`is_emoji_text`] recovers those: the renderer ORs the two signals
//! (`is_emoji_text(text, wide) || is_color_bitmap(rgba)`).
//!
//! ## Relationship to beamterm `beamterm-unicode::is_emoji`
//! The [`is_emoji_presentation`] codepoint table is mirrored **verbatim** from beamterm (a pure
//! Unicode fact). The multi-codepoint classification, however, deliberately diverges. beamterm
//! decides a multi-codepoint cluster with `UnicodeWidthStr::width() >= 2`; this crate does not
//! depend on `unicode-width` (the glyph_cache contract: width is core's job) and, more importantly,
//! `width() >= 2` is the *wrong* signal under justerm's per-char width — it misfires two ways:
//! - it **misses** a text-base + VS16 (`▶️`, `❤️`) or a keycap (`1️⃣`): justerm-core computes width
//!   per character (`UnicodeWidthChar`) with no VS16 promotion, so the base stays width-1 and the
//!   frame delivers `wide=false` where beamterm's string width would see 2;
//! - it **falsely matches** a wide *text* base + combining mark (CJK + a diacritic): wide, but not
//!   emoji — colour-sampling its grayscale glyph would render it white.
//!
//! So a multi-codepoint cluster is classified by **structural signals** instead: a VS16 (U+FE0F)
//! selector (VS15 U+FE0E, the text selector, excluded), a ZWJ (U+200D) joiner, or an
//! emoji-presentation lead (flags, skin-tone sequences). A single codepoint uses the table (BMP)
//! or the table gated on `wide` (SMP, whose range is broad). This is *more* correct than beamterm
//! on both misfire classes.
//!
//! ## Known tradeoff (accepted, #297)
//! The hybrid `is_emoji_text || is_color_bitmap` cannot distinguish a *correctly* achromatic emoji
//! (`⚫ ⚪`) from a **tofu / no-colour-font fallback** the browser draws in grayscale — both arrive
//! as `R=G=B` bitmaps. So in an environment with no colour-emoji font, a table emoji renders in the
//! font's monochrome fallback (white) via the emoji path rather than in the cell foreground. This
//! is the "no-colour-font fallback rendered white" case #297 named; real browsers ship colour emoji
//! fonts, so it is a rare, accepted fidelity tradeoff, not a mirror defect.

/// Whether a grapheme is an emoji that should be colour-sampled from the atlas even if the font
/// drew it monochrome (#297 case 2). `wide` is the core frame's width flag for the cell
/// (`WIDE_CHAR`), used only to gate the broad single-codepoint SMP range. A single BMP scalar is
/// decided by the exact [`is_emoji_presentation`] table; a multi-codepoint cluster by its
/// structural signals (VS16 / ZWJ / emoji-presentation lead) — see the module docs for why `wide`
/// alone is the wrong signal there.
#[must_use]
pub fn is_emoji_text(s: &str, wide: bool) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let is_multi = chars.next().is_some();

    if !is_multi {
        // Single codepoint. A BMP scalar is decided by the exact table (no width needed). An SMP
        // scalar (U+10000+) verifies with `wide` because the table's SMP range is broad — a
        // non-emoji SMP glyph core marks narrow must not slip through.
        return if (first as u32) <= 0xFFFF {
            is_emoji_presentation(first)
        } else {
            wide && is_emoji_presentation(first)
        };
    }

    // Multi-codepoint cluster. It is an emoji sequence iff a structural signal says so — NOT
    // merely because it is wide. beamterm proxies this with `s.width() >= 2`, but that misfires
    // two ways under justerm's per-char width: (a) it MISSES a text-base + VS16 / keycap, which
    // justerm-core never promotes to width 2 (base width-1, FE0F is a width-0 combining mark), and
    // (b) it FALSELY matches a wide *text* base + combining mark (CJK + a diacritic), which is wide
    // but not emoji — colour-sampling it would render the grayscale glyph white. The structural
    // signals below get both right:
    //  - VS16 (U+FE0F) forces emoji presentation (VS15 U+FE0E, the text selector, is excluded);
    //  - a ZWJ (U+200D) joiner marks a family/role sequence;
    //  - an emoji-presentation lead covers flags (regional indicators) and skin-tone sequences.
    s.contains('\u{FE0F}') || s.contains('\u{200D}') || is_emoji_presentation(first)
}

/// `true` for characters with emoji-presentation-by-default (rendered as colour emoji without a
/// VS16). Covers the 60 BMP code points (U+231A–U+2B55) and SMP emoji (U+1F000–U+1FFFF, minus
/// the CJK Enclosed Ideographic Supplement text symbols). Mirrors beamterm
/// `beamterm-unicode::is_emoji_presentation` (derived there by cross-referencing the `emojis`
/// crate against `unicode-width`).
fn is_emoji_presentation(c: char) -> bool {
    let cp = c as u32;
    match cp {
        // BMP emoji with default emoji presentation (60 code points, U+231A–U+2B55).
        0x231A..=0x2B55 => matches!(
            cp,
            0x231A..=0x231B   // ⌚⌛
            | 0x23E9..=0x23EC // ⏩⏪⏫⏬
            | 0x23F0          // ⏰
            | 0x23F3          // ⏳
            | 0x25FD..=0x25FE // ◽◾
            | 0x2614..=0x2615 // ☔☕
            | 0x2648..=0x2653 // ♈..♓
            | 0x267F          // ♿
            | 0x2693          // ⚓
            | 0x26A1          // ⚡
            | 0x26AA..=0x26AB // ⚪⚫
            | 0x26BD..=0x26BE // ⚽⚾
            | 0x26C4..=0x26C5 // ⛄⛅
            | 0x26CE          // ⛎
            | 0x26D4          // ⛔
            | 0x26EA          // ⛪
            | 0x26F2..=0x26F3 // ⛲⛳
            | 0x26F5          // ⛵
            | 0x26FA          // ⛺
            | 0x26FD          // ⛽
            | 0x2705          // ✅
            | 0x270A..=0x270B // ✊✋
            | 0x2728          // ✨
            | 0x274C          // ❌
            | 0x274E          // ❎
            | 0x2753..=0x2755 // ❓❔❕
            | 0x2757          // ❗
            | 0x2795..=0x2797 // ➕➖➗
            | 0x27B0          // ➰
            | 0x27BF          // ➿
            | 0x2B1B..=0x2B1C // ⬛⬜
            | 0x2B50          // ⭐
            | 0x2B55          // ⭕
        ),
        // SMP emoji: nearly all of U+1F000–U+1FFFF are emoji. Exclude the CJK Enclosed
        // Ideographic Supplement (EAW=W text symbols, not emoji).
        0x1F000..=0x1FFFF => !matches!(
            cp,
            0x1F200
                | 0x1F202..=0x1F219
                | 0x1F21B..=0x1F22E
                | 0x1F230..=0x1F231
                | 0x1F237
                | 0x1F23B..=0x1F24F
                | 0x1F260..=0x1F265
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Core-provided width for a grapheme, mirroring how the resolver derives `wide` from the
    // frame's WIDE_CHAR flag. Emoji-presentation graphemes and CJK are wide; text-presentation
    // symbols and ASCII are narrow.
    #[test]
    fn emoji_presentation_default_single_codepoint_is_emoji() {
        // Emoji_Presentation=Yes graphemes the font draws in its own palette — always emoji.
        for s in [
            "\u{1F680}",
            "\u{1F600}",
            "\u{1F389}",
            "\u{23E9}",
            "\u{231A}",
            "\u{26BD}",
        ] {
            assert!(is_emoji_text(s, true), "{s:?} is emoji-presentation");
        }
    }

    #[test]
    fn achromatic_emoji_are_emoji_the_bitmap_check_misses_them() {
        // The #297 case 2 crux: these are Emoji_Presentation=Yes but the font draws them in pure
        // grayscale (R=G=B), so is_color_bitmap returns false. The text check must catch them so
        // they colour-sample their own gray instead of being tinted to the cell fg.
        for s in ["\u{2B1B}", "\u{2B1C}", "\u{26AB}", "\u{26AA}"] {
            assert!(is_emoji_text(s, true), "achromatic emoji {s:?}");
        }
    }

    #[test]
    fn text_presentation_without_fe0f_is_not_emoji() {
        // Text-presentation-by-default symbols WITHOUT VS16 stay text (rendered in the cell fg).
        // ◼ ◻ (U+25FC/25FB) from the issue are in fact text-presentation — correctly NOT emoji.
        for s in [
            "\u{25B6}", "\u{25C0}", "\u{25FB}", "\u{25FC}", "\u{23ED}", "\u{2934}",
        ] {
            assert!(!is_emoji_text(s, false), "{s:?} is text-presentation");
        }
    }

    #[test]
    fn width_1_colour_glyph_without_vs16_is_not_text_emoji_relies_on_bitmap() {
        // #297 case 1 (❤ ☺ ♥ ✈ without VS16): text-presentation, so is_emoji_text is false — the
        // colour recovery for these comes from is_color_bitmap, NOT this table. Locks the two
        // fixes as orthogonal (a font that colours ❤ is caught by the bitmap half, narrow-region).
        for s in ["\u{2764}", "\u{263A}", "\u{2665}", "\u{2708}"] {
            assert!(
                !is_emoji_text(s, false),
                "{s:?} decided by bitmap, not table"
            );
        }
    }

    #[test]
    fn text_presentation_with_fe0f_is_emoji_even_when_core_marks_it_narrow() {
        // A text-presentation base + VS16 (FE0F) forces emoji presentation. CRITICAL: justerm-core
        // uses per-char UnicodeWidthChar with NO VS16 promotion (term.rs: the base gets width 1,
        // FE0F is width 0 → attached as a combining mark, the lead cell is NOT flagged WIDE_CHAR),
        // so the real pipeline delivers `wide=FALSE` for these — unlike beamterm's string-level
        // width. The VS16 code point itself is the robust signal, independent of `wide`.
        assert!(
            is_emoji_text("\u{25B6}\u{FE0F}", false),
            "▶️ (VS16) with wide=false"
        );
        assert!(
            is_emoji_text("\u{2764}\u{FE0F}", false),
            "❤️ (VS16) with wide=false"
        );
    }

    #[test]
    fn text_presentation_with_fe0e_text_selector_is_not_emoji() {
        // The mirror-image guard: VS15 (U+FE0E) requests TEXT presentation — it must NOT be
        // classified as emoji, even though it is also a variation selector following the same base.
        assert!(
            !is_emoji_text("\u{25B6}\u{FE0E}", false),
            "▶︎ (VS15) is text"
        );
        assert!(
            !is_emoji_text("\u{2764}\u{FE0E}", false),
            "❤︎ (VS15) is text"
        );
    }

    #[test]
    fn keycap_is_emoji_via_vs16_even_when_core_marks_it_narrow() {
        // Keycap "1️⃣" = '1' + FE0F + 20E3. The base '1' is ASCII width 1 → core delivers
        // wide=false; the VS16 signal recovers it.
        assert!(
            is_emoji_text("1\u{FE0F}\u{20E3}", false),
            "keycap 1️⃣ with wide=false"
        );
    }

    #[test]
    fn zwj_flag_and_skin_tone_sequences_are_emoji_via_lead_or_joiner() {
        // Genuine emoji sequences: a ZWJ family (joiner + emoji lead), a regional-indicator flag
        // (emoji-presentation lead), and a skin-tone sequence (emoji lead + modifier). Each is a
        // real emoji cluster the pipeline delivers wide=true for.
        assert!(
            is_emoji_text("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", true),
            "ZWJ family 👨‍👩‍👧 (joiner)"
        );
        assert!(
            is_emoji_text("\u{1F1F0}\u{1F1F7}", true),
            "flag 🇰🇷 (emoji lead)"
        );
        assert!(
            is_emoji_text("\u{1F44D}\u{1F3FD}", true),
            "skin-tone 👍🏽 (emoji lead)"
        );
    }

    #[test]
    fn wide_text_base_plus_combining_mark_is_not_emoji() {
        // The regression guard the hybrid must NOT trip: a WIDE text base + a combining mark
        // (CJK + a diacritic → cluster "中̀") is wide=true but is plain text. Classifying it as
        // emoji would colour-sample the grayscale atlas → render it WHITE instead of the cell fg.
        // beamterm's bare `width >= 2` misfires here; justerm keys off the lead/joiner instead.
        assert!(
            !is_emoji_text("\u{4E2D}\u{0301}", true),
            "中́ (wide CJK + combining) is text, not emoji"
        );
        assert!(
            !is_emoji_text("\u{65E5}\u{0301}", true),
            "日́ (wide CJK + combining) is text, not emoji"
        );
    }

    #[test]
    fn cjk_is_wide_but_not_emoji() {
        // The critical false-positive guard: a wide grapheme is NOT automatically emoji. CJK is
        // width-2 text; classifying it as emoji would colour-sample garbage / lose the fg tint.
        for s in ["\u{4E2D}", "\u{65E5}", "\u{AC00}"] {
            assert!(!is_emoji_text(s, true), "CJK {s:?} is wide text, not emoji");
        }
    }

    #[test]
    fn ascii_and_narrow_symbols_are_not_emoji() {
        assert!(!is_emoji_text("A", false));
        assert!(!is_emoji_text(" ", false));
        assert!(!is_emoji_text("\u{2192}", false), "→ arrow is text");
        assert!(!is_emoji_text("\u{2588}", false), "█ full block is text");
    }

    #[test]
    fn smp_cjk_enclosed_ideographic_supplement_is_excluded() {
        // U+1F200 (🈀) sits in the SMP range but is a CJK text symbol, not emoji — must be excluded
        // even though it is wide.
        assert!(
            !is_emoji_text("\u{1F200}", true),
            "🈀 is SMP text, not emoji"
        );
    }

    #[test]
    fn empty_string_is_not_emoji() {
        assert!(!is_emoji_text("", false));
        assert!(!is_emoji_text("", true));
    }
}
