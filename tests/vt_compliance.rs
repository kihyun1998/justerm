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

// ===========================================================================
// Scroll region (DECSTBM)
// ===========================================================================

/// A line-feed at the bottom margin scrolls only the region rows; content
/// outside the region stays fixed.
#[test]
fn linefeed_scrolls_only_within_region() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[4;1HZ"); // 'Z' at grid row 3 — below the region
    term.feed(b"\x1b[1;2r"); // DECSTBM: region = rows 1..2 (grid 0..=1)
    term.feed(b"\x1b[2;1HB"); // cursor to grid row 1 (bottom margin), write 'B'
    term.feed(b"\r\n"); // CR + LF at the bottom margin → scroll the region

    assert_eq!(term.grid().cell(0, 0).c, 'B'); // region scrolled: 'B' moved up
    assert_eq!(term.grid().cell(1, 0).c, ' '); // new blank line inside the region
    assert_eq!(term.grid().cell(3, 0).c, 'Z'); // outside the region: untouched
}

/// DECSTBM homes the cursor to the absolute top-left (origin-relative homing
/// under DECOM is a later slice).
#[test]
fn decstbm_homes_cursor() {
    let mut term = Engine::new(10, 5);
    term.feed(b"\x1b[3;5H"); // move cursor to grid (2, 4)
    term.feed(b"\x1b[2;4r"); // DECSTBM → should home the cursor

    assert_eq!((term.cursor().row, term.cursor().col), (0, 0));
}

/// An invalid region (top ≥ bottom) is ignored entirely — the margins stay at
/// the full screen, so a bottom-row line-feed still scrolls everything.
#[test]
fn invalid_scroll_region_is_ignored() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[3;2r"); // top=3 >= bottom=2 → invalid, must be ignored
    term.feed(b"\x1b[4;1Hd"); // 'd' on the last row
    term.feed(b"\r\n"); // LF at screen bottom → full-screen scroll

    // 'd' moved up a row: the region is still the whole screen.
    assert_eq!(term.grid().cell(2, 0).c, 'd');
}

// ===========================================================================
// Index / Reverse Index (IND / RI)
// ===========================================================================

/// RI (ESC M) at the top margin scrolls the region down: a blank line appears
/// at the top and existing rows are pushed down.
#[test]
fn reverse_index_at_top_scrolls_region_down() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[1;1Ha"); // 'a' at row 0
    term.feed(b"\x1b[2;1Hb"); // 'b' at row 1
    term.feed(b"\x1b[1;1H"); // cursor back to the top margin
    term.feed(b"\x1bM"); // RI at the top margin → scroll region down

    assert_eq!(term.grid().cell(0, 0).c, ' '); // blank inserted at the top
    assert_eq!(term.grid().cell(1, 0).c, 'a'); // 'a' pushed down
    assert_eq!(term.grid().cell(2, 0).c, 'b'); // 'b' pushed down
}

/// RI below the top margin just moves the cursor up — no scroll.
#[test]
fn reverse_index_below_top_moves_up() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[3;1Hx"); // cursor to grid row 2, write 'x'
    term.feed(b"\x1bM"); // RI: not at the top margin → move up

    assert_eq!(term.cursor().row, 1);
    assert_eq!(term.grid().cell(2, 0).c, 'x'); // content intact, no scroll
}

/// IND (ESC D) at the bottom margin scrolls the region up — a line-feed without
/// the carriage return.
#[test]
fn index_at_bottom_scrolls_region_up() {
    let mut term = Engine::new(4, 2);
    term.feed(b"\x1b[1;1Ha\x1b[2;1Hb"); // row 0 = 'a', row 1 = 'b'
    term.feed(b"\x1b[2;1H"); // cursor to the bottom margin
    term.feed(b"\x1bD"); // IND → scroll region up

    assert_eq!(term.grid().cell(0, 0).c, 'b'); // 'b' scrolled up
    assert_eq!(term.grid().cell(1, 0).c, ' '); // blank at the bottom
}

// ===========================================================================
// Alt-screen (DEC 1049)
// ===========================================================================

