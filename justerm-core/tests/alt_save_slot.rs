//! `?1049h` and `?1048h` save into the active screen's DECSC slot (#1092), as xterm
//! (`charproc.c:7730`, `:7908-7913`), xterm.js (`common/InputHandler.ts:2008-2012`), ghostty
//! (`src/terminal/Terminal.zig:4364`) and alacritty (`alacritty_terminal/src/term/mod.rs:719-720`)
//! do. Only the caret shape keeps a stash of its own across `?1049`, so leaving the alternate
//! screen still restores it (#927).

use justerm_core::{CursorShape, Engine};

fn cursor(t: &Engine) -> (usize, usize) {
    (t.cursor().row, t.cursor().col)
}

#[test]
fn an_alt_screen_round_trip_saves_into_the_decsc_slot() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[2;3H\x1b7\x1b[5;6H\x1b[?1049h\x1b[?1049l\x1b[1;1H\x1b8");
    assert_eq!(
        cursor(&t),
        (4, 5),
        "the ?1049h save overwrote the DECSC one"
    );
}

#[test]
fn mode_1048_set_is_decsc() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[2;3H\x1b7\x1b[5;6H\x1b[?1048h\x1b[1;1H\x1b8");
    assert_eq!(cursor(&t), (4, 5));
}

#[test]
fn mode_1048_reset_is_decrc() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[2;3H\x1b7\x1b[5;6H\x1b[?1048l");
    assert_eq!(cursor(&t), (1, 2));
}

#[test]
fn leaving_the_alt_screen_restores_the_charsets_saved_on_entering() {
    let mut t = Engine::new(10, 6);
    // G0 is DEC special graphics before ?1049h and ASCII inside; `q` after leaving is a line.
    t.feed(b"\x1b(0\x1b[?1049h\x1b(B\x1b[?1049lq");
    assert_eq!(t.grid().cell(0, 0).c(), '\u{2500}');
}

#[test]
fn leaving_the_alt_screen_restores_the_origin_mode_saved_on_entering() {
    let mut t = Engine::new(10, 6);
    // Margins rows 2..5 with DECOM on, turned off inside the alternate screen.
    t.feed(b"\x1b[2;5r\x1b[?6h\x1b[?1049h\x1b[?6l\x1b[?1049l\x1b[1;1H");
    assert_eq!(
        cursor(&t),
        (1, 0),
        "CUP 1;1 homes to the top margin under DECOM"
    );
}

#[test]
fn mode_1048_does_not_restore_the_caret_shape() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[6 q\x1b[?1048h\x1b[2 q\x1b[?1048l");
    assert_eq!(t.frame().cursor_shape, Some(CursorShape::Block));
}

#[test]
fn mode_1049_set_on_the_alt_screen_saves_into_its_slot() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[?1049h\x1b[3;4H\x1b[?1049h\x1b[1;1H\x1b8");
    assert_eq!(cursor(&t), (2, 3));
}

#[test]
fn leaving_the_alt_screen_keeps_the_cursor_visibility() {
    let mut t = Engine::new(10, 6);
    t.feed(b"\x1b[?1049h\x1b[?25l\x1b[?1049l");
    assert!(!t.cursor().visible, "DECTCEM is not part of the save");
}

#[test]
fn a_second_1049_set_keeps_the_shape_from_before_the_first() {
    let mut t = Engine::new(10, 6);
    // Bar before entering; Block inside; a second ?1049h on the alt screen; then leave.
    t.feed(b"\x1b[6 q\x1b[?1049h\x1b[2 q\x1b[?1049h\x1b[?1049l");
    assert_eq!(t.frame().cursor_shape, Some(CursorShape::Bar));
}
