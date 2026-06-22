//! Generate the golden wire-bytes fixture for the package decode smoke (#37).
//!
//! Encodes one known frame with the native `justerm::encode` (the single source —
//! the bytes are never hand-authored) and writes it to the path given as the
//! first argument. The smoke decodes it through the *published* WASM path and
//! asserts a known cell resolves to its expected colour.
//!
//! Usage: `cargo run -p justerm-wasm --example gen_smoke_frame -- <out-file>`

use justerm::{Cell, Color, Frame, FrameKind, Span, encode};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: gen_smoke_frame <out-file>");

    // One Partial span, one cell 'A' with fg = Indexed(196) — xterm cube pure red,
    // so the smoke can assert resolveRgb(fg[0]) == 0xff0000 end-to-end.
    let frame = Frame {
        cols: 80,
        rows: 24,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        scroll: None,
        spans: vec![Span {
            line: 0,
            left: 0,
            right: 0,
            cells: vec![Cell {
                c: 'A',
                fg: Color::Indexed(196),
                ..Cell::default()
            }],
        }],
        side_table: vec![],
        link_table: vec![],
    };

    std::fs::write(&path, encode(&frame)).expect("write golden fixture");
    eprintln!("gen_smoke_frame: wrote golden frame to {path}");
}
