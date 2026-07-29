//! A codepoint whose `unicode-width` is greater than 2 must still reach the grid as a
//! *pair* (#595, spine #594).
//!
//! `unicode-width` 0.2 reports **3** for `U+17D8` KHMER SIGN BEYYAL — a ligature that
//! renders as three characters. justerm's cell model has exactly one shape for a
//! multi-column glyph: a `WIDE_CHAR` lead plus exactly one `WIDE_CHAR_SPACER`. There is no
//! representation for a triple, so the engine coerces the width to 2 the way all three
//! references do (ghostty says it outright — *"we max out at 2 for wide characters (i.e.
//! 3-em dash becomes a 2-em dash)"*).
//!
//! **Every assertion here is paired with the same assertion on a width-2 control** —
//! `U+D55C` 한. That pairing is the point of the file: the failure this guards against is
//! fixing one of the two and silently breaking the other, and a lone width-3 assertion
//! could be satisfied by a change that mangles ordinary wide glyphs too.

use justerm_core::{Engine, SelectionType, Side};

/// `U+17D8` — `unicode-width` says 3.
const W3: char = '\u{17D8}';
/// `U+D55C` — an ordinary width-2 glyph, the control.
const W2: char = '\u{D55C}';

fn engine_with(text: &str) -> Engine {
    let mut e = Engine::new(12, 1);
    e.feed(text.as_bytes());
    e
}

/// The cell layout: lead flagged wide, exactly one spacer, nothing else touched.
fn assert_pair_layout(label: &str, glyph: char) {
    let e = engine_with(&format!("a{glyph}b"));
    let cells = e.viewport_line(0);

    assert_eq!(cells[0].c(), 'a', "{label}: cell 0");
    assert_eq!(cells[1].c(), glyph, "{label}: the glyph sits at cell 1");
    assert!(cells[1].is_wide(), "{label}: the lead carries WIDE_CHAR");
    assert!(
        cells[2].is_spacer(),
        "{label}: cell 2 is the pair's spacer, not an ordinary blank"
    );
    // The side condition that makes this a pair rather than a triple: the glyph claims
    // *two* columns, so 'b' lands at 3 and nothing occupies 4.
    assert_eq!(cells[3].c(), 'b', "{label}: 'b' follows the pair at cell 3");
    assert!(
        !cells[4].is_spacer() && cells[4].c() == ' ',
        "{label}: cell 4 belongs to no glyph"
    );
    assert_eq!(
        e.cursor().col,
        4,
        "{label}: the cursor advanced by 2 for the glyph, not by its raw width"
    );
}

#[test]
fn a_wide_glyph_occupies_exactly_one_pair() {
    assert_pair_layout("width-3", W3);
    assert_pair_layout("width-2 control", W2);
}

/// Text that is on screen must be findable. This is the user-visible half of the defect:
/// before the fix, `search` returned no match for a run the terminal was displaying.
#[test]
fn search_finds_a_run_containing_a_wide_glyph() {
    for (label, glyph) in [("width-3", W3), ("width-2 control", W2)] {
        let query = format!("a{glyph}b");
        let e = engine_with(&query);
        let hits = e.search(&query);
        assert_eq!(hits.len(), 1, "{label}: exactly one match for {query:?}");
        assert_eq!(hits[0].start_col, 0, "{label}: match starts at column 0");
        assert_eq!(
            hits[0].end_col, 3,
            "{label}: match ends on 'b' at column 3, past the pair"
        );
    }
}

/// Word selection must treat the pair as part of the word from *every* column it covers —
/// including the spacer. Before the fix the run split in two and the second half arrived
/// with a leading space the buffer never held.
#[test]
fn word_selection_covers_the_whole_run_from_every_column() {
    for (label, glyph) in [("width-3", W3), ("width-2 control", W2)] {
        let whole = format!("a{glyph}b");
        for col in 0..=3 {
            let mut e = engine_with(&whole);
            e.selection_begin(0, col, Side::Left, SelectionType::Word);
            e.selection_extend(0, col, Side::Right);
            assert_eq!(
                e.selection_text().as_deref(),
                Some(whole.as_str()),
                "{label}: word selection at column {col}"
            );
            // The side condition, and it has to be an *independent* observable: the extracted
            // text being right does not mean the range is. A split run used to surface as the
            // text `" b"` — correct-looking, and never in the buffer — but the range is what the
            // renderer highlights, so assert it separately. One span, covering the run's four
            // columns, with the pair not broken across two spans.
            let spans = e.selection_range();
            assert_eq!(spans.len(), 1, "{label}: one span at column {col}");
            assert_eq!(
                (spans[0].left, spans[0].right),
                (0, 3),
                "{label}: the span covers the whole run at column {col}"
            );
        }
    }
}

