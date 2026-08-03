//! #691 — a tracked point follows its content across every mover of the absolute
//! `[scrollback ++ grid]` index.
//!
//! The forcing case is a search anchor: a consumer remembers where the emphasis
//! sits and resolves it against the next result set. Held as a raw coordinate it
//! goes stale — the selection (`selection_evict_oldest` and its three siblings)
//! and markers (`markers_evict_oldest`, …) each carry a fixup for exactly that,
//! and a coordinate living outside the engine has none.
//!
//! Every test resolves the point back to **text**, by re-searching for the needle
//! it was anchored to. An ordinal alone cannot tell "the same occurrence,
//! renumbered" from "a different occurrence", which is the whole defect.

use justerm_core::Engine;

/// Where the sole occurrence of `needle` starts, as an absolute `(line, col)`.
fn find(t: &Engine, needle: &str) -> (usize, usize) {
    let mut hits = t.search(needle).into_iter();
    let m = hits
        .next()
        .unwrap_or_else(|| panic!("`{needle}` is not in the buffer"));
    assert!(
        hits.next().is_none(),
        "`{needle}` must be unique for this test to mean anything"
    );
    (m.start_line, m.start_col)
}

/// Write `count` numbered lines, each on its own row.
fn write_lines(t: &mut Engine, prefix: &str, count: usize) {
    for i in 0..count {
        t.feed(format!("{prefix}{i:02}\r\n").as_bytes());
    }
}

/// Past the scrollback cap the oldest line is dropped and **every** absolute
/// index shifts down by one. A point tracking content must come back naming that
/// same content, not the same number.
#[test]
fn a_tracked_point_keeps_its_content_across_cap_eviction() {
    let mut t = Engine::with_scrollback(20, 2, 10);
    write_lines(&mut t, "tag", 12);

    let before = find(&t, "tag09");
    let id = t.track_point(before.0, before.1);

    write_lines(&mut t, "pad", 3); // at the cap, each line evicts one

    let now = find(&t, "tag09");
    // The premise, asserted rather than assumed: if eviction did not renumber,
    // the assertion below would pass with no fixup at all and prove nothing.
    assert!(
        now.0 < before.0,
        "premise: the absolute index must have moved (before {before:?}, now {now:?})"
    );
    assert_eq!(
        t.tracked_point(id),
        Some(now),
        "the tracked point must still name tag09 after eviction renumbered it"
    );
}

/// Fill each of `rows` screen rows with a distinct letter (5 rows → a..e).
fn filled(cols: usize, rows: usize) -> Engine {
    let mut t = Engine::new(cols, rows);
    for i in 0..rows {
        if i > 0 {
            t.feed(b"\r\n");
        }
        t.feed(&[b'a' + i as u8]);
    }
    t
}

/// The evicted line is gone, so a point on it is gone — `None`, not a clamp onto
/// whatever content moved into that index.
#[test]
fn a_point_on_the_evicted_line_is_dropped() {
    let mut t = Engine::with_scrollback(20, 2, 10);
    write_lines(&mut t, "tag", 10);

    let at = find(&t, "tag00");
    assert_eq!(
        at.0, 0,
        "premise: tag00 must be the oldest line, i.e. the next to go"
    );
    let id = t.track_point(at.0, at.1);
    assert_eq!(
        t.tracked_point(id),
        Some(at),
        "premise: the point is live before the eviction"
    );

    write_lines(&mut t, "pad", 5);

    assert!(
        t.search("tag00").is_empty(),
        "premise: tag00 must have left the buffer"
    );
    assert_eq!(
        t.tracked_point(id),
        None,
        "a point whose line was evicted is gone, not clamped"
    );
}

/// A top-anchored sub-region scroll (#449) grows scrollback while the rows below
/// the bottom margin stay put on screen, so *their* absolute index rises by one.
/// The selection and markers are shifted for that; a tracked point must be too.
#[test]
fn a_tracked_point_follows_a_below_margin_shift() {
    let mut t = filled(10, 5);
    t.feed(b"\x1b[1;4r"); // DECSTBM rows 1..4 — top at the screen top, bottom above the last row

    let before = find(&t, "e"); // the row below the margin
    let id = t.track_point(before.0, before.1);

    // A linefeed *at the bottom margin* is what takes the accrual branch — the one
    // whose top is the screen top, so the evicted row enters scrollback while the
    // rows below the margin keep their grid position (#449). SU does not: it
    // rotates within the region.
    t.feed(b"\x1b[4;1H\n");

    let now = find(&t, "e");
    assert!(
        now.0 > before.0,
        "premise: the below-margin index must have risen (before {before:?}, now {now:?})"
    );
    assert_eq!(
        t.tracked_point(id),
        Some(now),
        "the tracked point must still name `e`"
    );
}

