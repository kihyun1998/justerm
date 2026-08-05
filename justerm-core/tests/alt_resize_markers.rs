//! An alt-screen resize reports a marker's line in a frame the buffer does not have.
//!
//! `Term::resize` runs the alt grid through `reflow_pane` with an **empty** scrollback and then
//! throws the returned scrollback away — the alt screen has none. But `reflow_pane` splits its input
//! into `[history ++ last rows]` regardless, and only the **cursor** is converted back to a
//! screen-relative row. The `extras` (markers, anchors) keep counting from the top of that discarded
//! history, and the caller then re-bases them onto the *primary* scrollback:
//!
//! ```text
//! m.line = new_base + r_alt.extras[i].0
//!          ^^^^^^^^   ^^^^^^^^^^^^^^^^^ index into [dropped ++ alt screen]
//!          primary scrollback length
//! ```
//!
//! Two different regions, added together. The bound in `reflow_pane` cannot catch it either: it is
//! computed from the alt pane's own discarded scrollback, so it permits exactly the rows that are
//! about to be thrown away.
//!
//! No reflow is required to reach it — a **rows-only** resize never calls `grid::reflow` at all,
//! so this is not a wrap-and-re-split problem. It is an absolute-index walk over
//! `[scrollback ++ grid]` reading the wrong region, the failure class `theflow.md` names as
//! unconditional-trigger #3: no crash, a plausible-looking number, and the wrong answer.
//!
//! What a marker that falls off the top *should* do is already settled by the alt-scroll path:
//! `markers_rotate_region` disposes the marker on the row leaving the region and fires
//! `MarkerDisposed`. The alt screen has no history to hold it, so a resize that drops the row is the
//! same event.

use justerm_core::{Engine, TermEvent};

/// Every marker line the frame publishes, in buffer coordinates.
fn marker_lines(t: &Engine) -> Vec<usize> {
    // Asks the engine directly since v16 removed the wire group this read (#490). Same
    // population one hop earlier — the group was projected from exactly this.
    t.marker_index()
        .markers
        .iter()
        .map(|m| m.line as usize)
        .collect()
}

/// The one-past-the-end of the buffer's absolute line space.
fn buffer_lines(t: &Engine) -> usize {
    t.scrollback_len() + t.grid().rows()
}

#[test]
fn a_rows_only_resize_keeps_an_alt_marker_inside_the_buffer() {
    // The tightest form: `cols` does not change, so `grid::reflow` is never entered.
    let mut t = Engine::new(8, 10);
    t.feed(b"\x1b[?1049h");
    for i in 0..8 {
        t.feed(format!("l{i}\r\n").as_bytes());
    }
    let id = t.add_marker(7);
    assert_eq!(marker_lines(&t), vec![7], "fixture");

    t.resize(8, 3);

    let lines = marker_lines(&t);
    assert!(
        lines.iter().all(|&l| l < buffer_lines(&t)),
        "a marker may not name a line the buffer does not have: {lines:?} in {} lines",
        buffer_lines(&t)
    );
    let _ = id;
}

#[test]
fn an_alt_marker_that_survives_the_resize_still_names_its_own_row() {
    // The bottom rows of the alt screen are the ones that survive a shrink, so a marker on the last
    // row must still be on the last row — not shifted by the count of rows that fell off.
    let mut t = Engine::new(8, 10);
    t.feed(b"\x1b[?1049h");
    for i in 0..9 {
        t.feed(format!("row{i}\r\n").as_bytes());
    }
    let id = t.add_marker(9); // the bottom row of the alt grid
    assert_eq!(marker_lines(&t), vec![9], "fixture");

    t.resize(8, 3);

    // Rows 7..9 survive as the new 0..2; the marker was on the last one.
    assert_eq!(
        marker_lines(&t),
        vec![t.scrollback_len() + 2],
        "the marker follows its row to the bottom of the smaller screen"
    );
    let _ = id;
}

#[test]
fn an_alt_marker_whose_row_falls_off_is_disposed() {
    // The alt screen has no history to hold it, and the alt-scroll path already settled what that
    // means: `markers_rotate_region` disposes the marker on the row leaving the region. A resize
    // that drops the row is the same event, so it owes the same `MarkerDisposed`.
    let mut t = Engine::new(8, 10);
    t.feed(b"\x1b[?1049h");
    for i in 0..9 {
        t.feed(format!("row{i}\r\n").as_bytes());
    }
    let id = t.add_marker(0); // the top row — the first to go
    let _ = t.drain_events();

    t.resize(8, 3);

    assert!(
        t.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "a marker whose row left the alt screen must be disposed, not silently relocated"
    );
    assert!(
        marker_lines(&t).is_empty(),
        "and it must not still be published: {:?}",
        marker_lines(&t)
    );
}

#[test]
fn a_primary_marker_is_untouched_by_the_alt_halfs_repair() {
    // The control. Primary markers ride the *primary* pane, which really does have scrollback to
    // absorb a displaced row — nothing about the alt repair may reach them.
    let mut t = Engine::with_scrollback(8, 4, 100);
    t.feed(b"primary0\r\nprimary1\r\n");
    let id = t.add_marker(1);
    let before = marker_lines(&t);
    assert_eq!(before, vec![1], "fixture");

    t.feed(b"\x1b[?1049h");
    t.resize(8, 3);
    t.feed(b"\x1b[?1049l");

    assert_eq!(
        marker_lines(&t),
        before,
        "the primary marker still names its own line after an alt excursion with a resize"
    );
    assert!(
        !t.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "and it was not disposed"
    );
}

#[test]
fn a_primary_marker_shifts_by_what_the_scrollback_cap_evicted() {
    // The other half of the same seam, and it was unpinned: `reflow_pane` now reports extras in its
    // own `[history ++ screen]` frame, so the primary caller subtracts what the cap threw away. That
    // subtraction had no test — measured, removing it left the whole suite green — while being
    // exactly the translation whose alt-side twin produced a line past the end of the buffer.
    //
    // Here the narrowing doubles every line, the buffer outgrows a cap of 2, and one row is evicted:
    // a marker below it must come down by one.
    let mut t = Engine::with_scrollback(8, 3, 2);
    for i in 0..5 {
        t.feed(format!("line{i}xx\r\n").as_bytes());
    }
    // Park the cursor **on** content rather than on the blank line after it. Otherwise the seam's
    // row budget (#567 ①) buys a row for the cursor, one more line scrolls into history, and the cap
    // evicts one more — which moves these numbers for a reason that has nothing to do with the
    // translation this test exists to pin. Isolating the variable, not chasing the expectation.
    t.feed(b"\x1b[1;1H");
    let ids: Vec<_> = (0..3).map(|r| t.add_marker(r)).collect();
    assert_eq!(marker_lines(&t), vec![2, 3, 4], "fixture");

    t.resize(4, 3);

    assert_eq!(
        marker_lines(&t),
        vec![1, 3, 4],
        "the marker above the eviction moves down by the one row the cap dropped"
    );
    assert!(
        marker_lines(&t).iter().all(|&l| l < buffer_lines(&t)),
        "and every one stays inside the buffer"
    );
    let _ = ids;
}
