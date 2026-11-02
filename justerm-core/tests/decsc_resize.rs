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
    t.feed(b"\x1b[?1049h\x1b[5;2H\x1b7\x1b[?1049l");
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
    assert_eq!(cursor(&t), (1, 2), "the primary's DECRC reads the primary's slot");
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
    // The `?1049h` save is taken on the empty last row, away from the DECSC point, so the two
    // slots differ; an empty cursor line is reflowed like any other.
    t.feed(b"0123456789abcdef\r\nxy\x1b7\x1b[6;1H\x1b[?1049h");
    t.resize(5, 6);
    t.feed(b"\x1b[?1049l\x1b[1;1H\x1b8X");
    // The parked cursor keeps its three rows below `xy`, so two rows of output went to history.
    assert_eq!(rows(&t), ["abcde", "f", "xyX", "", "", ""]);
    assert_eq!(cursor(&t), (2, 3));
}

#[test]
fn the_alt_slot_rides_the_alt_screens_refit() {
    let mut t = Engine::new(10, 6);
    // `z` on alt row 4, DECSC just after it, the live cursor parked on the last row.
    t.feed(b"\x1b[?1049h\x1b[5;1Hz\x1b7\x1b[6;1H");
    // Three rows shorter: the re-fit drops the top three, so `z`'s row becomes row 1.
    t.resize(10, 3);
    assert_eq!(rows(&t)[1], "z", "fixture: the re-fit moved z's row up three");
    t.feed(b"\x1b[1;1H\x1b8X");
    assert_eq!(rows(&t)[1], "zX");
    assert_eq!(cursor(&t), (1, 2));
}
