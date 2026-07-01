//! JS-boundary tests for the WASM decoder (#34 S3/AC2), run with
//! `wasm-pack test --node`.
//!
//! The pure `flatten` logic is unit-tested in `lib.rs` with plain `cargo test`;
//! these verify the wasm-bindgen layer that unit tests cannot reach — that the
//! typed-array column views carry the right values across the JS boundary,
//! malformed input throws, and the WASM build decodes identically to the native
//! path.
//!
//! wasm32-only: these call `js_sys` (typed-array views over WASM memory), which
//! panics off-wasm. The crate-level cfg makes a native `cargo test --workspace`
//! skip this file (compiling it to nothing) while `wasm-pack test --node` still
//! runs it on the wasm32 target.
#![cfg(target_arch = "wasm32")]

use justerm_core::{Cell, CellFlags, Color, Frame, FrameKind, Span, encode_color};
use justerm_wasm_decode::{build_palette, decode_frame, wire_version};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

// Cross-import the hand-written JS colour helpers so the parity tests below
// check them against the Rust encoder (the single mirror's safety net, #36).
#[wasm_bindgen(module = "/js/colors.js")]
extern "C" {
    #[wasm_bindgen(js_name = resolveRgb)]
    fn resolve_rgb(reference: u32, palette: &js_sys::Object, role: u32) -> u32;
    #[wasm_bindgen(js_name = decodeColorRef)]
    fn decode_color_ref(reference: u32) -> JsValue;
}

fn get_str(v: &JsValue, key: &str) -> String {
    js_sys::Reflect::get(v, &key.into())
        .unwrap()
        .as_string()
        .unwrap()
}

fn get_num(v: &JsValue, key: &str) -> f64 {
    js_sys::Reflect::get(v, &key.into())
        .unwrap()
        .as_f64()
        .unwrap()
}

/// A Partial frame: "hi" at row 0 col 0, "abc" at row 1 col 5.
fn sample_frame() -> Frame {
    let span = |line: u16, left: u16, s: &str| Span {
        cells: s
            .chars()
            .map(|c| Cell::from_parts(c, Color::Default, Color::Default, CellFlags::empty()))
            .collect(),
        line,
        left,
        right: left + s.chars().count() as u16 - 1,
        combining: Default::default(),
        links: Default::default(),
    };
    Frame {
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
        mouse_events: Default::default(),
        alt_screen: false,
        scroll: None,
        spans: vec![span(0, 0, "hi"), span(1, 5, "abc")],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    }
}

#[wasm_bindgen_test]
fn wire_version_is_nine() {
    assert_eq!(wire_version(), 9); // #149 bumped 8 -> 9 for the alt-screen flag
}

#[wasm_bindgen_test]
fn alt_screen_flag_crosses_the_boundary() {
    let mut frame = sample_frame();
    frame.alt_screen = true;
    let df = decode_frame(&justerm_core::encode(&frame)).expect("decode");
    assert!(df.alt_screen()); // #149: the a11y announce policy gates on this
}

#[wasm_bindgen_test]
fn mouse_wanted_events_crosses_the_boundary() {
    use justerm_core::MouseEvents;
    let mut frame = sample_frame();
    frame.mouse_events = MouseEvents::DOWN | MouseEvents::UP | MouseEvents::WHEEL;
    let df = decode_frame(&justerm_core::encode(&frame)).expect("decode");
    assert_eq!(df.mouse_wanted_events(), frame.mouse_events.bits());
    assert!(df.mouse_wanted_events() & MouseEvents::WHEEL.bits() != 0); // wheel routes to app
}

#[wasm_bindgen_test]
fn decode_frame_exposes_scroll_position() {
    let mut frame = sample_frame();
    frame.display_offset = 7;
    frame.scrollback_len = 250;
    let df = decode_frame(&justerm_core::encode(&frame)).expect("decode");
    assert_eq!(df.display_offset(), 7);
    assert_eq!(df.scrollback_len(), 250);
}

