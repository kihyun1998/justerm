//! #936 — `ED 3` (`CSI 3 J`) erases the saved lines: all of history goes, the screen and the
//! cursor stay (xterm `do_erase_display` case 3). Every holder of an absolute line is
//! repaired as the scrollback cap repairs it, all of history at once. #1096 — a consumer can
//! turn it off with `set_erase_saved_lines`; the `ignored_*` tests pin that side.

use justerm_core::{Engine, SelectionType, Side, TermEvent};

fn row_text(e: &Engine, row: usize) -> String {
    e.viewport_line(row)
        .iter()
        .map(|c| c.c())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn drops_history_and_leaves_the_screen() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    assert_eq!(e.scrollback_len(), 2, "precondition");

    e.feed(b"\x1b[3J");

    assert_eq!(e.scrollback_len(), 0);
    assert_eq!(
        (0..3).map(|r| row_text(&e, r)).collect::<Vec<_>>(),
        ["l3", "l4", "l5"]
    );
    assert_eq!((e.cursor().row, e.cursor().col), (2, 2));
    assert_eq!(e.accessible_text().trim_end(), "l3\nl4\nl5");
}

/// Scrolled up into history, the view returns to the bottom; the offset it held would
/// otherwise point past the start of the buffer.
#[test]
fn returns_a_scrolled_view_to_the_bottom() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    e.scroll_up(2);
    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(2, 1, Side::Right);

    e.feed(b"\x1b[3J");

    assert_eq!(e.frame().display_offset, 0);
    assert_eq!(row_text(&e, 0), "l3");
    // The selection ran l1..l3; its history end clamps to the new top.
    assert_eq!(e.selection_text().as_deref(), Some("l3"));
    assert_eq!(e.selection_range().len(), 1);
}

/// A selection end on any history line — not only the oldest — clamps to the start of
/// the new top line, and the other end moves with its content.
#[test]
fn a_selection_end_anywhere_in_history_clamps_to_the_new_top() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5"); // history: l1 l2
    e.scroll_up(2);
    e.selection_begin(1, 1, Side::Left, SelectionType::Char); // "2" of l2, line 1
    e.selection_extend(2, 1, Side::Right); // "3" of l3, line 2

    e.feed(b"\x1b[3J");

    assert_eq!(e.selection_text().as_deref(), Some("l3"));
}

/// A selection wholly inside history is gone with it, wherever in history it was.
#[test]
fn a_selection_wholly_in_history_is_cleared() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5"); // history: l1 l2
    e.scroll_up(2);
    e.selection_begin(1, 0, Side::Left, SelectionType::Char); // l2, line 1
    e.selection_extend(1, 1, Side::Right);

    e.feed(b"\x1b[3J");

    assert_eq!(e.selection_text(), None);
}

/// Scrolled up, the rows on show change completely, so the next frame repaints them all.
#[test]
fn a_scrolled_view_is_repainted() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    e.scroll_up(2);
    e.reset_damage();

    e.feed(b"\x1b[3J");

    assert_eq!(e.frame().kind, justerm_core::FrameKind::Full);
}

#[test]
fn disposes_the_marks_in_history_and_shifts_the_rest() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3");
    let old = e.add_marker(0); // "l1"
    e.feed(b"\r\nl4\r\nl5"); // history: l1 l2
    let live = e.add_marker(1); // "l4", absolute line 3
    e.drain_events();
    let before = e.marker_index();

    e.feed(b"\x1b[3J");

    let disposed: Vec<_> = e
        .drain_events()
        .into_iter()
        .filter_map(|ev| match ev {
            TermEvent::MarkerDisposed(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(disposed, vec![old]);
    let after = e.marker_index();
    assert_eq!(after.evicted_total - before.evicted_total, 2);
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.markers.len(), 1);
    assert_eq!((after.markers[0].id, after.markers[0].line), (live, 1));
}

#[test]
fn drops_the_tracked_points_in_history_and_shifts_the_rest() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    let gone = e.track_point(1, 0); // "l2"
    let kept = e.track_point(3, 1); // "l4"

    e.feed(b"\x1b[3J");

    assert_eq!(e.tracked_point(gone), None);
    assert_eq!(e.tracked_point(kept), Some((1, 1)));
}

