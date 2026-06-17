//! Issue #7 — resize / reflow.

use justerm::{CellFlags, Engine};

/// An auto-wrap marks the row it leaves as soft-wrapped (WRAPLINE on its last
/// cell); an explicit newline ends the line hard, so no WRAPLINE.
#[test]
fn auto_wrap_marks_wrapline_but_hard_newline_does_not() {
    let mut soft = Engine::new(3, 2);
    soft.feed(b"abcd"); // 'abc' fills row 0, 'd' auto-wraps to row 1
    assert!(soft.grid().cell(0, 2).flags.contains(CellFlags::WRAPLINE));

    let mut hard = Engine::new(3, 2);
    hard.feed(b"ab\r\nc"); // 'ab', then a hard CR/LF
    assert!(!hard.grid().cell(0, 1).flags.contains(CellFlags::WRAPLINE));
}

/// Growing the row count keeps existing content and adds blank rows at the
/// bottom.
#[test]
fn grow_rows_keeps_content_adds_blank_lines() {
    let mut term = Engine::new(4, 2);
    term.feed(b"ab\r\ncd"); // row 0 = ab, row 1 = cd

    term.resize(4, 3);

    assert_eq!((term.grid().cols(), term.grid().rows()), (4, 3));
    assert_eq!(term.grid().cell(0, 0).c, 'a'); // preserved
    assert_eq!(term.grid().cell(1, 0).c, 'c');
    assert_eq!(term.grid().cell(2, 0).c, ' '); // new blank row
}
