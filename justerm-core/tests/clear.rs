//! #936 — `Engine::clear`, a terminal's Clear command. The cursor's line becomes row 0,
//! the rows above it and all of history are dropped, the rows below are blanked, and the
//! view returns to the bottom (xterm.js `Terminal.clear`). It is out of band, so the parser
//! is untouched, and it does nothing on the alt screen (ghostty `Termio.clearScreen`).

use justerm_core::{Engine, FrameKind, MarkerKind, TermEvent};

fn row_text(e: &Engine, row: usize) -> String {
    e.viewport_line(row)
        .iter()
        .map(|c| c.c())
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn disposed(e: &mut Engine) -> Vec<u32> {
    let mut ids: Vec<u32> = e
        .drain_events()
        .into_iter()
        .filter_map(|ev| match ev {
            TermEvent::MarkerDisposed(id) => Some(id.0),
            _ => None,
        })
        .collect();
    ids.sort();
    ids
}

/// Six lines on a four-row screen with the cursor on row 1: two lines are history.
/// Row 1 moves to the top at the same column, and nothing else survives.
#[test]
fn keeps_the_cursor_line_at_the_top_and_drops_the_rest() {
    let mut e = Engine::new(10, 4);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5\r\nl6");
    e.feed(b"\x1b[2;3H"); // row 1 (holds "l4"), column 2

    assert!(e.clear());

    assert_eq!(e.scrollback_len(), 0);
    assert_eq!(row_text(&e, 0), "l4");
    for row in 1..4 {
        assert_eq!(row_text(&e, row), "", "row {row} is blank");
    }
    assert_eq!((e.cursor().row, e.cursor().col), (0, 2));
    assert_eq!(e.accessible_text().trim_end(), "l4");
}

/// Scrolled up into history, the view comes back to the bottom — there is no history
/// left to show, and an offset past it would underflow every viewport read.
#[test]
fn returns_the_view_to_the_bottom() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5\r\nl6");
    e.scroll_up(2);

    e.clear();

    let f = e.frame();
    assert_eq!(f.display_offset, 0);
    assert_eq!(f.scrollback_len, 0);
    assert_eq!(row_text(&e, 0), "l6");
}

/// The next frame repaints everything, with no row shift left over from before.
#[test]
fn the_next_frame_is_full_with_no_scroll() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3");
    e.reset_damage();
    e.feed(b"\r\nl4");
    assert!(
        e.scroll_delta().is_some(),
        "precondition: a scroll is pending"
    );

    e.clear();

    let f = e.frame();
    assert_eq!(f.kind, FrameKind::Full);
    assert_eq!(f.scroll, None);
}

/// Out of band: an escape sequence the host left half-written is still completed by the
/// bytes that follow, as though `clear` had never happened.
#[test]
fn leaves_a_half_written_sequence_to_complete() {
    let mut e = Engine::new(10, 3);
    e.feed(b"\x1b[3"); // "CSI 3" of a "CSI 31 m"

    e.clear();
    e.feed(b"1mX");

    assert_eq!(row_text(&e, 0), "X", "no stray '1mX' text");
    assert_eq!(
        e.viewport_line(0)[0].fg(),
        justerm_core::Color::Indexed(1),
        "the SGR finished and applied"
    );
}

/// The alternate screen is left alone: clearing under a TUI would move the cursor out
/// from under the application's own model of it.
#[test]
fn does_nothing_on_the_alt_screen() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4");
    e.feed(b"\x1b[?1049h\x1b[2;1Htui");

    assert!(!e.clear());

    assert_eq!(row_text(&e, 1), "tui");
    assert_eq!((e.cursor().row, e.cursor().col), (1, 3));
    e.feed(b"\x1b[?1049l");
    assert_eq!(e.scrollback_len(), 1, "the primary's history is untouched");
}

#[test]
fn clears_the_selection_and_search_highlights() {
    let mut e = Engine::new(10, 3);
    e.feed(b"abc\r\nabc");
    e.select_all();
    let hits = e.search("abc");
    e.set_search_highlights(hits.clone());
    e.set_active_search_match(Some(hits[0]));

    e.clear();

    assert_eq!(e.selection_text(), None);
    let f = e.frame();
    assert!(f.overlay.selection.is_empty());
    assert!(f.overlay.matches.is_empty());
}

