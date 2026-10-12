//! The Session ([#1043](https://github.com/kihyun1998/justerm/issues/1043)): one Engine bound to one
//! frame sink, driven through its public API with a sink that records every frame it is sent.

use std::sync::{Arc, Mutex};

use justerm_core::{Engine, FrameKind, MIN_COLUMNS, Session, TermDamage, TermEvent, decode};

/// Records every encoded frame a Session sends, readable after the sink has been handed over.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Vec<u8>>>>);

impl justerm_core::FrameSink for Recorder {
    fn send_frame(&mut self, frame: Vec<u8>) {
        self.0.lock().unwrap().push(frame);
    }
}

impl Recorder {
    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    fn kinds(&self) -> Vec<FrameKind> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|f| decode(f).unwrap().kind)
            .collect()
    }
}

/// A Session with a recorder already subscribed and its subscribe frame cleared off.
fn subscribed(cols: usize, rows: usize) -> (Session, Recorder) {
    let mut session = Session::new(Engine::new(cols, rows));
    let rec = Recorder::default();
    session.subscribe(Box::new(rec.clone()));
    rec.0.lock().unwrap().clear();
    (session, rec)
}

#[test]
fn a_pump_with_nothing_changed_sends_no_frame() {
    let (mut session, rec) = subscribed(10, 3);
    session.pump();
    session.pump();
    assert_eq!(rec.count(), 0);
}

#[test]
fn feed_then_pump_sends_one_frame_and_the_next_pump_sends_none() {
    let (mut session, rec) = subscribed(10, 3);
    session.feed(b"hi");
    session.pump();
    assert_eq!(rec.count(), 1);
    let frame = decode(&rec.0.lock().unwrap()[0]).unwrap();
    let text: String = frame
        .spans
        .iter()
        .flat_map(|s| s.cells.iter().map(|c| c.c()))
        .collect();
    assert!(
        text.starts_with("hi"),
        "the frame carries what was fed: {text:?}"
    );
    session.pump();
    assert_eq!(rec.count(), 1);
}

#[test]
fn a_query_comes_back_from_pump_as_reply_bytes_and_the_queue_is_then_empty() {
    let (mut session, _rec) = subscribed(10, 3);
    session.feed(b"\x1b[c");
    assert_eq!(session.pump().replies, b"\x1b[?62;22c");
    assert!(session.pump().replies.is_empty());
}

#[test]
fn events_come_back_from_pump_unfiltered_and_the_queue_is_then_empty() {
    let (mut session, _rec) = subscribed(10, 3);
    session.feed(b"\x1b]2;work\x07\x07");
    let events = session.pump().events;
    assert!(
        events.contains(&TermEvent::Title("work".into())),
        "{events:?}"
    );
    assert!(events.contains(&TermEvent::Bell), "{events:?}");
    assert!(session.pump().events.is_empty());
}

#[test]
fn a_late_subscriber_starts_from_a_full_frame() {
    let mut session = Session::new(Engine::new(10, 3));
    session.feed(b"before anyone watched");
    session.pump();
    let rec = Recorder::default();
    session.subscribe(Box::new(rec.clone()));
    assert_eq!(rec.kinds(), vec![FrameKind::Full]);
}

#[test]
fn subscribing_again_replaces_the_sink() {
    let (mut session, first) = subscribed(10, 3);
    let second = Recorder::default();
    session.subscribe(Box::new(second.clone()));
    session.feed(b"x");
    session.pump();
    assert_eq!(first.count(), 0, "the replaced sink receives nothing more");
    assert_eq!(second.kinds(), vec![FrameKind::Full, FrameKind::Partial]);
}

#[test]
fn resize_returns_the_size_the_engine_adopted() {
    let (mut session, _rec) = subscribed(10, 3);
    assert_eq!(session.resize(1, 5), (MIN_COLUMNS, 5));
    assert_eq!(session.resize(12, 4), (12, 4));
}

#[test]
fn a_resize_sends_a_frame_on_the_next_pump() {
    let (mut session, rec) = subscribed(10, 3);
    session.resize(12, 4);
    session.pump();
    assert_eq!(rec.count(), 1);
    let frame = decode(&rec.0.lock().unwrap()[0]).unwrap();
    assert_eq!((frame.cols, frame.rows), (12, 4));
}

#[test]
fn a_user_scroll_sends_a_frame_on_the_next_pump() {
    let (mut session, rec) = subscribed(10, 2);
    session.feed(b"1\r\n2\r\n3\r\n4");
    session.pump();
    rec.0.lock().unwrap().clear();
    session.edit(|engine| engine.scroll_up(1));
    session.pump();
    assert_eq!(rec.count(), 1);
    assert_eq!(decode(&rec.0.lock().unwrap()[0]).unwrap().display_offset, 1);
}

#[test]
fn an_ended_session_sends_a_view_change_without_a_pump() {
    let (mut session, rec) = subscribed(10, 2);
    session.feed(b"1\r\n2\r\n3\r\n4");
    session.pump();
    rec.0.lock().unwrap().clear();
    session.end();
    session.edit(|engine| engine.scroll_up(1));
    assert_eq!(
        rec.count(),
        1,
        "nothing pumps an ended session, so the change flushes itself"
    );
}

