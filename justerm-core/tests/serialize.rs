//! Issue #6 — binary wire format for a damage frame. The acceptance is a
//! round-trip: `decode(encode(frame)) == frame`, tested here through the public
//! API only (no engine/PTY/transport). Format spec: `docs/architecture.md`
//! §Serialization + ADR-0005.

use core::num::NonZeroU32;
use justerm_core::{
    Cell, CellFlags, Color, Engine, Frame, FrameKind, MarkerId, MarkerPosition, Overlay, ScrollOp,
    SelectionSpan, Span, decode, encode,
};
use std::collections::BTreeMap;

/// Tracer bullet: an empty Partial frame (header only, no scroll, no spans)
/// round-trips. Proves the frame envelope encodes and decodes end-to-end.
#[test]
fn round_trip_empty_partial_frame() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    let bytes = encode(&frame);
    assert_eq!(decode(&bytes).expect("decode"), frame);
}

/// #108: the overlay section round-trips both groups — selection spans and
/// search-match spans — as viewport `(row, left, right)` triples. Positions
/// only (no colour); the consumer resolves highlight colour.
#[test]
fn round_trip_overlay_selection_and_match_spans() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Overlay {
            selection: vec![
                SelectionSpan {
                    row: 0,
                    left: 2,
                    right: 7,
                },
                SelectionSpan {
                    row: 1,
                    left: 0,
                    right: 4,
                },
            ],
            matches: vec![SelectionSpan {
                row: 3,
                left: 10,
                right: 12,
            }],
            markers: vec![],
        },
    };
    let bytes = encode(&frame);
    assert_eq!(decode(&bytes).expect("decode"), frame);
}

/// #118: the overlay's third group — marker positions — round-trips as
/// `(marker_id, row)` pairs (a different record shape from the span groups).
#[test]
fn round_trip_overlay_marker_positions() {
    let mut frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Overlay::default(),
    };
    frame.overlay.markers = vec![
        MarkerPosition {
            id: MarkerId(5),
            row: 3,
        },
        MarkerPosition {
            id: MarkerId(99),
            row: 0,
        },
    ];
    let bytes = encode(&frame);
    assert_eq!(decode(&bytes).expect("decode"), frame);
}

/// Scroll position (display_offset + scrollback_len) survives the wire round-trip
/// (#112 / ADR-0013, wire v5). It rides in the header like the cursor, for the
/// consumer's scrollbar.
#[test]
fn round_trip_scroll_position() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 7,
        scrollback_len: 250,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded.display_offset, 7);
    assert_eq!(decoded.scrollback_len, 250);
    assert_eq!(decoded, frame);
}

/// Cursor position + visibility survive the wire round-trip (#38). The cursor
/// moves with almost every frame, so it rides in the frame header alongside
/// `cols`/`rows` rather than in a span.
#[test]
fn round_trip_cursor_position_and_visibility() {
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 9,
        cursor_col: 19,
        cursor_visible: false,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// A Partial frame with one span of plain ASCII cells round-trips — exercises
/// the fixed-width cell record and span bodies.
#[test]
fn round_trip_span_of_plain_cells() {
    let cells: Vec<Cell> = "hi!"
        .chars()
        .map(|c| Cell::from_parts(c, Color::Default, Color::Default, CellFlags::empty()))
        .collect();
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![Span {
            line: 3,
            left: 10,
            right: 12,
            cells,
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
        }],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    assert_eq!(decode(&encode(&frame)).expect("decode"), frame);
}

/// All three colour references round-trip and stay distinct — the format's
/// mandatory tag keeps `Default`, `Indexed(0)` and `Rgb(0,0,0)` from collapsing.
#[test]
fn round_trip_distinct_colour_references() {
    let mk = |fg, bg| Cell::from_parts('x', fg, bg, CellFlags::empty());
    let cells = vec![
        mk(Color::Default, Color::Default),
        mk(Color::Indexed(0), Color::Indexed(255)),
        mk(Color::Rgb(0, 0, 0), Color::Rgb(1, 2, 3)),
    ];
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: 2,
            cells,
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
        }],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    let d = decode(&encode(&frame)).expect("decode");
    assert_eq!(d, frame);
    let row = &d.spans[0].cells;
    assert_ne!(
        row[0].fg(),
        row[1].fg(),
        "Default must differ from Indexed(0)"
    );
    assert_ne!(
        row[1].fg(),
        row[2].fg(),
        "Indexed(0) must differ from Rgb(0,0,0)"
    );
}

