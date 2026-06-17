//! Issue #8 — VT compliance (common 90%). Grown test-first, one behaviour per
//! cycle. (The vttest-style conformance harness is a later slice of #8.)

use justerm::{Color, Engine, SelectionType, Side};

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

// ===========================================================================
// Intra-line editing — ICH (@) / DCH (P) / ECH (X)  [#16]
// ===========================================================================

/// Row `r` rendered as a string (trailing cells shown as their blanks).
fn row(term: &Engine, r: usize) -> String {
    let g = term.grid();
    (0..g.cols()).map(|c| g.cell(r, c).c).collect()
}

/// ECH (CSI Pn X) erases Pn cells in place at the cursor — no shift.
#[test]
fn ech_erases_in_place() {
    let mut term = Engine::new(6, 1);
    term.feed(b"abcdef");
    term.feed(b"\x1b[3G"); // cursor → col index 2
    term.feed(b"\x1b[2X"); // erase 2 cells

    assert_eq!(row(&term, 0), "ab  ef");
}

/// ICH (CSI Pn @) inserts Pn blanks at the cursor, shifting the rest right; cells
/// pushed past the right edge are lost.
#[test]
fn ich_inserts_blanks_shifting_right() {
    let mut term = Engine::new(6, 1);
    term.feed(b"abcdef");
    term.feed(b"\x1b[3G"); // cursor → col index 2
    term.feed(b"\x1b[2@"); // insert 2 blanks

    assert_eq!(row(&term, 0), "ab  cd"); // e,f shifted off the edge
}

/// ICH that splits a wide glyph destroys it — both halves become blank, leaving
/// no orphaned lead or spacer (the repo's no-orphan invariant; Alacritty leaves
/// the orphan). "a한bc": inserting one blank at the spacer column separates the
/// lead from its spacer, so both clear.
#[test]
fn ich_splitting_a_wide_glyph_clears_both_halves() {
    let mut term = Engine::new(6, 1);
    term.feed("a한bc".as_bytes()); // a(0) 한=lead(1)+spacer(2) b(3) c(4)
    term.feed(b"\x1b[3G"); // cursor → col index 2 (the spacer)
    term.feed(b"\x1b[1@"); // insert 1 blank, splitting 한

    assert_eq!(row(&term, 0), "a   bc"); // 한 gone, no orphan
}

/// ICH that pushes a wide glyph's spacer off the right edge orphans the lead at
/// the last column — it must be cleared. "ab한": inserting one blank at col 0
/// shifts 한's lead to the last column while its spacer falls off, so the lead
/// clears.
#[test]
fn ich_pushing_wide_spacer_off_edge_clears_lead() {
    let mut term = Engine::new(4, 1);
    term.feed("ab한".as_bytes()); // a(0) b(1) 한=lead(2)+spacer(3)
    term.feed(b"\x1b[1G"); // cursor → col index 0
    term.feed(b"\x1b[1@"); // insert 1 blank

    assert_eq!(row(&term, 0), " ab "); // 한 destroyed, no orphan lead
}

/// DCH (CSI Pn P) deletes Pn cells at the cursor, shifting the tail left; the
/// vacated cells at the right are BCE-blanked.
#[test]
fn dch_deletes_shifting_left() {
    let mut term = Engine::new(6, 1);
    term.feed(b"abcdef");
    term.feed(b"\x1b[3G"); // cursor → col index 2
    term.feed(b"\x1b[2P"); // delete 2 cells

    assert_eq!(row(&term, 0), "abef  "); // cd deleted, ef pulled left
}

/// DCH that deletes one half of a wide glyph clears the other — no orphan.
/// "a한bc": deleting at the spacer column pulls the tail over the spacer, so the
/// lead at the previous column is orphaned and clears.
#[test]
fn dch_deleting_wide_half_clears_the_other() {
    let mut term = Engine::new(6, 1);
    term.feed("a한bc".as_bytes()); // a(0) 한=lead(1)+spacer(2) b(3) c(4)
    term.feed(b"\x1b[3G"); // cursor → col index 2 (the spacer)
    term.feed(b"\x1b[1P"); // delete it

    assert_eq!(row(&term, 0), "a bc  "); // 한 lead orphaned → cleared
}