#[test]
fn a_live_session_waits_for_the_pump() {
    let (mut session, rec) = subscribed(10, 2);
    session.feed(b"1\r\n2\r\n3\r\n4");
    session.pump();
    rec.0.lock().unwrap().clear();
    session.edit(|engine| engine.scroll_up(1));
    assert_eq!(rec.count(), 0);
}

#[test]
fn a_pump_without_a_sink_still_hands_back_replies() {
    let mut session = Session::new(Engine::new(10, 3));
    session.feed(b"\x1b[c");
    assert_eq!(session.pump().replies, b"\x1b[?62;22c");
}

#[test]
fn output_while_scrolled_up_sends_a_frame_though_it_leaves_no_damage() {
    let (mut session, rec) = subscribed(10, 2);
    session.feed(b"1\r\n2\r\n3\r\n4");
    session.edit(|engine| engine.scroll_up(1));
    session.pump();
    rec.0.lock().unwrap().clear();
    session.feed(b"\r\n5");
    assert!(
        matches!(session.engine().damage(), TermDamage::Partial(ref lines) if lines.is_empty()),
        "the frozen viewport records no damage"
    );
    session.pump();
    assert_eq!(rec.count(), 1, "the header's scroll position still moved");
    let frame = decode(&rec.0.lock().unwrap()[0]).unwrap();
    assert_eq!((frame.display_offset, frame.scrollback_len), (2, 3));
}

#[test]
fn a_clear_sends_a_full_frame_on_the_next_pump() {
    let (mut session, rec) = subscribed(10, 2);
    session.feed(b"1\r\n2\r\n3");
    session.pump();
    rec.0.lock().unwrap().clear();
    assert!(session.edit(|engine| engine.clear()));
    session.pump();
    assert_eq!(rec.kinds(), vec![FrameKind::Full]);
}

#[test]
fn every_viewport_scroll_sends_a_frame_on_the_next_pump() {
    let (mut session, rec) = subscribed(10, 2);
    session.feed(b"1\r\n2\r\n3\r\n4\r\n5");
    session.pump();
    let offsets = |rec: &Recorder| -> Vec<u32> {
        rec.0
            .lock()
            .unwrap()
            .iter()
            .map(|f| decode(f).unwrap().display_offset)
            .collect()
    };
    rec.0.lock().unwrap().clear();

    session.edit(|engine| engine.scroll_up(3));
    session.pump();
    session.edit(|engine| engine.scroll_down(1));
    session.pump();
    session.edit(|engine| engine.scroll_to_bottom());
    session.pump();
    let m = session
        .engine()
        .search("1")
        .into_iter()
        .next()
        .expect("1 is in history");
    session.edit(|engine| engine.scroll_to_match(&m));
    session.pump();
    assert_eq!(offsets(&rec), vec![3, 2, 0, 3]);
}

#[test]
fn marking_the_screen_fully_damaged_sends_a_full_frame_on_the_next_pump() {
    let (mut session, rec) = subscribed(10, 2);
    session.edit(|engine| engine.mark_fully_damaged());
    session.pump();
    assert_eq!(rec.kinds(), vec![FrameKind::Full]);
}

#[test]
fn an_ended_session_sends_a_resize_without_a_pump() {
    let (mut session, rec) = subscribed(10, 3);
    session.end();
    assert_eq!(session.resize(12, 4), (12, 4));
    assert_eq!(rec.count(), 1, "resizing a dead pane is a view change too");
    let frame = decode(&rec.0.lock().unwrap()[0]).unwrap();
    assert_eq!((frame.cols, frame.rows), (12, 4));
}

#[test]
fn ending_a_session_sends_the_output_that_arrived_after_the_last_pump() {
    let (mut session, rec) = subscribed(10, 3);
    session.feed(b"bye");
    session.end();
    assert_eq!(
        rec.count(),
        1,
        "no pump will come for the source's last chunk"
    );
}

#[test]
fn a_paint_sends_a_frame_on_the_next_pump() {
    let (mut session, rec) = subscribed(10, 3);
    session.feed(b"ok");
    session.pump();
    rec.0.lock().unwrap().clear();
    let line = session
        .edit(|engine| engine.changed_logical_lines())
        .remove(0);
    let span = justerm_core::PaintSpan {
        start: 0,
        end: 2,
        fg: Some(justerm_core::Color::Indexed(1)),
        bg: None,
    };
    assert!(session.edit(|engine| engine.paint_logical_line(line.at, &line.text, &[span])));
    session.pump();
    assert_eq!(
        rec.count(),
        1,
        "a paint recolours cells, so the next frame differs"
    );
}

// --- Overlay-only changes (#1044): selection, search highlights, active match, markers ---