/// Entering the alt screen (?1049h) shows a fresh blank grid; leaving (?1049l)
/// brings the primary screen's content back.
#[test]
fn alt_screen_switches_and_restores() {
    let mut term = Engine::new(10, 3);
    term.feed(b"PRIMARY"); // content on the primary screen

    term.feed(b"\x1b[?1049h"); // enter alt → fresh, cleared screen
    assert_eq!(term.grid().cell(0, 0).c, ' ');
    term.feed(b"\x1b[1;1HALT"); // write on the alt screen
    assert_eq!(term.grid().cell(0, 0).c, 'A');

    term.feed(b"\x1b[?1049l"); // leave → primary content is back
    assert_eq!(term.grid().cell(0, 0).c, 'P');
}

/// Entering the alt screen saves the cursor; leaving restores it (the alt
/// screen's own cursor movement does not leak back to the primary).
#[test]
fn alt_screen_saves_and_restores_cursor() {
    let mut term = Engine::new(10, 3);
    term.feed(b"\x1b[2;5H"); // cursor to grid (1, 4) on the primary
    term.feed(b"\x1b[?1049h"); // enter alt → save cursor
    term.feed(b"\x1b[1;1H"); // move cursor on the alt screen
    term.feed(b"\x1b[?1049l"); // leave → restore the saved cursor

    assert_eq!((term.cursor().row, term.cursor().col), (1, 4));
}

/// Entering the alt screen resets the scroll position — the alt screen has no
/// scrollback, so a viewport scrolled up in the primary must not show primary
/// history on the alt screen.
#[test]
fn entering_alt_resets_scroll_position() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // history = [a], screen = b, c
    term.scroll_up(1); // scroll up into primary history

    term.feed(b"\x1b[?1049h"); // enter alt → view must snap to the (blank) alt screen

    assert_eq!(term.viewport_line(0)[0].c, ' '); // not primary's 'a'
}

/// Scroll intents are no-ops on the alt screen — there is no history to view.
#[test]
fn scroll_is_a_noop_on_alt_screen() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // primary history = [a]
    term.feed(b"\x1b[?1049h"); // enter alt

    term.scroll_up(5); // must not window into the primary's scrollback

    assert_eq!(term.viewport_line(0)[0].c, ' ');
}

/// A redundant ?1049h while already on the alt screen is a no-op — it must not
/// swap the primary screen in and clear it.
#[test]
fn double_enter_alt_is_idempotent() {
    let mut term = Engine::new(10, 2);
    term.feed(b"P"); // primary content
    term.feed(b"\x1b[?1049h"); // enter alt
    term.feed(b"\x1b[1;1HX"); // write on the alt screen
    term.feed(b"\x1b[?1049h"); // enter AGAIN — must be a no-op
    term.feed(b"\x1b[?1049l"); // a single leave → back to primary

    assert_eq!(term.grid().cell(0, 0).c, 'P'); // primary survived
}

// ===========================================================================
// Origin mode (DECOM ?6)
// ===========================================================================

/// With origin mode on, CUP row 1 is relative to the scroll region's top
/// margin, not the absolute top of the screen.
#[test]
fn origin_mode_makes_cup_region_relative() {
    let mut term = Engine::new(10, 6);
    term.feed(b"\x1b[3;5r"); // scroll region rows 3..5 → grid rows 2..=4
    term.feed(b"\x1b[?6h"); // DECOM on
    term.feed(b"\x1b[1;1HX"); // CUP to region row 1 → grid row 2

    assert_eq!(term.grid().cell(2, 0).c, 'X');
}

/// With origin mode on, a CUP past the bottom margin clamps to the region's
/// bottom, not the screen's bottom.
#[test]
fn origin_mode_clamps_to_region_bottom() {
    let mut term = Engine::new(10, 6);
    term.feed(b"\x1b[3;5r"); // region grid rows 2..=4
    term.feed(b"\x1b[?6h"); // DECOM on
    term.feed(b"\x1b[99;1HY"); // CUP far past the region → clamp to grid row 4

    assert_eq!(term.grid().cell(4, 0).c, 'Y');
}

