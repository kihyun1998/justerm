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
