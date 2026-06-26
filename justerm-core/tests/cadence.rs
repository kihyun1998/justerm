//! Issue #13 — cadence: the engine-side viewport-vs-screen damage mapping.
//! (Pacing — when to pull, vsync/RTT timing — is the consumer's transport, not
//! the engine; see architecture.md §Cadence.)

use justerm_core::{Engine, Frame, TermDamage};

/// Read the character a `Frame` carries at viewport `(row, col)` — locate the
/// span covering that row, then index by span-relative column. Panics if the
/// frame ships no cell there (a `Full` frame covers every row, so the
/// viewport-source tests below always find one).
fn frame_char(frame: &Frame, row: u16, col: u16) -> char {
    let span = frame
        .spans
        .iter()
        .find(|s| s.line == row && s.left <= col && col <= s.right)
        .expect("frame has no span covering this cell");
    span.cells[(col - span.left) as usize].c()
}

/// A user scroll changes which lines are visible, so the whole viewport must be
/// repainted → full damage (matches alacritty's scroll_display).
#[test]
fn user_scroll_marks_full_damage() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // history = [a], screen = b, c
    term.reset_damage();

    term.scroll_up(1); // user scrolls up into history

    assert!(matches!(term.damage(), TermDamage::Full));
}

/// While scrolled up (follow-bottom "stay"), new output scrolls the *screen* but
/// the *viewport* is frozen — so no scroll op is exposed (it would shift a frozen
/// view) and the off-screen change is not reported as viewport damage.
#[test]
fn content_scroll_while_scrolled_up_is_invisible() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc");
    term.scroll_up(1); // marks full (B1)
    term.reset_damage(); // ack that frame
    term.feed(b"\r\nd"); // new output scrolls the screen; viewport stays put

    assert!(
        term.scroll_delta().is_none(),
        "scroll op leaked to a frozen viewport"
    );
    assert!(
        matches!(term.damage(), TermDamage::Partial(ref l) if l.is_empty()),
        "off-screen change reported as viewport damage",
    );
}

/// #48 — `frame()` must source its cells from the viewport at `display_offset`,
/// the same rows `viewport_line` returns. After scrolling up, the frame ships
/// the scrolled-back rows, not the live bottom rows; otherwise a wire consumer
/// (cells reach the renderer only through `frame()`) can never display
/// scrollback — scrolling appears to do nothing.
#[test]
fn frame_cells_follow_the_scrolled_back_viewport() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // history = [a], screen = [b, c]
    term.scroll_up(1); // viewport = [a, b]; live bottom is still [b, c]

    let frame = term.frame();

    assert_eq!(
        frame_char(&frame, 0, 0),
        term.viewport_line(0)[0].c(),
        "frame top row must be the scrolled-back row, not the live grid",
    );
    assert_eq!(frame_char(&frame, 0, 0), 'a');
    assert_eq!(frame_char(&frame, 1, 0), 'b');
}

/// #48 — at the bottom (`display_offset == 0`) the viewport *is* the live grid,
/// so making `frame()` viewport-aware must not change the common case: the frame
/// still ships the live screen rows.
#[test]
fn frame_cells_unchanged_at_bottom() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // screen = [b, c], not scrolled

    let frame = term.frame();

    assert_eq!(frame_char(&frame, 0, 0), 'b');
    assert_eq!(frame_char(&frame, 1, 0), 'c');
    assert_eq!(frame_char(&frame, 0, 0), term.viewport_line(0)[0].c());
}

/// #48 — while scrolled up the live cursor is off the frozen viewport, so the
/// frame must report it hidden. A consumer renders the caret by cell-invert; a
/// `cursor_visible` frame here would ink a caret onto scrollback history. Matches
/// xterm.js (hide when `ydisp != ybase`) and alacritty (cursor off the
/// display_iter is not drawn). Returning to the bottom restores it.
#[test]
fn cursor_hidden_while_scrolled_up() {
    let mut term = Engine::new(4, 2);
    term.feed(b"a\r\nb\r\nc"); // cursor live on the bottom row

    term.scroll_up(1);
    assert!(
        !term.frame().cursor_visible,
        "caret reported visible over frozen scrollback",
    );

    term.scroll_to_bottom();
    assert!(
        term.frame().cursor_visible,
        "caret not restored after following the bottom",
    );
}