/// SGR attributes *and* layout markers (wide-char lead + spacer) survive the
/// `flags` field — the consumer needs both halves of a wide glyph to render it.
#[test]
fn round_trip_cell_flags_incl_layout_markers() {
    let lead = Cell::from_parts(
        '한',
        Color::Default,
        Color::Default,
        CellFlags::BOLD | CellFlags::WIDE_CHAR,
    );
    let spacer = Cell::from_parts(
        ' ',
        Color::Default,
        Color::Default,
        CellFlags::WIDE_CHAR_SPACER,
    );
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![Span {
            line: 5,
            left: 0,
            right: 1,
            cells: vec![lead, spacer],
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
        }],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
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

/// A buffer from a superseded wire version fails loudly with `BadVersion`,
/// never a silent misparse — a consumer pinned to the old encoder is rejected
/// at the version gate (#38 bumped `VERSION` 2 -> 3).
#[test]
fn decode_rejects_superseded_version() {
    let frame = Frame {
        cols: 1,
        rows: 1,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    let mut bytes = encode(&frame);
    bytes[2] = 2; // the VERSION byte sits right after the 2-byte magic
    assert!(matches!(
        decode(&bytes),
        Err(justerm_core::DecodeError::BadVersion(2))
    ));
}

/// The wire is gated at version 7 (the #118 overlay marker group, atop the #108
/// section). Both the exported `WIRE_VERSION` constant and the byte the encoder
/// emits must read 7 — the value the WASM decoder's `wire_version()` mirrors in
/// lockstep (ADR-0008), so a drift here trips before it can desync a binding.
#[test]
fn wire_version_is_seven() {
    assert_eq!(justerm_core::WIRE_VERSION, 7);
    let mut term = Engine::new(1, 1);
    term.feed(b"x");
    let bytes = encode(&term.frame());
    assert_eq!(bytes[2], justerm_core::WIRE_VERSION); // VERSION byte after magic
}

/// A combining-mark cluster round-trips: the cell carries only its combining bit,
/// the frame-local index rides on the span's `combining` map, and the code points
/// live in `side_table`.
#[test]
fn round_trip_grapheme_side_table() {
    let mut accented = Cell::from_parts('e', Color::Default, Color::Default, CellFlags::empty());
    accented.set_combined(true);
    let plain = Cell::from_parts('x', Color::Default, Color::Default, CellFlags::empty());
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: 1,
            cells: vec![accented, plain],
            // column 0 -> side_table[0] (1-based index)
            combining: BTreeMap::from([(0, NonZeroU32::new(1).unwrap())]),
            links: BTreeMap::new(),
        }],
        side_table: vec![vec!['\u{0301}']], // combining acute accent
        link_table: vec![],
        overlay: Default::default(),
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
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: (n - 1) as u16,
            cells: vec![Cell::default(); n],
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
        }],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
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
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: Some(ScrollOp {
            top: 0,
            bottom: 23,
            count: 3,
        }),
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
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
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
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
        .flat_map(|s| s.cells.iter().map(|c| c.c()))
        .collect();
    assert!(chars.contains('h') && chars.contains('i'), "got {chars:?}");
}

/// The frame reports the live cursor position (#38). The engine only exposes
/// where the cursor is; *drawing* it stays the consumer's renderer adapter.
#[test]
fn engine_frame_reports_cursor_position() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[10;20H"); // CUP to row 10, col 20 (1-based) -> (9, 19)
    let f = term.frame();
    assert_eq!((f.cursor_row, f.cursor_col), (9, 19));
    assert!(f.cursor_visible, "cursor is visible by default");
}

/// Cursor visibility follows DECTCEM (`?25l` hides, `?25h` shows) — the frame
/// reflects the engine's hide/show so the consumer can stop drawing the caret.
#[test]
fn engine_frame_reports_cursor_visibility_via_dectcem() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?25l"); // DECTCEM hide
    assert!(!term.frame().cursor_visible, "hidden after ?25l");
    term.feed(b"\x1b[?25h"); // DECTCEM show
    assert!(term.frame().cursor_visible, "visible again after ?25h");
}

