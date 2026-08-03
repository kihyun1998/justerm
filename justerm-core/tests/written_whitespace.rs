//! #685 — a whitespace character the user selected is not a whitespace character
//! to strip. The trailing trim exists to drop a row's **unwritten padding**, and a
//! blank cell packs `' '` — so `' '` is the whole of what it may remove.
//!
//! justerm used `str::trim_end()` / `char::is_whitespace()`, the Unicode
//! `White_Space` **property**, which also eats a non-breaking space (U+00A0), an
//! EM SPACE (U+2003) and every other space the application actually printed. All
//! three references reach the opposite answer by three unrelated mechanisms —
//! alacritty scans back while `cell.c == ' '` (`term/cell.rs:271`), ghostty defers
//! blanks on `codepoint() == ' '` (`formatter.zig:1145`), xterm.js bounds by
//! *written extent* (`BufferLine.ts:484`) — so this is justerm alone drifting.
//!
//! Four independent trims implement the one rule (`extract_lines`, the Block arm of
//! `selection_text`, `viewport_logical_lines`, and `search`'s haystack). Nothing
//! stated they must agree; this file is that statement. Every case therefore has a
//! **padding control** beside it, because the trim must keep doing its old job:
//! a fix that simply stopped trimming would pass every keeping-assertion here.

use justerm_core::{Engine, SearchOptions, SelectionType, Side};

const NBSP: char = '\u{a0}';

// ===========================================================================
// selection_text — all four selection types share `extract_lines` except Block
// ===========================================================================

/// The reported case, at its widest: the NBSP is not even at the row's end —
/// `"cd"` follows it — and it was still dropped, because the trim ran over the
/// *selected text* rather than the row's padding.
///
/// The side condition is the original symptom and the reason this is a defect
/// rather than a preference: `selection_range()` highlights three cells, so a
/// text of `"ab"` means the user is shown a cell that never reaches the clipboard.
#[test]
fn char_selection_keeps_a_selected_non_breaking_space() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}cd").as_bytes());

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 2, Side::Right);

    let text = term.selection_text().expect("a selection is active");
    assert_eq!(text, format!("ab{NBSP}"));

    let spans = term.selection_range();
    assert_eq!(spans.len(), 1, "one viewport row is selected");
    assert_eq!(spans[0].right, 2, "the highlight covers column 2");
    assert_eq!(
        text.chars().count(),
        spans[0].right - spans[0].left + 1,
        "the copied text and the highlight must cover the same cells"
    );
}

/// A word ending in a non-breaking space. U+00A0 is deliberately **not** in
/// `DEFAULT_WORD_SEPARATORS` (#545), so the word walk steps onto it and the range
/// includes it — the text must follow.
#[test]
fn word_selection_keeps_a_trailing_non_breaking_space() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}").as_bytes());

    term.selection_begin(0, 0, Side::Left, SelectionType::Word);

    assert_eq!(
        term.selection_text().as_deref(),
        Some(format!("ab{NBSP}").as_str())
    );
}

/// Line selection takes the whole row, so it carries both halves at once: the
/// written NBSP survives *and* the 17 columns of padding behind it do not.
#[test]
fn line_selection_keeps_the_written_space_and_drops_the_padding() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}").as_bytes());

    term.selection_begin(0, 0, Side::Left, SelectionType::Line);

    assert_eq!(
        term.selection_text().as_deref(),
        Some(format!("ab{NBSP}").as_str()),
        "the padding is trimmed, the written space is not"
    );
}

/// The Block arm does **not** go through `extract_lines` — it has its own
/// per-row trim, which is why it is asserted separately rather than assumed to
/// follow. (It was missing from the issue's own acceptance list for that reason.)
#[test]
fn block_selection_keeps_a_selected_non_breaking_space() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}cd").as_bytes());

    term.selection_begin(0, 0, Side::Left, SelectionType::Block);
    term.selection_extend(0, 2, Side::Right);

    assert_eq!(
        term.selection_text().as_deref(),
        Some(format!("ab{NBSP}").as_str())
    );
}

