//! Reflow trims a hard-ended row's trailing blanks by **content**, not by full-cell equality.
//!
//! A blank the app never wrote (`Cell::default()`) and a blank it *erased with a coloured
//! background* (BCE) are both "no content" — reflow measures where the logical line ends, and a
//! background is not content. Trimming by `cells.last() == Cell::default()` instead kept a BCE
//! tail on the logical line, so a narrowing resize re-split it into an extra row of coloured
//! blanks the app never typed.
//!
//! Both references trim on content only — xterm.js `getTrimmedLength` tests `HAS_CONTENT_MASK`
//! (`BufferLine.ts:484`), alacritty `line_length` tests `c != ' '` — and xterm keeps the
//! background-aware variant (`getNoBgTrimmedLength`) as a *separate* function for the callers that
//! actually want it (the DOM renderer), which reflow is not.
//!
//! This does not touch what a blank cell *is* (#530: a freed/erased blank keeps the current
//! background). Trimming finds where a hard-ended line ends; it does not erase a cell. A column
//! that survives on screen keeps its colour — see `the_surviving_cells_keep_their_background`.

use justerm_core::{Color, Engine};

/// The number of grid rows that are not entirely `Cell::default()` — i.e. rows that "exist" for
/// reflow, counting a row of coloured-but-empty cells. This is what a content-only glyph count
/// misses: a BCE tail re-split into its own row holds no glyph yet is not a default row.
fn occupied_rows(t: &Engine) -> usize {
    let g = t.grid();
    (0..g.rows())
        .filter(|&r| (0..g.cols()).any(|c| *g.cell(r, c) != Default::default()))
        .count()
}

#[test]
fn a_bce_tail_does_not_cost_a_row_on_reflow() {
    // "abcde" then the rest of the row erased to red. On master the red tail rode the logical
    // line and `resize(5)` spent a whole extra row on coloured blanks — invisible to a glyph
    // count, visible as an occupied row.
    let mut t = Engine::new(8, 4);
    t.feed(b"abcde\x1b[1;6H\x1b[41m\x1b[K"); // columns 5..8 erased under a red pen
    assert_eq!(occupied_rows(&t), 1, "fixture: one line");
    assert_eq!(t.scrollback_len(), 0, "fixture");

    t.resize(5, 4);

    assert_eq!(
        t.accessible_text().trim_end(),
        "abcde",
        "the hard-ended line is just its content after reflow"
    );
    assert_eq!(
        occupied_rows(&t),
        1,
        "still one row — the BCE tail is not content and does not re-split into a second row"
    );
}

#[test]
fn a_bce_tail_does_not_push_content_into_scrollback() {
    // The user-visible cost of the phantom row: on a short screen it scrolls the next line off.
    // The second line ("XY") is short enough to still fit after the resize, so the ONLY thing
    // that could steal a row is the red tail on line 1 — isolating the variable.
    let mut t = Engine::new(8, 2);
    t.feed(b"abcde\x1b[1;6H\x1b[41m\x1b[K"); // line 0: abcde + a red BCE tail
    t.feed(b"\r\nXY"); // line 1: fits in 5 cols with room to spare
    assert_eq!(t.scrollback_len(), 0, "fixture");

    t.resize(5, 2);

    assert_eq!(
        t.scrollback_len(),
        0,
        "both lines fit — the red tail did not re-split line 0 into an extra row"
    );
    assert_eq!(t.accessible_text().trim_end(), "abcde\nXY");
}

#[test]
fn the_surviving_cells_keep_their_background() {
    // #530 is untouched: trimming decides where a line *ends*, it does not blank a cell. A red
    // column that is still on screen after the resize is still red.
    let mut t = Engine::new(8, 3);
    t.feed(b"\x1b[41mabcdefgh"); // a full red row, all with content
    assert_eq!(t.grid().cell(0, 4).bg(), Color::Indexed(1), "fixture");

    t.resize(5, 3);

    // "abcdefgh" on 5 cols re-wraps; every surviving glyph cell keeps its red background.
    assert_eq!(t.grid().cell(0, 4).bg(), Color::Indexed(1));
    assert_eq!(t.grid().cell(1, 0).bg(), Color::Indexed(1));
    assert_eq!(t.accessible_text().replace('\n', ""), "abcdefgh");
}

#[test]
fn a_wide_char_tail_still_splits_correctly() {
    // Guard: a real wide glyph at the end is content and must survive the trim.
    let mut t = Engine::new(6, 3);
    t.feed("abcd\u{D55C}".as_bytes()); // 한 wraps to the next row already
    t.resize(4, 3);
    assert_eq!(t.accessible_text().trim_end(), "abcd\u{D55C}");
}

#[test]
fn a_wide_glyph_ending_a_hard_line_is_not_trimmed() {
    // The marker case the trim MUST spare: a wide glyph whose spacer is the last cell of a
    // hard-ended line. The spacer's base char is a space, so a `c() == ' '` trim would eat it and
    // orphan the glyph. `is_blank` checks the marker bits, not just the glyph — and the resize
    // must actually change the width, or reflow never runs the trim.
    let mut t = Engine::new(4, 3);
    t.feed("ab\u{D55C}".as_bytes()); // ab 한 — 한 at cols 2-3, its spacer is the trailing cell
    assert!(t.grid().cell(0, 3).is_wide_spacer(), "fixture");

    t.resize(6, 3); // widen — reflow joins and re-splits, with 한's spacer at the line's tail

    assert_eq!(t.accessible_text().trim_end(), "ab\u{D55C}");
    assert!(t.grid().cell(0, 2).is_wide(), "still a wide glyph");
    assert!(
        t.grid().cell(0, 3).is_wide_spacer(),
        "its spacer survived the trim — a lost spacer orphans the wide glyph"
    );
}