/// A subscribed session showing `lines`, with that output already pumped and the recorder cleared.
fn quiet(lines: &[u8]) -> (Session, Recorder) {
    let (mut session, rec) = subscribed(20, 4);
    session.feed(lines);
    session.pump();
    rec.0.lock().unwrap().clear();
    (session, rec)
}

/// Apply one overlay change, check it left no cell damage (ADR-0014), pump, and return the one
/// frame it produced.
fn one_frame_after(
    session: &mut Session,
    rec: &Recorder,
    change: impl FnOnce(&mut Engine),
) -> justerm_core::Frame {
    rec.0.lock().unwrap().clear();
    session.edit(change);
    assert!(
        matches!(session.engine().damage(), TermDamage::Partial(ref lines) if lines.is_empty()),
        "an overlay change is not cell damage (ADR-0014)"
    );
    session.pump();
    let frames = rec.0.lock().unwrap();
    assert_eq!(
        frames.len(),
        1,
        "the overlay change sends exactly one frame on the next pump"
    );
    decode(&frames[0]).unwrap()
}

#[test]
fn a_selection_begin_alone_sends_a_frame() {
    let (mut s, rec) = quiet(b"hello world");
    let f = one_frame_after(&mut s, &rec, |e| {
        e.selection_begin(
            0,
            0,
            justerm_core::Side::Left,
            justerm_core::SelectionType::Word,
        )
    });
    assert!(
        !f.overlay.selection.is_empty(),
        "a word selection covers `hello`"
    );
}

#[test]
fn a_selection_extend_sends_a_frame() {
    let (mut s, rec) = quiet(b"hello world");
    s.edit(|e| {
        e.selection_begin(
            0,
            0,
            justerm_core::Side::Left,
            justerm_core::SelectionType::Char,
        )
    });
    s.pump();
    let f = one_frame_after(&mut s, &rec, |e| {
        e.selection_extend(0, 8, justerm_core::Side::Right)
    });
    assert!(!f.overlay.selection.is_empty());
}

#[test]
fn select_all_sends_a_frame() {
    let (mut s, rec) = quiet(b"hello");
    let f = one_frame_after(&mut s, &rec, |e| e.select_all());
    assert!(!f.overlay.selection.is_empty());
}

#[test]
fn clearing_a_selection_sends_a_frame_without_it() {
    let (mut s, rec) = quiet(b"hello");
    s.edit(|e| e.select_all());
    s.pump();
    let f = one_frame_after(&mut s, &rec, |e| e.selection_clear());
    assert!(f.overlay.selection.is_empty());
}

#[test]
fn setting_and_clearing_search_highlights_each_send_a_frame() {
    let (mut s, rec) = quiet(b"ab ab ab");
    let matches = s.engine().search("ab");
    assert_eq!(matches.len(), 3);
    let f = one_frame_after(&mut s, &rec, |e| e.set_search_highlights(matches));
    assert_eq!(f.overlay.matches.len(), 3);
    let f = one_frame_after(&mut s, &rec, |e| e.set_search_highlights(Vec::new()));
    assert!(f.overlay.matches.is_empty());
}

#[test]
fn designating_an_active_highlight_sends_a_frame() {
    let (mut s, rec) = quiet(b"ab ab ab");
    let matches = s.engine().search("ab");
    s.edit(|e| e.set_search_highlights(matches));
    s.pump();
    let f = one_frame_after(&mut s, &rec, |e| e.set_active_search_highlight(Some(1)));
    assert!(!f.overlay.active_match.is_empty());
}

#[test]
fn designating_an_active_match_sends_a_frame() {
    let (mut s, rec) = quiet(b"ab ab ab");
    let m = s.engine().search("ab").remove(2);
    let f = one_frame_after(&mut s, &rec, |e| e.set_active_search_match(Some(m)));
    assert!(!f.overlay.active_match.is_empty());
}

#[test]
fn adding_and_removing_a_marker_each_send_a_frame() {
    let (mut s, rec) = quiet(b"one\r\ntwo");
    let mut id = None;
    let f = one_frame_after(&mut s, &rec, |e| id = Some(e.add_marker(1)));
    assert_eq!(f.overlay.markers.len(), 1);
    let f = one_frame_after(&mut s, &rec, |e| e.remove_marker(id.unwrap()));
    assert!(f.overlay.markers.is_empty());
}

#[test]
fn the_selection_resolution_setters_each_send_a_frame() {
    let (mut s, rec) = quiet(b"a-b c");
    s.edit(|e| {
        e.selection_begin(
            0,
            0,
            justerm_core::Side::Left,
            justerm_core::SelectionType::Word,
        )
    });
    s.pump();
    one_frame_after(&mut s, &rec, |e| e.set_word_separators(" -"));
    one_frame_after(&mut s, &rec, |e| e.set_selection_carries_line_end(true));
}

#[test]
fn an_ended_session_sends_a_selection_change_without_a_pump() {
    let (mut s, rec) = quiet(b"hello");
    s.end();
    rec.0.lock().unwrap().clear();
    s.edit(|e| e.select_all());
    assert_eq!(
        rec.count(),
        1,
        "nothing pumps an ended session, so the selection flushes itself"
    );
}