/// Symmetric DCH case: deleting the lead orphans the spacer pulled to the cursor.
#[test]
fn dch_deleting_wide_lead_clears_the_spacer() {
    let mut term = Engine::new(6, 1);
    term.feed("a한bc".as_bytes()); // a(0) 한=lead(1)+spacer(2) b(3) c(4)
    term.feed(b"\x1b[2G"); // cursor → col index 1 (the lead)
    term.feed(b"\x1b[1P"); // delete it

    assert_eq!(row(&term, 0), "a bc  "); // spacer pulled to cursor → cleared
}

/// ICH fills the opened gap with the current SGR background (BCE).
#[test]
fn ich_fills_gap_with_bce_background() {
    let mut term = Engine::new(6, 1);
    term.feed(b"abcdef");
    term.feed(b"\x1b[3G"); // cursor → col index 2
    term.feed(b"\x1b[41m"); // bg red
    term.feed(b"\x1b[2@"); // insert 2

    assert_eq!(term.grid().cell(0, 2).bg, Color::Indexed(1));
    assert_eq!(term.grid().cell(0, 2).c, ' ');
}

/// DCH fills the vacated tail with the current SGR background (BCE).
#[test]
fn dch_fills_tail_with_bce_background() {
    let mut term = Engine::new(6, 1);
    term.feed(b"abcdef");
    term.feed(b"\x1b[3G"); // cursor → col index 2
    term.feed(b"\x1b[41m"); // bg red
    term.feed(b"\x1b[2P"); // delete 2 → tail cols 4,5 BCE-blanked

    assert_eq!(term.grid().cell(0, 5).bg, Color::Indexed(1));
    assert_eq!(term.grid().cell(0, 5).c, ' ');
}

/// Intra-line edits do not clear pending-wrap (xterm/alacritty: these ops leave
/// `wrapnext` untouched). After filling the last column, an ECH then a print
/// still wraps to the next row.
#[test]
fn editing_preserves_pending_wrap() {
    let mut term = Engine::new(3, 2);
    term.feed(b"abc"); // fills row 0, cursor parks at col 2 with pending-wrap
    term.feed(b"\x1b[1X"); // ECH at the last column — must not clear pending-wrap
    term.feed(b"d"); // should still wrap to row 1

    assert_eq!(term.grid().cell(1, 0).c, 'd');
}

// ===========================================================================
// Line/region editing — IL (L) / DL (M) / SU (S) / SD (T)  [#17]
// ===========================================================================

/// SU (CSI Pn S) scrolls the whole region up by Pn; exposed bottom lines blank.
#[test]
fn su_scrolls_region_up() {
    let mut term = Engine::new(2, 4);
    term.feed(b"A\r\nB\r\nC\r\nD"); // rows A,B,C,D

    term.feed(b"\x1b[2S"); // scroll up 2

    assert_eq!(row(&term, 0), "C ");
    assert_eq!(row(&term, 1), "D ");
    assert_eq!(row(&term, 2), "  ");
    assert_eq!(row(&term, 3), "  ");
}

/// SD (CSI Pn T) scrolls the whole region down by Pn; exposed top lines blank.
#[test]
fn sd_scrolls_region_down() {
    let mut term = Engine::new(2, 4);
    term.feed(b"A\r\nB\r\nC\r\nD");

    term.feed(b"\x1b[2T"); // scroll down 2

    assert_eq!(row(&term, 0), "  ");
    assert_eq!(row(&term, 1), "  ");
    assert_eq!(row(&term, 2), "A ");
    assert_eq!(row(&term, 3), "B ");
}

/// IL (CSI Pn L) inserts blank lines at the cursor, scrolling [cursor..bottom]
/// down; the bottom of that range falls off.
#[test]
fn il_inserts_lines_at_cursor() {
    let mut term = Engine::new(2, 4);
    term.feed(b"A\r\nB\r\nC\r\nD");
    term.feed(b"\x1b[2;1H"); // cursor → row 1
    term.feed(b"\x1b[1L"); // insert 1 line

    assert_eq!(row(&term, 0), "A "); // above the cursor: fixed
    assert_eq!(row(&term, 1), "  "); // new blank line
    assert_eq!(row(&term, 2), "B "); // B,C pushed down; D fell off
    assert_eq!(row(&term, 3), "C ");
}

/// DL (CSI Pn M) deletes lines at the cursor, scrolling [cursor..bottom] up; the
/// bottom of that range is blanked.
#[test]
fn dl_deletes_lines_at_cursor() {
    let mut term = Engine::new(2, 4);
    term.feed(b"A\r\nB\r\nC\r\nD");
    term.feed(b"\x1b[2;1H"); // cursor → row 1
    term.feed(b"\x1b[1M"); // delete 1 line (B)

    assert_eq!(row(&term, 0), "A "); // above: fixed
    assert_eq!(row(&term, 1), "C "); // C,D pulled up
    assert_eq!(row(&term, 2), "D ");
    assert_eq!(row(&term, 3), "  "); // bottom blanked
}

