//! #113 (ADR-0017) — viewport logical lines: soft-wrap-joined text + a
//! per-char map back to viewport cells, the buffer-wide mechanism a frame-mode
//! consumer needs to run URL detection (the regex/validation stay consumer-side).
//! Built TDD. Reuses the search/selection coordinate model (wrap-join,
//! wide-spacer skip).

use justerm_core::Engine;

/// A single unwrapped row yields one logical line whose text is the row content
/// (trailing blanks trimmed) and whose map sends each char to its `(row, col)`.
#[test]
fn logical_line_maps_each_char_to_its_viewport_cell() {
    let mut term = Engine::new(20, 1);
    term.feed(b"hello");

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "hello");
    assert_eq!(lines[0].cells, vec![(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
}

/// Soft-wrapped rows are one logical line: the text joins across the wrap and
/// the map carries each char's actual `(row, col)` on both rows.
#[test]
fn soft_wrapped_rows_join_into_one_logical_line() {
    let mut term = Engine::new(5, 3);
    term.feed(b"abcdefgh"); // 8 chars on 5 cols → row0 "abcde" (wrapped), row1 "fgh"

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "abcdefgh");
    assert_eq!(
        lines[0].cells,
        vec![
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4), // row 0
            (1, 0),
            (1, 1),
            (1, 2), // row 1
        ]
    );
}

/// A wide char is one text char mapped to its body column; the trailing spacer
/// column is skipped, so the next char's column jumps by two.
#[test]
fn wide_char_is_one_text_char_at_its_body_col() {
    let mut term = Engine::new(10, 1);
    term.feed("a한b".as_bytes()); // 'a' col0, '한' body col1 + spacer col2, 'b' col3

    let lines = term.viewport_logical_lines();

    assert_eq!(lines[0].text, "a한b");
    assert_eq!(lines[0].cells, vec![(0, 0), (0, 1), (0, 3)]);
}

/// A logical line that wraps in from above the viewport top is assembled in
/// full — the off-screen prefix gets negative viewport rows so a URL spanning
/// the edge still matches; the consumer highlights only the in-range cells.
#[test]
fn logical_line_includes_off_screen_wrapped_prefix() {
    let mut term = Engine::new(5, 2);
    term.feed(b"abcdefghijklmn"); // "abcde"|"fghij"|"klmn"; row "abcde" scrolls off the top

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "abcdefghijklmn");
    assert_eq!(
        lines[0].cells,
        vec![
            (-1, 0),
            (-1, 1),
            (-1, 2),
            (-1, 3),
            (-1, 4), // "abcde" off-screen above
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4), // "fghij" viewport row 0
            (1, 0),
            (1, 1),
            (1, 2),
            (1, 3), // "klmn" viewport row 1
        ]
    );
}

/// Hard line breaks (not soft-wraps) stay separate logical lines.
#[test]
fn separate_rows_are_separate_logical_lines() {
    let mut term = Engine::new(20, 3);
    term.feed(b"foo\r\nbar");

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text, "foo");
    assert_eq!(lines[1].text, "bar");
    assert_eq!(lines[1].cells, vec![(1, 0), (1, 1), (1, 2)]);
}

/// A blank viewport yields no logical lines (empty rows are dropped).
#[test]
fn empty_viewport_yields_no_lines() {
    let term = Engine::new(20, 3);

    assert_eq!(term.viewport_logical_lines(), vec![]);
}

/// A wide char that wraps because it can't fit in the last column keeps the row
/// soft-wrapped — the logical line joins across the boundary (regression for the
/// missing WRAPLINE on the wide-wrap path, term.rs `write_glyph`).
#[test]
fn wide_char_wrap_keeps_the_line_joined() {
    let mut term = Engine::new(5, 2);
    term.feed("abcd한".as_bytes()); // 'd' fills col3, col4 free but '한' needs 2 → wraps

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "abcd한");
    // 'a'..'d' on row 0 (col4 left blank by the wrap), '한' body on row 1 col 0.
    assert_eq!(lines[0].cells, vec![(0, 0), (0, 1), (0, 2), (0, 3), (1, 0)]);
}

/// When scrolled up, a logical line whose tail runs off the *bottom* is still
/// assembled in full — the below-viewport rows get rows `>= rows` (the mirror of
/// the off-screen-above case).
#[test]
fn logical_line_includes_off_screen_wrapped_suffix() {
    let mut term = Engine::new(5, 2);
    term.feed(b"abcdefghijklmno"); // "abcde"|"fghij"|"klmno" (one wrapped line)
    term.scroll_up(1); // view "abcde"|"fghij"; "klmno" now below the bottom

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "abcdefghijklmno");
    assert_eq!(
        lines[0].cells,
        vec![
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4), // "abcde" viewport row 0
            (1, 0),
            (1, 1),
            (1, 2),
            (1, 3),
            (1, 4), // "fghij" viewport row 1
            (2, 0),
            (2, 1),
            (2, 2),
            (2, 3),
            (2, 4), // "klmno" off-screen below (row 2)
        ]
    );
}

/// Combining marks ride their base cell: the text carries `base + marks` (like
/// `selection_text`), each mapped to the same cell so `text` stays 1:1 with
/// `cells`.
#[test]
fn combining_marks_ride_their_base_cell() {
    let mut term = Engine::new(10, 1);
    term.feed("e\u{0301}x".as_bytes()); // 'e' + combining acute (one cell), then 'x'

    let lines = term.viewport_logical_lines();

    assert_eq!(lines[0].text, "e\u{0301}x");
    assert_eq!(lines[0].cells, vec![(0, 0), (0, 0), (0, 1)]); // base + mark share col 0
}

/// On the alt screen, the up-walk must NOT cross into *primary* scrollback — the
/// alt buffer is separate (selection clears on alt-swap for the same reason).
/// Regression for the unguarded scrollback walk-up.
#[test]
fn alt_screen_does_not_join_into_primary_scrollback() {
    let mut term = Engine::new(5, 2);
    term.feed(b"abcdefghijklmno"); // primary: "abcde"(WRAPLINE) evicts to scrollback
    term.feed(b"\x1b[?1049h\x1b[H"); // enter alt screen (separate buffer), home cursor
    term.feed(b"XY"); // alt content at row 0

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "XY"); // NOT "abcde…XY"
    assert_eq!(lines[0].cells, vec![(0, 0), (0, 1)]);
}

/// Interior spaces at a soft-wrap boundary are real content and survive — the
/// whole line is assembled, then trimmed *once* at the end (only the last row
/// can carry trailing blanks). Locks an intentional advantage over xterm's
/// per-row trimRight (which would corrupt "ab  cd" → "abcd").
#[test]
fn interior_wrap_boundary_spaces_are_preserved() {
    let mut term = Engine::new(4, 2);
    term.feed(b"ab  cd"); // "ab  "(WRAPLINE, 2 real spaces) | "cd"

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "ab  cd");
}

/// Degenerate 1-column grid: each char wraps to its own row, joined into one
/// logical line.
#[test]
fn single_column_grid_joins_each_row() {
    let mut term = Engine::new(1, 3);
    term.feed(b"ab"); // "a"(WRAPLINE) | "b"

    let lines = term.viewport_logical_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "ab");
    assert_eq!(lines[0].cells, vec![(0, 0), (1, 0)]);
}
