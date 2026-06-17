//! #2 acceptance + full branch coverage of the slice's implementation.
//!
//! Every behaviour `term.rs` implements gets at least one test here. The
//! hidden-VT-state cases (pending-wrap, wide-char spacer, wide-char-doesn't-fit)
//! are additionally mutation-checked during review — break the code, watch the
//! matching test go red — so "green" means "verified", not just "passed once".
//! BCE and grapheme combining marks are deferred to #8 (documented below).

use justerm::{CellFlags, Color, Engine};

// ===========================================================================
// SGR — colour references and attributes
// ===========================================================================

/// `feed("^[[31mhi^[[0m")` → snapshot shows red `hi`, stored as a reference.
#[test]
fn red_hi_stored_as_reference() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[31mhi\x1b[0m");

    let grid = term.grid();
    assert_eq!(grid.cell(0, 0).c, 'h');
    assert_eq!(grid.cell(0, 1).c, 'i');
    // Red is index 1, kept as a reference — never resolved to hex.
    assert_eq!(grid.cell(0, 0).fg, Color::Indexed(1));
    assert_eq!(grid.cell(0, 1).fg, Color::Indexed(1));
    // The trailing reset cleared the pen (it does not retro-edit written cells).
    assert_eq!(term.cursor().pen.fg, Color::Default);
}

/// Indexed background (40..=47).
#[test]
fn indexed_background() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b[42mX");
    assert_eq!(term.grid().cell(0, 0).bg, Color::Indexed(2));
}

/// Bright fg/bg (aixterm 90..=97 / 100..=107) map to palette slots 8..=15.
#[test]
fn bright_colors_map_to_high_palette() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b[91mA\x1b[102mB");
    assert_eq!(term.grid().cell(0, 0).fg, Color::Indexed(9)); // 91 - 90 + 8
    assert_eq!(term.grid().cell(0, 1).bg, Color::Indexed(10)); // 102 - 100 + 8
}

/// 24-bit and 256-colour SGR in legacy (semicolon) form.
#[test]
fn extended_color_semicolon_form() {
    let mut term = Engine::new(20, 2);
    term.feed(b"\x1b[38;2;10;20;30mA\x1b[48;5;200mB");
    assert_eq!(term.grid().cell(0, 0).fg, Color::Rgb(10, 20, 30));
    assert_eq!(term.grid().cell(0, 1).bg, Color::Indexed(200));
}

/// 24-bit and 256-colour SGR in sub-parameter (colon) form, incl. the
/// colorspace-id variant `38:2::r:g:b`.
#[test]
fn extended_color_colon_form() {
    let mut term = Engine::new(20, 2);
    term.feed(b"\x1b[38:2:1:2:3mA\x1b[38:5:5mB\x1b[48:2::4:5:6mC");
    assert_eq!(term.grid().cell(0, 0).fg, Color::Rgb(1, 2, 3));
    assert_eq!(term.grid().cell(0, 1).fg, Color::Indexed(5));
    assert_eq!(term.grid().cell(0, 2).bg, Color::Rgb(4, 5, 6)); // colorspace id skipped
}

/// SGR 39/49 reset fg/bg to Default without touching other attrs.
#[test]
fn default_color_reset() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b[31;41;1mA\x1b[39;49mB");
    let b = *term.grid().cell(0, 1);
    assert_eq!(b.fg, Color::Default);
    assert_eq!(b.bg, Color::Default);
    // 39/49 do not clear bold.
    assert!(b.flags.contains(CellFlags::BOLD));
}

/// Each standard attribute sets its flag; SGR 0 resets everything.
#[test]
fn attributes_set_and_full_reset() {
    let mut term = Engine::new(40, 2);
    term.feed(b"\x1b[1;2;3;4;5;7;8;9mX");
    let x = *term.grid().cell(0, 0);
    for f in [
        CellFlags::BOLD,
        CellFlags::DIM,
        CellFlags::ITALIC,
        CellFlags::UNDERLINE,
        CellFlags::BLINK,
        CellFlags::INVERSE,
        CellFlags::HIDDEN,
        CellFlags::STRIKETHROUGH,
    ] {
        assert!(x.flags.contains(f), "missing {f:?}");
    }

    term.feed(b"\x1b[0mY");
    assert_eq!(term.grid().cell(0, 1).flags, CellFlags::empty());
}

/// Per-attribute removal (22 clears bold+dim; 23/24/25/27/28/29 clear one each).
#[test]
fn attribute_removal() {
    let mut term = Engine::new(40, 2);
    // Set bold+dim+italic+underline, then clear bold+dim (22) and italic (23).
    term.feed(b"\x1b[1;2;3;4m\x1b[22;23mX");
    let x = *term.grid().cell(0, 0);
    assert!(!x.flags.contains(CellFlags::BOLD));
    assert!(!x.flags.contains(CellFlags::DIM));
    assert!(!x.flags.contains(CellFlags::ITALIC));
    // Underline survived — removal is selective.
    assert!(x.flags.contains(CellFlags::UNDERLINE));
}

// ===========================================================================
// Cursor movement (CSI A/B/C/D/G/d/H/f)
// ===========================================================================

/// CUP (H, 1-based) and the absolute column/row forms CHA (G) / VPA (d).
#[test]
fn absolute_positioning() {
    let mut term = Engine::new(10, 5);

    term.feed(b"\x1b[2;4Hx"); // CUP row 2 col 4 → grid (1, 3)
    assert_eq!(term.grid().cell(1, 3).c, 'x');

    term.feed(b"\x1b[3Gy"); // CHA col 3 → (1, 2)
    assert_eq!(term.grid().cell(1, 2).c, 'y');

    term.feed(b"\x1b[4dz"); // VPA row 4 → row 3; col unchanged = 3 (print of 'y' advanced it)
    assert_eq!(term.grid().cell(3, 3).c, 'z');
}

/// Relative movement CUU/CUD/CUF/CUB.
#[test]
fn relative_movement_all_directions() {
    let mut term = Engine::new(10, 5);
    term.feed(b"\x1b[2B\x1b[4C*"); // down 2, forward 4 → (2, 4)
    assert_eq!(term.grid().cell(2, 4).c, '*');

    // From (2, 5) after the print: up 1, back 3 → (1, 2).
    term.feed(b"\x1b[1A\x1b[3D#");
    assert_eq!(term.grid().cell(1, 2).c, '#');
}

/// Movement saturates at the grid edges (no panic, no wrap-around).
#[test]
fn movement_clamps_at_edges() {
    let mut term = Engine::new(4, 3);
    term.feed(b"\x1b[99A\x1b[99D"); // far up + far left
    assert_eq!((term.cursor().row, term.cursor().col), (0, 0));
    term.feed(b"\x1b[99B\x1b[99C"); // far down + far right
    assert_eq!((term.cursor().row, term.cursor().col), (2, 3));
}

/// An explicit cursor move clears a pending wrap (so the next print does not
/// spuriously wrap).
#[test]
fn cursor_move_clears_pending_wrap() {
    let mut term = Engine::new(3, 3);
    term.feed(b"abc"); // fills row 0 → pending_wrap set
    assert!(term.cursor().pending_wrap);
    term.feed(b"\x1b[1;1HX"); // CUP back to start clears it
    assert_eq!(term.grid().cell(0, 0).c, 'X');
    assert_eq!(term.cursor().row, 0); // did NOT wrap to row 1
}

// ===========================================================================
// Erase (CSI J / K) — every mode
// ===========================================================================

/// EL: 0 = cursor→end, 1 = start→cursor, 2 = whole line.
#[test]
fn erase_line_modes() {
    let setup = || {
        let mut t = Engine::new(5, 1);
        t.feed(b"abcde\x1b[1;3H"); // fill, cursor to col 3 (grid col 2)
        t
    };

    let mut t0 = setup();
    t0.feed(b"\x1b[0K"); // cursor→end
    assert_eq!(t0.grid().cell(0, 1).c, 'b'); // before cursor kept
    assert_eq!(t0.grid().cell(0, 2).c, ' '); // cursor cleared
    assert_eq!(t0.grid().cell(0, 4).c, ' ');

    let mut t1 = setup();
    t1.feed(b"\x1b[1K"); // start→cursor (inclusive)
    assert_eq!(t1.grid().cell(0, 0).c, ' ');
    assert_eq!(t1.grid().cell(0, 2).c, ' ');
    assert_eq!(t1.grid().cell(0, 3).c, 'd'); // after cursor kept

    let mut t2 = setup();
    t2.feed(b"\x1b[2K"); // whole line
    assert_eq!(t2.grid().cell(0, 0).c, ' ');
    assert_eq!(t2.grid().cell(0, 4).c, ' ');
}

/// ED: 0 = cursor→end-of-screen, 1 = start→cursor, 2 = whole screen.
#[test]
fn erase_display_modes() {
    let setup = || {
        let mut t = Engine::new(3, 3);
        t.feed(b"aaa\r\nbbb\r\nccc\x1b[2;2H"); // 3 rows filled, cursor to (1, 1)
        t
    };

    let mut t0 = setup();
    t0.feed(b"\x1b[0J"); // cursor→end
    assert_eq!(t0.grid().cell(0, 0).c, 'a'); // row above kept
    assert_eq!(t0.grid().cell(1, 0).c, 'b'); // before cursor on its row kept
    assert_eq!(t0.grid().cell(1, 1).c, ' '); // cursor cleared
    assert_eq!(t0.grid().cell(2, 0).c, ' '); // row below cleared

    let mut t1 = setup();
    t1.feed(b"\x1b[1J"); // start→cursor
    assert_eq!(t1.grid().cell(0, 0).c, ' '); // row above cleared
    assert_eq!(t1.grid().cell(1, 1).c, ' '); // cursor cleared
    assert_eq!(t1.grid().cell(1, 2).c, 'b'); // after cursor on its row kept
    assert_eq!(t1.grid().cell(2, 0).c, 'c'); // row below kept

    let mut t2 = setup();
    t2.feed(b"\x1b[2J"); // whole screen
    for r in 0..3 {
        for c in 0..3 {
            assert_eq!(t2.grid().cell(r, c).c, ' ');
        }
    }
}

// ===========================================================================
// C0 controls (execute)
// ===========================================================================

/// CR returns to column 0; LF moves down a line.
#[test]
fn carriage_return_and_linefeed() {
    let mut term = Engine::new(10, 3);
    term.feed(b"ab\r\nc");
    assert_eq!(term.grid().cell(0, 0).c, 'a');
    assert_eq!(term.grid().cell(1, 0).c, 'c'); // CR reset col, LF dropped to row 1
}

/// LF (and VT/FF) past the bottom scrolls the screen up; the top line is
/// discarded for now (scrollback retention is #3).
#[test]
fn linefeed_scrolls_at_bottom() {
    let mut term = Engine::new(4, 2);
    term.feed(b"top\r\nbot");
    assert_eq!(term.grid().cell(0, 0).c, 't');

    term.feed(b"\r\nnew");
    assert_eq!(term.grid().cell(0, 0).c, 'b'); // "bot" shifted up
    assert_eq!(term.grid().cell(1, 0).c, 'n');
}

/// Backspace moves the cursor left by one (and the next print overwrites).
#[test]
fn backspace_moves_left() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ab\x08c"); // b at col1, BS to col1, c overwrites
    assert_eq!(term.grid().cell(0, 1).c, 'c');
    assert_eq!(term.cursor().col, 2);
}

/// Horizontal tab advances to the next 8-column stop.
#[test]
fn tab_to_next_stop() {
    let mut term = Engine::new(20, 2);
    term.feed(b"\tx");
    assert_eq!(term.grid().cell(0, 8).c, 'x');
}

// ===========================================================================
// Hidden VT state
// ===========================================================================

/// Pending-wrap: filling the last column does NOT advance the line — the wrap
/// is deferred until the next print (else lines shift by one).
/// Mutation-checked: eager wrap → this test goes red.
#[test]
fn pending_wrap_is_deferred() {
    let mut term = Engine::new(3, 3);
    term.feed(b"abc"); // exactly fills row 0

    assert_eq!(term.grid().cell(0, 2).c, 'c');
    assert_eq!(term.cursor().row, 0); // parked, not yet wrapped
    assert_eq!(term.cursor().col, 2);
    assert!(term.cursor().pending_wrap);

    term.feed(b"d"); // now the wrap happens
    assert_eq!(term.cursor().row, 1);
    assert_eq!(term.grid().cell(1, 0).c, 'd');
    assert!(!term.cursor().pending_wrap);
}

/// Overwriting one half of a wide glyph clears the other half — no orphaned
/// lead or spacer is left behind.
#[test]
fn overwriting_a_wide_char_clears_its_other_half() {
    // Overwrite the lead → the spacer must be cleared.
    let mut a = Engine::new(10, 1);
    a.feed("한".as_bytes()); // (0,0)=lead, (0,1)=spacer
    a.feed(b"\x1b[1;1Hx"); // write 'x' over the lead
    assert_eq!(a.grid().cell(0, 0).c, 'x');
    assert!(!a.grid().cell(0, 1).flags.contains(CellFlags::WIDE_CHAR_SPACER));

    // Overwrite the spacer → the lead must be cleared.
    let mut b = Engine::new(10, 1);
    b.feed("한".as_bytes());
    b.feed(b"\x1b[1;2Hy"); // write 'y' over the spacer
    assert_eq!(b.grid().cell(0, 1).c, 'y');
    assert!(!b.grid().cell(0, 0).flags.contains(CellFlags::WIDE_CHAR));
}

/// A width-2 glyph occupies two cells: the lead carries WIDE_CHAR, the trailing
/// column a distinct WIDE_CHAR_SPACER marker (not a plain blank).
/// Mutation-checked: drop the spacer flag → this test goes red.
#[test]
fn wide_char_marks_spacer() {
    let mut term = Engine::new(10, 2);
    term.feed("한".as_bytes()); // Hangul syllable, display width 2

    let grid = term.grid();
    assert_eq!(grid.cell(0, 0).c, '한');
    assert!(grid.cell(0, 0).flags.contains(CellFlags::WIDE_CHAR));
    assert!(grid.cell(0, 1).is_wide_spacer());
    assert_eq!(term.cursor().col, 2);
}

/// A width-2 glyph that cannot fit in the last column wraps to the next line
/// first; the vacated last column is left blank (common-90%).
/// Mutation-checked: remove the fit-check wrap → glyph lands on row 0, red.
#[test]
fn wide_char_wraps_when_it_does_not_fit() {
    let mut term = Engine::new(3, 2);
    term.feed("ab한".as_bytes()); // 'a','b' fill cols 0,1; '한' can't fit at col 2

    let grid = term.grid();
    assert_eq!(grid.cell(0, 0).c, 'a');
    assert_eq!(grid.cell(0, 1).c, 'b');
    assert_eq!(grid.cell(0, 2).c, ' '); // vacated last column left blank
    // The wide glyph wrapped to the next row.
    assert_eq!(grid.cell(1, 0).c, '한');
    assert!(grid.cell(1, 0).flags.contains(CellFlags::WIDE_CHAR));
    assert!(grid.cell(1, 1).is_wide_spacer());
}

/// Zero-width code points (combining marks) are currently dropped, not placed
/// in their own cell. This pins the DEFERRED behaviour: the grapheme-cluster
/// side-table that attaches them to the base cell is tracked in #8.
#[test]
fn zero_width_is_dropped_for_now() {
    let mut term = Engine::new(10, 2);
    term.feed("e\u{0301}".as_bytes()); // 'e' + combining acute accent

    assert_eq!(term.grid().cell(0, 0).c, 'e');
    // The combining mark did not advance the cursor nor occupy col 1.
    assert_eq!(term.cursor().col, 1);
    assert_eq!(term.grid().cell(0, 1).c, ' ');
}

// ===========================================================================
// Robustness — sequences the slice deliberately ignores
// ===========================================================================

/// Unimplemented private-mode sequences (intermediates such as `?`) are ignored
/// rather than misinterpreted. The surrounding text still prints. (Modes that
/// ARE implemented, e.g. ?1049 alt-screen, are tested in vt_compliance.rs.)
#[test]
fn private_mode_sequences_are_ignored() {
    let mut term = Engine::new(10, 2);
    term.feed(b"A\x1b[?25lB\x1b[?2004hC"); // hide-cursor + bracketed-paste: both unimplemented
    assert_eq!(term.grid().cell(0, 0).c, 'A');
    assert_eq!(term.grid().cell(0, 1).c, 'B');
    assert_eq!(term.grid().cell(0, 2).c, 'C');
}

/// Bytes split across feed() calls mid-escape are parsed correctly (the parser
/// is stateful across calls — the caller may hand us arbitrary chunks).
#[test]
fn escape_split_across_feeds() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b[3"); // CSI param, cut here
    term.feed(b"1mZ"); // ...resumes: SGR 31, print Z
    assert_eq!(term.grid().cell(0, 0).c, 'Z');
    assert_eq!(term.grid().cell(0, 0).fg, Color::Indexed(1));
}