/// Setting DECOM homes the cursor to the region top; unsetting it leaves the
/// cursor where it is (the xterm/alacritty asymmetry we follow).
#[test]
fn decom_set_homes_to_region_unset_does_not_move() {
    let mut term = Engine::new(10, 6);
    term.feed(b"\x1b[3;5r"); // region grid rows 2..=4

    term.feed(b"\x1b[?6h"); // DECOM set → home to region top
    assert_eq!((term.cursor().row, term.cursor().col), (2, 0));

    term.feed(b"\x1b[2;3H"); // move within the region → grid (3, 2)
    term.feed(b"\x1b[?6l"); // DECOM unset → cursor must NOT move
    assert_eq!((term.cursor().row, term.cursor().col), (3, 2));
}

// ===========================================================================
// Scrollback (#3)
// ===========================================================================

/// A line scrolled off the top of the primary screen enters scrollback history.
#[test]
fn scroll_accrues_history() {
    let mut term = Engine::new(4, 2); // 2 visible rows
    assert_eq!(term.scrollback_len(), 0);

    term.feed(b"a\r\nb\r\nc"); // 'a' is pushed off when the 3rd line starts
    assert_eq!(term.scrollback_len(), 1);
}

/// At the bottom (no scroll), the viewport is the live screen.
#[test]
fn viewport_shows_live_screen_at_bottom() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // history=['a'], screen rows 'b','c'

    assert_eq!(term.viewport_line(0)[0].c, 'b');
    assert_eq!(term.viewport_line(1)[0].c, 'c');
}

/// Scrolling up windows the viewport into history (acceptance: feed lines,
/// scroll up → older lines show).
#[test]
fn scroll_up_reveals_history() {
    let mut term = Engine::new(4, 4);
    // 6 lines into a 4-row screen → 2 lines (a, b) in history.
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf");
    assert_eq!(term.scrollback_len(), 2);

    term.scroll_up(2); // reveal the two oldest lines at the top
    assert_eq!(term.viewport_line(0)[0].c, 'a');
    assert_eq!(term.viewport_line(1)[0].c, 'b');
    assert_eq!(term.viewport_line(2)[0].c, 'c');
}

/// New output while scrolled up keeps the view stable (follow-bottom = stay):
/// the offset is bumped so the same lines stay visible, not yanked to bottom.
#[test]
fn new_output_while_scrolled_stays_put() {
    let mut term = Engine::new(4, 4);
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf"); // history=[a,b]
    term.scroll_up(2);
    assert_eq!(term.viewport_line(0)[0].c, 'a'); // viewing the top of history

    term.feed(b"\r\ng"); // new line scrolls history; view must stay on 'a'
    assert_eq!(term.viewport_line(0)[0].c, 'a');
}

/// scroll_down walks back toward the live screen; scroll_to_bottom jumps there.
#[test]
fn scroll_down_and_to_bottom() {
    let mut term = Engine::new(4, 4);
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf"); // history=[a,b], screen c,d,e,f
    term.scroll_up(2);
    assert_eq!(term.viewport_line(0)[0].c, 'a');

    term.scroll_down(1);
    assert_eq!(term.viewport_line(0)[0].c, 'b'); // one line back toward bottom

    term.scroll_to_bottom();
    assert_eq!(term.viewport_line(0)[0].c, 'c'); // live screen top
}

/// Scrollback is capped: the oldest lines are evicted once the limit is hit.
#[test]
fn scrollback_caps_oldest_evicted() {
    let mut term = Engine::with_scrollback(4, 2, 2); // keep at most 2 history lines
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne"); // a,b,c scroll off, but cap = 2

    assert_eq!(term.scrollback_len(), 2);
    term.scroll_up(2);
    assert_eq!(term.viewport_line(0)[0].c, 'b'); // 'a' was evicted; 'b' is oldest
}

