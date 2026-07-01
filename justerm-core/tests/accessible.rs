//! #150 — `accessible_text`: the full buffer (scrollback + screen) as one text
//! document for a screen-reader accessible view. Reuses the selection/logical-
//! line extraction (wrap-join, wide-spacer skip, trailing-trim at the logical
//! end) over the whole buffer instead of the viewport.

use justerm_core::Engine;

/// The document spans the whole buffer — lines that scrolled off the 2-row screen
/// into scrollback are still present, in order, with the trailing blank cursor
/// row trimmed (asserted exactly — no filter hiding trailing noise).
#[test]
fn accessible_text_includes_scrollback() {
    let mut term = Engine::new(10, 2);
    for i in 0..5 {
        term.feed(format!("line{i}\r\n").as_bytes());
    }

    assert_eq!(term.accessible_text(), "line0\nline1\nline2\nline3\nline4");
}

/// Trailing blank *rows* (a tall screen showing few lines) are trimmed — the
/// noise the accessible view exists to escape. A 24-row screen with 2 lines of
/// output must not end in 22 blank lines.
#[test]
fn accessible_text_trims_trailing_blank_rows() {
    let mut term = Engine::new(20, 24);
    term.feed(b"one\r\ntwo");

    assert_eq!(term.accessible_text(), "one\ntwo");
}

/// A fresh buffer (no output) is an empty document, not a run of blank lines.
#[test]
fn accessible_text_fresh_buffer_is_empty() {
    let term = Engine::new(20, 24);

    assert_eq!(term.accessible_text(), "");
}

/// Internal blank lines (paragraph breaks between outputs) are KEPT — only
/// trailing ones are trimmed. A document wants these; the viewport tree drops all.
#[test]
fn accessible_text_keeps_internal_blank_lines() {
    let mut term = Engine::new(10, 4);
    term.feed(b"a\r\n\r\nb"); // "a", blank, "b"

    assert_eq!(term.accessible_text(), "a\n\nb");
}

/// Combining marks ride the base cell into the document (reuses append_cell).
#[test]
fn accessible_text_carries_combining_marks() {
    let mut term = Engine::new(10, 1);
    term.feed("e\u{0301}".as_bytes()); // e + combining acute

    assert_eq!(term.accessible_text(), "e\u{0301}");
}

/// Soft-wrapped physical rows are one logical line — joined with no separator,
/// so a word split across the wrap reads as one word (alacritty WRAPLINE rule).
#[test]
fn accessible_text_joins_soft_wrapped_rows() {
    let mut term = Engine::new(5, 3);
    term.feed(b"abcdefgh"); // 5 cols → "abcde" (wrapped) + "fgh"

    assert!(term.accessible_text().contains("abcdefgh"));
}

/// Trailing blanks are trimmed at the logical line's end (noise to a listener).
#[test]
fn accessible_text_trims_trailing_blanks() {
    let mut term = Engine::new(10, 1);
    term.feed(b"hi"); // "hi" + 8 blank cols

    assert_eq!(term.accessible_text(), "hi");
}

/// A wide glyph is emitted once; its spacer half contributes nothing.
#[test]
fn accessible_text_emits_wide_glyph_once() {
    let mut term = Engine::new(10, 1);
    term.feed("가b".as_bytes()); // 가 spans 2 cols, then b

    assert_eq!(term.accessible_text(), "가b");
}

/// On the alt screen the document shows only the alt buffer — the scrollback
/// belongs to the *primary* buffer, not this full-screen app, so mixing them
/// would read primary history the user can't see.
#[test]
fn accessible_text_on_alt_shows_only_the_alt_buffer() {
    let mut term = Engine::new(10, 2);
    for i in 0..4 {
        term.feed(format!("p{i}\r\n").as_bytes()); // primary output → scrollback
    }
    term.feed(b"\x1b[?1049h"); // enter alt screen
    term.feed(b"ALT");

    let text = term.accessible_text();
    assert!(text.contains("ALT"), "alt content shown: {text:?}");
    assert!(
        !text.contains("p0"),
        "primary scrollback hidden on alt: {text:?}"
    );
}

/// On the alt screen a wrapped line joins within the alt buffer, and no primary
/// scrollback bleeds across the alt floor.
#[test]
fn accessible_text_on_alt_joins_wrapped_without_primary_bleed() {
    let mut term = Engine::new(5, 2);
    for i in 0..4 {
        term.feed(format!("p{i}\r\n").as_bytes()); // primary → scrollback
    }
    term.feed(b"\x1b[?1049h"); // alt screen
    term.feed(b"abcdefgh"); // 5 cols → "abcde" (wrapped) + "fgh"

    let text = term.accessible_text();
    assert!(text.contains("abcdefgh"), "wrap-joined on alt: {text:?}");
    assert!(
        !text.contains("p0"),
        "no primary bleed across alt floor: {text:?}"
    );
}
