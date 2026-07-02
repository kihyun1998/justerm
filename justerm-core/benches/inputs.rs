//! Deterministic input generators for the throughput bench (#9).
//!
//! Defined in their own module so both the criterion harness
//! (`benches/throughput.rs`) and a `cargo test`-discoverable integration test
//! (`tests/bench_inputs.rs`, via `#[path]`) compile the *same* source — the
//! byte streams the bench measures are the ones the tests pin down.
//!
//! No external files and no RNG: each stream is reproducible across runs.

// This module is `#[path]`-included by several consumers (the two bench harnesses,
// the dump example, the pinning test), and each uses only the subset of generators
// it needs — so a given generator being unused *in one consumer* is expected, not a
// defect. Silence dead-code here rather than sprinkling `#[allow]` at each include.
#![allow(dead_code)]

/// ~28 KiB of plain printable ASCII in CRLF-terminated lines. Real PTY output
/// arrives CRLF (the tty's ONLCR maps `\n` -> `\r\n`), so a bare `\n` here would
/// be both unrepresentative and a staircase — justerm's LF is a raw line feed
/// with no carriage return, so each line would start where the last ended.
pub fn ascii_input() -> Vec<u8> {
    let line = b"The quick brown fox jumps over the lazy dog while 1234567890 ticks by. ";
    let mut buf = Vec::with_capacity(32 * 1024);
    for _ in 0..400 {
        buf.extend_from_slice(line);
        buf.extend_from_slice(b"\r\n");
    }
    buf
}

/// ~24 KiB of SGR-dense output: a 256-colour foreground change before every
/// glyph, the worst case for the escape parser + pen writes.
pub fn ansi_input() -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 * 1024);
    for i in 0..2000u32 {
        buf.extend_from_slice(format!("\x1b[38;5;{}m#", i % 256).as_bytes());
        if i % 70 == 69 {
            buf.extend_from_slice(b"\r\n");
        }
    }
    buf.extend_from_slice(b"\x1b[0m");
    buf
}

/// The rotating run of CJK ideographs / Hangul syllables `cjk_input` lays down —
/// all width 2 under `unicode-width`.
pub const CJK_GLYPHS: [char; 10] = ['世', '界', '한', '글', '터', '미', '널', '安', '寧', '語'];

/// ~36 KiB of CJK (each glyph is width 2, 3 UTF-8 bytes), exercising the
/// wide-cell write, pending-wrap at the right edge, and spacer cells.
pub fn cjk_input() -> Vec<u8> {
    let mut buf = Vec::with_capacity(48 * 1024);
    for _ in 0..400 {
        for g in CJK_GLYPHS.iter().cycle().take(30) {
            let mut tmp = [0u8; 4];
            buf.extend_from_slice(g.encode_utf8(&mut tmp).as_bytes());
        }
        buf.extend_from_slice(b"\r\n");
    }
    buf
}

/// ~40 KiB of short content lines, far more than a screen's worth, so each line
/// feed at the bottom margin drives the scroll routine continuously.
pub fn scrolling_input() -> Vec<u8> {
    let mut buf = Vec::with_capacity(48 * 1024);
    for i in 0..2000u32 {
        buf.extend_from_slice(format!("line {:05}: scrolling through history\r\n", i).as_bytes());
    }
    buf
}

/// Cols the wrap-run inputs (#206) assume: fed at this width, a full row is
/// exactly `WRAP_COLS` printable chars, so `one_wrap_run_input` auto-wraps every
/// row into one continuous `WRAPLINE` run. The `wrap_run` bench and its pinning
/// test build the engine at this width.
pub const WRAP_COLS: usize = 80;

/// Content rows both wrap-run shapes lay down — chosen far larger than any
/// viewport so the single-run walk's `O(scrollback)` cost dwarfs the
/// `O(viewport)` cost the many-lines control pays. The bench's scrollback cap is
/// sized to `WRAP_ROWS`, so the whole run is retained (never evicted).
pub const WRAP_ROWS: usize = 10_000;

/// The printable, whitespace-free, newline-free pattern both wrap-run inputs lay
/// down. No whitespace so nothing hard-breaks the run; the exact bytes are
/// irrelevant to the assembly cost being measured, but the two shapes share them
/// so a `search` over either walks identical content.
const WRAP_PATTERN: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// A single unbroken, newline-free line of `WRAP_COLS * WRAP_ROWS` printable
/// chars. Fed at width `WRAP_COLS` it auto-wraps every row, so the *whole buffer
/// is ONE `WRAPLINE` run* — the pathological logical line #206 tracks: one
/// `search` / `viewport_logical_lines` call must assemble the entire run into a
/// single `String`/`Vec<char>` (`O(scrollback)`).
pub fn one_wrap_run_input() -> Vec<u8> {
    let n = WRAP_COLS * WRAP_ROWS;
    let mut buf = Vec::with_capacity(n);
    for i in 0..n {
        buf.push(WRAP_PATTERN[i % WRAP_PATTERN.len()]);
    }
    buf
}

/// The *same* `WRAP_COLS * WRAP_ROWS` content chars as `one_wrap_run_input`, but
/// hard-broken with CRLF every `WRAP_COLS` chars: many short logical lines and
/// *no* long wrap run. The control shape — `viewport_logical_lines` over it is
/// `O(viewport)` (only visible rows join), so the delta against
/// `one_wrap_run_input` isolates the single-run blow-up the cap would target.
pub fn many_lines_input() -> Vec<u8> {
    let mut buf = Vec::with_capacity(WRAP_COLS * WRAP_ROWS + WRAP_ROWS * 2);
    let mut k = 0usize;
    for _ in 0..WRAP_ROWS {
        for _ in 0..WRAP_COLS {
            buf.push(WRAP_PATTERN[k % WRAP_PATTERN.len()]);
            k += 1;
        }
        buf.extend_from_slice(b"\r\n");
    }
    buf
}

/// ~5 MiB of short CRLF lines — the *at-cap flood* the harness times. Short
/// lines mean the most newlines per MiB (the worst case for scroll), and the
/// line count (~120k) dwarfs any sane scrollback cap, so once the cap is full
/// every line evicts + recycles a row: the steady-state, bandwidth-bound regime
/// a real `cat huge.log` produces. The small inputs never reach the cap, so this
/// is the one that measures row recycling rather than history growth. [#42]
pub fn flood_input() -> Vec<u8> {
    let mut buf = Vec::with_capacity(5 * 1024 * 1024 + 64);
    let mut i = 0u32;
    while buf.len() < 5 * 1024 * 1024 {
        buf.extend_from_slice(format!("line {i:08}: flooding the scrollback ring\r\n").as_bytes());
        i += 1;
    }
    buf
}
