//! Issue #4 — damage (line + column span) + first-class scroll op.
//! Model: incremental bounds, ack-gated reset, recorded scroll op (ADR-0003).

use justerm::{Engine, TermDamage};

/// Writing one glyph damages only its line, with a column span of just that cell.
#[test]
fn single_cell_change_damages_its_line_span() {
    let mut term = Engine::new(10, 3);
    term.reset_damage(); // clean baseline (the "last ack")
    term.feed(b"x"); // one glyph at (0, 0)

    match term.damage() {
        TermDamage::Partial(lines) => {
            assert_eq!(lines.len(), 1);
            assert_eq!((lines[0].line, lines[0].left, lines[0].right), (0, 0, 0));
        }
        other => panic!("expected partial damage, got {other:?}"),
    }
}

/// Erasing records damage over the cleared span.
#[test]
fn erase_damages_the_cleared_span() {
    let mut term = Engine::new(10, 1);
    term.feed(b"abcde");
    term.reset_damage(); // baseline after the writes
    term.feed(b"\x1b[1;3H\x1b[K"); // cursor to col 2, erase to end of line

    match term.damage() {
        TermDamage::Partial(lines) => {
            assert_eq!(lines.len(), 1);
            assert_eq!((lines[0].line, lines[0].left, lines[0].right), (0, 2, 9));
        }
        other => panic!("{other:?}"),
    }
}
