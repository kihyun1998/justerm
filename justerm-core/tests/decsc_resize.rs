//! The DECSC slot across a resize, and one slot per screen (#1088).
//!
//! DECSC (`ESC 7`) saves a point in the screen's content; a column resize that reflows that content
//! carries the saved point with it, so DECRC (`ESC 8`) returns to the same line. Each screen keeps its
//! own slot, as xterm (`screen->sc[screen->whichBuf]`), xterm.js (`Buffer.savedX` / `savedY`),
//! ghostty (`Screen.saved_cursor`) and alacritty (`grid.saved_cursor`) do.

use justerm_core::Engine;

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
fn decrc_after_a_narrowing_returns_to_the_saved_line() {
    let mut t = Engine::new(10, 6);
    t.feed(b"0123456789abcdef\r\nxy\x1b7");
    t.resize(5, 6);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t), ["01234", "56789", "abcde", "f", "xyX", ""]);
    assert_eq!(cursor(&t), (4, 3));
}

#[test]
fn decrc_after_a_widening_returns_to_the_saved_line() {
    let mut t = Engine::new(5, 6);
    t.feed(b"0123456789\r\nxy\x1b7");
    t.resize(10, 6);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t), ["0123456789", "xyX", "", "", "", ""]);
    assert_eq!(cursor(&t), (1, 3));
}

#[test]
fn decrc_after_a_narrowing_with_history_returns_to_the_saved_line() {
    let mut t = Engine::new(10, 3);
    // Three lines of history, then DECSC just after `f` (screen row 1), on a row above the
    // cursor's. The save comes after the last scroll: a scroll does not move a saved screen row.
    t.feed(b"L1\r\nL2\r\nL3\r\n0123456789abcdef\r\nxy\x1b[2;7H\x1b7\x1b[3;3H");
    t.resize(5, 3);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t), ["abcde", "fX", "xy"]);
    assert_eq!(cursor(&t), (1, 2));
}

#[test]
fn a_decsc_point_whose_line_was_evicted_lands_on_the_top_row() {
    let mut t = Engine::with_scrollback(10, 3, 0);
    // DECSC on the first line, which the narrowing pushes off a pane with no history.
    t.feed(b"0123456789abcdef\r\nxy\x1b[1;3H\x1b7\x1b[3;3H");
    t.resize(5, 3);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t), ["abXde", "f", "xy"]);
    assert_eq!(cursor(&t), (0, 3));
}

#[test]
fn the_alt_slot_rides_a_resize_made_while_the_primary_is_up() {
    let mut t = Engine::new(10, 6);
    // Every alt row holds content, so the shrink has no blank row to trim (#1089).
    t.feed(b"\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\r\n6\x1b[5;2H\x1b7\x1b[?1049l");
    // Three rows shorter: the alt grid's re-fit drops its top three rows, and the slot with them.
    t.resize(10, 3);
    t.feed(b"\x1b[?1049h\x1b8");
    assert_eq!(cursor(&t), (1, 1));
}

#[test]
fn decrc_with_no_resize_restores_the_saved_row_column_and_pen() {
    let mut t = Engine::new(10, 4);
    t.feed(b"\x1b[3;5H\x1b[1m\x1b7\x1b[0m\x1b[1;1Hab\x1b8X");
    assert_eq!(cursor(&t), (2, 5));
    assert_eq!(rows(&t)[2], "    X");
    assert!(
        t.grid()
            .cell(2, 4)
            .flags()
            .contains(justerm_core::CellFlags::BOLD),
        "the saved pen comes back with the position"
    );
}

#[test]
fn each_screen_keeps_its_own_slot() {
    let mut t = Engine::new(10, 6);
    // DECSC on the primary at (1, 2); the `?1049h` save is taken at the same point.
    t.feed(b"\x1b[2;3H\x1b7\x1b[?1049h");
    // DECSC on the alt screen at (3, 4), then leave.
    t.feed(b"\x1b[4;5H\x1b7\x1b[?1049l");
    t.feed(b"\x1b[1;1H\x1b8");
    assert_eq!(
        cursor(&t),
        (1, 2),
        "the primary's DECRC reads the primary's slot"
    );
}

#[test]
fn a_decrc_on_the_alt_screen_does_not_read_the_primary_slot() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[3;4H\x1b7\x1b[?1049h\x1b[5;6H\x1b8");
    assert_eq!(
        cursor(&t),
        (0, 0),
        "the alt screen's slot is unwritten, so DECRC goes home"
    );
}

#[test]
fn the_primary_slot_rides_a_resize_made_while_the_alt_screen_is_up() {
    let mut t = Engine::new(10, 6);
    // `?1049h` saves into the primary's DECSC slot just after `xy` (#1092).
    t.feed(b"0123456789abcdef\r\nxy\x1b[?1049h");
    t.resize(5, 6);
    t.feed(b"\x1b[?1049lX");
    assert_eq!(rows(&t), ["01234", "56789", "abcde", "f", "xyX", ""]);
    assert_eq!(cursor(&t), (4, 3));
}

#[test]
fn a_decsc_point_past_the_lines_content_keeps_its_column_on_a_widening() {
    let mut t = Engine::new(10, 6);
    t.feed(b"xy\x1b[1;8H\x1b7\x1b[3;1Habc");
    t.resize(20, 6);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t)[0], "xy     X");
    assert_eq!(cursor(&t), (0, 8));
}

#[test]
fn a_decsc_point_after_a_trailing_space_keeps_it_on_a_narrowing() {
    let mut t = Engine::new(10, 6);
    t.feed(b"$ \x1b7\x1b[4;1Hstatus");
    t.resize(8, 6);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t)[0], "$ X");
    assert_eq!(cursor(&t), (0, 3));
}