/// An in-screen region scroll (top margin > 0) moves content within the screen;
/// a point inside the region follows it, and one on the dropped edge is gone.
#[test]
fn a_tracked_point_rotates_with_an_in_screen_region_scroll() {
    let mut t = filled(10, 5);
    t.feed(b"\x1b[2;4r"); // rows 1..3 — a true in-screen region, nothing accrues

    let inside = find(&t, "c");
    let edge = find(&t, "b"); // the region's top row, dropped by an up-scroll
    let below = find(&t, "e"); // below the bottom margin — must not move at all
    let above = find(&t, "a"); // ABOVE the top margin — likewise
    let inside_id = t.track_point(inside.0, inside.1);
    let edge_id = t.track_point(edge.0, edge.1);
    let below_id = t.track_point(below.0, below.1);
    let above_id = t.track_point(above.0, above.1);

    t.feed(b"\x1b[1S");

    let now = find(&t, "c");
    assert!(
        now.0 < inside.0,
        "premise: in-region content must have moved up (before {inside:?}, now {now:?})"
    );
    assert_eq!(
        t.tracked_point(inside_id),
        Some(now),
        "an in-region point follows its content"
    );
    assert!(
        t.search("b").is_empty(),
        "premise: `b` must have left the buffer"
    );
    assert_eq!(
        t.tracked_point(edge_id),
        None,
        "a point on the dropped edge is gone"
    );
    // The region's *bottom* bound is load-bearing, and only this asserts it: a
    // rotate that tested `line < top` alone would drag every point below the
    // margin along with content that never moved.
    assert_eq!(
        find(&t, "e"),
        below,
        "premise: content below the margin does not move"
    );
    assert_eq!(
        t.tracked_point(below_id),
        Some(below),
        "a point below the region is untouched"
    );
    // …and the top bound is load-bearing for the same reason, in the other
    // direction: a guard that tested only `line > bottom` would drag everything
    // above the margin — including every point in scrollback — along with content
    // that never moved.
    assert_eq!(
        find(&t, "a"),
        above,
        "premise: content above the margin does not move"
    );
    assert_eq!(
        t.tracked_point(above_id),
        Some(above),
        "a point above the region is untouched"
    );
}

/// A rewrap rewrites every absolute index; the engine already maps the cursor,
/// the selection and markers through it, and a tracked point rides the same pass.
#[test]
fn a_tracked_point_maps_through_reflow() {
    let mut t = Engine::with_scrollback(10, 3, 20);
    t.feed(b"0123456789abcdef\r\n"); // two rows at width 10, one at width 20
    t.feed(b"needle\r\n");

    let before = find(&t, "needle");
    let id = t.track_point(before.0, before.1);

    t.resize(20, 3); // unwrap: the line above `needle` collapses to one row

    let now = find(&t, "needle");
    assert!(
        now.0 < before.0,
        "premise: the rewrap must have moved it (before {before:?}, now {now:?})"
    );
    assert_eq!(
        t.tracked_point(id),
        Some(now),
        "the tracked point must still name `needle`"
    );
}

/// The alt screen's content is not archived — leaving it destroys the buffer a
/// point there was anchored to, so the point dies with it (xterm's
/// `clearAllMarkers`, which `alt_markers` already follows).
#[test]
fn an_alt_tracked_point_dies_with_the_alt_screen() {
    let mut t = Engine::new(20, 3);
    t.feed(b"\x1b[?1049h");
    t.feed(b"alt-needle");

    let at = find(&t, "alt-needle");
    let id = t.track_point(at.0, at.1);
    assert_eq!(
        t.tracked_point(id),
        Some(at),
        "premise: the point is live on the alt screen"
    );

    t.feed(b"\x1b[?1049l");
    assert_eq!(
        t.tracked_point(id),
        None,
        "leaving alt destroys what the point named"
    );
}

