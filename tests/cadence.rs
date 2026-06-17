//! Issue #13 — cadence: the engine-side viewport-vs-screen damage mapping.
//! (Pacing — when to pull, vsync/RTT timing — is the consumer's transport, not
//! the engine; see architecture.md §Cadence.)

use justerm::{Engine, TermDamage};

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

    assert!(term.scroll_delta().is_none(), "scroll op leaked to a frozen viewport");
    assert!(
        matches!(term.damage(), TermDamage::Partial(ref l) if l.is_empty()),
        "off-screen change reported as viewport damage",
    );
}
