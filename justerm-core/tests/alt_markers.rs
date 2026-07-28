//! #187 (S1) — alt-screen decoration marker lifecycle. With per-buffer marker
//! storage (#186) the alt guards come out: `add_marker` on the alt screen now
//! anchors an *alt-scoped* marker that rides the alt frame, rotates with an
//! alt-screen region scroll, and is disposed on alt-leave — the primary buffer's
//! markers untouched throughout. Supersedes the #164 decline-on-alt behavior.
//! Grounded against xterm `BufferSet` (per-buffer `markers`, `clearAllMarkers`).

use justerm_core::{Engine, MarkerKind, MarkerPosition, TermEvent, decode, encode};

fn plain(id: justerm_core::MarkerId, row: usize) -> MarkerPosition {
    MarkerPosition {
        id,
        row,
        kind: MarkerKind::Plain,
    }
}

/// Fill 5 rows a..e, cursor at the last row.
fn filled() -> Engine {
    let mut t = Engine::new(10, 5);
    t.feed(b"a\r\nb\r\nc\r\nd\r\ne");
    t
}

/// `add_marker` on the alt screen creates an alt-scoped marker that rides the
/// *alt* frame's `marker_positions` (no longer declined).
#[test]
fn add_marker_on_alt_rides_the_alt_frame() {
    let mut t = filled();
    t.feed(b"\x1b[?1049h"); // enter alt
    let id = t.add_marker(2);

    assert_eq!(t.frame().overlay.markers, vec![plain(id, 2)]);
}

/// Leaving the alt screen disposes the alt marker (fires `MarkerDisposed`), and
/// nothing pins the primary buffer afterwards.
#[test]
fn alt_marker_disposed_on_leave_primary_stays_clean() {
    let mut t = filled();
    t.feed(b"\x1b[?1049h");
    let id = t.add_marker(2);
    t.drain_events();

    t.feed(b"\x1b[?1049l"); // leave alt

    assert!(
        t.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "alt marker disposed on leave"
    );
    assert!(
        t.frame().overlay.markers.is_empty(),
        "no marker pinned on the primary after leave"
    );
}

/// A primary marker is untouched by an alt excursion that adds (and drops) its
/// own alt marker — the two lists are isolated (the aliasing #164 guarded).
#[test]
fn primary_marker_survives_alt_marker_excursion() {
    let mut t = filled();
    let pid = t.add_marker(1); // primary marker on "b"
    t.feed(b"\x1b[?1049h");
    let _aid = t.add_marker(3); // alt-scoped marker
    t.feed(b"\x1b[?1049l");

    assert_eq!(
        t.frame().overlay.markers,
        vec![plain(pid, 1)],
        "only the primary marker remains, at its row"
    );
}

/// An alt-screen region scroll (SU) rotates the alt marker with the content —
/// the `markers_rotate_region` alt guard is gone.
#[test]
fn alt_marker_rotates_on_alt_screen_scroll() {
    let mut t = Engine::new(10, 5);
    t.feed(b"\x1b[?1049h");
    t.feed(b"A\r\nB\r\nC\r\nD\r\nE"); // fill the alt screen
    let id = t.add_marker(2);

    t.feed(b"\x1b[1S"); // scroll up 1 → marker row 2 → row 1

    assert_eq!(t.frame().overlay.markers, vec![plain(id, 1)]);
}

/// An alt marker on the dropped edge of an alt-screen scroll leaves the buffer
/// and is disposed (same edge-dispose as a primary marker).
#[test]
fn alt_marker_on_dropped_edge_is_disposed() {
    let mut t = Engine::new(10, 5);
    t.feed(b"\x1b[?1049h");
    t.feed(b"A\r\nB\r\nC\r\nD\r\nE");
    let id = t.add_marker(0); // row 0 = the edge SU drops
    t.drain_events();

    t.feed(b"\x1b[1S"); // SU drops row 0

    assert!(t.drain_events().contains(&TermEvent::MarkerDisposed(id)));
    assert!(t.frame().overlay.markers.is_empty());
}

/// DoD ④ wire proof: an alt frame carrying a marker survives encode→decode
/// (ADR-0005) — the alt marker rides the wire like a primary one.
#[test]
fn alt_marker_survives_wire_roundtrip() {
    let mut t = filled();
    t.feed(b"\x1b[?1049h");
    let id = t.add_marker(2);

    let frame = t.frame();
    assert_eq!(frame.overlay.markers, vec![plain(id, 2)]);
    assert_eq!(
        decode(&encode(&frame)).expect("decode"),
        frame,
        "alt frame's marker round-trips through the wire"
    );
}

/// An alt marker reflows with the alt content on a column-width resize. justerm
/// column-reflows the alt grid (unlike xterm, which disables alt reflow), so an
/// alt marker must ride that reflow or it drifts off its content. (#187
/// completeness pass — both lenses flagged this as the one gap.)
#[test]
fn an_alt_marker_stays_put_because_its_content_does() {
    // This test used to assert the opposite — that the marker followed its content *up* as the line
    // unwrapped — and it was the only thing in the workspace that depended on the alt screen being
    // reflowed at all. That was never a decision: `8f09d58` needed both screens to end up at the new
    // *dimensions*, reached for the `reflow_pane` helper, and re-splitting came along with it. #187
    // then built on the side effect; its own acceptance criteria are five lifecycle items and
    // mention neither resize nor reflow.
    //
    // The alt screen no longer reflows (#567), so `"abcdefgh"` keeps its wrap at 8 columns until the
    // application repaints — and a marker that followed content which did not move would be the
    // defect. What #187 actually built (alt-scoped creation, rotation on alt scroll, disposal on
    // alt-leave) is untouched and still covered by the tests around this one.
    let mut t = Engine::new(4, 4);
    t.feed(b"\x1b[?1049h");
    t.feed(b"abcdefgh"); // 4 cols: row0 "abcd"(wrap) → row1 "efgh"
    let id = t.add_marker(1); // marker on the "efgh" row
    assert_eq!(t.frame().overlay.markers, vec![plain(id, 1)]);

    t.resize(8, 4); // widen — the alt grid re-fits its rows, it does not re-split them

    assert_eq!(
        t.frame().overlay.markers,
        vec![plain(id, 1)],
        "the content stayed on row 1, so the marker did too"
    );
    assert!(
        t.grid().is_row_wrapped(0),
        "and the row still claims its wrap: nothing re-split it"
    );
}

/// A normal primary-screen `add_marker` still registers (the change is alt-only).
#[test]
fn add_marker_on_primary_still_registers() {
    let mut t = Engine::new(10, 5);
    t.feed(b"a\r\nb\r\nc");

    let _id = t.add_marker(1);

    assert_eq!(t.frame().overlay.markers.len(), 1);
}