/// IL/DL are a no-op when the cursor is outside the scroll region.
#[test]
fn il_outside_region_is_a_noop() {
    let mut term = Engine::new(2, 4);
    term.feed(b"A\r\nB\r\nC\r\nD");
    term.feed(b"\x1b[2;3r"); // region rows 1..=2; homes cursor to (0,0) — outside
    term.feed(b"\x1b[1L"); // IL at row 0, outside the region → no-op

    assert_eq!(row(&term, 0), "A ");
    assert_eq!(row(&term, 1), "B ");
    assert_eq!(row(&term, 2), "C ");
    assert_eq!(row(&term, 3), "D ");
}

/// SU scrolls only within the scroll region; rows outside it stay fixed.
#[test]
fn su_respects_scroll_region() {
    let mut term = Engine::new(2, 4);
    term.feed(b"A\r\nB\r\nC\r\nD");
    term.feed(b"\x1b[2;3r"); // region rows 1..=2 (B,C)
    term.feed(b"\x1b[1S"); // scroll the region up 1

    assert_eq!(row(&term, 0), "A "); // outside region: fixed
    assert_eq!(row(&term, 1), "C "); // B fell off, C pulled up
    assert_eq!(row(&term, 2), "  "); // exposed blank inside region
    assert_eq!(row(&term, 3), "D "); // outside region: fixed
}

/// The lines a region scroll exposes are BCE-filled (current SGR background).
#[test]
fn region_scroll_exposed_lines_use_bce() {
    let mut term = Engine::new(2, 2);
    term.feed(b"A\r\nB");
    term.feed(b"\x1b[41m"); // bg red
    term.feed(b"\x1b[1S"); // scroll up 1 → row 1 exposed

    assert_eq!(term.grid().cell(1, 0).bg, Color::Indexed(1));
    assert_eq!(term.grid().cell(1, 0).c, ' ');
}

// ===========================================================================
// Cursor save/restore — DECSC/DECRC (ESC 7 / ESC 8) + CSI s/u  [#18]
// ===========================================================================

/// DECSC (ESC 7) saves the cursor position; DECRC (ESC 8) restores it.
#[test]
fn decsc_decrc_restores_position() {
    let mut term = Engine::new(10, 5);
    term.feed(b"\x1b[3;4H"); // cursor → row 2, col 3
    term.feed(b"\x1b7"); // DECSC save
    term.feed(b"\x1b[1;1H"); // move to home
    term.feed(b"\x1b8"); // DECRC restore

    assert_eq!((term.cursor().row, term.cursor().col), (2, 3));
}

/// DECSC/DECRC restores the pen (SGR) — a glyph after restore uses the saved fg.
#[test]
fn decsc_decrc_restores_pen() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b[31m"); // fg red
    term.feed(b"\x1b7"); // save (cursor at 0,0, pen red)
    term.feed(b"\x1b[0m"); // reset pen
    term.feed(b"\x1b8"); // restore → pen red again
    term.feed(b"X");

    assert_eq!(term.grid().cell(0, 0).fg, Color::Indexed(1));
}

/// DECRC restores origin mode (ADR-0004: the DEC spec mandates it, Alacritty
/// omits it). With a region set and origin toggled off between save and restore,
/// DECRC brings origin back on — so a region-relative CUP lands in the region.
#[test]
fn decrc_restores_origin_mode() {
    let mut term = Engine::new(10, 5);
    term.feed(b"\x1b[2;4r"); // scroll region rows 1..=3 (0-based)
    term.feed(b"\x1b[?6h"); // origin mode ON
    term.feed(b"\x1b7"); // DECSC (origin = on)
    term.feed(b"\x1b[?6l"); // origin mode OFF
    term.feed(b"\x1b8"); // DECRC → origin restored ON
    term.feed(b"\x1b[1;1HX"); // origin-relative CUP → region top (screen row 1)

    assert_eq!(term.grid().cell(1, 0).c, 'X'); // would be row 0 if origin were off
    assert_eq!(term.grid().cell(0, 0).c, ' ');
}