// The ack-gated diff primitive (built in #4) is what cadence rides on. These
// characterize that contract: accumulate-until-ack (flow control, no discards)
// and intermediate-state skip.

/// Changes accumulate into one diff until the consumer acks (reset_damage) — a
/// slow consumer gets a single larger diff, never a pile-up or a lost update.
#[test]
fn damage_accumulates_until_ack() {
    let mut term = Engine::new(10, 1);
    term.reset_damage();
    term.feed(b"ab"); // cols 0..1
    term.feed(b"cde"); // cols 2..4 — no ack between

    match term.damage() {
        TermDamage::Partial(l) => {
            assert_eq!(l.len(), 1);
            assert_eq!((l[0].line, l[0].left, l[0].right), (0, 0, 4)); // one merged span
        }
        other => panic!("{other:?}"),
    }
    term.reset_damage(); // ack
    assert!(matches!(term.damage(), TermDamage::Partial(ref l) if l.is_empty()));
}

/// Intermediate states are skipped: overwriting a cell before the ack reports a
/// single span for the final state, not one per write.
#[test]
fn intermediate_writes_collapse_into_one_diff() {
    let mut term = Engine::new(10, 1);
    term.reset_damage();
    term.feed(b"\x1b[1;1Hx"); // write x at col 0
    term.feed(b"\x1b[1;1Hy"); // overwrite with y — x never needs to be drawn

    match term.damage() {
        TermDamage::Partial(l) => {
            assert_eq!(l.len(), 1);
            assert_eq!((l[0].line, l[0].left, l[0].right), (0, 0, 0));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(term.viewport_line(0)[0].c(), 'y'); // the consumer sees only the final state
}

/// While scrolled to the very top with the cap full, new output evicts the
/// oldest (visible) line and the viewport advances — that shift must be
/// reported, not suppressed by the "scrolled up = frozen" rule.
#[test]
fn cap_eviction_while_scrolled_to_top_is_not_suppressed() {
    let mut term = Engine::with_scrollback(4, 2, 2); // cap = 2 history lines
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne"); // fills history to the cap
    term.scroll_up(99); // to the very top of history
    let top_before = term.viewport_line(0)[0].c();
    term.reset_damage(); // ack the scrolled frame

    term.feed(b"\r\nf"); // new line: cap evicts the oldest visible line, view shifts

    let top_after = term.viewport_line(0)[0].c();
    assert_ne!(
        top_before, top_after,
        "precondition: the viewport actually shifted"
    );
    assert!(
        !matches!(term.damage(), TermDamage::Partial(ref l) if l.is_empty()),
        "the viewport shift from cap eviction was suppressed",
    );
}

/// Resizing while scrolled up leaves cadence in a consistent state: full damage
/// (resize repaints), no stale scroll op, viewport reads in range, and normal
/// partial damage resumes afterward.
#[test]
fn resize_while_scrolled_up_is_full_and_coherent() {
    let mut term = Engine::new(4, 3);
    term.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf"); // build some history
    term.scroll_up(2);
    term.reset_damage(); // ack the scroll frame

    term.resize(6, 4); // resize while scrolled up

    assert!(matches!(term.damage(), TermDamage::Full));
    assert!(term.scroll_delta().is_none());
    for r in 0..term.grid().rows() {
        let _ = term.viewport_line(r); // must not panic / go out of range
    }

    // Resize keeps the scroll position (alacritty clamps, doesn't reset), so a
    // write while still scrolled up stays suppressed. Back at the bottom,
    // cadence reports partial damage normally again.
    term.scroll_to_bottom();
    term.reset_damage();
    term.feed(b"\x1b[1;1Hz");
    assert!(matches!(term.damage(), TermDamage::Partial(ref l) if !l.is_empty()));
}