/// RIS drops coordinates (`docs/map/invariant/ris-keeps-configuration-drops-coordinates.md`).
#[test]
fn ris_drops_every_tracked_point() {
    let mut t = Engine::new(20, 3);
    t.feed(b"needle\r\n");
    let at = find(&t, "needle");
    let id = t.track_point(at.0, at.1);

    t.feed(b"\x1bc");
    assert_eq!(
        t.tracked_point(id),
        None,
        "RIS emptied the buffer the point named"
    );
}

/// The holder says when it is done; nothing else can know.
#[test]
fn untrack_point_releases_the_id() {
    let mut t = Engine::new(20, 3);
    t.feed(b"needle\r\n");
    let at = find(&t, "needle");
    let id = t.track_point(at.0, at.1);
    assert_eq!(
        t.tracked_point(id),
        Some(at),
        "premise: live before release"
    );

    t.untrack_point(id);
    assert_eq!(t.tracked_point(id), None);
    t.untrack_point(id); // a second release is a no-op, not a panic
}

/// ADR-0026 D1/D2/D3: an out-of-range coordinate from a public surface is
/// **bounded, never asserted**, bounded where it is read back (the engine owns no
/// producer for it — it is the consumer's, like a `Match`), and bounded at *both*
/// ends rather than one.
#[test]
fn an_out_of_range_point_is_bounded_at_the_read() {
    let mut t = Engine::new(10, 3);
    t.feed(b"needle\r\n");

    let id = t.track_point(9_999, 9_999);
    let (line, col) = t
        .tracked_point(id)
        .expect("an out-of-range point is bounded, not dropped");
    assert!(
        line < 3,
        "the line is bounded into the buffer's own range, got {line}"
    );
    assert_eq!(
        col, 10,
        "the column is bounded to the GRID width (ADR-0026 D4), domain [0, cols]"
    );
}

/// RIS rebuilds the engine wholesale, resetting the id counter with it. A tracked
/// point has no disposal event by design, so the *only* thing that keeps a stale
/// id honest is that it is never reissued — otherwise a holder that kept one
/// across the reset is silently handed a different point's position. (Markers
/// solve the same hazard the other way, by announcing each disposal; the comment
/// in `full_reset` names it.)
#[test]
fn a_tracked_id_is_not_reissued_after_ris() {
    let mut t = Engine::new(20, 3);
    t.feed(b"needle\r\n");
    let old = t.track_point(0, 0);

    t.feed(b"\x1bc");
    t.feed(b"fresh\r\n");
    let new = t.track_point(0, 0);

    assert_ne!(old, new, "RIS must not reissue a tracked id");
    assert_eq!(t.tracked_point(old), None, "the pre-RIS id stays dead");
}

/// On the alt screen the absolute space floors at the alt grid's first line —
/// `scrollback` there holds the *primary* buffer's history, a different logical
/// space (`docs/map/invariant/alt-screen-buffer-floor.md`, #113/#144/#207). A
/// coordinate handed in below that floor is bounded up to it, not answered with a
/// primary-history position.
#[test]
fn an_alt_read_floors_at_the_alt_grid() {
    let mut t = Engine::new(20, 3);
    write_lines(&mut t, "tag", 6);
    t.feed(b"\x1b[?1049h");

    let floor = t.scrollback_len();
    assert!(
        floor > 0,
        "premise: there must be primary history below the floor to land in"
    );

    let id = t.track_point(0, 0); // a primary-history line, handed in while on alt
    assert_eq!(
        t.tracked_point(id).map(|p| p.0),
        Some(floor),
        "an alt read floors at the alt grid, never at primary history"
    );
}

