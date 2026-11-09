//! A resize leaves the history alone when nothing below the cursor pushed it out, and
//! `Engine::set_grow_from_history(false)` stops a row grow from taking rows out of it (#1100).

use justerm_core::Engine;

fn row(t: &Engine, r: usize) -> String {
    t.grid()
        .row(r)
        .iter()
        .map(|c| c.c())
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn cursor_row(t: &Engine) -> usize {
    t.cursor().row
}

fn lines(t: &mut Engine, n: usize) {
    for i in 1..=n {
        t.feed(format!("L{i}\r\n").as_bytes());
    }
}

/// 80x24, 40 lines, then a clear and a prompt: 17 lines of history, the prompt on row 0.
fn cleared() -> Engine {
    let mut t = Engine::new(80, 24);
    lines(&mut t, 40);
    t.feed(b"\x1b[H\x1b[2J$ ");
    assert_eq!((t.scrollback_len(), cursor_row(&t)), (17, 0));
    t
}

#[test]
fn a_row_grow_over_a_cleared_screen_keeps_the_history() {
    let mut t = cleared();
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 0);
    assert_eq!(row(&t, 0), "$");
    assert_eq!(row(&t, 29), "");
}

#[test]
fn a_column_change_over_a_cleared_screen_keeps_the_history() {
    for cols in [100, 60] {
        let mut t = cleared();
        t.resize(cols, 24);
        assert_eq!(t.scrollback_len(), 17, "{cols} columns");
        assert_eq!(cursor_row(&t), 0, "{cols} columns");
        assert_eq!(row(&t, 0), "$", "{cols} columns");
    }
}

/// `count` lines of 70+ cells, each wrapping at 40 columns, cleared.
fn long_lines_cleared(cols: usize, count: usize) -> Engine {
    let mut t = Engine::new(cols, 24);
    for i in 1..=count {
        t.feed(format!("L{i}-{}\r\n", "x".repeat(70)).as_bytes());
    }
    t.feed(b"\x1b[H\x1b[2J$ ");
    t
}

#[test]
fn history_that_rewraps_stays_in_the_history() {
    let mut t = long_lines_cleared(80, 40);
    assert_eq!(t.scrollback_len(), 17);
    t.resize(40, 24);
    // 17 logical lines, each two rows at 40 columns.
    assert_eq!(t.scrollback_len(), 34);
    assert_eq!(cursor_row(&t), 0);
    assert_eq!(row(&t, 0), "$");

    let mut t = long_lines_cleared(40, 40);
    assert_eq!(t.scrollback_len(), 57);
    t.resize(80, 24);
    // 57 rows at 40 columns are 28 two-row lines and the head of a 29th: 29 rows at 80.
    assert_eq!(t.scrollback_len(), 29);
    assert_eq!(cursor_row(&t), 0);
    assert_eq!(row(&t, 0), "$");
}

#[test]
fn a_blank_screen_keeps_the_cursor_on_its_row() {
    let mut t = Engine::new(80, 24);
    lines(&mut t, 40);
    t.feed(b"\x1b[H\x1b[2J\x1b[6;6H");
    t.resize(100, 24);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!((t.cursor().row, t.cursor().col), (5, 5));
    t.resize(100, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!((t.cursor().row, t.cursor().col), (5, 5));
}

#[test]
fn output_above_the_cursor_widened_keeps_the_history() {
    let mut t = cleared();
    t.feed(b"a\r\nb\r\n$ ");
    t.resize(100, 24);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 2);
    assert_eq!(row(&t, 1), "b");
}

#[test]
fn a_screen_that_narrows_past_its_rows_still_sends_its_top_to_history() {
    let mut t = Engine::new(80, 6);
    lines(&mut t, 8);
    t.feed(b"\x1b[H\x1b[2J");
    for _ in 0..4 {
        t.feed(format!("{}\r\n", "y".repeat(79)).as_bytes());
    }
    assert_eq!(t.scrollback_len(), 3);
    assert_eq!(cursor_row(&t), 4);
    t.resize(40, 6);
    // 3 history rows, 8 rows of content, the cursor's blank row: 12 rows, 6 on the screen.
    assert_eq!(t.scrollback_len(), 6);
    assert_eq!(cursor_row(&t), 5);
}

#[test]
fn a_cursor_on_the_last_row_still_takes_a_grow_from_history() {
    let mut t = Engine::new(80, 24);
    lines(&mut t, 40);
    assert_eq!(cursor_row(&t), 23);
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 11);
    assert_eq!(cursor_row(&t), 29);
    assert_eq!(row(&t, 0), "L12");
}