#[test]
fn drops_the_search_highlights() {
    let mut e = Engine::new(10, 3);
    e.feed(b"ab\r\nab\r\nab\r\nab");
    let hits = e.search("ab");
    e.set_search_highlights(hits);

    e.feed(b"\x1b[3J");

    assert!(e.frame().overlay.matches.is_empty());
}

/// With no history there is nothing to drop, and nothing is repainted.
#[test]
fn is_quiet_with_no_history() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1");
    e.reset_damage();

    e.feed(b"\x1b[3J");

    assert!(e.frame().spans.is_empty());
}

/// On the alt screen it drops the primary's history underneath, as xterm does, and the
/// alt screen's own lines — which sit above that history — shift with it.
#[test]
fn on_the_alt_screen_drops_the_primary_history() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4"); // history: l1
    e.feed(b"\x1b[?1049h\x1b[2;1Htui");
    let point = e.track_point(e.scrollback_len() + 1, 0);
    let mark = e.add_marker(1);

    e.feed(b"\x1b[3J");

    assert_eq!(row_text(&e, 1), "tui");
    assert_eq!(e.tracked_point(point), Some((1, 0)));
    let index = e.marker_index();
    assert_eq!(index.markers.len(), 1);
    assert_eq!((index.markers[0].id, index.markers[0].line), (mark, 1));
    e.feed(b"\x1b[?1049l");
    assert_eq!(e.scrollback_len(), 0);
}

/// Two lines of history, `l3`..`l5` on screen, and `ED 3` set to change nothing.
fn ignoring_ed3() -> Engine {
    let mut e = Engine::new(10, 3);
    e.set_erase_saved_lines(false);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    assert_eq!(e.scrollback_len(), 2, "precondition");
    e
}

#[test]
fn ignored_it_leaves_the_history_the_screen_and_the_cursor() {
    let mut e = ignoring_ed3();

    e.feed(b"\x1b[3J");

    assert_eq!(e.scrollback_len(), 2);
    assert_eq!(
        (0..3).map(|r| row_text(&e, r)).collect::<Vec<_>>(),
        ["l3", "l4", "l5"]
    );
    assert_eq!((e.cursor().row, e.cursor().col), (2, 2));
}

/// What `clear` sends under an `E3` terminfo entry: the screen goes, the history stays.
#[test]
fn ignored_a_clear_empties_only_the_screen() {
    let mut e = ignoring_ed3();

    e.feed(b"\x1b[H\x1b[2J\x1b[3J");

    assert_eq!(e.scrollback_len(), 2);
    assert_eq!(
        (0..3).map(|r| row_text(&e, r)).collect::<Vec<_>>(),
        ["", "", ""]
    );
    assert_eq!((e.cursor().row, e.cursor().col), (0, 0));
}

#[test]
fn ignored_it_is_consumed_when_split_across_feeds() {
    let mut e = ignoring_ed3();

    e.feed(b"\x1b[3");
    e.feed(b"J");

    assert_eq!(e.scrollback_len(), 2);
    assert_eq!(row_text(&e, 2), "l5");
    assert_eq!((e.cursor().row, e.cursor().col), (2, 2));
}

#[test]
fn ignored_it_leaves_a_scrolled_view_where_it_is() {
    let mut e = ignoring_ed3();
    e.scroll_up(2);
    e.reset_damage();

    e.feed(b"\x1b[3J");

    assert_eq!(e.frame().display_offset, 2);
    assert_eq!(row_text(&e, 0), "l1");
    assert!(
        e.frame().spans.is_empty(),
        "nothing changed, so nothing repaints"
    );
}

