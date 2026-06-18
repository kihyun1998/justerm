//! Issue #6 — binary wire format for a damage frame. The acceptance is a
//! round-trip: `decode(encode(frame)) == frame`, tested here through the public
//! API only (no engine/PTY/transport). Format spec: `docs/architecture.md`
//! §Serialization + ADR-0005.

use core::num::NonZeroU32;
use justerm::{Cell, CellFlags, Color, Engine, Frame, FrameKind, ScrollOp, Span, decode, encode};

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
        link_table: vec![],
    };
    let bytes = encode(&frame);
    assert_eq!(decode(&bytes).expect("decode"), frame);
}

/// A Partial frame with one span of plain ASCII cells round-trips — exercises
/// the fixed-width cell record and span bodies.
#[test]
fn round_trip_span_of_plain_cells() {
    let cells: Vec<Cell> = "hi!"
        .chars()
        .map(|c| Cell {
            c,
            ..Cell::default()
        })
        .collect();
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span {
            line: 3,
            left: 10,
            right: 12,
            cells,
        }],
        side_table: vec![],
        link_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// All three colour references round-trip and stay distinct — the format's
/// mandatory tag keeps `Default`, `Indexed(0)` and `Rgb(0,0,0)` from collapsing.
#[test]
fn round_trip_distinct_colour_references() {
    let mk = |fg, bg| Cell {
        c: 'x',
        fg,
        bg,
        ..Cell::default()
    };
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
        spans: vec![Span {
            line: 0,
            left: 0,
            right: 2,
            cells,
        }],
        side_table: vec![],
        link_table: vec![],
    };
    let d = decode(&encode(&frame)).expect("decode");
    assert_eq!(d, frame);
    let row = &d.spans[0].cells;
    assert_ne!(row[0].fg, row[1].fg, "Default must differ from Indexed(0)");
    assert_ne!(
        row[1].fg, row[2].fg,
        "Indexed(0) must differ from Rgb(0,0,0)"
    );
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
        spans: vec![Span {
            line: 5,
            left: 0,
            right: 1,
            cells: vec![lead, spacer],
        }],
        side_table: vec![],
        link_table: vec![],
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
    let plain = Cell {
        c: 'x',
        ..Cell::default()
    };
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: 1,
            cells: vec![accented, plain],
        }],
        side_table: vec![vec!['\u{0301}']], // combining acute accent
        link_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// Acceptance #2: cells stay fixed-width — each added cell costs exactly 18
/// bytes (16 + the v2 hyperlink `link` u16, #26), so a grapheme or linked cell
/// is no wider than a plain one (cluster/URI live in the side-tables). Measured
/// as the per-cell delta, independent of header size.
#[test]
fn cell_record_is_fixed_18_bytes() {
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
        link_table: vec![],
    };
    let one = encode(&span_of(1)).len();
    let two = encode(&span_of(2)).len();
    assert_eq!(two - one, 18, "each added cell must cost exactly 18 bytes");
}

/// A recorded scroll op round-trips. It is encoded ahead of the spans so the
/// decoder shifts rows before applying column damage (ADR-0003).
#[test]
fn round_trip_scroll_op() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: Some(ScrollOp {
            top: 0,
            bottom: 23,
            count: 3,
        }),
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
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
        link_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

// ===========================================================================
// Engine -> Frame producer (#6 next slice): build a Frame from live engine
// state (damage + grid + grapheme pool), remapping the global side-table to
// frame-local indices.
// ===========================================================================

/// Tracer: a fresh engine fed "hi" produces a Partial frame whose line-0 spans
/// carry the written cells.
#[test]
fn engine_frame_captures_written_cells() {
    let mut term = Engine::new(5, 2);
    term.feed(b"hi");
    let f = term.frame();
    assert_eq!((f.cols, f.rows), (5, 2));
    assert_eq!(f.kind, FrameKind::Partial);
    let chars: String = f
        .spans
        .iter()
        .filter(|s| s.line == 0)
        .flat_map(|s| s.cells.iter().map(|c| c.c))
        .collect();
    assert!(chars.contains('h') && chars.contains('i'), "got {chars:?}");
}

/// The recorded scroll op reaches the frame (content scrolled off the top).
#[test]
fn engine_frame_carries_scroll_op() {
    let mut term = Engine::new(5, 2);
    term.feed(b"x\r\ny\r\nz"); // 3 lines into a 2-row screen -> one scroll
    assert!(term.frame().scroll.is_some());
}

/// After a resize the frame is Full and ships every row at the new dimensions.
#[test]
fn engine_frame_is_full_after_resize() {
    let mut term = Engine::new(5, 2);
    term.feed(b"hi");
    term.resize(6, 3);
    let f = term.frame();
    assert_eq!(f.kind, FrameKind::Full);
    assert_eq!((f.cols, f.rows), (6, 3));
    assert_eq!(f.spans.len(), 3, "Full ships every row");
}

