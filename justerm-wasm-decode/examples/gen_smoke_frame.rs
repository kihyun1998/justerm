//! Generate the golden wire-bytes fixture for the package decode smoke (#37).
//!
//! Encodes one known frame with the native `justerm_core::encode` (the single source —
//! the bytes are never hand-authored) and writes it to the path given as the
//! first argument. The smoke decodes it through the *published* WASM path and
//! asserts a known cell resolves to its expected colour.
//!
//! Usage: `cargo run -p justerm-wasm --example gen_smoke_frame -- <out-file>`

use justerm_core::{Cell, CellFlags, Color, Frame, FrameKind, Span, encode};
use std::collections::BTreeMap;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: gen_smoke_frame <out-file>");

    // One Partial span, one cell 'A' with fg = Indexed(196) — xterm cube pure red,
    // so the smoke can assert resolveRgb(fg[0]) == 0xff0000 end-to-end. The cursor
    // carries a known non-default value (3, 7, hidden) so the smoke can assert the
    // v3 cursor getters cross the assembled-package boundary (#38).
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 3,
        cursor_col: 7,
        cursor_visible: false,
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
            cells: vec![Cell::from_parts(
                'A',
                Color::Indexed(196),
                Color::Default,
                CellFlags::empty(),
            )],
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
            ucolors: BTreeMap::new(),
        }],
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };

    std::fs::write(&path, encode(&frame)).expect("write golden fixture");
    eprintln!("gen_smoke_frame: wrote golden frame to {path}");
}