/// Scrolled to the very top with the cap full, new output must not push the
/// display offset past history (a usize underflow) — when the oldest visible
/// line is evicted the view advances by one, matching xterm.js trimming both
/// ybase and ydisp.
#[test]
fn new_output_at_cap_while_scrolled_to_top() {
    let mut term = Engine::with_scrollback(4, 2, 2);
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne"); // history capped to [b, c]
    term.scroll_up(99); // clamp to the very top
    assert_eq!(term.viewport_line(0)[0].c, 'b');

    term.feed(b"\r\nf"); // evicts 'b' at the cap; must not panic
    assert_eq!(term.viewport_line(0)[0].c, 'c');
}

/// The alt screen has no scrollback — scrolling it accrues no history.
#[test]
fn alt_screen_produces_no_scrollback() {
    let mut term = Engine::new(4, 2);
    term.feed(b"\x1b[?1049h"); // enter alt screen
    term.feed(b"a\r\nb\r\nc\r\nd"); // scroll several lines on the alt screen

    assert_eq!(term.scrollback_len(), 0);
}

/// A scroll region that is NOT top-anchored (scroll_top != 0) accrues no
/// history — the rule is `scroll_top == 0`, not "the full screen".
#[test]
fn non_top_anchored_region_produces_no_scrollback() {
    let mut term = Engine::new(4, 3);
    term.feed(b"\x1b[2;3r"); // region rows 2..3 → scroll_top = 1
    term.feed(b"a\r\nb\r\nc\r\nd"); // a scroll happens inside the region

    assert_eq!(term.scrollback_len(), 0);
}

/// Auto-wrap at the bottom margin scrolls only the scroll region; content below
/// the region stays fixed (print path goes through the region-aware line-feed).
#[test]
fn autowrap_at_bottom_margin_scrolls_only_the_region() {
    let mut term = Engine::new(3, 3);
    term.feed(b"\x1b[1;2r"); // region = rows 1..2 (grid 0..=1); homes cursor
    term.feed(b"\x1b[3;1HZ"); // 'Z' below the region at grid row 2

    term.feed(b"\x1b[1;1Habcdefg"); // abc|def fill the region, 'g' wraps → region scrolls

    assert_eq!(term.grid().cell(0, 0).c, 'd'); // region scrolled up
    assert_eq!(term.grid().cell(1, 0).c, 'g'); // new content on the bottom margin
    assert_eq!(term.grid().cell(2, 0).c, 'Z'); // below the region: untouched
}

// ===========================================================================
// Cursor visibility (DEC ?25)
// ===========================================================================

/// ?25l hides the cursor, ?25h shows it; the cursor is visible by default. The
/// engine only reports visibility (blink is a renderer concern).
#[test]
fn cursor_visibility_toggles() {
    let mut term = Engine::new(10, 2);
    assert!(term.cursor().visible); // visible by default

    term.feed(b"\x1b[?25l"); // hide
    assert!(!term.cursor().visible);

    term.feed(b"\x1b[?25h"); // show
    assert!(term.cursor().visible);
}

// ===========================================================================
// Bracketed paste mode (DEC ?2004)
// ===========================================================================

/// ?2004h enables bracketed-paste mode, ?2004l disables it; off by default.
/// The engine owns the flag — wrapping pasted input in markers is the input
/// encoder's job (#11).
#[test]
fn bracketed_paste_mode_toggles() {
    let mut term = Engine::new(10, 2);
    assert!(!term.bracketed_paste()); // off by default

    term.feed(b"\x1b[?2004h"); // enable
    assert!(term.bracketed_paste());

    term.feed(b"\x1b[?2004l"); // disable
    assert!(!term.bracketed_paste());
}

// ===========================================================================
// NEL — Next Line (ESC E)
// ===========================================================================

/// NEL moves the cursor to the first column of the next line (a CR + LF).
#[test]
fn nel_moves_to_start_of_next_line() {
    let mut term = Engine::new(10, 3);
    term.feed(b"ab"); // cursor at (0, 2)
    term.feed(b"\x1bE"); // NEL → (1, 0)
    term.feed(b"c");

    assert_eq!(term.grid().cell(1, 0).c, 'c');
    assert_eq!((term.cursor().row, term.cursor().col), (1, 1));
}