#[test]
fn a_decsc_point_on_an_empty_line_between_content_keeps_its_column() {
    let mut t = Engine::new(10, 6);
    t.feed(b"a\r\n\r\nb\x1b[2;6H\x1b7\x1b[5;1H");
    t.resize(12, 6);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t)[1], "     X");
    assert_eq!(cursor(&t), (1, 6));
}

#[test]
fn a_decsc_point_past_the_new_width_lands_on_the_last_column() {
    let mut t = Engine::new(10, 6);
    t.feed(b"xy\x1b[1;9H\x1b7\x1b[3;1Habc");
    t.resize(5, 6);
    t.feed(b"\x1b8X");
    assert_eq!(rows(&t)[0], "xy  X");
}

#[test]
fn a_decsc_point_just_past_a_row_the_narrowing_filled_keeps_the_next_write_after_it() {
    let mut t = Engine::new(10, 6);
    // DECSC just after `d`; at 4 columns `abcd` fills its row, so the point is one past it.
    t.feed(b"abcd\x1b7\r\n\r\n");
    t.resize(4, 6);
    t.feed(b"\x1b8X");
    assert_eq!(&rows(&t)[..2], ["abcd", "X"]);
    assert!(t.grid().is_row_wrapped(0), "X continues abcd's line");
    assert_eq!(cursor(&t), (1, 1));
}

#[test]
fn a_1048_save_keeps_a_column_past_the_lines_content() {
    let mut t = Engine::new(10, 6);
    // `?1048h` saves at col 7 past `xy`; `?47` switches without touching that slot.
    t.feed(b"xy\x1b[1;8H\x1b[?1048h\x1b[3;1Habc\x1b[?47h");
    t.resize(20, 6);
    t.feed(b"\x1b[?47l\x1b[?1048lX");
    assert_eq!(rows(&t)[0], "xy     X");
}

#[test]
fn a_saved_deferred_wrap_off_the_cursor_line_rides_the_reflow() {
    let mut t = Engine::new(10, 6);
    // DECSC parked after `9` with the wrap deferred, then the cursor leaves the line.
    t.feed(b"0123456789\x1b7\x1b[4;1H");
    t.resize(5, 6);
    t.feed(b"\x1b8X");
    assert_eq!(&rows(&t)[..3], ["01234", "56789", "X"]);
}

#[test]
fn decstr_resets_only_the_active_screens_slot() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[2;3H\x1b7\x1b[?1049h\x1b[4;5H\x1b7\x1b[!p\x1b[?1049l\x1b[1;1H\x1b8");
    assert_eq!(
        cursor(&t),
        (1, 2),
        "the primary's slot survived the alt screen's DECSTR"
    );
}

#[test]
fn the_alt_slot_is_clamped_not_re_split_on_a_narrowing() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[?1049h\x1b[2;1H0123456789\x1b[2;8H\x1b7\x1b[6;1H");
    t.resize(5, 6);
    t.feed(b"\x1b8");
    assert_eq!(cursor(&t), (1, 4));
}

#[test]
fn the_alt_slot_rides_the_alt_screens_refit() {
    let mut t = Engine::new(10, 6);
    // `z` on alt row 4, DECSC just after it, the live cursor parked on the last row.
    t.feed(b"\x1b[?1049h\x1b[5;1Hz\x1b7\x1b[6;1H");
    // Three rows shorter: the re-fit drops the top three, so `z`'s row becomes row 1.
    t.resize(10, 3);
    assert_eq!(
        rows(&t)[1],
        "z",
        "fixture: the re-fit moved z's row up three"
    );
    t.feed(b"\x1b[1;1H\x1b8X");
    assert_eq!(rows(&t)[1], "zX");
    assert_eq!(cursor(&t), (1, 2));
}

#[test]
fn the_primary_slot_past_its_content_keeps_its_column_across_a_resize_on_the_alt_screen() {
    let mut t = Engine::new(10, 6);
    // `?1049h` saves at column 5 of the empty line between `a` and `b`; an empty cursor line is
    // reflowed, not kept, so the saved point goes through the past-content mapping.
    t.feed(b"a\r\n\r\nb\x1b[2;6H\x1b[?1049h");
    t.resize(12, 6);
    t.feed(b"\x1b[?1049lX");
    assert_eq!(rows(&t)[1], "     X");
}

#[test]
fn a_decsc_point_on_the_kept_cursor_line_is_clamped_without_a_deferred_wrap() {
    let mut t = Engine::new(10, 6);
    // The cursor stays on the saved line, so the narrowing keeps that line and cuts it.
    t.feed(b"0123456789\x1b[1;9H\x1b7");
    t.resize(5, 6);
    t.feed(b"\x1b8X");
    assert_eq!(&rows(&t)[..2], ["0123X", ""]);
    assert_eq!(cursor(&t), (0, 4));
}

#[test]
fn a_decsc_point_past_the_content_of_a_row_the_narrowing_filled_keeps_the_next_write_after_it() {
    let mut t = Engine::new(10, 6);
    // DECSC one blank past `abcd`; at 4 columns `abcd` fills its row, so the point is past it.
    t.feed(b"abcd\x1b[1;6H\x1b7\x1b[3;1H");
    t.resize(4, 6);
    t.feed(b"\x1b8X");
    assert_eq!(
        &rows(&t)[..2],
        ["abcd", "X"],
        "the d survives, as for a point right after it"
    );
    assert_eq!(cursor(&t), (1, 1));
}
