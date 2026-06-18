//! Tests for the throughput-bench input generators (#9).
//!
//! The generators live in `benches/inputs.rs` (a `harness = false` bench can't
//! expose `cargo test`-discoverable tests), so we compile that exact source
//! here via `#[path]`. These pin the *behaviour* each input is meant to drive —
//! asserted through the public `Engine` API, so they survive a rewrite of how
//! the bytes are generated.

#[path = "../benches/inputs.rs"]
mod inputs;

use inputs::*;
use justerm::{CellFlags, Color, Engine};

const COLS: usize = 80;
const ROWS: usize = 24;

#[test]
fn ascii_feeds_printable_text() {
    let mut term = Engine::new(COLS, ROWS);
    term.feed(&ascii_input());
    // "The quick brown fox..." — the first glyph lands at the top-left cell.
    assert_eq!(term.grid().cell(0, 0).c, 'T');
}

#[test]
fn ansi_actually_colours_cells() {
    let mut term = Engine::new(COLS, ROWS);
    term.feed(&ansi_input());
    // The input is "ANSI-heavy" only if the SGR sequences land: the first cell
    // is `\x1b[38;5;0m#`, so it must carry an indexed colour, not the default.
    let first = term.grid().cell(0, 0);
    assert_eq!(first.c, '#');
    assert!(
        matches!(first.fg, Color::Indexed(_)),
        "SGR-dense input must leave indexed colours, got {:?}",
        first.fg
    );
}

#[test]
fn cjk_glyphs_are_all_wide() {
    // The whole point of the CJK input is to drive the width-2 path: every
    // glyph in the set must occupy two columns — a WIDE_CHAR lead plus a
    // WIDE_CHAR_SPACER. (A width-1 char slipping into the set — like an earlier
    // typo'd entry — would fail here.)
    for g in CJK_GLYPHS {
        let mut term = Engine::new(COLS, ROWS);
        let mut tmp = [0u8; 4];
        term.feed(g.encode_utf8(&mut tmp).as_bytes());
        let lead = term.grid().cell(0, 0);
        let spacer = term.grid().cell(0, 1);
        assert_eq!(lead.c, g);
        assert!(
            lead.flags.contains(CellFlags::WIDE_CHAR),
            "{g:?} should be a wide lead cell"
        );
        assert!(
            spacer.flags.contains(CellFlags::WIDE_CHAR_SPACER),
            "{g:?} should leave a spacer in the next column"
        );
    }
}

#[test]
fn scrolling_fills_scrollback() {
    let mut term = Engine::new(COLS, ROWS);
    term.feed(&scrolling_input());
    // Far more lines than a screen holds, so rows must have spilled off the top
    // into history — that eviction is the scroll path this input exists to time.
    assert!(
        term.scrollback_len() > 0,
        "scrolling input should push lines into scrollback"
    );
}

#[test]
fn every_input_is_non_empty_and_feeds_cleanly() {
    // criterion divides by buffer length for MB/s, so an empty buffer would be a
    // divide-by-zero-shaped lie; and every stream must survive `feed` intact.
    let inputs: [(&str, Vec<u8>); 4] = [
        ("ascii", ascii_input()),
        ("ansi", ansi_input()),
        ("cjk", cjk_input()),
        ("scrolling", scrolling_input()),
    ];
    for (name, bytes) in inputs {
        assert!(!bytes.is_empty(), "{name} input must not be empty");
        let mut term = Engine::new(COLS, ROWS);
        term.feed(&bytes); // must not panic
    }
}
