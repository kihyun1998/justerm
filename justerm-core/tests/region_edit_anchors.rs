//! #162 — SU/SD/IL/DL (`scroll_region_lines`) must rotate the marker (#118/#158)
//! and selection (#3) anchors like `linefeed`/`reverse_index` already do. The
//! CSI line-editing verbs move primary content, so an anchor in the affected
//! region has to follow it or it points at the wrong line. Pre-existing gap
//! surfaced by #158's completeness pass; grounded against alacritty
//! `scroll_up_relative`/`scroll_down_relative` (selection + vi cursor) and
//! xterm.js marker `onInsert`/`onDelete`.

use justerm_core::{Engine, MarkerKind, MarkerPosition, SelectionSpan, SelectionType, Side};

/// Fill each of the `rows` screen rows with a distinct letter, cursor left at the
/// last row. (5 rows → a,b,c,d,e on rows 0..4.)
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

fn marker(id: justerm_core::MarkerId, row: usize) -> MarkerPosition {
    MarkerPosition {
        id,
        row,
        kind: MarkerKind::Plain,
    }
}

// ---- markers ---------------------------------------------------------------

/// SU (CSI S) scrolls the region up: content — and a marker anchored to it —
/// moves up one row.
#[test]
fn su_rotates_marker_up() {
    let mut t = filled(10, 5);
    let id = t.add_marker(2); // "c" at row 2
    t.feed(b"\x1b[1S"); // scroll up 1
    assert_eq!(t.frame().overlay.markers, vec![marker(id, 1)]);
}

/// SD (CSI T) scrolls the region down: the marker moves down one row.
#[test]
fn sd_rotates_marker_down() {
    let mut t = filled(10, 5);
    let id = t.add_marker(2);
    t.feed(b"\x1b[1T"); // scroll down 1
    assert_eq!(t.frame().overlay.markers, vec![marker(id, 3)]);
}

/// IL (CSI L) inserts blank lines at the cursor, scrolling `[cursor..=bottom]`
/// down. A marker below the cursor follows down; one above is untouched.
#[test]
fn il_rotates_marker_below_cursor_down() {
    let mut t = filled(10, 5);
    let above = t.add_marker(0); // "a", above the cursor
    let below = t.add_marker(3); // "d", below the cursor
    t.feed(b"\x1b[2;1H"); // cursor to row 1
    t.feed(b"\x1b[1L"); // insert 1 line at row 1
    let mut got = t.frame().overlay.markers;
    got.sort_by_key(|m| m.id.0);
    assert_eq!(got, vec![marker(above, 0), marker(below, 4)]);
}

/// DL (CSI M) deletes lines at the cursor, scrolling `[cursor..=bottom]` up. A
/// marker below the cursor moves up.
#[test]
fn dl_rotates_marker_below_cursor_up() {
    let mut t = filled(10, 5);
    let id = t.add_marker(3); // "d"
    t.feed(b"\x1b[2;1H"); // cursor to row 1
    t.feed(b"\x1b[1M"); // delete 1 line at row 1
    assert_eq!(t.frame().overlay.markers, vec![marker(id, 2)]);
}

/// A marker on the dropped edge leaves the buffer and is disposed (SU drops the
/// region top).
#[test]
fn su_disposes_marker_on_dropped_edge() {
    let mut t = filled(10, 5);
    t.add_marker(0); // region top — dropped by SU
    t.feed(b"\x1b[1S");
    assert!(t.frame().overlay.markers.is_empty());
}

/// Multi-line SU by N rotates by N (the single-line rotate composed N times).
#[test]
fn su_by_n_rotates_marker_by_n() {
    let mut t = filled(10, 5);
    let id = t.add_marker(3); // "d"
    t.feed(b"\x1b[2S"); // scroll up 2
    assert_eq!(t.frame().overlay.markers, vec![marker(id, 1)]);
}

/// Alt-screen SU/SD/IL/DL must NOT move primary markers — they anchor primary
/// content, which an alt-screen edit never touches (mirrors #158's guard).
#[test]
fn alt_screen_region_edit_leaves_primary_marker() {
    let mut t = filled(10, 5);
    let id = t.add_marker(2);
    t.feed(b"\x1b[?1049h"); // enter alt
    t.feed(b"\x1b[1S\x1b[1T\x1b[1L\x1b[1M"); // SU/SD/IL/DL on the alt screen
    t.feed(b"\x1b[?1049l"); // leave alt
    assert_eq!(t.frame().overlay.markers, vec![marker(id, 2)]);
}

// ---- selection (the identical pre-existing gap) ----------------------------

/// SU rotates a live selection up the same way — the selection is not left
/// pointing at the content that slid out from under it.
#[test]
fn su_rotates_selection_up() {
    let mut t = filled(10, 5);
    t.selection_begin(2, 0, Side::Left, SelectionType::Char);
    t.selection_extend(2, 3, Side::Right);
    t.feed(b"\x1b[1S");
    assert_eq!(
        t.frame().overlay.selection,
        vec![SelectionSpan {
            row: 1,
            left: 0,
            right: 3
        }]
    );
}

/// DL rotates the selection up from the cursor region too.
#[test]
fn dl_rotates_selection_up() {
    let mut t = filled(10, 5);
    t.selection_begin(3, 0, Side::Left, SelectionType::Char);
    t.selection_extend(3, 2, Side::Right);
    t.feed(b"\x1b[2;1H"); // cursor to row 1
    t.feed(b"\x1b[1M"); // delete 1 line
    assert_eq!(
        t.frame().overlay.selection,
        vec![SelectionSpan {
            row: 2,
            left: 0,
            right: 2
        }]
    );
}