#[wasm_bindgen_test]
fn decode_frame_exposes_cursor_scalars() {
    let mut frame = sample_frame();
    frame.cursor_row = 9;
    frame.cursor_col = 19;
    frame.cursor_visible = false;
    frame.cursor_shape = justerm_core::CursorShape::Bar;
    frame.cursor_blink = true;
    let df = decode_frame(&justerm_core::encode(&frame)).expect("decode");
    assert_eq!(df.cursor_row(), 9);
    assert_eq!(df.cursor_col(), 19);
    assert!(!df.cursor_visible());
    assert_eq!(df.cursor_shape(), 2); // Bar (#81)
    assert!(df.cursor_blink());
}

#[wasm_bindgen_test]
fn decode_frame_exposes_scalars() {
    let bytes = justerm_core::encode(&sample_frame());
    let df = decode_frame(&bytes).expect("decode");
    assert_eq!(df.cols(), 80);
    assert_eq!(df.rows(), 24);
    assert_eq!(df.kind(), 1); // Partial
    assert!(!df.has_scroll());
}

#[wasm_bindgen_test]
fn soa_columns_carry_values_across_the_boundary() {
    let bytes = justerm_core::encode(&sample_frame());
    let df = decode_frame(&bytes).expect("decode");
    let cp = df.codepoints();
    // 2 + 3 cells, one entry per cell.
    assert_eq!(cp.length(), 5);
    assert_eq!(cp.get_index(0), 'h' as u32);
    assert_eq!(cp.get_index(1), 'i' as u32);
    assert_eq!(cp.get_index(2), 'a' as u32); // start of "abc"
    // Every column is one-per-cell and reaches JS as its own typed array.
    assert_eq!(df.fg().length(), 5);
    assert_eq!(df.bg().length(), 5);
    assert_eq!(df.flags().length(), 5);
    assert_eq!(df.extra().length(), 5);
    assert_eq!(df.link().length(), 5);
}

#[wasm_bindgen_test]
fn colour_and_flag_columns_carry_tagged_values() {
    let cell = Cell::from_parts('A', Color::Indexed(9), Color::Default, CellFlags::BOLD);
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
        mouse_events: Default::default(),
        alt_screen: false,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: 0,
            cells: vec![cell],
            combining: Default::default(),
            links: Default::default(),
        }],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };
    let bytes = justerm_core::encode(&frame);
    let df = decode_frame(&bytes).expect("decode");
    // fg column carries the tagged-u32 colour ref: Indexed(9) = (1 << 24) | 9.
    assert_eq!(df.fg().get_index(0), (1 << 24) | 9);
    // flags column carries the raw CellFlags bits.
    assert_eq!(df.flags().get_index(0), CellFlags::BOLD.bits());
}

#[wasm_bindgen_test]
fn span_directory_view_maps_cells_to_rows() {
    let bytes = justerm_core::encode(&sample_frame());
    let df = decode_frame(&bytes).expect("decode");
    let spans = df.spans();
    assert_eq!(spans.length(), 2 * 5);
    // span 0: line 0, left 0, right 1, offset 0, count 2
    assert_eq!(spans.get_index(0), 0);
    assert_eq!(spans.get_index(1), 0);
    assert_eq!(spans.get_index(2), 1);
    assert_eq!(spans.get_index(3), 0);
    assert_eq!(spans.get_index(4), 2);
    // span 1: line 1, left 5, right 7, offset 2, count 3
    assert_eq!(spans.get_index(5), 1);
    assert_eq!(spans.get_index(6), 5);
    assert_eq!(spans.get_index(8), 2);
    assert_eq!(spans.get_index(9), 3);
}