/// A point belongs to the buffer it was registered on, and the number this API
/// returns cannot carry its own frame — the primary grid and the alt grid occupy
/// the **same** absolute indices, so a primary grid row and an alt row are the
/// same integer naming different content. Measured below: `tag05` sits on primary
/// line 5, and after the switch the alt screen's own second row answers 5 too.
/// A point of the inactive buffer therefore resolves to `None` rather than to a
/// plausible coordinate for the wrong screen.
#[test]
fn a_point_of_the_inactive_buffer_does_not_resolve() {
    let mut t = Engine::new(20, 4);
    write_lines(&mut t, "tag", 6);

    let grid_at = find(&t, "tag05"); // a primary GRID row, not scrollback
    let history_at = find(&t, "tag02"); // and one in scrollback, below the alt floor
    let grid_id = t.track_point(grid_at.0, grid_at.1);
    let history_id = t.track_point(history_at.0, history_at.1);
    assert_eq!(
        t.tracked_point(grid_id),
        Some(grid_at),
        "premise: correct on the primary screen"
    );

    t.feed(b"\x1b[?1049h");
    t.feed(b"X0\r\nX1\r\nX2");
    // The premise that makes `None` the only honest answer: the alt screen reuses
    // the primary grid's absolute indices, so the stored number is ambiguous rather
    // than merely out of range.
    assert!(
        grid_at.0 >= t.scrollback_len(),
        "premise: tag05 must be on the primary GRID, not in scrollback — that is the ambiguous half"
    );
    assert!(
        grid_at.0 < t.scrollback_len() + 4,
        "premise: that index is ALSO an alt-grid row index — the alt screen occupies \
         [scrollback_len, scrollback_len + rows), so the stored number is ambiguous rather \
         than merely out of range"
    );

    assert_eq!(
        t.tracked_point(grid_id),
        None,
        "an inactive-buffer point does not resolve into the active frame"
    );
    assert_eq!(
        t.tracked_point(history_id),
        None,
        "and that holds below the alt floor too — same rule, not a clamp"
    );

    t.feed(b"\x1b[?1049l");
    assert_eq!(
        t.tracked_point(grid_id),
        Some(grid_at),
        "leaving alt makes it resolvable again — it was never gone"
    );
    assert_eq!(
        t.tracked_point(history_id),
        Some(history_at),
        "and so does the one in scrollback"
    );
}

/// The rotate is called from three separate write-path sites, and a test that only
/// drives SU proves one of them. This is the `linefeed` site: a newline at the
/// bottom margin of an in-screen region (top > 0, so nothing accrues).
#[test]
fn a_tracked_point_rotates_on_a_linefeed_inside_a_region() {
    let mut t = filled(10, 5);
    t.feed(b"\x1b[2;4r"); // rows 1..3

    let inside = find(&t, "c");
    let edge = find(&t, "b"); // the region's top row, dropped by the scroll
    let inside_id = t.track_point(inside.0, inside.1);
    let edge_id = t.track_point(edge.0, edge.1);

    t.feed(b"\x1b[4;1H\n"); // cursor to the bottom margin, then LF

    let now = find(&t, "c");
    assert!(
        now.0 < inside.0,
        "premise: the linefeed must have moved in-region content up"
    );
    assert_eq!(
        t.tracked_point(inside_id),
        Some(now),
        "the linefeed site rotates too"
    );
    assert!(t.search("b").is_empty(), "premise: `b` left the buffer");
    assert_eq!(t.tracked_point(edge_id), None);
}

/// …and this is the `reverse_index` site, which scrolls the region the OTHER way:
/// the dropped edge is the region's *bottom*, and everything else moves down.
#[test]
fn a_tracked_point_rotates_on_reverse_index() {
    let mut t = filled(10, 5);
    t.feed(b"\x1b[2;4r"); // rows 1..3

    let inside = find(&t, "c");
    let edge = find(&t, "d"); // the region's bottom row — RI drops this one
    let inside_id = t.track_point(inside.0, inside.1);
    let edge_id = t.track_point(edge.0, edge.1);

    t.feed(b"\x1b[2;1H\x1bM"); // cursor to the top margin, then RI

    let now = find(&t, "c");
    assert!(
        now.0 > inside.0,
        "premise: RI must have moved in-region content DOWN (before {inside:?}, now {now:?})"
    );
    assert_eq!(
        t.tracked_point(inside_id),
        Some(now),
        "the direction flag is honoured, not assumed"
    );
    assert!(
        t.search("d").is_empty(),
        "premise: the bottom edge left the buffer"
    );
    assert_eq!(
        t.tracked_point(edge_id),
        None,
        "RI drops the bottom edge, not the top"
    );
}

/// A reflow can evict past the cap. The point's content is then gone, so it is
/// released — not clamped to line 0, which is what the marker loop beside it does.
#[test]
fn a_reflow_that_evicts_releases_the_point() {
    let mut t = Engine::with_scrollback(20, 2, 4);
    for i in 0..3 {
        t.feed(format!("tag{i:02}-cdefghijklmnop\r\n").as_bytes()); // 20 cols → 4 rows at width 5
    }

    let at = find(&t, "tag00");
    let id = t.track_point(at.0, at.1);
    assert_eq!(
        t.tracked_point(id),
        Some(at),
        "premise: live before the resize"
    );

    t.resize(5, 2); // narrower: every line rewraps, the extra rows evict past the cap

    assert!(
        t.search("tag00").is_empty(),
        "premise: the rewrap must have evicted tag00"
    );
    assert_eq!(
        t.tracked_point(id),
        None,
        "released, not relocated to line 0"
    );
}