/// Trap #2: a frame ships only *referenced* clusters, renumbered frame-local —
/// an orphaned global pool entry is not shipped and the live one is re-indexed.
#[test]
fn engine_frame_remaps_orphaned_global_index() {
    let mut term = Engine::new(5, 1);
    term.feed("e\u{0301}".as_bytes()); // pool[0]: cell0 -> global index 1
    term.feed(b"\rx"); // CR to col0, overwrite 'e' with 'x' -> orphans pool[0]
    term.feed("o\u{0308}".as_bytes()); // pool[1]: cell1 -> global index 2
    let f = term.frame();
    assert_eq!(
        f.side_table,
        vec![vec!['\u{0308}']],
        "only the live cluster ships"
    );
    let g = f
        .spans
        .iter()
        .flat_map(|s| &s.cells)
        .find(|c| c.extra.is_some())
        .unwrap();
    assert_eq!(g.c, 'o');
    assert_eq!(
        g.extra.unwrap().get(),
        1,
        "global index 2 remapped to frame-local 1"
    );
}

/// Integration: feed colours + a wide glyph + a combining mark, then the live
/// engine's frame survives a full encode/decode round-trip.
#[test]
fn engine_frame_round_trips_through_bytes() {
    let mut term = Engine::new(8, 1);
    term.feed("\x1b[31m한e\u{0301}".as_bytes());
    let f = term.frame();
    assert_eq!(decode(&encode(&f)).expect("decode"), f);
}

/// Integration on real captured streams (the #20 dogfood fixtures): after
/// replaying a real app, the live engine's frame round-trips through the wire
/// format — exercising colours, wide glyphs, scroll, and full-screen content.
#[test]
fn engine_frame_round_trips_real_captures() {
    for raw in [
        include_bytes!("fixtures/vim_redraw.raw").as_slice(),
        include_bytes!("fixtures/top.raw").as_slice(),
        include_bytes!("fixtures/htop.raw").as_slice(),
    ] {
        let mut term = Engine::new(80, 24);
        term.feed(raw);
        let f = term.frame();
        assert_eq!(
            decode(&encode(&f)).expect("decode"),
            f,
            "real-capture round-trip"
        );
    }
}

/// A crafted frame whose span has left > right must be a clean error, not a
/// u16-underflow panic — decode consumes untrusted bytes off a transport.
#[test]
fn decode_rejects_span_with_left_past_right() {
    let mut b = Vec::new();
    b.extend_from_slice(b"JT"); // magic
    b.push(1); // version
    b.push(0); // scroll flag
    b.push(1); // kind = Partial
    b.extend_from_slice(&80u16.to_le_bytes()); // cols
    b.extend_from_slice(&24u16.to_le_bytes()); // rows
    b.extend_from_slice(&1u16.to_le_bytes()); // span count
    b.extend_from_slice(&0u16.to_le_bytes()); // line
    b.extend_from_slice(&5u16.to_le_bytes()); // left = 5
    b.extend_from_slice(&0u16.to_le_bytes()); // right = 0  (< left!)
    assert!(decode(&b).is_err(), "left>right must error, not panic");
}

/// A Full frame carrying real cells (not just the kind flag) round-trips.
#[test]
fn round_trip_full_frame_with_cells() {
    let row = |line| Span {
        line,
        left: 0,
        right: 2,
        cells: "abc"
            .chars()
            .map(|c| Cell {
                c,
                ..Cell::default()
            })
            .collect(),
    };
    let frame = Frame {
        cols: 3,
        rows: 2,
        kind: FrameKind::Full,
        scroll: None,
        spans: vec![row(0), row(1)],
        side_table: vec![],
        link_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// A downward scroll (negative count, e.g. RI at the top margin) round-trips —
/// the count is signed on the wire.
#[test]
fn round_trip_negative_scroll_count() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: Some(ScrollOp {
            top: 2,
            bottom: 23,
            count: -4,
        }),
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// Trap #6: with nothing damaged since the ack, the engine yields an *empty
/// Partial* frame (0 spans, no scroll) — not Full and not "no frame" — so the
/// consumer can ack without redrawing.
#[test]
fn engine_frame_undamaged_is_empty_partial_not_full() {
    let mut term = Engine::new(5, 2);
    term.feed(b"hi");
    term.reset_damage(); // consumer applied + ack'd the frame
    let f = term.frame();
    assert_eq!(f.kind, FrameKind::Partial);
    assert!(f.spans.is_empty(), "no damage since ack -> no spans");
    assert!(f.scroll.is_none());
}
