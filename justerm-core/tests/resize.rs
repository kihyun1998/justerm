//! Issue #7 — resize / reflow.

use justerm_core::{CellFlags, Engine};

/// An auto-wrap marks the row it leaves as soft-wrapped; an explicit newline ends the line hard.
/// Asked of the *row*: the flag moved off the last cell in #538, because a cell write there used
/// to destroy it.
#[test]
fn auto_wrap_marks_wrapline_but_hard_newline_does_not() {
    let mut soft = Engine::new(3, 2);
    soft.feed(b"abcd"); // 'abc' fills row 0, 'd' auto-wraps to row 1
    assert!(soft.grid().is_row_wrapped(0));

    let mut hard = Engine::new(3, 2);
    hard.feed(b"ab\r\nc"); // 'ab', then a hard CR/LF
    assert!(!hard.grid().is_row_wrapped(0));
}

/// Growing the row count keeps existing content and adds blank rows at the
/// bottom.
#[test]
fn grow_rows_keeps_content_adds_blank_lines() {
    let mut term = Engine::new(4, 2);
    term.feed(b"ab\r\ncd"); // row 0 = ab, row 1 = cd

    term.resize(4, 3);

    assert_eq!((term.grid().cols(), term.grid().rows()), (4, 3));
    assert_eq!(term.grid().cell(0, 0).c(), 'a'); // preserved
    assert_eq!(term.grid().cell(1, 0).c(), 'c');
    assert_eq!(term.grid().cell(2, 0).c(), ' '); // new blank row
}

/// Shrinking the row count scrolls the top lines into scrollback (preserved),
/// keeping the bottom rows visible.
#[test]
fn shrink_rows_preserves_top_lines_in_scrollback() {
    let mut term = Engine::new(4, 3);
    term.feed(b"a\r\nb\r\nc"); // rows a, b, c

    term.resize(4, 2); // shrink → 'a' scrolls into scrollback

    assert_eq!((term.grid().rows(), term.scrollback_len()), (2, 1));
    assert_eq!(term.grid().cell(0, 0).c(), 'b'); // bottom rows stay visible
    assert_eq!(term.grid().cell(1, 0).c(), 'c');

    term.scroll_up(1);
    assert_eq!(term.viewport_line(0)[0].c(), 'a'); // preserved in history
}

/// Narrowing the column count re-wraps a soft-wrapped logical line at the new
/// width (acceptance: resize narrower → wrapped lines reflow).
#[test]
fn shrink_cols_rewraps_soft_wrapped_line() {
    let mut term = Engine::new(4, 4);
    term.feed(b"abcdef"); // "abcd"(WRAPLINE) + "ef"
    assert!(term.grid().is_row_wrapped(0));

    term.resize(2, 4); // narrow to 2 cols → "abcdef" rewraps as ab|cd|ef

    assert_eq!(
        (term.grid().cell(0, 0).c(), term.grid().cell(0, 1).c()),
        ('a', 'b')
    );
    assert!(term.grid().is_row_wrapped(0));
    assert_eq!(
        (term.grid().cell(1, 0).c(), term.grid().cell(1, 1).c()),
        ('c', 'd')
    );
    assert!(term.grid().is_row_wrapped(1));
    assert_eq!(
        (term.grid().cell(2, 0).c(), term.grid().cell(2, 1).c()),
        ('e', 'f')
    );
    assert!(!term.grid().cell(2, 1).flags().contains(CellFlags::WRAPLINE)); // last segment is hard
}

