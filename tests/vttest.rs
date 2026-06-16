//! Issue #8 — a vttest-*style* conformance harness.
//!
//! Real vttest is interactive/visual and esctest needs a query/response path
//! the engine does not have yet (DSR/CPR replies — #11/#12). So this is an
//! in-process, data-driven net: feed known VT input, dump the whole screen, and
//! compare against an inline golden. Unlike the per-cell assertions in
//! `vt_compliance.rs`, a full-screen golden catches changes *anywhere* — the
//! "systematic net" that surfaces hidden state we did not think to assert.
//!
//! This file is a growing net: add cases as dogfood reveals tail behaviour.

use justerm::Engine;

/// Render the active screen to a deterministic text dump: one bar-delimited line
/// per row (so trailing spaces stay visible), then a cursor line. Chars + cursor
/// only — attributes/colours are a later layer.
fn dump(term: &Engine) -> String {
    let grid = term.grid();
    let mut s = String::new();
    for row in 0..grid.rows() {
        s.push('|');
        for col in 0..grid.cols() {
            s.push(grid.cell(row, col).c);
        }
        s.push_str("|\n");
    }
    let cur = term.cursor();
    s.push_str(&format!(
        "cursor=({},{}) visible={}\n",
        cur.row, cur.col, cur.visible
    ));
    s
}

/// Feed `input` into a fresh `cols`×`rows` engine and assert its screen dump
/// equals `expected`.
fn check(cols: usize, rows: usize, input: &[u8], expected: &str) {
    let mut term = Engine::new(cols, rows);
    term.feed(input);
    assert_eq!(dump(&term), expected);
}

#[test]
fn print_basic() {
    check(
        5,
        2,
        b"hi",
        "\
|hi   |
|     |
cursor=(0,2) visible=true
",
    );
}

/// Autowrap: the 4th char wraps to the next row (deferred last-column wrap).
#[test]
fn autowrap() {
    check(
        3,
        2,
        b"abcd",
        "\
|abc|
|d  |
cursor=(1,1) visible=true
",
    );
}

/// HT advances to the 8-column tab stop.
#[test]
fn tab_stop() {
    check(
        20,
        1,
        b"\tX",
        "\
|        X           |
cursor=(0,9) visible=true
",
    );
}

/// Scroll region: IND at the bottom margin scrolls rows [2..=3] up; the rows
/// outside the region (A at top, D at bottom) stay fixed.
#[test]
fn scroll_region_index() {
    check(
        4,
        4,
        b"\x1b[2;3r\x1b[1;1HA\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[3;1H\x1bD",
        "\
|A   |
|C   |
|    |
|D   |
cursor=(2,0) visible=true
",
    );
}

/// Alt-screen: after ?1049h the screen is fresh; the primary content (AB) is
/// hidden and only the alt write (Z) shows.
#[test]
fn alt_screen() {
    check(
        5,
        2,
        b"AB\x1b[?1049h\x1b[1;1HZ",
        "\
|Z    |
|     |
cursor=(0,1) visible=true
",
    );
}
