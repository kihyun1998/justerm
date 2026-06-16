//! Issue #8 — VT compliance (common 90%). Grown test-first, one behaviour per
//! cycle. (The vttest-style conformance harness is a later slice of #8.)

use justerm::{Color, Engine};

// ===========================================================================
// Background Color Erase (BCE)
// ===========================================================================

/// Erasing fills cleared cells with the current SGR background, not the
/// default — xterm/alacritty BCE semantics.
#[test]
fn erase_line_uses_current_background() {
    let mut term = Engine::new(5, 1);
    term.feed(b"abc"); // some content to clear
    term.feed(b"\x1b[41m"); // set background = red (index 1)
    term.feed(b"\x1b[2K"); // erase whole line

    assert_eq!(term.grid().cell(0, 0).bg, Color::Indexed(1));
    assert_eq!(term.grid().cell(0, 0).c, ' '); // still blank
}

/// BCE carries the background only — the pen's foreground and text attributes
/// are NOT applied to erased cells (they reset to default).
#[test]
fn erase_carries_background_only_not_fg_or_attrs() {
    let mut term = Engine::new(5, 1);
    term.feed(b"abcde");
    term.feed(b"\x1b[41;32;1m"); // bg red, fg green, bold
    term.feed(b"\x1b[2K");

    let cell = *term.grid().cell(0, 0);
    assert_eq!(cell.bg, Color::Indexed(1)); // background carried
    assert_eq!(cell.fg, Color::Default); // foreground NOT carried
    assert!(cell.flags.is_empty()); // attributes NOT carried
}

// ===========================================================================
// Tab stops (HTS / TBC / HT)
// ===========================================================================

/// HTS (ESC H) sets a tab stop at the cursor; HT then advances to it instead
/// of the default 8-column stop.
#[test]
fn custom_tab_stop_is_honored() {
    let mut term = Engine::new(20, 1);
    term.feed(b"\x1b[1;4H"); // cursor to column 4 → grid col 3
    term.feed(b"\x1bH"); // HTS: set a tab stop at col 3
    term.feed(b"\r"); // carriage return to col 0
    term.feed(b"\t"); // HT → next set stop

    assert_eq!(term.cursor().col, 3);
}

/// TBC param 0 clears the tab stop at the cursor; HT then skips it and lands on
/// the next default stop.
#[test]
fn tbc_clears_stop_at_cursor() {
    let mut term = Engine::new(20, 1);
    term.feed(b"\x1b[1;9H"); // cursor to column 9 → grid col 8 (a default stop)
    term.feed(b"\x1b[0g"); // TBC: clear the stop at the cursor
    term.feed(b"\r\t"); // back to col 0, then HT

    assert_eq!(term.cursor().col, 16); // col 8 skipped → next default stop
}

/// TBC param 3 clears all tab stops; HT then advances to the last column
/// (no stop remains, no wrap).
#[test]
fn tbc_clears_all_stops() {
    let mut term = Engine::new(20, 1);
    term.feed(b"\x1b[3g"); // TBC: clear every stop
    term.feed(b"\t"); // HT with no stops → last column

    assert_eq!(term.cursor().col, 19);
}
