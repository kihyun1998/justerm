//! A column resize leaves the logical line holding the cursor un-reflowed (#1082).
//!
//! A shell redraws its prompt on SIGWINCH by relative motion counted in the *old* width's rows
//! (`CR`, `EL`, one `CUU` per row the cursor sat below the prompt's start, then the prompt at the
//! new width). Re-splitting the cursor's line changes that distance, so the redraw starts on the
//! wrong row. The cursor's line is cut or padded to the new width instead; every other line
//! reflows.

use justerm_core::Engine;

/// Row `i` of the viewport, trailing blanks trimmed.
fn row(t: &Engine, i: usize) -> String {
    let s: String = t.viewport_line(i).iter().map(|c| c.c()).collect();
    s.trim_end_matches(' ').to_string()
}

fn rows(t: &Engine, n: usize) -> Vec<String> {
    (0..n).map(|i| row(t, i)).collect()
}

fn cursor(t: &Engine) -> (usize, usize, bool) {
    let c = t.cursor();
    (c.row, c.col, c.pending_wrap)
}

#[test]
fn the_issue_case_redraws_over_its_own_prompt() {
    let mut t = Engine::new(30, 10);
    t.feed(b"user@host:~$ ");
    t.resize(10, 10);
    assert_eq!(cursor(&t), (0, 9, false));
    t.feed(b"\r\x1b[K\ruser@host:~\r~$ ");
    assert_eq!(rows(&t, 3), ["user@host:", "~$", ""]);
    assert_eq!(cursor(&t), (1, 3, false));
}

#[test]
fn a_cursor_after_trailing_blanks_keeps_its_column_across_a_resize() {
    for cols in [20, 40] {
        let mut t = Engine::new(30, 6);
        t.feed(b"ab  ");
        t.resize(cols, 6);
        assert_eq!(cursor(&t), (0, 4, false), "width {cols}");
        t.feed(b"X");
        assert_eq!(row(&t, 0), "ab  X", "width {cols}");
    }
}

#[test]
fn an_unfinished_line_is_cut_at_the_new_width_and_the_next_print_starts_a_row() {
    let mut t = Engine::new(30, 6);
    t.feed(b"progress: 0123456789abcdefghij");
    assert_eq!(cursor(&t), (0, 29, true));
    t.resize(10, 6);
    assert_eq!(rows(&t, 2), ["progress:", ""]);
    assert_eq!(cursor(&t), (0, 9, true));
    t.feed(b"X");
    assert_eq!(rows(&t, 2), ["progress:", "X"]);
}

#[test]
fn a_soft_wrapped_line_above_the_cursor_still_reflows() {
    let mut t = Engine::new(20, 6);
    t.feed(b"0123456789abcdefghij\r\nuser@host:~$ ");
    t.resize(10, 6);
    assert_eq!(rows(&t, 3), ["0123456789", "abcdefghij", "user@host:"]);
    assert_eq!(cursor(&t), (2, 9, false));
}

#[test]
fn a_cursor_moved_past_the_new_width_is_clamped_without_arming_a_wrap() {
    // Parking a cursor that was not parked would glue its row to the next line on the next print.
    let mut t = Engine::new(8, 4);
    t.feed(b"abcdef\r\n$ \x1b[1;7H");
    t.resize(3, 4);
    assert_eq!(cursor(&t), (0, 2, false));
    t.feed(b"Z");
    assert_eq!(rows(&t, 2), ["abZ", "$"]);
}

#[test]
fn a_cursor_on_a_blank_line_costs_no_row_on_a_pane_without_history() {
    let mut t = Engine::with_scrollback(6, 4, 0);
    t.feed(b"aaaaaa\r\nbbbbbb\r\ncccccc\r\n   ");
    t.resize(3, 4);
    assert_eq!(rows(&t, 4), ["bbb", "bbb", "ccc", "ccc"]);
    assert_eq!(cursor(&t), (3, 2, false));
}

#[test]
fn a_parked_cursor_widening_takes_the_one_past_column() {
    let mut t = Engine::new(13, 6);
    t.feed(b"user@host:~$ ");
    assert_eq!(cursor(&t), (0, 12, true));
    t.resize(20, 6);
    assert_eq!(cursor(&t), (0, 13, false));
}