/// A marker on the kept line stays on it; a marker anywhere else — in history, above the
/// cursor, below it — is disposed and announced.
#[test]
fn keeps_the_marks_on_the_kept_line_and_disposes_the_rest() {
    let mut e = Engine::new(10, 4);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4");
    let history = e.add_marker(0); // "l1", about to scroll into history
    e.feed(b"\r\nl5\r\nl6"); // screen: l3 l4 l5 l6; history: l1 l2
    let above = e.add_marker(0); // "l3"
    let kept = e.add_marker(1); // "l4"
    let below = e.add_marker(3); // "l6"
    e.drain_events();
    e.feed(b"\x1b[2;1H"); // cursor to "l4"

    e.clear();

    let mut want = vec![history.0, above.0, below.0];
    want.sort();
    assert_eq!(disposed(&mut e), want);
    let index = e.marker_index();
    assert_eq!(index.markers.len(), 1);
    assert_eq!(index.markers[0].id, kept);
    assert_eq!(index.markers[0].line, 0);
}

/// Every surviving absolute line moved by the same amount — the lines dropped off the
/// front — so a held index rebases by the `evicted_total` delta alone.
#[test]
fn advances_evicted_total_by_the_lines_dropped() {
    let mut e = Engine::new(10, 4);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5\r\nl6"); // history: l1 l2
    e.feed(b"\x1b[3;1H"); // cursor on row 2 ("l5"), absolute line 4
    let kept = e.add_marker(2);
    let before = e.marker_index();
    let held = before.markers[0].line;

    e.clear();

    let after = e.marker_index();
    let delta = after.evicted_total - before.evicted_total;
    assert_eq!(
        delta, 4,
        "two history lines and the two rows above the cursor"
    );
    assert_eq!(
        after.epoch, before.epoch,
        "a uniform shift needs no re-pull"
    );
    assert_eq!(u64::from(held) - delta, u64::from(after.markers[0].line));
    assert_eq!(after.markers[0].id, kept);
}

/// A tracked point in history is gone; one on the kept line follows it to line 0.
#[test]
fn tracked_points_follow_the_kept_line() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nl3\r\nl4"); // history: l1; screen: l2 l3 l4
    let gone = e.track_point(0, 1); // "l1"
    let kept = e.track_point(2, 1); // "l3"
    e.feed(b"\x1b[2;1H"); // cursor to "l3"

    e.clear();

    assert_eq!(e.tracked_point(gone), None);
    assert_eq!(e.tracked_point(kept), Some((0, 1)));
}

/// The command being typed when the user clears keeps its `CommandStart`, so it is still
/// reported once it runs. xterm.js disposes every marker here; justerm keeps the kept
/// line's, for the reason `EL` retires nothing (#750).
#[test]
fn the_command_being_typed_is_still_reported() {
    let mut e = Engine::new(20, 4);
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07old\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls");

    e.clear();
    e.feed(b"\r\n\x1b]133;C\x07a b\r\n\x1b]133;D;0\x07");

    let cmds: Vec<String> = e.command_lines().into_iter().map(|c| c.command).collect();
    assert_eq!(cmds, vec!["ls\n".to_string()]);
    // The previous command's `D` was printed on the prompt line, so it is kept with it.
    let kept: Vec<(usize, MarkerKind)> = e
        .command_marks()
        .into_iter()
        .filter(|m| m.1 == 0)
        .map(|m| (m.1, m.2))
        .collect();
    assert_eq!(
        kept,
        vec![
            (0, MarkerKind::CommandFinished(Some(0))),
            (0, MarkerKind::PromptStart),
            (0, MarkerKind::CommandStart),
        ]
    );
}

/// The kept line may have been wrapping onto the row below it, which is now blank — so
/// it no longer continues, and copy does not join it to the empty row.
#[test]
fn the_kept_line_stops_wrapping() {
    let mut e = Engine::new(4, 3);
    e.feed(b"abcdef"); // "abcd" wraps onto "ef"
    e.feed(b"\x1b[1;1H");
    assert!(e.grid().is_row_wrapped(0), "precondition");

    e.clear();

    assert!(!e.grid().is_row_wrapped(0));
}

/// `REP` repeats the last printed glyph by reading back the cell it wrote. Out of band,
/// `clear` must not change what a repeat that follows it prints — the cell moved to row 0
/// with the kept line, and the repeat reads it there.
///
/// The print has to be the last thing fed: every CSI, CR or LF disarms the anchor, so a
/// cursor move before `clear` leaves nothing for this to observe.
#[test]
fn a_repeat_after_clear_repeats_the_last_glyph() {
    let mut e = Engine::new(10, 3);
    e.feed(b"l1\r\nl2\r\nab"); // "b" arms REP at (2, 1)

    e.clear();
    e.feed(b"\x1b[2b");

    assert_eq!(row_text(&e, 0), "abbb");
    assert_eq!((e.cursor().row, e.cursor().col), (0, 4));
}
