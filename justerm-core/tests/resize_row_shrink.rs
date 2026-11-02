//! A resize that removes rows trims blank rows below the cursor before it sends any row to
//! history (#1089), as ghostty does (`src/terminal/PageList.zig:2415-2431` @ `e6e26e1`). A row with
//! content is never dropped from below the cursor, and the cursor's own row is never trimmed.

use justerm_core::{CellFlags, Engine, SelectionType, Side};

fn rows(t: &Engine) -> Vec<String> {
    let g = t.grid();
    (0..g.rows())
        .map(|r| {
            g.row(r)
                .iter()
                .map(|c| c.c())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn cursor(t: &Engine) -> (usize, usize) {
    (t.cursor().row, t.cursor().col)
}

#[test]
fn blank_rows_below_the_cursor_go_before_any_row_goes_to_history() {
    let mut t = Engine::new(10, 6);
    t.feed(b"a\r\nb\r\nc\x1b[2;1H");
    t.resize(10, 3);
    assert_eq!(rows(&t), ["a", "b", "c"]);
    assert_eq!(cursor(&t), (1, 0));
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn content_below_the_cursor_is_kept_and_the_rest_comes_off_the_top() {
    let mut t = Engine::new(10, 6);
    t.feed(b"a\r\nb\r\nc\r\nd\x1b[2;1H");
    t.resize(10, 3);
    assert_eq!(rows(&t), ["b", "c", "d"]);
    assert_eq!(cursor(&t), (0, 0));
    assert_eq!(t.scrollback_len(), 1);
}

#[test]
fn blank_rows_cover_only_part_of_the_shrink() {
    let mut t = Engine::new(10, 6);
    // One blank row below the cursor's; the shrink needs three.
    t.feed(b"a\r\nb\r\nc\r\nd\r\ne");
    t.resize(10, 3);
    assert_eq!(rows(&t), ["c", "d", "e"]);
    assert_eq!(cursor(&t), (2, 1));
    assert_eq!(t.scrollback_len(), 2);
}

#[test]
fn the_cursors_own_blank_row_is_never_trimmed() {
    let mut t = Engine::new(10, 6);
    // The cursor waits on blank row 2; rows 3..5 below it are blank too.
    t.feed(b"a\r\nb\r\n");
    t.resize(10, 2);
    assert_eq!(rows(&t), ["b", ""]);
    assert_eq!(cursor(&t), (1, 0));
    assert_eq!(t.scrollback_len(), 1);
}

#[test]
fn a_row_with_only_a_background_counts_as_blank() {
    let mut t = Engine::new(10, 6);
    // Row 4 erased under a blue background: no content.
    t.feed(b"a\r\nb\x1b[44m\x1b[5;1H\x1b[2K\x1b[0m\x1b[2;1H");
    t.resize(10, 3);
    assert_eq!(rows(&t), ["a", "b", ""]);
    assert_eq!(cursor(&t), (1, 0));
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn the_alt_screen_keeps_the_content_beside_its_cursor() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[?1049ha\r\nb\r\nc\x1b[2;1H");
    t.resize(10, 3);
    assert_eq!(rows(&t), ["a", "b", "c"]);
    assert_eq!(cursor(&t), (1, 0));
}

#[test]
fn the_primary_under_the_alt_screen_keeps_the_content_beside_its_cursor() {
    let mut t = Engine::new(10, 6);
    t.feed(b"a\r\nb\r\nc\x1b[2;1H\x1b[?1049h");
    t.resize(10, 3);
    t.feed(b"\x1b[?1049l");
    assert_eq!(rows(&t), ["a", "b", "c"]);
    assert_eq!(cursor(&t), (1, 0));
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn growing_rows_trims_nothing() {
    let mut t = Engine::new(10, 3);
    // History above, `x` beside the cursor, two blank rows below it.
    t.feed(b"1\r\n2\r\n3\r\n4\r\n5\r\nx\r\n\r\n\x1b[1;1H");
    t.resize(10, 4);
    assert_eq!(rows(&t), ["5", "x", "", ""]);
    assert_eq!(cursor(&t), (1, 0));
    assert_eq!(t.scrollback_len(), 4);
}

#[test]
fn an_empty_pane_with_no_history_narrows_and_shrinks() {
    let mut t = Engine::with_scrollback(10, 3, 0);
    t.resize(5, 2);
    assert_eq!(rows(&t), ["", ""]);
    assert_eq!(cursor(&t), (0, 0));
}

#[test]
fn a_decsc_point_on_a_blank_row_below_the_cursor_stops_the_trim_there() {
    let mut t = Engine::new(10, 6);
    // DECSC on blank row 3, then the cursor goes back up to `b`.
    t.feed(b"a\r\nb\r\nc\r\n\x1b7\x1b[2;1H");
    t.resize(10, 3);
    t.feed(b"\x1b8Z");
    assert_eq!(
        rows(&t),
        ["b", "c", "Z"],
        "the restore lands below `c`, not on it"
    );
    assert_eq!(t.scrollback_len(), 1);
}

#[test]
fn a_tracked_point_on_a_blank_row_below_the_cursor_stays_off_the_content() {
    let mut t = Engine::new(10, 6);
    t.feed(b"a\r\nb\r\nc\x1b[2;1H");
    let id = t.track_point(4, 3);
    t.resize(10, 3);
    let (line, col) = t.tracked_point(id).expect("its row was kept, not evicted");
    let row = line - t.scrollback_len();
    assert_eq!(rows(&t)[row], "", "the point is still on a blank row");
    assert_eq!(col, 3);
}

#[test]
fn a_selection_of_blank_rows_below_the_cursor_takes_in_no_content() {
    let mut t = Engine::new(10, 6);
    t.feed(b"a\r\nb\r\nc\x1b[2;1H");
    t.selection_begin(4, 0, Side::Left, SelectionType::Char);
    t.selection_extend(5, 9, Side::Right);
    t.resize(10, 3);
    let text = t.selection_text().unwrap_or_default();
    assert!(!text.contains('c'), "selection took in {text:?}");
}

#[test]
fn a_row_of_styled_spaces_is_content() {
    let mut t = Engine::new(10, 6);
    // Row 4 holds ten inverse spaces the application drew.
    t.feed(b"a\r\nb\x1b[5;1H\x1b[7m          \x1b[0m\x1b[2;1H");
    t.resize(10, 3);
    let g = t.grid();
    let inverse_row = (0..g.rows()).find(|&r| g.cell(r, 0).flags().contains(CellFlags::INVERSE));
    assert!(inverse_row.is_some(), "the inverse row was dropped");
}

#[test]
fn a_blank_row_a_wrapped_row_continues_into_is_kept() {
    let mut t = Engine::new(10, 6);
    // Eleven spaces from row 1 wrap into row 2; `x` on row 0 holds the cursor.
    t.feed(b"\x1b[2;1H           \x1b[1;1Hx");
    t.resize(10, 2);
    assert!(
        !t.grid().is_row_wrapped(t.grid().rows() - 1),
        "the last row does not wrap into a row that is gone"
    );
}

#[test]
fn the_alt_screen_under_the_primary_gives_up_its_blank_rows_too() {
    let mut t = Engine::new(10, 6);
    // The alt screen's DECSC slot at (1, 1) over a blank alt grid.
    t.feed(b"\x1b[?1049h\x1b[2;2H\x1b7\x1b[?1049l");
    t.resize(10, 3);
    t.feed(b"\x1b[?1049h\x1b8");
    assert_eq!(
        cursor(&t),
        (1, 1),
        "the slot's row was not pushed off the top"
    );
}

#[test]
fn a_row_of_linked_spaces_is_content() {
    let mut t = Engine::new(10, 6);
    // Row 4 holds three spaces inside an OSC 8 hyperlink.
    t.feed(b"a\r\nb\x1b[5;1H\x1b]8;;https://example.com\x07   \x1b]8;;\x07\x1b[2;1H");
    t.resize(10, 3);
    let g = t.grid();
    let linked_row = (0..g.rows()).find(|&r| g.cell(r, 0).is_linked());
    assert!(linked_row.is_some(), "the linked row was dropped");
}
