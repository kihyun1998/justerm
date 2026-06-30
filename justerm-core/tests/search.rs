//! #10 search — literal substring search over the grid + scrollback, returning
//! match ranges in absolute buffer coordinates. Built TDD. Smart-case: a query
//! with no uppercase matches case-insensitively. Matches cross soft-wraps and
//! skip wide-char spacers (reusing the selection coordinate model).

use justerm_core::{Engine, SelectionSpan};

/// A single-line literal match returns the inclusive range of the hit.
#[test]
fn search_finds_single_line_match() {
    let mut term = Engine::new(20, 2);
    term.feed(b"hello world");

    let matches = term.search("world");

    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert_eq!((m.start_line, m.start_col), (0, 6)); // 'w'
    assert_eq!((m.end_line, m.end_col), (0, 10)); // 'd' (inclusive)
}

/// Multiple non-overlapping matches are returned in order.
#[test]
fn search_returns_matches_in_order() {
    let mut term = Engine::new(20, 1);
    term.feed(b"ab ab ab");

    let m = term.search("ab");

    assert_eq!(m.len(), 3);
    assert_eq!((m[0].start_col, m[1].start_col, m[2].start_col), (0, 3, 6));
}

/// Smart-case: a lowercase query matches any case; a query with an uppercase
/// char is case-sensitive.
#[test]
fn search_is_smart_case() {
    let mut term = Engine::new(20, 1);
    term.feed(b"Hello hello");

    assert_eq!(term.search("hello").len(), 2); // no uppercase → case-insensitive
    let m = term.search("Hello");
    assert_eq!(m.len(), 1); // uppercase present → case-sensitive
    assert_eq!(m[0].start_col, 0);
}

/// A match in scrollback (above the screen) is found, with its absolute line.
#[test]
fn search_finds_match_in_scrollback() {
    let mut term = Engine::new(6, 2);
    term.feed(b"aaa\r\nbbb\r\nccc"); // "aaa" scrolls into history (abs line 0)

    let m = term.search("aaa");

    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 0));
}

/// A match spans a soft wrap (one logical line across two rows).
#[test]
fn search_crosses_soft_wrap() {
    let mut term = Engine::new(4, 3);
    term.feed(b"abcdef"); // "abcd"(wrap) / "ef"

    let m = term.search("cdef");

    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 2)); // 'c'
    assert_eq!((m[0].end_line, m[0].end_col), (1, 1)); // 'f' on the wrapped row
}

/// A wide glyph is one searchable char at its lead; the spacer is skipped.
#[test]
fn search_skips_wide_char_spacer() {
    let mut term = Engine::new(6, 1);
    term.feed("a한b".as_bytes()); // a(0) 한=lead(1)+spacer(2) b(3)

    let m = term.search("한");
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 1));
    assert_eq!((m[0].end_line, m[0].end_col), (0, 1)); // single cell (the lead)
}

/// No match → empty.
#[test]
fn search_no_match_is_empty() {
    let mut term = Engine::new(10, 1);
    term.feed(b"hello");

    assert!(term.search("zzz").is_empty());
    assert!(term.search("").is_empty()); // empty query never matches
}

/// `scroll_to_match` moves the viewport so a history match becomes visible.
#[test]
fn scroll_to_match_reveals_history_match() {
    let mut term = Engine::new(6, 2);
    term.feed(b"aaa\r\nbbb\r\nccc"); // "aaa" in history; bottom view shows bbb/ccc
    let m = term.search("aaa");
    assert_eq!(m.len(), 1);

    term.scroll_to_match(&m[0]);

    let top: String = term.viewport_line(0).iter().map(|c| c.c()).collect();
    assert_eq!(top.trim_end(), "aaa");
}

/// `match_spans` projects a match to inclusive viewport spans for highlighting.
#[test]
fn match_spans_project_to_viewport() {
    let mut term = Engine::new(20, 2);
    term.feed(b"hello world");
    let m = term.search("world");

    assert_eq!(
        term.match_spans(&m[0]),
        vec![SelectionSpan {
            row: 0,
            left: 6,
            right: 10
        }]
    );
}

/// `match_spans` clips: an off-screen (history) match yields no spans until the
/// viewport is scrolled to it.
#[test]
fn match_spans_clip_off_screen() {
    let mut term = Engine::new(6, 2);
    term.feed(b"aaa\r\nbbb\r\nccc"); // "aaa" in history; bottom view = bbb/ccc
    let m = term.search("aaa");

    assert!(term.match_spans(&m[0]).is_empty()); // above the viewport → clipped

    term.scroll_to_match(&m[0]);
    assert_eq!(
        term.match_spans(&m[0]),
        vec![SelectionSpan {
            row: 0,
            left: 0,
            right: 2
        }]
    );
}

/// A query spanning a wide char that wrapped off the right edge matches: the
/// vacated column is a leading spacer the haystack skips, so the join is clean
/// ("abcd한", not "abcd 한"). Regression for the wide-wrap WRAPLINE/spacer fix.
#[test]
fn search_crosses_wide_char_wrap_boundary() {
    let mut term = Engine::new(5, 2);
    term.feed("abcd한".as_bytes()); // '한' can't fit col4 → wraps: "abcd" | "한"

    let m = term.search("d한");

    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 3)); // 'd'
    assert_eq!((m[0].end_line, m[0].end_col), (1, 0)); // '한' body
}