#[wasm_bindgen_test]
fn overlay_span_views_cross_the_boundary() {
    use justerm_core::{Overlay, SelectionSpan};
    let mut frame = sample_frame();
    frame.overlay = Overlay {
        selection: vec![SelectionSpan {
            row: 0,
            left: 2,
            right: 7,
        }],
        matches: vec![
            SelectionSpan {
                row: 1,
                left: 0,
                right: 3,
            },
            SelectionSpan {
                row: 4,
                left: 9,
                right: 9,
            },
        ],
        markers: vec![],
    };
    let df = decode_frame(&justerm_core::encode(&frame)).expect("decode");

    // selectionSpans: one (row, left, right) triple.
    let sel = df.selection_spans();
    assert_eq!(sel.length(), 3);
    assert_eq!(sel.get_index(0), 0);
    assert_eq!(sel.get_index(1), 2);
    assert_eq!(sel.get_index(2), 7);

    // matchSpans: two triples, flat.
    let m = df.match_spans();
    assert_eq!(m.length(), 6);
    assert_eq!(m.get_index(0), 1);
    assert_eq!(m.get_index(2), 3);
    assert_eq!(m.get_index(3), 4);
    assert_eq!(m.get_index(5), 9);
}

#[wasm_bindgen_test]
fn marker_position_view_crosses_the_boundary() {
    use justerm_core::{MarkerId, MarkerPosition};
    let mut frame = sample_frame();
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
    let df = decode_frame(&justerm_core::encode(&frame)).expect("decode");

    let m = df.marker_positions();
    assert_eq!(m.length(), 4); // two (id, row) pairs
    assert_eq!(m.get_index(0), 5); // id
    assert_eq!(m.get_index(1), 3); // row
    assert_eq!(m.get_index(2), 99);
    assert_eq!(m.get_index(3), 0);
}

#[wasm_bindgen_test]
fn decode_frame_throws_on_bad_magic() {
    assert!(decode_frame(b"\x00\x00\x00").is_err());
}

#[wasm_bindgen_test]
fn decode_frame_throws_on_truncated() {
    let mut bytes = justerm_core::encode(&sample_frame());
    bytes.truncate(bytes.len() - 4); // chop into the last cell record
    assert!(decode_frame(&bytes).is_err());
}

// --- #36: colour-helper parity (Rust encode_color = source of truth) ---

#[wasm_bindgen_test]
fn decode_color_ref_matches_rust_encoding() {
    let v = decode_color_ref(encode_color(Color::Default));
    assert_eq!(get_str(&v, "kind"), "default");

    let v = decode_color_ref(encode_color(Color::Indexed(200)));
    assert_eq!(get_str(&v, "kind"), "indexed");
    assert_eq!(get_num(&v, "index"), 200.0);

    let v = decode_color_ref(encode_color(Color::Rgb(10, 20, 30)));
    assert_eq!(get_str(&v, "kind"), "rgb");
    assert_eq!(get_num(&v, "r"), 10.0);
    assert_eq!(get_num(&v, "g"), 20.0);
    assert_eq!(get_num(&v, "b"), 30.0);
}

#[wasm_bindgen_test]
fn resolve_rgb_matches_rust_encoding() {
    // colors[16..] come from the xterm formula; ANSI 0..15 unused here.
    let colors = build_palette(&[0u32; 16]);
    let palette = js_sys::Object::new();
    js_sys::Reflect::set(
        &palette,
        &"colors".into(),
        &js_sys::Uint32Array::from(&colors[..]),
    )
    .unwrap();
    js_sys::Reflect::set(&palette, &"defaultFg".into(), &0x111111_u32.into()).unwrap();
    js_sys::Reflect::set(&palette, &"defaultBg".into(), &0x222222_u32.into()).unwrap();

    // Default resolves to the role's default (0 = fg, 1 = bg).
    assert_eq!(
        resolve_rgb(encode_color(Color::Default), &palette, 0),
        0x111111
    );
    assert_eq!(
        resolve_rgb(encode_color(Color::Default), &palette, 1),
        0x222222
    );
    // Indexed -> palette.colors[i] (196 is the cube's pure red).
    assert_eq!(
        resolve_rgb(encode_color(Color::Indexed(196)), &palette, 0),
        0xff0000
    );
    // Rgb -> packed passthrough.
    assert_eq!(
        resolve_rgb(encode_color(Color::Rgb(10, 20, 30)), &palette, 0),
        0x0a141e
    );
}