/// DECRC does NOT restore cursor visibility (DECTCEM is separate from DECSC).
#[test]
fn decrc_does_not_restore_visibility() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b7"); // save (cursor visible by default)
    term.feed(b"\x1b[?25l"); // hide cursor
    term.feed(b"\x1b8"); // restore — visibility must stay as-is

    assert!(!term.cursor().visible);
}

/// CSI s / CSI u are SCOSC/SCORC aliases of DECSC/DECRC.
#[test]
fn csi_s_u_alias_save_restore() {
    let mut term = Engine::new(10, 5);
    term.feed(b"\x1b[3;4H"); // row 2, col 3
    term.feed(b"\x1b[s"); // save
    term.feed(b"\x1b[1;1H"); // home
    term.feed(b"\x1b[u"); // restore

    assert_eq!((term.cursor().row, term.cursor().col), (2, 3));
}

/// DECSC/DECRC round-trips pending-wrap: saved at a filled last column, a print
/// after restore still wraps (the saved `wrapnext` came back).
#[test]
fn decsc_decrc_restores_pending_wrap() {
    let mut term = Engine::new(3, 2);
    term.feed(b"abc"); // fills row 0; cursor parks at col 2 with pending-wrap
    term.feed(b"\x1b7"); // save (pending-wrap = true)
    term.feed(b"\x1b[1;1H"); // home clears pending-wrap
    term.feed(b"\x1b8"); // restore → pending-wrap back
    term.feed(b"d"); // should wrap to row 1

    assert_eq!(term.grid().cell(1, 0).c, 'd');
}

// ===========================================================================
// Grapheme clusters — combining marks attach to the previous base cell  [#19]
// ===========================================================================

/// Select the single cell `(row, col)` and return its copied text (base glyph
/// plus any attached combining marks).
fn cell_text(term: &mut Engine, row: usize, col: usize) -> String {
    term.selection_begin(row, col, Side::Left, SelectionType::Char);
    term.selection_extend(row, col, Side::Right);
    term.selection_text().unwrap_or_default()
}

/// A combining mark (width 0) is not dropped — it attaches to the preceding base
/// glyph, so the cell still holds one base char and copies as the full cluster.
#[test]
fn combining_mark_attaches_to_base() {
    let mut term = Engine::new(5, 1);
    term.feed("e\u{0301}".as_bytes()); // 'e' + combining acute → é

    assert_eq!(term.grid().cell(0, 0).c, 'e'); // base stays a single char
    assert_eq!(cell_text(&mut term, 0, 0), "e\u{0301}"); // cluster copied whole
}

/// A combining mark after a wide glyph attaches to the lead, not the spacer
/// (the attach point backs up over the WIDE_CHAR_SPACER).
#[test]
fn combining_mark_after_wide_glyph_attaches_to_lead() {
    let mut term = Engine::new(5, 1);
    term.feed("한\u{0301}".as_bytes()); // 한 = lead(0)+spacer(1), then combining

    assert_eq!(cell_text(&mut term, 0, 0), "한\u{0301}");
}

/// A combining mark at pending-wrap attaches to the last-column glyph in place —
/// it must not back up, and must not fire the deferred wrap.
#[test]
fn combining_mark_at_pending_wrap_attaches_in_place() {
    let mut term = Engine::new(2, 2);
    term.feed(b"ab"); // fills row 0; cursor parks at col 1 with pending-wrap
    term.feed("\u{0301}".as_bytes()); // combining → attaches to 'b', no wrap

    assert_eq!(cell_text(&mut term, 0, 1), "b\u{0301}");
    assert_eq!(term.grid().cell(1, 0).c, ' '); // no wrap fired
    term.feed(b"c"); // pending-wrap still set → this wraps
    assert_eq!(term.grid().cell(1, 0).c, 'c');
}

/// Multiple combining marks accumulate on the same base cell, in order.
#[test]
fn multiple_combining_marks_accumulate() {
    let mut term = Engine::new(5, 1);
    term.feed("a\u{0301}\u{0302}".as_bytes()); // a + acute + circumflex

    assert_eq!(cell_text(&mut term, 0, 0), "a\u{0301}\u{0302}");
}

/// A combining mark survives a column resize: its side-table index travels with
/// the cell through reflow, so the cluster is still intact afterward.
#[test]
fn combining_mark_survives_resize() {
    let mut term = Engine::new(5, 2);
    term.feed("e\u{0301}".as_bytes());
    term.resize(3, 2); // column change → reflow

    assert_eq!(cell_text(&mut term, 0, 0), "e\u{0301}");
}