/// Control for every case above: the trim still removes a row's unwritten
/// padding. A fix that removed the trim instead of narrowing it passes all the
/// keeping-assertions and fails here.
#[test]
fn selection_still_drops_the_rows_unwritten_padding() {
    let mut term = Engine::new(20, 4);
    term.feed(b"ab");

    term.selection_begin(0, 0, Side::Left, SelectionType::Line);

    assert_eq!(term.selection_text().as_deref(), Some("ab"));
}

// ===========================================================================
// The other three joiners — same rule, separate implementations
// ===========================================================================

/// `viewport_logical_lines` feeds the consumer's URL detection, so a dropped
/// character shifts every `cells` index after it as well as losing the char.
#[test]
fn viewport_logical_lines_keeps_a_trailing_non_breaking_space() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}").as_bytes());

    let lines = term.viewport_logical_lines();
    let first = lines.first().expect("one non-empty row");
    assert_eq!(first.text, format!("ab{NBSP}"));
    assert_eq!(
        first.cells.len(),
        first.text.chars().count(),
        "text and cells stay 1:1 through the trim"
    );
}

#[test]
fn accessible_text_keeps_a_trailing_non_breaking_space() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}").as_bytes());

    assert_eq!(term.accessible_text(), format!("ab{NBSP}"));
}

/// The sharpest of the four: this one is not "the copy loses a character" but
/// "the character does not exist". `search()` trims its haystack with the same
/// predicate, so a trailing non-breaking space is **unfindable**.
#[test]
fn search_finds_a_trailing_non_breaking_space() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}").as_bytes());

    assert_eq!(
        term.search(&NBSP.to_string()).len(),
        1,
        "a written NBSP at a row's end is findable"
    );
}

/// Control for the search half — the haystack trim still keeps a regex `$`
/// anchor off the grid's blank padding.
#[test]
fn search_still_ignores_the_rows_unwritten_padding() {
    let mut term = Engine::new(20, 4);
    term.feed(b"ab");

    assert_eq!(
        term.search(" ").len(),
        0,
        "blank padding is not searchable content"
    );
}

// ===========================================================================
// The property, not the codepoint — U+00A0 is not a special case
// ===========================================================================

/// U+3000 IDEOGRAPHIC SPACE is the sharpest case: it is `White_Space`, it is
/// **width 2**, and it *is* in `DEFAULT_WORD_SEPARATORS`. So the old predicate
/// deleted a whole wide glyph from every surface while the word walk still
/// stopped on it — the two halves of #545's policy disagreeing about the same
/// character. Its width is why this is not a duplicate of the EM SPACE case.
#[test]
fn a_written_ideographic_space_is_not_padding() {
    let mut term = Engine::new(20, 4);
    term.feed("ab\u{3000}".as_bytes());

    term.selection_begin(0, 0, Side::Left, SelectionType::Line);

    assert_eq!(term.selection_text().as_deref(), Some("ab\u{3000}"));
    assert_eq!(term.accessible_text(), "ab\u{3000}");
    assert_eq!(term.search("\u{3000}").len(), 1);
}

/// The trim moves a regex `$` anchor, because it decides where the haystack
/// ends. On a row ending in a written NBSP the old predicate anchored `$` to
/// column 1, so `b$` matched a `b` that is **not** the last thing on the row.
#[test]
fn a_regex_end_anchor_lands_on_the_last_written_cell() {
    let mut term = Engine::new(20, 4);
    term.feed(format!("ab{NBSP}").as_bytes());
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };

    assert!(
        term.search_with("b$", opts).is_empty(),
        "`b` is not the last written cell — the NBSP after it is"
    );
    let last = term.search_with(".$", opts);
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].start_col, 2, "`$` anchors to the NBSP's column");
}

/// U+2003 EM SPACE is `White_Space` and is neither a word separator nor `' '`.
/// Asserting it alongside U+00A0 is what makes this a rule about the *predicate*
/// rather than a patch for one codepoint: a fix that special-cased NBSP passes
/// every test above and fails this one.
#[test]
fn an_em_space_the_application_printed_is_not_padding() {
    let mut term = Engine::new(20, 4);
    term.feed("ab\u{2003}".as_bytes());

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 2, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("ab\u{2003}"));
}