#[test]
fn the_cursor_line_keeps_its_rows_on_a_widening() {
    // Two rows at 13, the cursor on the second: readline climbs one row on SIGWINCH.
    let mut t = Engine::new(13, 6);
    t.feed(b"abcdefghijklmnopqrstuvwxyz ");
    assert_eq!(cursor(&t), (2, 1, false));
    t.resize(40, 6);
    assert_eq!(rows(&t, 3), ["abcdefghijklm", "nopqrstuvwxyz", ""]);
    assert_eq!(cursor(&t), (2, 1, false));
}

#[test]
fn a_selection_on_the_cursor_line_is_clamped_to_what_is_left_of_it() {
    let mut t = Engine::new(30, 6);
    t.feed(b"user@host:~$ ls");
    t.selection_begin(
        0,
        0,
        justerm_core::Side::Left,
        justerm_core::SelectionType::Char,
    );
    t.selection_extend(0, 14, justerm_core::Side::Right);
    assert_eq!(
        t.selection_text().as_deref(),
        Some("user@host:~$ ls"),
        "fixture"
    );
    t.resize(10, 6);
    assert_eq!(t.selection_text().as_deref(), Some("user@host:"));
}

#[test]
fn a_cursor_on_a_blank_line_between_lines_keeps_its_column() {
    let mut t = Engine::new(10, 6);
    t.feed(b"abc\r\n\r\nxyz\x1b[2;5H");
    assert_eq!(cursor(&t), (1, 4, false), "fixture");
    t.resize(8, 6);
    assert_eq!(cursor(&t), (1, 4, false));
}

#[test]
fn a_cursor_on_the_first_row_of_a_wrapped_line_keeps_the_whole_line() {
    // readline with the cursor moved back onto the prompt's first row.
    // Its second row is wider than the new width, so re-splitting it would add a row.
    let mut t = Engine::new(30, 6);
    t.feed(b"0123456789abcdefghij0123456789ABCDEFGHIJKLMNO\x1b[1;5H");
    t.resize(10, 6);
    assert_eq!(rows(&t, 3), ["0123456789", "ABCDEFGHIJ", ""]);
    assert_eq!(cursor(&t), (0, 4, false));
}

#[test]
fn a_command_mark_on_the_cursor_line_is_bounded_by_the_new_width() {
    // `col == cols` is a mark's exclusive bound, "all of this row".
    let mut t = Engine::new(30, 6);
    t.feed(b"user@host:~$ ls\x1b]133;B\x07");
    t.resize(10, 6);
    let ix = t.marker_index();
    assert_eq!((ix.markers[0].line, ix.markers[0].col), (0, 10));
}

#[test]
fn under_the_alt_screen_the_kept_line_is_the_primarys_cursor_line() {
    // `?47` / `?1047` save no cursor, and `?1048h` on the alt screen writes alt coordinates into
    // the slot, so neither can name the primary's line: the line kept is the one the cursor was on
    // when the screen switched.
    for enter in [
        &b"\x1b[?47h"[..],
        b"\x1b[?1047h",
        b"\x1b[?1049h\x1b[H\x1b[?1048h",
    ] {
        let mut t = Engine::new(20, 5);
        t.feed(b"0123456789abcdefghijKLMNOPQRST\r\n$ ");
        t.feed(enter);
        t.resize(10, 5);
        t.feed(b"\x1b[?47l\x1b[?1049l");
        assert_eq!(
            rows(&t, 4),
            ["0123456789", "abcdefghij", "KLMNOPQRST", "$"],
            "{enter:?}"
        );
    }
}

#[test]
fn a_second_resize_under_the_alt_screen_keeps_the_same_line() {
    // The first resize re-splits the line above, so the primary's cursor line moves down a row.
    let mut t = Engine::new(20, 6);
    t.feed(b"0123456789abcdefghijKLMNOPQRST\r\n$ \x1b[?47h");
    t.resize(10, 6);
    t.resize(15, 6);
    t.feed(b"\x1b[?47l");
    assert_eq!(rows(&t, 3), ["0123456789abcde", "fghijKLMNOPQRST", "$"]);
}

#[test]
fn a_wide_glyph_cut_from_the_cursor_rows_last_row_stays_a_lead() {
    // A hard-ended row: the stranded lead is ADR-0025's legal state (#529), as on the alt pane.
    let mut t = Engine::new(10, 5);
    t.feed("abcdefgh中".as_bytes());
    t.resize(9, 5);
    assert!(t.viewport_line(0)[8].is_wide());
}

