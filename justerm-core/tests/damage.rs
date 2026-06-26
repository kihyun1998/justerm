//! Issue #4 — damage (line + column span) + first-class scroll op.
//! Model: incremental bounds, ack-gated reset, recorded scroll op (ADR-0003).

use justerm_core::{Engine, TermDamage};

// ScrollOp is brought in per-test where needed.

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

/// A pure cursor move puts the old *and* new cursor cells into the rendered
/// frame, even though no cell *content* changed — the consumer draws the caret
/// by cell-invert (beamterm has no cursor primitive), so without this the old
/// position keeps the inverted cell (a ghost) and the new one is never inverted.
/// Caret cells live in `frame()` (the render producer), not `damage()` (which
/// stays content-only for cadence); mirrors Alacritty's `last_cursor`. (#38)
#[test]
fn cursor_move_puts_old_and_new_cells_in_frame() {
    let mut term = Engine::new(80, 24);
    term.feed(b"abc"); // content on row 0; cursor now at (0, 3)
    term.reset_damage(); // ack: consumer has seen the cursor at (0, 3)
    term.feed(b"\x1b[10;20H"); // CUP to (9, 19); no glyph written

    let covers = |spans: &[justerm_core::Span], line: u16, col: u16| {
        spans
            .iter()
            .any(|s| s.line == line && s.left <= col && s.right >= col)
    };
    let f = term.frame();
    assert!(
        covers(&f.spans, 0, 3),
        "old cursor cell (0,3) must be in the frame, got {:?}",
        f.spans
    );
    assert!(
        covers(&f.spans, 9, 19),
        "new cursor cell (9,19) must be in the frame, got {:?}",
        f.spans
    );

    // `damage()` stays content-only: a pure move records no content change.
    assert!(
        matches!(term.damage(), TermDamage::Partial(ref l) if l.is_empty()),
        "damage() is content-only and must stay empty on a pure cursor move"
    );
}

/// A scroll is a first-class op, not full-screen damage.
#[test]
fn scroll_emits_first_class_op_not_full_damage() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb"); // a → row 0, b → row 1
    term.reset_damage();
    term.feed(b"\r\nc"); // CR+LF at the bottom → scroll up; c on the new bottom

    let op = term.scroll_delta().expect("expected a scroll op");
    assert_eq!((op.top, op.bottom, op.count), (0, 1, 1)); // rows [0..=1] up by 1
    assert!(!matches!(term.damage(), TermDamage::Full)); // not a full redraw
}

/// Switching to the alt screen replaces the whole screen → full damage.
#[test]
fn alt_screen_switch_is_full_damage() {
    let mut term = Engine::new(4, 2);
    term.feed(b"ab");
    term.reset_damage();
    term.feed(b"\x1b[?1049h"); // enter alt → entire screen swapped + cleared

    assert!(matches!(term.damage(), TermDamage::Full));
}

/// Resize clears any pending scroll op — a stale op points at the old rows and
/// would be out of range against the resized screen.
#[test]
fn resize_clears_stale_scroll_op() {
    let mut term = Engine::new(4, 5);
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf"); // line-feeds at the bottom record a scroll op
    assert!(term.scroll_delta().is_some());

    term.resize(4, 3);

    assert!(term.scroll_delta().is_none());
}

/// Reverse index at the top margin scrolls down → a negative-count scroll op.
#[test]
fn reverse_index_emits_down_scroll_op() {
    let mut term = Engine::new(4, 2);
    term.feed(b"\x1b[1;1H"); // cursor at the top margin
    term.reset_damage();
    term.feed(b"\x1bM"); // RI → scroll the region down

    let op = term.scroll_delta().expect("expected a scroll op");
    assert_eq!((op.top, op.bottom, op.count), (0, 1, -1));
}

/// The common path: print on the bottom row, then a line-feed scrolls it up.
/// Damage must follow the content to its new row (and the exposed bottom row is
/// new blank content), so the consumer redraws the right rows after the shift.
#[test]
fn write_then_scroll_realigns_damage_with_content() {
    let mut term = Engine::new(4, 2);
    term.feed(b"\x1b[2;1H"); // cursor to the bottom row
    term.reset_damage();
    term.feed(b"Z\n"); // write Z at the bottom, then LF scrolls it up

    let lines = match term.damage() {
        TermDamage::Partial(l) => l,
        other => panic!("{other:?}"),
    };
    // Z ended up on row 0 after the scroll — its damage must point there.
    assert!(
        lines.iter().any(|d| d.line == 0),
        "row 0 not damaged: {lines:?}"
    );
    // The newly exposed bottom row is new blank content → damaged too.
    assert!(
        lines.iter().any(|d| d.line == 1),
        "row 1 not damaged: {lines:?}"
    );
}

/// Several scrolls of the same region between acks accumulate into one op (flow
/// control: a slow consumer gets a single larger shift, never a pile-up).
#[test]
fn repeated_scroll_accumulates_count() {
    let mut term = Engine::new(4, 3);
    term.feed(b"a\r\nb\r\nc"); // fill 3 rows
    term.reset_damage();
    term.feed(b"\r\nd\r\ne"); // two line-feeds → two full-region scrolls

    let op = term.scroll_delta().expect("scroll op");
    assert_eq!((op.top, op.bottom, op.count), (0, 2, 2));
}

/// Scrolls of *different* regions in one frame cannot be expressed as a single
/// op, so they degrade to full damage rather than silently dropping one.
#[test]
fn scrolls_of_different_regions_degrade_to_full() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[4;1H"); // cursor to the bottom of the full screen
    term.reset_damage();
    term.feed(b"\n"); // scroll the full region [0..=3]

    term.feed(b"\x1b[2;3r"); // DECSTBM region rows 2..3 → [1..=2] (homes cursor)
    term.feed(b"\x1b[3;1H\n"); // cursor to the region bottom, scroll [1..=2]

    assert!(matches!(term.damage(), TermDamage::Full));
}
