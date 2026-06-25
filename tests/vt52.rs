//! VT52 compatibility mode (DECANM ?2) tests (#84). Resetting DECANM (`CSI ?2l`)
//! puts the terminal into the pre-ANSI VT52 escape dialect; `ESC <` returns to
//! ANSI. Neither xterm.js (marks ?2 `#N`) nor alacritty implement VT52, so the
//! authority here is the xterm `ctlseqs` "VT52 Mode" section + the DEC VT100
//! manual. Tested through the public `Engine` API by observable behavior.

use justerm::Engine;

/// `CSI ?2l` enters VT52 mode, where `ESC A` moves the cursor up — a byte that
/// is inert in ANSI mode. This is the tracer: it proves the mode flag re-routes
/// `esc_dispatch` into the VT52 dialect.
#[test]
fn vt52_cursor_up() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[6;1H"); // ANSI CUP → row 5 (0-based)
    t.feed(b"\x1b[?2l"); // DECANM reset → enter VT52
    t.feed(b"\x1bA"); // VT52 cursor up
    assert_eq!(t.cursor().row, 4);
}

/// `ESC <` leaves VT52 and returns to ANSI: the same `ESC A` that moved the
/// cursor in VT52 becomes inert again.
#[test]
fn vt52_exit_returns_to_ansi() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[6;1H"); // row 5
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bA"); // VT52 cursor up → row 4
    assert_eq!(t.cursor().row, 4);
    t.feed(b"\x1b<"); // exit VT52 → ANSI
    t.feed(b"\x1bA"); // ANSI: inert
    assert_eq!(t.cursor().row, 4, "ESC A must be inert once back in ANSI");
}

/// VT52 `ESC B`/`ESC C`/`ESC D` move the cursor down/right/left by one.
#[test]
fn vt52_cursor_down_right_left() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[6;6H"); // row 5, col 5
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bB"); // down → row 6
    assert_eq!((t.cursor().row, t.cursor().col), (6, 5));
    t.feed(b"\x1bC"); // right → col 6
    assert_eq!((t.cursor().row, t.cursor().col), (6, 6));
    t.feed(b"\x1bD"); // left → col 5
    assert_eq!((t.cursor().row, t.cursor().col), (6, 5));
}

/// VT52 `ESC H` homes the cursor to (0, 0). (In ANSI `ESC H` is HTS — set tab
/// stop — so the mode branch must pick the VT52 meaning.)
#[test]
fn vt52_home() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[6;6H"); // row 5, col 5
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bH"); // home
    assert_eq!((t.cursor().row, t.cursor().col), (0, 0));
}

/// VT52 `ESC I` is a reverse line feed: off the top it just moves the cursor up.
#[test]
fn vt52_reverse_line_feed() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[4;1H"); // row 3
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bI"); // reverse line feed
    assert_eq!(t.cursor().row, 2);
}

/// VT52 `ESC K` erases from the cursor to the end of the line.
#[test]
fn vt52_erase_to_end_of_line() {
    let mut t = Engine::new(80, 24);
    t.feed(b"hello"); // cols 0..5 on row 0; cursor at col 5
    t.feed(b"\x1b[1;3H"); // back to col 2 (the second 'l')
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bK"); // erase to end of line
    assert_eq!(t.grid().cell(0, 1).c(), 'e', "before cursor is untouched");
    assert_eq!(t.grid().cell(0, 2).c(), ' ', "from cursor is cleared");
    assert_eq!(t.grid().cell(0, 4).c(), ' ');
}

/// VT52 `ESC J` erases from the cursor to the end of the screen.
#[test]
fn vt52_erase_to_end_of_screen() {
    let mut t = Engine::new(80, 24);
    t.feed(b"a\r\nb\r\nc"); // 'a'@(0,0), 'b'@(1,0), 'c'@(2,0)
    t.feed(b"\x1b[2;1H"); // row 1, col 0 (the 'b')
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bJ"); // erase to end of screen
    assert_eq!(t.grid().cell(0, 0).c(), 'a', "above cursor untouched");
    assert_eq!(t.grid().cell(1, 0).c(), ' ', "from cursor cleared");
    assert_eq!(t.grid().cell(2, 0).c(), ' ', "below cursor cleared");
}