/// Widening merges soft-wrapped segments back into one line — reflow is
/// symmetric, so a narrow→wide round-trip restores the logical line.
#[test]
fn widen_cols_merges_wrapped_segments() {
    let mut term = Engine::new(2, 4);
    term.feed(b"abcdef"); // 2 cols → ab|cd|ef across three wrapped rows

    term.resize(6, 4); // widen → merge back onto one row

    for (col, ch) in "abcdef".chars().enumerate() {
        assert_eq!(term.grid().cell(0, col).c(), ch);
    }
    assert!(!term.grid().cell(0, 5).flags().contains(CellFlags::WRAPLINE)); // fits, no wrap
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
    assert_eq!((top[0].c(), top[1].c()), ('a', 'b'));
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

/// Reflow must not split a wide char from its spacer across the new column
/// boundary — the glyph wraps whole to the next row instead.
#[test]
fn reflow_keeps_wide_char_together_at_boundary() {
    let mut term = Engine::new(4, 4);
    term.feed("a한".as_bytes()); // 'a' + a width-2 glyph

    term.resize(2, 4); // 'a' takes col 0; '한' can't fit in the last col → wraps whole

    assert_eq!(term.grid().cell(1, 0).c(), '한');
    assert!(
        term.grid()
            .cell(1, 0)
            .flags()
            .contains(CellFlags::WIDE_CHAR)
    );
    assert!(
        term.grid()
            .cell(1, 1)
            .flags()
            .contains(CellFlags::WIDE_CHAR_SPACER)
    );
}

/// Resizing while on the alt screen must resize BOTH screens — the inactive
/// (primary) screen must not be left at the old dimensions.
#[test]
fn resize_while_on_alt_resizes_both_screens() {
    let mut term = Engine::new(10, 5);
    term.feed(b"primary");
    term.feed(b"\x1b[?1049h"); // enter alt
    term.feed(b"alt");

    term.resize(20, 8); // resize while on the alt screen
    assert_eq!((term.grid().cols(), term.grid().rows()), (20, 8));

    term.feed(b"\x1b[?1049l"); // leave alt → primary returns
    assert_eq!((term.grid().cols(), term.grid().rows()), (20, 8)); // primary also resized
}

/// After an alt → resize → leave round-trip the primary screen is coherent and
/// usable: content preserved, and feeding / damage / viewport reads don't panic
/// or go out of range. (Stress-checks the resize refactor's output state.)
#[test]
fn primary_is_usable_after_alt_resize_roundtrip() {
    let mut term = Engine::new(10, 5);
    term.feed(b"line1\r\nline2");
    term.feed(b"\x1b[?1049h"); // enter alt
    term.resize(20, 8); // resize on alt
    term.feed(b"\x1b[?1049l"); // back to primary at the new size

    assert_eq!((term.grid().cols(), term.grid().rows()), (20, 8));
    assert_eq!(term.grid().cell(0, 0).c(), 'l'); // primary content preserved

    // Keep using the primary — must stay coherent (no panic / out-of-range).
    term.feed(b"\r\nline3");
    term.reset_damage();
    term.feed(b"x");
    let _ = term.damage();
    for r in 0..term.grid().rows() {
        let _ = term.viewport_line(r);
    }
    assert_eq!(term.grid().cell(0, 0).c(), 'l');
}

/// The rows of a 6-row screen after a line-feed on row 4. With a DECSTBM region
/// covering rows 2..4 still in force only those three rows move; without one the
/// line-feed is an ordinary cursor step and nothing scrolls. Asserting the region's
/// *effect* rather than a field is what makes this a behaviour test.
///
/// **The region is set by the caller, before the resize, and never here.** Setting it
/// inside this helper made the first of the three tests below pass vacuously: what it
/// observed was a region the helper had just re-established, not one that survived. The
/// two tests expecting a *reset* are what exposed it.
fn rows_after_a_feed_at_the_region_bottom(term: &mut Engine) -> Vec<String> {
    term.feed(b"\x1b[2;1HA\x1b[3;1HB\x1b[4;1HC\x1b[6;1HZ");
    term.feed(b"\x1b[4;1H\n"); // a line-feed at the region's bottom row
    (0..6)
        .map(|r| {
            (0..6)
                .map(|c| term.grid().cell(r, c).c())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// A resize to the geometry the terminal already has is not a geometry change, and
/// it must not discard the scroll region. `resize` has no early return and
/// `ResizePort` states no idempotency guarantee, so a consumer re-asserting its size
/// reaches this — and the region is state only the *application* can restore, which
/// it never does, because nothing tells it the region is gone.
///
/// Every reference is guarded against this by an early return over the whole
/// function; this engine has none, so the guard is on the reset itself.
#[test]
fn a_resize_to_the_same_geometry_keeps_the_scroll_region() {
    let mut term = Engine::new(6, 6);
    term.feed(b"\x1b[2;4r");
    term.resize(6, 6); // identical geometry

    let rows = rows_after_a_feed_at_the_region_bottom(&mut term);
    assert_eq!(rows, ["", "B", "C", "", "", "Z"]); // only rows 2..4 moved
}

/// A row change redefines the rows the region is a range over, so it goes. This is
/// the half that was already right, and it is asserted so the gate above cannot be
/// widened into "never reset".
#[test]
fn a_rows_only_resize_resets_the_scroll_region() {
    let mut term = Engine::new(6, 7);
    term.feed(b"\x1b[2;4r");
    term.resize(6, 6);

    let rows = rows_after_a_feed_at_the_region_bottom(&mut term);
    assert_eq!(rows, ["", "A", "B", "C", "", "Z"]); // no region: the feed only steps
}

/// So does a column change, though the region names no column. The references reset
/// on any geometry change, and the spec's own width change is explicit about it:
/// xterm's DECCOLM handler calls `resetMargins` under a `DEC 070, pp 5-71 to 5-72`
/// citation. Gating on the row axis alone would pass every other case here.
#[test]
fn a_cols_only_resize_resets_the_scroll_region() {
    let mut term = Engine::new(7, 6);
    term.feed(b"\x1b[2;4r");
    term.resize(6, 6);

    let rows = rows_after_a_feed_at_the_region_bottom(&mut term);
    assert_eq!(rows, ["", "A", "B", "C", "", "Z"]);
}
