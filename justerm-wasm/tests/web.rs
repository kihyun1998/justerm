//! JS-boundary tests for the WASM decoder (#34 S3/AC2), run with
//! `wasm-pack test --node`.
//!
//! The pure `flatten` logic is unit-tested in `lib.rs` with plain `cargo test`;
//! these verify the wasm-bindgen layer that unit tests cannot reach — that the
//! typed-array views carry the right bytes across the JS boundary, malformed
//! input throws, and the WASM build decodes identically to the native path.
//!
//! wasm32-only: these call `js_sys` (typed-array views over WASM memory), which
//! panics off-wasm. The crate-level cfg makes a native `cargo test --workspace`
//! skip this file (compiling it to nothing) while `wasm-pack test --node` still
//! runs it on the wasm32 target.
#![cfg(target_arch = "wasm32")]

use justerm::{Cell, Frame, FrameKind, Span};
use justerm_wasm::{decode_frame, wire_version};
use wasm_bindgen_test::*;

/// A Partial frame: "hi" at row 0 col 0, "abc" at row 1 col 5.
fn sample_frame() -> Frame {
    let span = |line: u16, left: u16, s: &str| Span {
        cells: s
            .chars()
            .map(|c| Cell {
                c,
                ..Cell::default()
            })
            .collect(),
        line,
        left,
        right: left + s.chars().count() as u16 - 1,
    };
    Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        scroll: None,
        spans: vec![span(0, 0, "hi"), span(1, 5, "abc")],
        side_table: vec![],
        link_table: vec![],
    }
}

#[wasm_bindgen_test]
fn wire_version_is_two() {
    assert_eq!(wire_version(), 2);
}

#[wasm_bindgen_test]
fn decode_frame_exposes_scalars() {
    let bytes = justerm::encode(&sample_frame());
    let df = decode_frame(&bytes).expect("decode");
    assert_eq!(df.cols(), 80);
    assert_eq!(df.rows(), 24);
    assert_eq!(df.kind(), 1); // Partial
    assert!(!df.has_scroll());
}

#[wasm_bindgen_test]
fn cells_view_carries_record_bytes_across_the_boundary() {
    let bytes = justerm::encode(&sample_frame());
    let df = decode_frame(&bytes).expect("decode");
    let cells = df.cells();
    // 2 + 3 cells, 18 bytes each.
    assert_eq!(cells.length(), 5 * 18);
    // First record's codepoint u32 (LE) is 'h' = 0x68.
    assert_eq!(cells.get_index(0), b'h');
    assert_eq!(cells.get_index(1), 0);
    // Third cell is 'a' (start of "abc"), at record index 2 → byte 2*18.
    assert_eq!(cells.get_index(2 * 18), b'a');
}

#[wasm_bindgen_test]
fn span_directory_view_maps_cells_to_rows() {
    let bytes = justerm::encode(&sample_frame());
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
fn decode_frame_throws_on_bad_magic() {
    assert!(decode_frame(b"\x00\x00\x00").is_err());
}

#[wasm_bindgen_test]
fn decode_frame_throws_on_truncated() {
    let mut bytes = justerm::encode(&sample_frame());
    bytes.truncate(bytes.len() - 4); // chop into the last cell record
    assert!(decode_frame(&bytes).is_err());
}
