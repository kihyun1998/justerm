//! Generate a wire-bytes fixture from a **real `Engine`**, for the end-to-end decode
//! smoke (#657).
//!
//! Sibling of `gen_smoke_frame`, and deliberately not a replacement for it. That one
//! hand-builds a `Frame` struct, which pins the *encoder* against known values but
//! leaves the engine out of the loop entirely — so a group the engine stopped
//! populating would still round-trip, because the fixture decides the contents. This
//! one feeds VT bytes to `Engine` and encodes whatever the engine produced, which is
//! the only way the composition `feed -> frame -> encode -> decodeFrame` is observed.
//!
//! It carries the two columns nothing else proves end to end: a **grapheme cluster**
//! (`extra` indexing `sideTable`) and an **OSC 8 hyperlink** (`link` indexing
//! `linkTable`).
//!
//! Usage: `cargo run -p justerm-wasm-decode --example gen_engine_frame -- <out-file>`

use justerm_core::{Engine, encode};

/// What the engine is fed. Kept as one place so the smoke's expected values can be
/// read off the source of the bytes rather than guessed.
///
/// - `\x1b[38;5;196m` — xterm cube pure red, so a colour assertion resolves to
///   `0xff0000` exactly as `gen_smoke_frame`'s does.
/// - `e\u{301}` — LATIN SMALL LETTER E + COMBINING ACUTE ACCENT. One grapheme, two
///   codepoints, so the cell carries a `combining` entry and the decoded frame must
///   put a non-zero `extra` index on it pointing into `sideTable`.
/// - the OSC 8 pair — a hyperlink around `LINK`, so those cells carry a `link` index
///   into `linkTable`.
const INPUT: &str = concat!(
    "\x1b[38;5;196mR\x1b[0m",
    "e\u{301}",
    "\x1b]8;;https://example.com\x1b\\LINK\x1b]8;;\x1b\\",
);

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: gen_engine_frame <out-file>");

    let mut engine = Engine::new(80, 24);
    engine.feed(INPUT.as_bytes());
    let frame = engine.frame();

    // Diagnostics on stderr: the smoke asserts values, this says where they came from.
    eprintln!(
        "gen_engine_frame: kind={:?} spans={} link_table={:?}",
        frame.kind,
        frame.spans.len(),
        frame.link_table
    );
    for (i, s) in frame.spans.iter().enumerate() {
        eprintln!(
            "  span[{i}] line={} left={} right={} cells={} combining={:?} links={:?}",
            s.line,
            s.left,
            s.right,
            s.cells.len(),
            s.combining,
            s.links
        );
        let text: String = s.cells.iter().map(|c| c.c()).collect();
        eprintln!("    text={text:?}");
    }

    std::fs::write(&path, encode(&frame)).expect("write engine fixture");
    eprintln!("gen_engine_frame: wrote engine-produced frame to {path}");
}