/// A resize taken while an application holds the alt screen reflows the *primary*
/// pane in its own branch — ~40 lines the SU/reflow tests never reach. The point is
/// unresolvable during the excursion (it belongs to the inactive buffer), so the
/// assertion is made after leaving: it must still name its text.
#[test]
fn a_primary_point_survives_a_resize_taken_on_the_alt_screen() {
    let mut t = Engine::with_scrollback(10, 3, 40);
    t.feed(b"0123456789abcdef\r\n"); // wraps at width 10
    t.feed(b"needle\r\n");

    let before = find(&t, "needle");
    let id = t.track_point(before.0, before.1);

    t.feed(b"\x1b[?1049h");
    t.resize(20, 3); // the alt-active branch: alt pane + primary pane, separately
    t.feed(b"\x1b[?1049l");

    let now = find(&t, "needle");
    assert!(
        now.0 < before.0,
        "premise: the rewrap moved it (before {before:?}, now {now:?})"
    );
    assert_eq!(
        t.tracked_point(id),
        Some(now),
        "the alt-active branch reflows primary points too"
    );
}

/// The other half of that branch: a point on the alt pane itself, through a resize
/// that keeps its row.
#[test]
fn an_alt_point_maps_through_a_resize_on_the_alt_screen() {
    let mut t = Engine::with_scrollback(10, 3, 40);
    write_lines(&mut t, "tag", 4);
    t.feed(b"\x1b[?1049h");
    t.feed(b"ALT-NEEDLE");

    let before = find(&t, "ALT-NEEDLE");
    let id = t.track_point(before.0, before.1);

    t.resize(20, 3);

    let now = find(&t, "ALT-NEEDLE");
    assert_eq!(
        t.tracked_point(id),
        Some(now),
        "an alt point rides the alt pane's own reflow"
    );
}

/// A rows-shrink taken on the alt screen pushes rows off a screen with no history
/// to hold them, so a point on a departing row is released — the alt marker's rule,
/// and the `row < rows` bound in the alt pane's own write-back.
#[test]
fn an_alt_point_on_a_departing_row_is_released_by_a_shrink() {
    let mut t = Engine::with_scrollback(20, 3, 40);
    write_lines(&mut t, "tag", 4);
    t.feed(b"\x1b[?1049h");
    t.feed(b"TOPROW\r\nMIDROW\r\nENDROW");

    let top = find(&t, "TOPROW");
    let mid = find(&t, "MIDROW");
    let top_id = t.track_point(top.0, top.1);
    let mid_id = t.track_point(mid.0, mid.1);
    assert_eq!(
        t.tracked_point(top_id),
        Some(top),
        "premise: live before the shrink"
    );

    t.resize(20, 2);

    assert!(
        t.search("TOPROW").is_empty(),
        "premise: the shrink must have dropped TOPROW"
    );
    assert_eq!(
        t.tracked_point(top_id),
        None,
        "a point on a departed alt row is released"
    );
    let mid_now = find(&t, "MIDROW");
    assert_eq!(
        t.tracked_point(mid_id),
        Some(mid_now),
        "and a surviving alt row is re-anchored, not left stale"
    );
}

/// The release arm of the *alt-active* branch's primary half: a reflow taken while
/// an app holds the alt screen can still evict primary history past the cap.
#[test]
fn a_primary_point_evicted_by_a_reflow_on_alt_is_released() {
    let mut t = Engine::with_scrollback(20, 2, 4);
    for i in 0..3 {
        t.feed(format!("tag{i:02}-cdefghijklmnop\r\n").as_bytes());
    }
    let at = find(&t, "tag00");
    let id = t.track_point(at.0, at.1);

    t.feed(b"\x1b[?1049h");
    t.resize(5, 2); // the rewrap of the *primary* pane evicts past the cap
    t.feed(b"\x1b[?1049l");

    assert!(
        t.search("tag00").is_empty(),
        "premise: the rewrap must have evicted tag00"
    );
    assert_eq!(
        t.tracked_point(id),
        None,
        "released by the alt-active branch too, not clamped to 0"
    );
}
