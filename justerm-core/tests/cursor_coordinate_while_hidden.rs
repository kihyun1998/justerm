//! The cursor COORDINATE stays live while the cursor is reported hidden (#921).
//!
//! `cursor_visible` is `self.cursor.visible && self.display_offset == 0` — a statement about
//! *drawing*, made for #48 (a cell-invert caret would ink over scrollback). `cursor_row` and
//! `cursor_col` are not gated by it: they are grid coordinates sampled in the same `Term::frame`
//! body, and they keep describing where the cursor is for as long as the view is away.
//!
//! This is pinned because a consumer depends on it. `justerm-web` retains the cursor cell from
//! every frame to anchor the hidden textarea the IME reads (#631, ADR-0028 D4), and it does so
//! **without consulting `cursor_visible`** — which is only correct while the third assertion below
//! holds. Before #921 it did consult it, froze the anchor for the whole scrolled-up excursion, and
//! a composition begun on the way back latched a cell the cursor had left.

use justerm_core::Engine;

/// Ten lines into a five-row grid, so there is scrollback to scroll into.
fn scrolled_engine() -> Engine {
    let mut t = Engine::new(20, 5);
    for i in 0..10 {
        t.feed(format!("line{i}\r\n").as_bytes());
    }
    t
}

#[test]
fn the_caret_is_hidden_while_scrolled_up() {
    let mut t = scrolled_engine();
    assert!(t.frame().cursor_visible, "control: visible at the bottom");
    t.scroll_up(3);
    assert!(!t.frame().cursor_visible, "#48: hidden while scrolled up");
}

#[test]
fn the_coordinate_keeps_moving_while_the_caret_is_hidden() {
    let mut t = scrolled_engine();
    t.scroll_up(3);
    let before = t.frame();
    t.feed(b"abc");
    let after = t.frame();

    assert!(!after.cursor_visible, "still scrolled up, so still hidden");
    assert_ne!(
        (before.cursor_row, before.cursor_col),
        (after.cursor_row, after.cursor_col),
        "output that arrives while the view is away still moves the reported cell",
    );
}

/// The OTHER term of the predicate. `cursor_visible` is a conjunction, and until #921 the consumer
/// consulted the whole of it — so the scrolled-up tests above, which only ever move
/// `display_offset`, cannot see a regression in the `cursor.visible` half. They stay green against
/// a `cursor_row` that is zeroed whenever DECTCEM is off, and the consumer that now retains on
/// every frame would anchor the IME at row 0 for the life of any application that hides its caret.
#[test]
fn the_coordinate_survives_dectcem_too() {
    let mut t = scrolled_engine();
    t.feed(b"[?25l"); // DECTCEM off: the application hides its caret, at the bottom
    t.feed(
        b"
xyz",
    );
    let hidden = t.frame();
    assert!(
        !hidden.cursor_visible,
        "precondition: hidden by DECTCEM, not by the scroll — display_offset is 0 here",
    );
    assert_eq!(
        hidden.display_offset, 0,
        "this arm must not be the scrolled one"
    );

    // Showing the caret again changes the bit and nothing else.
    t.feed(b"[?25h");
    let shown = t.frame();
    assert!(shown.cursor_visible);
    assert_eq!(
        (hidden.cursor_row, hidden.cursor_col),
        (shown.cursor_row, shown.cursor_col),
        "the cell reported while DECTCEM-hidden is the cell reported once it is shown",
    );
    assert_eq!(
        (shown.cursor_row, shown.cursor_col),
        (4, 3),
        "and it is where `xyz` actually left the cursor, not a value that happens to match",
    );
}

#[test]
fn the_coordinate_reported_while_hidden_is_the_one_true_after_the_snap() {
    let mut t = scrolled_engine();
    t.scroll_up(3);
    // Output while the user is away, on both axes: `abc` moves the column, the newline the row.
    t.feed(b"abc\r\nxy");
    let hidden = t.frame();
    assert!(
        !hidden.cursor_visible,
        "precondition: the caret is hidden here"
    );

    t.scroll_to_bottom();
    let shown = t.frame();

    assert!(shown.cursor_visible, "the caret is back");
    // THE CONTRACT the consumer's anchor rests on: nothing about the coordinate had to be
    // recomputed by returning to the bottom. What was on the wire while hidden was already right.
    assert_eq!(
        (hidden.cursor_row, hidden.cursor_col),
        (shown.cursor_row, shown.cursor_col),
        "the cell reported while hidden is the cell the cursor occupies once the view returns",
    );
}
