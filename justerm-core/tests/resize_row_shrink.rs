//! A resize that removes rows trims blank rows below the cursor before it sends any row to
//! history (#1089), as ghostty does (`src/terminal/PageList.zig:2415-2431` @ `e6e26e1`). A row with
//! content is never dropped from below the cursor, and the cursor's own row is never trimmed.

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