#[test]
fn content_below_the_cursor_still_takes_a_grow_from_history() {
    let mut t = Engine::new(80, 24);
    lines(&mut t, 40);
    t.feed(b"\x1b[5;1H");
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 11);
    assert_eq!(cursor_row(&t), 10);
    assert_eq!(row(&t, 0), "L12");
}

#[test]
fn a_row_with_only_a_background_counts_as_blank_below_the_cursor() {
    let mut t = cleared();
    t.feed(b"\x1b[44m\x1b[10;1H\x1b[2K\x1b[0m\x1b[1;3H");
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 0);
}

#[test]
fn a_row_shrink_with_nothing_below_the_cursor_is_unchanged() {
    let mut t = cleared();
    t.resize(80, 20);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 0);
    assert_eq!(row(&t, 0), "$");
}

#[test]
fn the_primary_keeps_its_history_while_the_alt_screen_is_up() {
    let mut t = cleared();
    t.feed(b"\x1b[?1049h\x1b[12;1Halt");
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    t.feed(b"\x1b[?1049l");
    assert_eq!(cursor_row(&t), 0);
    assert_eq!(row(&t, 0), "$");
}

/// 80x24, 40 lines, no clear, setting off: 17 lines of history, the cursor on the last row.
fn full_no_grow() -> Engine {
    let mut t = Engine::new(80, 24);
    t.set_grow_from_history(false);
    lines(&mut t, 40);
    assert_eq!((t.scrollback_len(), cursor_row(&t)), (17, 23));
    t
}

#[test]
fn the_setting_is_on_by_default_and_reads_back() {
    let mut t = Engine::new(80, 24);
    assert!(t.grow_from_history());
    t.set_grow_from_history(false);
    assert!(!t.grow_from_history());
    t.set_grow_from_history(true);
    assert!(t.grow_from_history());
}

#[test]
fn off_a_row_grow_adds_blank_rows_at_the_bottom() {
    let mut t = full_no_grow();
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 23);
    assert_eq!(row(&t, 0), "L18");
    assert_eq!(row(&t, 22), "L40");
    for r in 23..30 {
        assert_eq!(row(&t, r), "", "row {r}");
    }
}

#[test]
fn off_a_row_grow_with_content_below_the_cursor_takes_nothing_from_history() {
    let mut t = full_no_grow();
    t.feed(b"\x1b[5;1H");
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 4);
    assert_eq!(row(&t, 0), "L18");
}

#[test]
fn off_a_grow_in_both_axes_takes_nothing_from_history() {
    let mut t = full_no_grow();
    t.resize(100, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 23);
    assert_eq!(row(&t, 0), "L18");
}

#[test]
fn off_a_row_shrink_still_sends_rows_to_history() {
    let mut t = full_no_grow();
    t.resize(80, 20);
    assert_eq!(t.scrollback_len(), 21);
    assert_eq!(cursor_row(&t), 19);
}

#[test]
fn off_the_primary_takes_nothing_from_history_while_the_alt_screen_is_up() {
    let mut t = full_no_grow();
    t.feed(b"\x1b[?1049h");
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    t.feed(b"\x1b[?1049l");
    assert_eq!(cursor_row(&t), 23);
    assert_eq!(row(&t, 0), "L18");
}

#[test]
fn the_setting_survives_a_full_reset_and_still_governs_a_grow() {
    let mut t = Engine::new(80, 24);
    t.set_grow_from_history(false);
    t.feed(b"\x1bc");
    assert!(!t.grow_from_history());
    lines(&mut t, 40);
    t.resize(80, 30);
    assert_eq!(t.scrollback_len(), 17);
    assert_eq!(cursor_row(&t), 23);
}

#[test]
fn off_a_column_change_with_no_rows_to_add_still_takes_from_history() {
    let mut t = Engine::new(4, 3);
    t.set_grow_from_history(false);
    // History `a`..`d`, a line wrapped over two rows, the cursor on the blank last row.
    t.feed(b"a\r\nb\r\nc\r\nd\r\nefghij\r\n");
    assert_eq!(t.scrollback_len(), 4);
    assert_eq!(cursor_row(&t), 2);
    t.resize(8, 3);
    assert_eq!(t.scrollback_len(), 3);
    assert_eq!(cursor_row(&t), 2);
    assert_eq!(row(&t, 0), "d");
    assert_eq!(row(&t, 1), "efghij");
}