/// VT52 `ESC Y row col` directly addresses the cursor. Each coordinate is a
/// single byte encoded as `value + 0x20` (space = 0). vte tokenizes `ESC Y` as a
/// final and hands the two coordinate bytes to `print()` afterward, so the engine
/// captures them with a pending-coordinate counter, not as part of the sequence.
#[test]
fn vt52_direct_cursor_address() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l"); // enter VT52
    // row 2 = 0x20 + 2 = 0x22 ('"'), col 4 = 0x20 + 4 = 0x24 ('$').
    t.feed(b"\x1bY\x22\x24");
    assert_eq!((t.cursor().row, t.cursor().col), (2, 4));
    // The coordinate bytes must be consumed, NOT printed as glyphs.
    assert_eq!(
        t.grid().cell(0, 0).c(),
        ' ',
        "coords must not reach the screen"
    );
}

/// `ESC Y` and its coordinates may be split across `feed()` calls — the
/// pending-coordinate state lives on `Term`, so it survives the boundary.
#[test]
fn vt52_direct_address_split_across_feeds() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l");
    t.feed(b"\x1bY"); // final arrives alone
    t.feed(b"\x22"); // row byte in a later feed
    t.feed(b"\x24"); // col byte in yet another feed
    assert_eq!((t.cursor().row, t.cursor().col), (2, 4));
}

/// VT52 `ESC Z` (Identify) replies `ESC / Z` — "I am a VT52" — queued for the
/// consumer to write back to the PTY via `drain_replies`.
#[test]
fn vt52_identify_replies() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bZ"); // identify
    assert_eq!(t.drain_replies(), b"\x1b/Z");
}

/// VT52 `ESC =` / `ESC >` enter/exit alternate keypad mode — reusing the same
/// `application_keypad` flag as DECKPAM/DECNKM (#74). Observed via DECRQM ?66.
#[test]
fn vt52_alternate_keypad() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1b="); // enter alternate keypad
    t.feed(b"\x1b[?66$p"); // DECRQM ?66 (CSI still parses; vte is mode-agnostic)
    assert_eq!(t.drain_replies(), b"\x1b[?66;1$y", "keypad set");
    t.feed(b"\x1b>"); // exit alternate keypad
    t.feed(b"\x1b[?66$p");
    assert_eq!(t.drain_replies(), b"\x1b[?66;2$y", "keypad reset");
}

/// VT52 graphics `ESC F` / `ESC G` are a documented non-goal in the first cut:
/// the VT52 graphics glyph set differs from DEC Special Graphics, so mapping to
/// that charset would render *wrong* glyphs. They are no-ops — a printable byte
/// after `ESC F` stays its literal self, not a line-drawing glyph.
#[test]
fn vt52_graphics_is_a_documented_noop() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bF"); // "enter graphics mode" — no-op
    t.feed(b"q"); // would be '─' under DEC Special Graphics
    assert_eq!(
        t.grid().cell(0, 0).c(),
        'q',
        "graphics mode is not implemented"
    );
}

/// DECRQM ?2 reports DECANM: *set* (1) in the normal ANSI state, *reset* (2)
/// once in VT52 mode. (DECANM set = ANSI, so the report is `!vt52_mode`.)
#[test]
fn vt52_decrqm_reports_decanm() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2$p"); // default: ANSI
    assert_eq!(t.drain_replies(), b"\x1b[?2;1$y", "ANSI → set");
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1b[?2$p");
    assert_eq!(t.drain_replies(), b"\x1b[?2;2$y", "VT52 → reset");
}

/// RIS (`ESC c`) returns to ANSI mode: after a hard reset, `ESC A` is inert
/// again and DECRQM ?2 reports set. (Free — `full_reset` rebuilds `Term`.)
#[test]
fn vt52_ris_returns_to_ansi() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l"); // enter VT52
    t.feed(b"\x1bc"); // RIS
    t.feed(b"\x1b[?2$p");
    assert_eq!(t.drain_replies(), b"\x1b[?2;1$y", "RIS → ANSI");
    t.feed(b"\x1b[6;1H"); // row 5
    t.feed(b"\x1bA"); // ANSI: inert
    assert_eq!(t.cursor().row, 5, "ESC A inert after RIS");
}

/// `ESC Y` coordinates are clamped to the screen. The coordinate byte range that
/// reaches `print` is `0x20..=0x7E` (C0 controls go to `execute`, so VT52 coords
/// are always printable); `~` (0x7E) decodes to 94, far past an 80×24 screen, and
/// must clamp to the last row/col. `space` (0x20) decodes to 0 with no underflow.
#[test]
fn vt52_direct_address_is_clamped() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2l");
    t.feed(b"\x1bY~~"); // (94, 94) → clamp to (23, 79)
    assert_eq!((t.cursor().row, t.cursor().col), (23, 79));
    t.feed(b"\x1bY\x20\x20"); // (0, 0) → home, no underflow
    assert_eq!((t.cursor().row, t.cursor().col), (0, 0));
}
