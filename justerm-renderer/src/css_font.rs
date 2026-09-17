//! The CSS `font` shorthand the rasteriser draws with, and the font weight a consumer puts in it for
//! regular and for bold text (#928).

use crate::glyph_cache::FontStyle;

/// A CSS font weight in `[1, 1000]`. The keywords are the numbers they stand for, so `"bold"` and
/// `700` are one weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontWeight(f32);

/// The CSS weight keywords a consumer may name, besides a number.
const WEIGHT_KEYWORDS: [(&str, f32); 11] = [
    ("normal", 400.0),
    ("bold", 700.0),
    ("100", 100.0),
    ("200", 200.0),
    ("300", 300.0),
    ("400", 400.0),
    ("500", 500.0),
    ("600", 600.0),
    ("700", 700.0),
    ("800", 800.0),
    ("900", 900.0),
];

impl FontWeight {
    /// Regular text's default, CSS `normal`.
    pub const NORMAL: FontWeight = FontWeight(400.0);
    /// Bold text's default, CSS `bold`.
    pub const BOLD: FontWeight = FontWeight(700.0);

    /// A numeric weight: finite and within `[1, 1000]`, else `None`.
    pub fn from_number(v: f64) -> Option<FontWeight> {
        (v.is_finite() && (1.0..=1000.0).contains(&v)).then_some(FontWeight(v as f32))
    }

    /// A weight named as a string: `"normal"`, `"bold"`, or `"100"`..`"900"` in hundreds, else `None`.
    pub fn from_keyword(s: &str) -> Option<FontWeight> {
        WEIGHT_KEYWORDS
            .iter()
            .find(|(k, _)| *k == s)
            .map(|&(_, v)| FontWeight(v))
    }

    /// The weight as a number, e.g. `400.0`.
    pub fn value(self) -> f32 {
        self.0
    }

    /// Rebuild a weight from [`value`](Self::value) — for a key that stores it as bits.
    pub fn from_value(v: f32) -> FontWeight {
        FontWeight(v)
    }
}

/// A CSS `font` shorthand for the family/size/style, with `weight` for regular text and
/// `weight_bold` for bold text.
pub fn font_string(
    family: &str,
    size: f32,
    style: FontStyle,
    weight: FontWeight,
    weight_bold: FontWeight,
) -> String {
    let (bold, italic) = match style {
        FontStyle::Normal => (false, false),
        FontStyle::Bold => (true, false),
        FontStyle::Italic => (false, true),
        FontStyle::BoldItalic => (true, true),
    };
    let italic = if italic { "italic " } else { "" };
    let weight = if bold { weight_bold } else { weight };
    format!("{italic}{} {size}px {family}, monospace", weight.value())
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: FontWeight = FontWeight::NORMAL;
    const B: FontWeight = FontWeight::BOLD;

    #[test]
    fn the_defaults_are_the_css_keywords() {
        assert_eq!(FontWeight::from_keyword("normal"), Some(W));
        assert_eq!(FontWeight::from_keyword("bold"), Some(B));
        assert_eq!(W.value(), 400.0);
        assert_eq!(B.value(), 700.0);
    }

    #[test]
    fn a_keyword_and_its_number_are_one_weight() {
        assert_eq!(
            FontWeight::from_keyword("700"),
            FontWeight::from_number(700.0)
        );
        assert_eq!(FontWeight::from_keyword("400"), Some(W));
    }

    #[test]
    fn every_hundred_is_a_keyword() {
        for n in (100..=900).step_by(100) {
            assert_eq!(
                FontWeight::from_keyword(&n.to_string()).map(FontWeight::value),
                Some(n as f32)
            );
        }
    }

    #[test]
    fn a_string_outside_the_keywords_is_refused() {
        for s in [
            "", "Bold", "bolder", "lighter", "450", "1000", "400px", " bold", "0",
        ] {
            assert_eq!(FontWeight::from_keyword(s), None, "{s:?}");
        }
    }

    #[test]
    fn a_number_is_a_weight_only_within_one_to_a_thousand() {
        assert_eq!(
            FontWeight::from_number(1.0).map(FontWeight::value),
            Some(1.0)
        );
        assert_eq!(
            FontWeight::from_number(1000.0).map(FontWeight::value),
            Some(1000.0)
        );
        assert_eq!(
            FontWeight::from_number(350.5).map(FontWeight::value),
            Some(350.5)
        );
        for v in [0.0, 0.999, 1000.001, -400.0, f64::NAN, f64::INFINITY] {
            assert_eq!(FontWeight::from_number(v), None, "{v}");
        }
    }

    #[test]
    fn regular_text_draws_at_the_regular_weight() {
        let w = FontWeight::from_number(300.0).unwrap();
        let b = FontWeight::from_number(900.0).unwrap();
        assert_eq!(
            font_string("Fira Code", 32.0, FontStyle::Normal, w, b),
            "300 32px Fira Code, monospace"
        );
        assert_eq!(
            font_string("Fira Code", 32.0, FontStyle::Italic, w, b),
            "italic 300 32px Fira Code, monospace"
        );
    }

    #[test]
    fn bold_text_draws_at_the_bold_weight() {
        let w = FontWeight::from_number(300.0).unwrap();
        let b = FontWeight::from_number(900.0).unwrap();
        assert_eq!(
            font_string("monospace", 16.0, FontStyle::Bold, w, b),
            "900 16px monospace, monospace"
        );
        assert_eq!(
            font_string("monospace", 16.0, FontStyle::BoldItalic, w, b),
            "italic 900 16px monospace, monospace"
        );
    }

    #[test]
    fn the_default_weights_draw_what_the_keywords_did() {
        assert_eq!(
            font_string("monospace", 16.5, FontStyle::Normal, W, B),
            "400 16.5px monospace, monospace"
        );
        assert_eq!(
            font_string("monospace", 16.0, FontStyle::BoldItalic, W, B),
            "italic 700 16px monospace, monospace"
        );
    }
}
