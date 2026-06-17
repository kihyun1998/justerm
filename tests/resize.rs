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

/// Shrinking the row count scrolls the top lines into scrollback (preserved),
/// keeping the bottom rows visible.
#[test]
fn shrink_rows_preserves_top_lines_in_scrollback() {
    let mut term = Engine::new(4, 3);
    term.feed(b"a\r\nb\r\nc"); // rows a, b, c

    term.resize(4, 2); // shrink → 'a' scrolls into scrollback

    assert_eq!((term.grid().rows(), term.scrollback_len()), (2, 1));
    assert_eq!(term.grid().cell(0, 0).c, 'b'); // bottom rows stay visible
    assert_eq!(term.grid().cell(1, 0).c, 'c');

    term.scroll_up(1);
    assert_eq!(term.viewport_line(0)[0].c, 'a'); // preserved in history
}

/// Narrowing the column count re-wraps a soft-wrapped logical line at the new
/// width (acceptance: resize narrower → wrapped lines reflow).
#[test]
fn shrink_cols_rewraps_soft_wrapped_line() {
    let mut term = Engine::new(4, 4);
    term.feed(b"abcdef"); // "abcd"(WRAPLINE) + "ef"
    assert!(term.grid().cell(0, 3).flags.contains(CellFlags::WRAPLINE));

    term.resize(2, 4); // narrow to 2 cols → "abcdef" rewraps as ab|cd|ef

    assert_eq!((term.grid().cell(0, 0).c, term.grid().cell(0, 1).c), ('a', 'b'));
    assert!(term.grid().cell(0, 1).flags.contains(CellFlags::WRAPLINE));
    assert_eq!((term.grid().cell(1, 0).c, term.grid().cell(1, 1).c), ('c', 'd'));
    assert!(term.grid().cell(1, 1).flags.contains(CellFlags::WRAPLINE));
    assert_eq!((term.grid().cell(2, 0).c, term.grid().cell(2, 1).c), ('e', 'f'));
    assert!(!term.grid().cell(2, 1).flags.contains(CellFlags::WRAPLINE)); // last segment is hard
}

/// Widening merges soft-wrapped segments back into one line — reflow is
/// symmetric, so a narrow→wide round-trip restores the logical line.
#[test]
fn widen_cols_merges_wrapped_segments() {
    let mut term = Engine::new(2, 4);
    term.feed(b"abcdef"); // 2 cols → ab|cd|ef across three wrapped rows

    term.resize(6, 4); // widen → merge back onto one row

    for (col, ch) in "abcdef".chars().enumerate() {
        assert_eq!(term.grid().cell(0, col).c, ch);
    }
    assert!(!term.grid().cell(0, 5).flags.contains(CellFlags::WRAPLINE)); // fits, no wrap
}

/// Reflow applies to scrollback history too, not just the visible screen — a
/// resized terminal must not leave old-width rows in history.
#[test]
fn resize_reflows_scrollback_too() {
    let mut term = Engine::new(4, 2);
    term.feed(b"abcdefgh"); // "abcd"(WRAPLINE) | "efgh" fills both screen rows
    term.feed(b"\r\nX"); // scroll: "abcd" (soft-wrapped) goes into scrollback
    assert_eq!(term.scrollback_len(), 1);

    term.resize(2, 2); // narrow — scrollback must reflow to width 2

    let total = term.scrollback_len();
    term.scroll_up(total);
    let top = term.viewport_line(0);
    assert_eq!(top.len(), 2, "scrollback row left at the old width");
    assert_eq!((top[0].c, top[1].c), ('a', 'b'));
}

/// The cursor follows its content through a reflow instead of being clamped to
/// a stale position.
#[test]
fn cursor_follows_content_through_reflow() {
    let mut term = Engine::new(4, 4);
    term.feed(b"abcdef"); // "abcd"(WRAPLINE) | "ef"
    term.feed(b"\x1b[1;3H"); // cursor onto 'c' at (0, 2) — logical position 2

    term.resize(2, 4); // "abcdef" rewraps as ab|cd|ef; 'c' moves to (1, 0)

    assert_eq!((term.cursor().row, term.cursor().col), (1, 0));
}

/// A degenerate resize to zero is clamped to a 1x1 minimum, not a panic.
#[test]
fn resize_to_zero_is_clamped_not_a_panic() {
    let mut term = Engine::new(4, 4);
    term.feed(b"hi");

    term.resize(0, 0); // must not panic

    assert!(term.grid().cols() >= 1);
    assert!(term.grid().rows() >= 1);
}
