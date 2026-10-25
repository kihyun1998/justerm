//! A narrowing resize and a cursor parked on a prompt that fills its row exactly (#1076).
//!
//! The prompt's trailing space is the last cell, and the cursor is parked on it in the deferred
//! wrap. The reflow trims that space as padding, so the cursor comes back "just past a full row";
//! read as the start of the next row, it put the cursor one row below the one the shell believes
//! it is on, and the shell's SIGWINCH redraw (`\r ESC[K \r <prompt>`) left a second prompt.

use justerm_core::Engine;

/// Row `i` of the viewport, trailing blanks trimmed.
fn row(t: &Engine, i: usize) -> String {
    let s: String = t.viewport_line(i).iter().map(|c| c.c()).collect();
    s.trim_end_matches(' ').to_string()
}

fn cursor(t: &Engine) -> (usize, usize, bool) {
    let c = t.cursor();
    (c.row, c.col, c.pending_wrap)
}

#[test]
fn a_one_column_narrowing_keeps_a_parked_cursor_on_its_row() {
    let mut t = Engine::new(13, 6);
    t.feed(b"user@host:~$ ");
    assert_eq!(cursor(&t), (0, 12, true));

    t.resize(12, 6);
    assert_eq!(cursor(&t), (0, 11, true));

    // bash's redraw, as the issue recorded it: one prompt, the cursor on the row after it.
    t.feed(b"\r\x1b[K\ruser@host:~$ \r ");
    assert_eq!(row(&t, 0), "user@host:~$");
    assert_eq!(row(&t, 1), "");
    assert_eq!(cursor(&t), (1, 1, false));
}

#[test]
fn the_next_print_after_the_narrowing_wraps_past_the_prompt() {
    let mut t = Engine::new(13, 6);
    t.feed(b"user@host:~$ ");
    t.resize(12, 6);
    t.feed(b"X");
    assert_eq!(row(&t, 0), "user@host:~$");
    assert_eq!(row(&t, 1), "X");
    assert_eq!(cursor(&t), (1, 1, false));
}

#[test]
fn a_parked_prompt_on_the_bottom_row_does_not_scroll_the_screen() {
    let mut t = Engine::new(13, 3);
    t.feed(b"one\r\ntwo\r\nuser@host:~$ ");
    assert_eq!(cursor(&t), (2, 12, true));

    t.resize(12, 3);
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(row(&t, 0), "one");
    assert_eq!(cursor(&t), (2, 11, true));
}

#[test]
fn real_text_past_the_new_width_still_reflows_and_the_cursor_follows_it() {
    let mut t = Engine::new(13, 6);
    t.feed(b"abcdefghijklm");
    t.resize(12, 6);
    assert_eq!(row(&t, 0), "abcdefghijkl");
    assert_eq!(row(&t, 1), "m");
    assert_eq!(cursor(&t), (1, 1, false));
}

#[test]
fn a_widening_from_the_same_state_steps_the_cursor_past_the_space() {
    let mut t = Engine::new(13, 6);
    t.feed(b"user@host:~$ ");
    t.resize(14, 6);
    assert_eq!(cursor(&t), (0, 13, false));
}

#[test]
fn a_widening_that_lands_just_past_a_full_row_keeps_the_next_row() {
    // 26 visible cells and the space wrap to two rows at 13 columns, parked on the second. readline
    // believes the cursor is one row below the prompt's start and climbs one row on SIGWINCH, so
    // the cursor stays on the row after the content.
    let mut t = Engine::new(13, 6);
    t.feed(b"\r\n\r\nabcdefghijklmnopqrstuvwxy ");
    assert_eq!(cursor(&t), (3, 12, true));

    t.resize(25, 6);
    assert_eq!(row(&t, 2), "abcdefghijklmnopqrstuvwxy");
    assert_eq!(cursor(&t), (3, 1, false));
}

#[test]
fn a_saved_cursor_parked_on_the_prompt_returns_to_its_row_after_the_alt_screen() {
    let mut t = Engine::new(13, 6);
    t.feed(b"user@host:~$ \x1b[?1049h");
    t.resize(12, 6);
    t.feed(b"\x1b[?1049l");
    assert_eq!(cursor(&t), (0, 11, true));
}

// ---- the recording ---------------------------------------------------------------------------

/// `capture-resize-prompt.sh`: records of `<cols> <len>\n<len bytes>`, the first fed at its width,
/// each later one after a resize to its width.
const RESIZE_PROMPT: &[u8] = include_bytes!("fixtures/resize_prompt.raw");
const PROMPT: &str = "abcdefghijklmnopqrstuvwxyz";
const PROMPT_CELLS: usize = PROMPT.len() + 1;

fn records(mut raw: &[u8]) -> Vec<(usize, &[u8])> {
    let mut out = Vec::new();
    while !raw.is_empty() {
        let nl = raw.iter().position(|&b| b == b'\n').expect("header line");
        let header = std::str::from_utf8(&raw[..nl]).expect("ascii header");
        let (cols, len) = header.split_once(' ').expect("`<cols> <len>`");
        let (cols, len): (usize, usize) = (cols.parse().unwrap(), len.parse().unwrap());
        out.push((cols, &raw[nl + 1..nl + 1 + len]));
        raw = &raw[nl + 1 + len..];
    }
    out
}

#[test]
fn bash_redrawing_its_prompt_across_a_narrowing_drag_leaves_one_prompt() {
    let recs = records(RESIZE_PROMPT);
    assert_eq!(
        recs.iter().map(|r| r.0).collect::<Vec<_>>(),
        (20..=30).rev().collect::<Vec<_>>()
    );
    // The recording passes through the width where the prompt fills the row exactly.
    assert!(recs.iter().any(|r| r.0 == PROMPT_CELLS));

    let mut t = Engine::new(recs[0].0, 6);
    for (i, &(cols, bytes)) in recs.iter().enumerate() {
        if i > 0 {
            t.resize(cols, 6);
        }
        t.feed(bytes);

        let rows: Vec<String> = (0..6).map(|r| row(&t, r)).collect();
        let joined: String = rows.concat();
        assert_eq!(joined, PROMPT, "width {cols}: {rows:?}");
        // The cursor sits just after the prompt's trailing space.
        let (r, c, parked) = cursor(&t);
        assert_eq!(r * cols + c + usize::from(parked), PROMPT_CELLS, "width {cols}");
    }
}