/// The shrunk case from the proptest that first surfaced this (`robustness.rs`'s
/// `feed_resize_and_frame_never_panic`): a `MIN_COLUMNS`-wide grid, which is exactly wide
/// enough for one pair and not for a triple. This is where the raw width overran the row.
///
/// Asserted on the **cells**, deliberately, not on the recorded damage span. A span
/// assertion looks like the right one — the raw width is what tripped `damage_span`'s
/// bound — but `damage_span` clamps immediately after asserting, so in a release build the
/// span is in range whether or not this bug is present. Measured: with the fix reverted,
/// a span-based version of this test still passed under `--release` while the three above
/// failed. That asymmetry is also why the defect shipped at all — the guard that would
/// have caught it is a `debug_assert`, and production only ever ran the clamp.
#[test]
fn a_wide_glyph_fills_a_two_column_grid_as_a_pair() {
    for (label, glyph) in [("width-3", W3), ("width-2 control", W2)] {
        let mut e = Engine::new(1, 1); // clamps up to MIN_COLUMNS = 2
        e.feed(glyph.to_string().as_bytes());

        assert_eq!(e.grid().cols(), 2, "{label}: the grid is MIN_COLUMNS wide");
        let cells = e.viewport_line(0);
        assert_eq!(cells[0].c(), glyph, "{label}: the lead occupies column 0");
        assert!(cells[0].is_wide(), "{label}: the lead carries WIDE_CHAR");
        assert!(
            cells[1].is_spacer(),
            "{label}: column 1 is the pair's spacer — the row holds exactly one pair"
        );
    }
}

/// The **other** width path. Under grapheme-cluster mode (DECSET 2027, #295) `print` tries
/// `try_grapheme_join` *before* it ever reaches `c.width()`, and that path measures a
/// cluster with `UnicodeWidthStr` — a second width reader, which `print`'s clamp does not
/// cover.
///
/// It cannot produce this defect, and the reason is stronger than "it only acts on 2 and
/// 1": the cluster path never *creates* a cell. It joins a scalar into the side table of a
/// cell some earlier `print` already wrote through the clamped path, so every base it
/// touches is a well-formed pair before it starts. **That is the condition to re-check if
/// the cluster path ever gains the ability to synthesise a cell from a measured width.**
#[test]
fn the_clamp_holds_under_grapheme_cluster_mode() {
    const ON: &str = "\x1b[?2027h";
    const MARK: char = '\u{0301}'; // a combining acute — grapheme-extends its base

    // A lone width-3 glyph joins nothing, so it still falls through to the clamped path.
    for (label, glyph) in [("width-3", W3), ("width-2 control", W2)] {
        let mut e = Engine::new(12, 1);
        e.feed(format!("{ON}a{glyph}b").as_bytes());
        let cells = e.viewport_line(0);
        assert!(cells[1].is_wide(), "{label}: lead is a pair under 2027");
        assert!(
            cells[2].is_spacer(),
            "{label}: spacer follows it under 2027"
        );
        assert_eq!(cells[3].c(), 'b', "{label}: 'b' at column 3 under 2027");
    }

    // And a *cluster* whose measured width is 3 — the width-3 base with a mark joined onto
    // it — leaves the already-clamped pair exactly as it found it.
    for (label, glyph) in [("width-3", W3), ("width-2 control", W2)] {
        let mut e = Engine::new(12, 1);
        e.feed(format!("{ON}{glyph}{MARK}b").as_bytes());
        let cells = e.viewport_line(0);
        assert!(cells[0].is_wide(), "{label}: the joined base stays a pair");
        assert!(
            cells[1].is_spacer(),
            "{label}: its spacer survives the join"
        );
        assert_eq!(
            cells[2].c(),
            'b',
            "{label}: the join consumed no extra column"
        );
        // The pair took columns 0–1 and 'b' took 2, so the cursor sits at 3. The mark
        // itself moved it by nothing — that is what "joined into the side table" means.
        assert_eq!(
            e.cursor().col,
            3,
            "{label}: the joined mark consumed no column of its own"
        );
    }
}
