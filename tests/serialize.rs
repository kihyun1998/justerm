//! Issue #6 — binary wire format for a damage frame. The acceptance is a
//! round-trip: `decode(encode(frame)) == frame`, tested here through the public
//! API only (no engine/PTY/transport). Format spec: `docs/architecture.md`
//! §Serialization + ADR-0005.

use core::num::NonZeroU32;
use justerm::{Cell, CellFlags, Color, Frame, FrameKind, ScrollOp, Span, decode, encode};

/// Tracer bullet: an empty Partial frame (header only, no scroll, no spans)
/// round-trips. Proves the frame envelope encodes and decodes end-to-end.
#[test]
fn round_trip_empty_partial_frame() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![],
        side_table: vec![],
    };
    let bytes = encode(&frame);
    assert_eq!(decode(&bytes).expect("decode"), frame);
}

/// A Partial frame with one span of plain ASCII cells round-trips — exercises
/// the fixed-width cell record and span bodies.
#[test]
fn round_trip_span_of_plain_cells() {
    let cells: Vec<Cell> = "hi!".chars().map(|c| Cell { c, ..Cell::default() }).collect();
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span { line: 3, left: 10, right: 12, cells }],
        side_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// All three colour references round-trip and stay distinct — the format's
/// mandatory tag keeps `Default`, `Indexed(0)` and `Rgb(0,0,0)` from collapsing.
#[test]
fn round_trip_distinct_colour_references() {
    let mk = |fg, bg| Cell { c: 'x', fg, bg, ..Cell::default() };
    let cells = vec![
        mk(Color::Default, Color::Default),
        mk(Color::Indexed(0), Color::Indexed(255)),
        mk(Color::Rgb(0, 0, 0), Color::Rgb(1, 2, 3)),
    ];
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span { line: 0, left: 0, right: 2, cells }],
        side_table: vec![],
    };
    let d = decode(&encode(&frame)).expect("decode");
    assert_eq!(d, frame);
    let row = &d.spans[0].cells;
    assert_ne!(row[0].fg, row[1].fg, "Default must differ from Indexed(0)");
    assert_ne!(row[1].fg, row[2].fg, "Indexed(0) must differ from Rgb(0,0,0)");
}

/// SGR attributes *and* layout markers (wide-char lead + spacer) survive the
/// `flags` field — the consumer needs both halves of a wide glyph to render it.
#[test]
fn round_trip_cell_flags_incl_layout_markers() {
    let lead = Cell {
        c: '한',
        flags: CellFlags::BOLD | CellFlags::WIDE_CHAR,
        ..Cell::default()
    };
    let spacer = Cell {
        flags: CellFlags::WIDE_CHAR_SPACER,
        ..Cell::default()
    };
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span { line: 5, left: 0, right: 1, cells: vec![lead, spacer] }],
        side_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// Decode rejects malformed input instead of panicking — a consumer feeds bytes
/// straight off a transport, so the error path is part of the contract.
#[test]
fn decode_rejects_malformed_input() {
    assert!(decode(&[]).is_err(), "empty");
    assert!(decode(b"XXxxxxxxx").is_err(), "bad magic");
    assert!(decode(b"JT").is_err(), "truncated mid-header");
}

/// A cell whose `extra` references a combining-mark cluster round-trips: the
/// cell carries only a frame-local index, the code points live in `side_table`.
#[test]
fn round_trip_grapheme_side_table() {
    let accented = Cell {
        c: 'e',
        extra: NonZeroU32::new(1), // 1-based index into side_table[0]
        ..Cell::default()
    };
    let plain = Cell { c: 'x', ..Cell::default() };
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span { line: 0, left: 0, right: 1, cells: vec![accented, plain] }],
        side_table: vec![vec!['\u{0301}']], // combining acute accent
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// Acceptance #2: cells stay fixed-width — each added cell costs exactly 16
/// bytes, so a grapheme cell is no wider than a plain one (its cluster lives in
/// the side-table). Measured as the per-cell delta, independent of header size.
#[test]
fn cell_record_is_fixed_16_bytes() {
    let span_of = |n: usize| Frame {
        cols: 1,
        rows: 1,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: (n - 1) as u16,
            cells: vec![Cell::default(); n],
        }],
        side_table: vec![],
    };
    let one = encode(&span_of(1)).len();
    let two = encode(&span_of(2)).len();
    assert_eq!(two - one, 16, "each added cell must cost exactly 16 bytes");
}

/// A recorded scroll op round-trips. It is encoded ahead of the spans so the
/// decoder shifts rows before applying column damage (ADR-0003).
#[test]
fn round_trip_scroll_op() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: Some(ScrollOp { top: 0, bottom: 23, count: 3 }),
        spans: vec![],
        side_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// A `Full` frame round-trips its kind (resize / alt-clear redraw-everything).
#[test]
fn round_trip_full_frame_kind() {
    let frame = Frame {
        cols: 40,
        rows: 12,
        kind: FrameKind::Full,
        scroll: None,
        spans: vec![],
        side_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}
