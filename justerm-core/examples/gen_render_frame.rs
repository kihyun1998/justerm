//! Encode a representative full-screen frame to wire bytes, so the WASM decode
//! path (justerm-wasm-decode `decodeFrame`, what penterm's webview runs per frame) can
//! be timed on a realistic payload instead of the one-cell smoke frame.
//!
//! A Full 80x24 frame = 1920 cells, the worst-case repaint a renderer decodes in
//! one go. Content is mixed: printable text with an indexed-colour change every
//! few cells, to exercise the colour/flag columns of the structure-of-arrays,
//! not just a single run.
//!
//! Usage: `cargo run --release --example gen_render_frame -- <out-file>`

use justerm_core::{Cell, CellFlags, Color, Frame, FrameKind, Span, encode};
use std::collections::BTreeMap;

const COLS: u16 = 80;
const ROWS: u16 = 24;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: gen_render_frame <out-file>");

    let text = b"The quick brown fox jumps over the lazy dog while 1234567890 ticks by. ";
    let mut spans = Vec::with_capacity(ROWS as usize);
    for row in 0..ROWS {
        let cells: Vec<Cell> = (0..COLS)
            .map(|col| {
                let ch = text[(row as usize * 7 + col as usize) % text.len()] as char;
                // A colour change every 5 cells: realistic SGR density, and it
                // forces the decoder to populate the fg column per cell.
                let fg = Color::Indexed(((col / 5) % 256) as u8);
                Cell::from_parts(ch, fg, Color::Default, CellFlags::empty())
            })
            .collect();
        spans.push(Span {
            line: row,
            left: 0,
            right: COLS - 1,
            cells,
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
        });
    }

    let frame = Frame {
        cols: COLS,
        rows: ROWS,
        kind: FrameKind::Full,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        mouse_events: Default::default(),
        scroll: None,
        spans,
        side_table: vec![],
        link_table: vec![],
        overlay: Default::default(),
    };

    let bytes = encode(&frame);
    std::fs::write(&path, &bytes).expect("write frame");
    eprintln!(
        "gen_render_frame: wrote {} ({} wire bytes, {} cells)",
        path,
        bytes.len(),
        COLS as usize * ROWS as usize
    );
}