/// DECTCEM visibility is a standalone mode, *not* part of the alt-screen
/// (`?1049`) cursor save/restore — matches xterm/alacritty, where `?25` is a
/// Term mode never carried by the alt grid swap. So hiding the cursor *on* the
/// alt screen persists after leaving it, and showing it persists too. (#38: the
/// alt path used to restore the whole saved `Cursor` incl. `visible`, wrongly
/// resurrecting / re-hiding the caret.)
#[test]
fn engine_cursor_visibility_is_independent_of_alt_screen() {
    // Hide on alt → stays hidden after leaving.
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1049h\x1b[?25l\x1b[?1049l");
    assert!(
        !term.frame().cursor_visible,
        "?25l on alt must persist after ?1049l"
    );

    // Hidden before alt, shown on alt → stays shown after leaving.
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?25l\x1b[?1049h\x1b[?25h\x1b[?1049l");
    assert!(
        term.frame().cursor_visible,
        "?25h on alt must persist after ?1049l"
    );
}

/// Interaction (cursor × resize): shrinking the screen below the cursor's old
/// position must not panic when the next frame folds cursor-move damage, and the
/// reported cursor is clamped into the new bounds. (#38 adversarial)
#[test]
fn engine_frame_cursor_survives_resize_shrink() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[20;70H"); // cursor to (19, 69)
    term.reset_damage(); // prev_cursor = (19, 69)
    term.resize(10, 5); // shrink well below the old cursor
    let f = term.frame(); // must not panic
    assert!(
        f.cursor_row < 5 && f.cursor_col < 10,
        "cursor clamped to new bounds, got ({}, {})",
        f.cursor_row,
        f.cursor_col
    );
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

/// Trap #2: a frame ships only *live* clusters, indexed frame-local. Overwriting
/// a combined cell clears its bit, so its (now stale) row-map entry is never
/// gathered — only the surviving combined cell contributes to `side_table`, at
/// frame-local index 1.
#[test]
fn engine_frame_ships_only_live_combining_clusters() {
    let mut term = Engine::new(5, 1);
    term.feed("e\u{0301}".as_bytes()); // col0 'e' + acute -> combined
    term.feed(b"\rx"); // CR to col0, overwrite 'e' with 'x' -> col0 bit cleared
    term.feed("o\u{0308}".as_bytes()); // col1 'o' + diaeresis -> combined
    let f = term.frame();
    assert_eq!(
        f.side_table,
        vec![vec!['\u{0308}']],
        "only the live cluster ships"
    );
    // Exactly one span column carries combining; it is the 'o', at frame-local 1.
    let span = f
        .spans
        .iter()
        .find(|s| !s.combining.is_empty())
        .expect("a span with combining");
    let (&col, idx) = span.combining.iter().next().unwrap();
    assert_eq!(idx.get(), 1, "the live cluster is frame-local index 1");
    assert_eq!(span.cells[col].c(), 'o');
    assert!(span.cells[col].is_combined());
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

/// A span with `left=0, right=65535` is a valid `right >= left`, but the run
/// length `right - left + 1` overflows `u16` (65535 + 1). decode must widen
/// before the arithmetic and return a typed error (the truncated buffer can't
/// supply 65536 cells), never panic. Found by `cargo fuzz run serialize` (#33).
#[test]
fn decode_rejects_span_length_u16_overflow() {
    let mut b = Vec::new();
    b.extend_from_slice(b"JT"); // magic
    b.push(1); // version
    b.push(0); // scroll flag
    b.push(1); // kind = Partial
    b.extend_from_slice(&80u16.to_le_bytes()); // cols
    b.extend_from_slice(&24u16.to_le_bytes()); // rows
    b.extend_from_slice(&1u16.to_le_bytes()); // span count
    b.extend_from_slice(&0u16.to_le_bytes()); // line
    b.extend_from_slice(&0u16.to_le_bytes()); // left = 0
    b.extend_from_slice(&65535u16.to_le_bytes()); // right = 65535 -> len overflows u16
    assert!(
        decode(&b).is_err(),
        "u16-overflowing span length must error, not panic"
    );
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
            .map(|c| Cell::from_parts(c, Color::Default, Color::Default, CellFlags::empty()))
            .collect(),
        combining: BTreeMap::new(),
        links: BTreeMap::new(),
    };
    let frame = Frame {
        cols: 3,
        rows: 2,
        kind: FrameKind::Full,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: None,
        spans: vec![row(0), row(1)],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
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
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        scroll: Some(ScrollOp {
            top: 2,
            bottom: 23,
            count: -4,
        }),
        spans: vec![],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
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
