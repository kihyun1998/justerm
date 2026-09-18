//! #936 — `ED 3` (`CSI 3 J`) erases the saved lines: all of history goes, the screen and the
//! cursor stay (xterm `do_erase_display` case 3). Every holder of an absolute line is
//! repaired as the scrollback cap repairs it, all of history at once.

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

    e.feed(b"\x1b[3J");

    assert_eq!(row_text(&e, 1), "tui");
    assert_eq!(e.tracked_point(point), Some((1, 0)));
    e.feed(b"\x1b[?1049l");
    assert_eq!(e.scrollback_len(), 0);
}