#[test]
fn ignored_everything_held_on_a_history_line_survives() {
    let mut e = Engine::new(10, 3);
    e.set_erase_saved_lines(false);
    e.feed(b"l1\r\nl2\r\nl3");
    let mark = e.add_marker(0); // "l1"
    e.feed(b"\r\nl4\r\nl5"); // history: l1 l2
    let point = e.track_point(1, 1); // "2" of l2
    e.scroll_up(2);
    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(1, 1, Side::Right);
    let hits = e.search("l1");
    e.set_search_highlights(hits);
    assert_eq!(e.frame().overlay.matches.len(), 1, "precondition");
    e.drain_events();

    e.feed(b"\x1b[3J");

    assert_eq!(e.selection_text().as_deref(), Some("l1\nl2"));
    assert_eq!(e.frame().overlay.matches.len(), 1);
    assert_eq!(e.tracked_point(point), Some((1, 1)));
    let index = e.marker_index();
    assert_eq!(index.evicted_total, 0);
    assert_eq!(index.markers.len(), 1);
    assert_eq!((index.markers[0].id, index.markers[0].line), (mark, 0));
    assert!(
        !e.drain_events()
            .iter()
            .any(|ev| matches!(ev, TermEvent::MarkerDisposed(_)))
    );
}

/// On the alt screen the primary's history underneath stays too.
#[test]
fn ignored_on_the_alt_screen_the_primary_history_stays() {
    let mut e = ignoring_ed3();
    e.feed(b"\x1b[?1049h\x1b[2;1Htui");
    let point = e.track_point(e.scrollback_len() + 1, 0);

    e.feed(b"\x1b[3J");

    assert_eq!(row_text(&e, 1), "tui");
    assert_eq!(e.tracked_point(point), Some((3, 0)));
    e.feed(b"\x1b[?1049l");
    assert_eq!(e.scrollback_len(), 2);
    assert_eq!(row_text(&e, 0), "l3");
}

/// A terminal's Clear command is the consumer's own, so the setting does not reach it.
#[test]
fn ignored_the_out_of_band_clear_still_drops_the_history() {
    let mut e = ignoring_ed3();

    assert!(e.clear());

    assert_eq!(e.scrollback_len(), 0);
}

#[test]
fn it_erases_by_default_and_follows_the_setting_while_the_session_lives() {
    let mut e = Engine::new(10, 3);
    assert!(e.erase_saved_lines());
    e.set_erase_saved_lines(false);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");

    e.feed(b"\x1b[3J");
    assert_eq!(e.scrollback_len(), 2, "off");

    e.set_erase_saved_lines(true);
    e.feed(b"\x1b[3J");
    assert_eq!(e.scrollback_len(), 0, "on");

    e.feed(b"\r\nl6\r\nl7");
    assert_eq!(e.scrollback_len(), 2, "precondition");
    e.set_erase_saved_lines(false);
    e.feed(b"\x1b[3J");
    assert_eq!(e.scrollback_len(), 2, "off again");
}

/// Changing the setting changes nothing on show, so it owes the consumer no frame.
#[test]
fn changing_the_setting_owes_no_frame() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1");
    e.reset_damage();

    e.set_erase_saved_lines(false);

    assert!(!e.frame_pending());
}

/// The setting is the embedder's, so an application's `ESC c` does not turn it back on.
#[test]
fn ignored_it_stays_ignored_after_a_full_reset() {
    let mut e = ignoring_ed3();

    e.feed(b"\x1bc");
    assert_eq!(
        e.scrollback_len(),
        0,
        "the reset itself still drops history"
    );
    e.feed(b"a\r\nb\r\nc\r\nd");
    assert_eq!(e.scrollback_len(), 1, "precondition");

    e.feed(b"\x1b[3J");

    assert_eq!(e.scrollback_len(), 1);
    assert!(!e.erase_saved_lines());
}