#[test]
fn a_wide_glyph_cut_from_a_wrapped_cursor_row_leaves_the_wrap_artefact() {
    // `中` straddles the new boundary on a row that continues; left as a lead it would come back
    // mid-line on a later reflow and take its neighbour for its spacer.
    let mut t = Engine::new(10, 5);
    t.feed("abcdefgh中XYZ".as_bytes());
    t.resize(9, 5);
    t.feed(b"\r\n$ ");
    t.resize(12, 5);
    assert_eq!(row(&t, 0), "abcdefghXYZ");
    t.feed(b"\x1b[1;9HW");
    assert_eq!(row(&t, 0), "abcdefghWYZ");
}

// ---- the recording ---------------------------------------------------------------------------

/// `capture-resize-shell-redraw.sh`: records of `<case> <cols> <len>\n<len bytes>`, three per case
/// — start-up at the start width, the redraw after a resize, the reply to an Enter.
const RECORDING: &[u8] = include_bytes!("fixtures/resize_shell_redraw.raw");

fn records(mut raw: &[u8]) -> Vec<(&str, usize, &[u8])> {
    let mut out = Vec::new();
    while !raw.is_empty() {
        let nl = raw.iter().position(|&b| b == b'\n').expect("header line");
        let header = std::str::from_utf8(&raw[..nl]).expect("ascii header");
        let mut it = header.split(' ');
        let case = it.next().expect("case");
        let cols: usize = it.next().expect("cols").parse().unwrap();
        let len: usize = it.next().expect("len").parse().unwrap();
        out.push((case, cols, &raw[nl + 1..nl + 1 + len]));
        raw = &raw[nl + 1 + len..];
    }
    out
}

/// `text` laid out at `cols`, each row trimmed, a row holding only the trailing space dropped.
fn laid_out(text: &str, cols: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = chars
        .chunks(cols)
        .map(|c| c.iter().collect::<String>().trim_end().to_string())
        .collect();
    if out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// What each case shows on its prompt line: the prompt and any typed text.
fn prompt_line(case: &str) -> &'static str {
    match case {
        c if c.contains("13typed") => "user@host:~$ echo hello world",
        c if c.contains("13") => "user@host:~$ ",
        c if c.contains("32") => "user@host:~/a/very/long/path/x$ ",
        c if c.contains("26") => "user@host:~/projects/app$ ",
        _ => unreachable!("{case}"),
    }
}

#[test]
fn a_shell_redraw_after_a_one_call_resize_leaves_one_prompt() {
    let recs = records(RECORDING);
    assert_eq!(recs.len() % 3, 0);
    let cases: Vec<&str> = recs.iter().step_by(3).map(|r| r.0).collect();
    assert_eq!(cases.len(), 7, "{cases:?}");

    for case in recs.chunks(3) {
        let [(name, from, start), (_, to, redraw), (_, _, enter)] = case else {
            unreachable!()
        };
        let line = prompt_line(name);
        let mut t = Engine::new(*from, 16);
        t.feed(b"OUT1\r\nOUT2\r\n");
        t.feed(start);
        t.resize(*to, 16);
        t.feed(redraw);

        let mut want = vec!["OUT1".to_string(), "OUT2".to_string()];
        want.extend(laid_out(line, *to));
        let n = want.len();
        assert_eq!(rows(&t, n + 1)[..n], want[..], "{name} after the redraw");
        assert_eq!(row(&t, n), "", "{name}: nothing below the prompt");
        // The cursor sits just after the prompt line.
        let (r, c, parked) = cursor(&t);
        assert_eq!(
            (r - 2) * to + c + usize::from(parked),
            line.chars().count(),
            "{name} cursor"
        );

        // An Enter adds exactly one prompt below.
        t.feed(enter);
        let prompt = if name.contains("typed") {
            want.extend(laid_out("hello world", *to));
            "user@host:~$ "
        } else {
            line
        };
        want.extend(laid_out(prompt, *to));
        let n = want.len();
        assert_eq!(rows(&t, n + 1)[..n], want[..], "{name} after Enter");
        assert_eq!(row(&t, n), "", "{name}: nothing below the new prompt");
    }
}
