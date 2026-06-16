//! Issue #2 acceptance + the hidden-VT-state cases flagged in review
//! (pending-wrap, wide-char spacer). BCE is deferred to #7.

use justerm::{CellFlags, Color, Engine};

/// `feed("^[[31mhi^[[0m")` → snapshot shows red `hi`, stored as a reference.
#[test]
fn red_hi_stored_as_reference() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[31mhi\x1b[0m");

    let grid = term.grid();
    let h = grid.cell(0, 0);
    let i = grid.cell(0, 1);

    assert_eq!(h.c, 'h');
    assert_eq!(i.c, 'i');
    // Red is index 1, kept as a reference — never resolved to hex.
    assert_eq!(h.fg, Color::Indexed(1));
    assert_eq!(i.fg, Color::Indexed(1));
    // The trailing reset cleared the pen (it does not retro-edit written cells).
    assert_eq!(term.cursor().pen.fg, Color::Default);
}

/// 24-bit and 256-colour SGR both land as references, in semicolon form.
#[test]
fn truecolor_and_indexed_references() {
    let mut term = Engine::new(20, 2);
    term.feed(b"\x1b[38;2;10;20;30mA\x1b[48;5;200mB");

    assert_eq!(term.grid().cell(0, 0).fg, Color::Rgb(10, 20, 30));
    assert_eq!(term.grid().cell(0, 1).bg, Color::Indexed(200));
}

/// Standard attributes accumulate on the pen and reset on SGR 0.
#[test]
fn attributes_set_and_reset() {
    let mut term = Engine::new(20, 2);
    term.feed(b"\x1b[1;3mX");
    let x = *term.grid().cell(0, 0);
    assert!(x.flags.contains(CellFlags::BOLD));
    assert!(x.flags.contains(CellFlags::ITALIC));

    term.feed(b"\x1b[0mY");
    let y = *term.grid().cell(0, 1);
    assert!(!y.flags.contains(CellFlags::BOLD));
    assert!(!y.flags.contains(CellFlags::ITALIC));
}

/// Cursor positioning (CUP) and erase-in-line (EL) work together.
#[test]
fn cursor_move_and_erase() {
    let mut term = Engine::new(10, 3);
    term.feed(b"abc");

    // CUP is 1-based: row 1, col 1 → grid (0, 0). Overwrite 'a' with 'x'.
    term.feed(b"\x1b[1;1Hx");
    assert_eq!(term.grid().cell(0, 0).c, 'x');

    // Back to start of line, erase to end of line (EL default mode 0).
    term.feed(b"\x1b[1;1H\x1b[K");
    assert_eq!(term.grid().cell(0, 0).c, ' ');
    assert_eq!(term.grid().cell(0, 1).c, ' ');
    assert_eq!(term.grid().cell(0, 2).c, ' ');
}

/// Relative cursor movement (CUU/CUD/CUF/CUB).
#[test]
fn relative_cursor_movement() {
    let mut term = Engine::new(10, 5);
    // Down 2, forward 3, then print: lands at (2, 3).
    term.feed(b"\x1b[2B\x1b[3C*");
    assert_eq!(term.grid().cell(2, 3).c, '*');
    // The print advanced the cursor to col 4.
    term.feed(b"\x1b[1A"); // up 1 → row 1
    assert_eq!(term.cursor().row, 1);
}

/// ED mode 2 clears the whole screen.
#[test]
fn erase_display_all() {
    let mut term = Engine::new(5, 2);
    term.feed(b"hello");
    term.feed(b"\x1b[2J");
    for col in 0..5 {
        assert_eq!(term.grid().cell(0, col).c, ' ');
    }
}

/// Pending-wrap: filling the last column does NOT advance the line — the wrap
/// is deferred until the next print (else lines shift by one).
#[test]
fn pending_wrap_is_deferred() {
    let mut term = Engine::new(3, 3);
    term.feed(b"abc"); // exactly fills row 0

    assert_eq!(term.grid().cell(0, 2).c, 'c');
    // Still on row 0, parked on the last column, with the wrap pending.
    assert_eq!(term.cursor().row, 0);
    assert_eq!(term.cursor().col, 2);
    assert!(term.cursor().pending_wrap);

    term.feed(b"d"); // now the wrap happens
    assert_eq!(term.cursor().row, 1);
    assert_eq!(term.grid().cell(1, 0).c, 'd');
    assert!(!term.cursor().pending_wrap);
}

/// A width-2 glyph occupies two cells: the lead carries WIDE_CHAR, the trailing
/// column a distinct WIDE_CHAR_SPACER marker (not a plain blank).
#[test]
fn wide_char_marks_spacer() {
    let mut term = Engine::new(10, 2);
    term.feed("한".as_bytes()); // Hangul syllable, display width 2

    let grid = term.grid();
    assert_eq!(grid.cell(0, 0).c, '한');
    assert!(grid.cell(0, 0).flags.contains(CellFlags::WIDE_CHAR));
    assert!(grid.cell(0, 1).is_wide_spacer());
    // Cursor advanced by two columns.
    assert_eq!(term.cursor().col, 2);
}

/// Line-feed past the bottom scrolls the screen up (top line discarded for now).
#[test]
fn linefeed_scrolls_at_bottom() {
    let mut term = Engine::new(4, 2);
    term.feed(b"top\r\nbot"); // row0="top", row1="bot"
    assert_eq!(term.grid().cell(0, 0).c, 't');

    term.feed(b"\r\nnew"); // LF at bottom scrolls: "bot" up, "new" on the bottom
    assert_eq!(term.grid().cell(0, 0).c, 'b'); // was row 1, now row 0
    assert_eq!(term.grid().cell(1, 0).c, 'n');
}
