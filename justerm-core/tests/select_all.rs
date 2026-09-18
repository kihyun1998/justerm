//! #935 — `select_all` selects the active buffer in absolute coordinates, with no
//! viewport coordinate and no scroll, so a frame-mode consumer can select scrollback
//! it is not showing. The extent runs from the first to the last non-blank cell (ghostty
//! `Screen.selectAll`), and an all-blank buffer selects nothing.

use justerm_core::{Engine, SelectionSpan, SelectionType, Side};

/// Five lines on a three-row screen: two are in scrollback, and all five are copied.
#[test]
fn copies_scrollback_and_screen() {
    let mut term = Engine::new(10, 3);
    term.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");

    term.select_all();

    assert_eq!(term.selection_text().as_deref(), Some("l1\nl2\nl3\nl4\nl5"));
}

/// The view is not moved to make the selection: scrolled up to the top of the history,
/// the highlight covers exactly the rows on show.
#[test]
fn leaves_the_view_where_it_is() {
    let mut term = Engine::new(10, 3);
    term.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    term.scroll_up(2);

    term.select_all();

    assert_eq!(
        term.selection_range(),
        vec![
            SelectionSpan { row: 0, left: 0, right: 9 },
            SelectionSpan { row: 1, left: 0, right: 9 },
            SelectionSpan { row: 2, left: 0, right: 9 },
        ]
    );
    assert_eq!(term.selection_text().as_deref(), Some("l1\nl2\nl3\nl4\nl5"));
}

/// Blank rows and cells before the first glyph and after the last are left out, so a
/// half-empty screen does not copy a tail of newlines.
#[test]
fn trims_blank_edges() {
    let mut term = Engine::new(10, 6);
    term.feed(b"\r\n  ab\r\ncd");

    term.select_all();

    assert_eq!(term.selection_text().as_deref(), Some("ab\ncd"));
    assert_eq!(
        term.selection_range(),
        vec![
            SelectionSpan { row: 1, left: 2, right: 9 },
            SelectionSpan { row: 2, left: 0, right: 1 },
        ]
    );
}

/// A buffer with nothing written selects nothing, and drops a selection already there.
#[test]
fn all_blank_buffer_selects_nothing() {
    let mut term = Engine::new(10, 3);
    term.selection_begin(0, 0, Side::Left, SelectionType::Line);

    term.select_all();

    assert_eq!(term.selection_text(), None);
    assert!(term.selection_range().is_empty());
}

/// It replaces whatever was selected, whatever its type.
#[test]
fn replaces_an_existing_selection() {
    let mut term = Engine::new(10, 3);
    term.feed(b"ab cd\r\nef");
    term.selection_begin(0, 3, Side::Left, SelectionType::Block);
    term.selection_extend(1, 4, Side::Right);

    term.select_all();

    assert_eq!(term.selection_text().as_deref(), Some("ab cd\nef"));
}

/// A wide glyph at either edge is selected whole.
#[test]
fn wide_glyph_at_an_edge_is_whole() {
    let mut term = Engine::new(10, 3);
    term.feed("漢a漢".as_bytes());

    term.select_all();

    assert_eq!(term.selection_text().as_deref(), Some("漢a漢"));
    assert_eq!(
        term.selection_range(),
        vec![SelectionSpan { row: 0, left: 0, right: 4 }]
    );
}

/// On the alternate screen "all" is the alternate screen: the primary history underneath
/// is not part of it.
#[test]
fn alt_screen_selects_only_the_alt_screen() {
    let mut term = Engine::new(10, 3);
    term.feed(b"p1\r\np2\r\np3\r\np4\r\np5");
    term.feed(b"\x1b[?1049h\x1b[Halt");

    term.select_all();

    assert_eq!(term.selection_text().as_deref(), Some("alt"));
}

/// Only U+0020 is blank: a written no-break space at an edge is content and stays in.
#[test]
fn only_u0020_is_a_blank_edge() {
    let mut term = Engine::new(10, 3);
    term.feed("\u{a0}ab\u{a0}".as_bytes());

    term.select_all();

    assert_eq!(term.selection_text().as_deref(), Some("\u{a0}ab\u{a0}"));
}
