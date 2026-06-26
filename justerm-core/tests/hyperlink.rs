//! OSC 8 hyperlink tests (#26): cells printed under an open link carry a link
//! index that resolves to the URI, survives scroll, and stops at close.
//!
//! Driven through the public API — feed OSC 8 + text, read the link index via
//! `Engine::link_at` / `viewport_link_at` (the link rides a per-row map now, not
//! the cell, #46) and resolve via `Engine::hyperlink`.

use justerm_core::{Engine, decode, encode};

const OPEN: &[u8] = b"\x1b]8;;https://example.com\x07";
const CLOSE: &[u8] = b"\x1b]8;;\x07";

#[test]
fn cells_under_open_link_carry_the_uri() {
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed(b"ab");
    t.feed(CLOSE);
    assert_eq!(t.grid().cell(0, 0).c(), 'a');
    let link = t.link_at(0, 0).expect("'a' should carry a link");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
    // Both cells of one link share the same side-table index.
    assert_eq!(t.link_at(0, 1), Some(link));
}

#[test]
fn text_before_open_and_after_close_has_no_link() {
    let mut t = Engine::new(80, 24);
    t.feed(b"x");
    t.feed(OPEN);
    t.feed(b"y");
    t.feed(CLOSE);
    t.feed(b"z");
    assert_eq!(t.grid().cell(0, 0).c(), 'x');
    assert_eq!(t.link_at(0, 0), None);
    assert!(t.link_at(0, 1).is_some()); // 'y'
    assert_eq!(t.grid().cell(0, 2).c(), 'z');
    assert_eq!(t.link_at(0, 2), None);
}

#[test]
fn sgr_reset_does_not_close_a_hyperlink() {
    // OSC 8 close is the only closer — an SGR reset must not drop the link.
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed(b"a\x1b[0mb"); // SGR reset between the two linked glyphs
    assert!(t.link_at(0, 0).is_some());
    assert_eq!(t.link_at(0, 1), t.link_at(0, 0));
}

#[test]
fn wide_glyph_lead_and_spacer_share_the_link() {
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed("世".as_bytes());
    t.feed(CLOSE);
    assert_eq!(t.grid().cell(0, 0).c(), '世');
    assert!(t.link_at(0, 0).is_some());
    assert_eq!(t.link_at(0, 1), t.link_at(0, 0)); // spacer shares the lead's link
}

#[test]
fn link_survives_scroll_into_scrollback() {
    // The link rides the row's map into scrollback — the renderer can still
    // resolve a link in history.
    let mut t = Engine::new(80, 2);
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE);
    t.feed(b"\r\nsecond\r\n"); // line-feed at the bottom evicts row 0 ('L') to scrollback
    assert!(t.scrollback_len() >= 1);
    t.scroll_up(1);
    assert_eq!(t.viewport_line(0)[0].c(), 'L');
    let link = t
        .viewport_link_at(0, 0)
        .expect("link survives scroll into scrollback");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
}

#[test]
fn frame_carries_a_scrolled_back_links_uri() {
    // #48: `frame()` sources cells *and* the per-row link map from the viewport
    // at `display_offset`. With the link scrolled into history, the frame's
    // link_table must still carry the URI and the span must reference it —
    // otherwise a wire consumer (which only sees `frame()`) loses the link.
    let mut t = Engine::new(80, 2);
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE);
    t.feed(b"\r\nsecond\r\n"); // evicts the linked row 0 into scrollback
    t.scroll_up(1); // viewport row 0 is the linked 'L' again

    let frame = t.frame();
    assert_eq!(
        frame.link_table,
        vec!["https://example.com".to_string()],
        "the scrolled-back link did not reach the frame's link_table",
    );
    let span = frame
        .spans
        .iter()
        .find(|s| s.line == 0)
        .expect("full frame covers row 0");
    let idx = span.links.get(&0).expect("col 0 references the link");
    assert_eq!(
        frame.link_table[idx.get() as usize - 1],
        "https://example.com"
    );
}

#[test]
fn plain_output_carries_no_link() {
    let mut t = Engine::new(80, 24);
    t.feed(b"plain");
    assert_eq!(t.link_at(0, 0), None);
}

#[test]
fn link_follows_an_insert_shift() {
    // ICH shifts cells right — the link map must follow, like combining (#46).
    let mut t = Engine::new(6, 1);
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE); // col0 'L' linked
    t.feed(b"\x1b[1;1H"); // cursor home
    t.feed(b"\x1b[2@"); // ICH 2 -> 'L' shifts to col2
    assert_eq!(t.grid().cell(0, 2).c(), 'L');
    let link = t.link_at(0, 2).expect("link followed the insert shift");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
    assert_eq!(t.link_at(0, 0), None, "the opened gap carries no link");
}

#[test]
fn link_follows_a_delete_shift() {
    // DCH shifts the tail left — the link map must follow.
    let mut t = Engine::new(6, 1);
    t.feed(b"xy");
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE); // col0 'x', col1 'y', col2 'L' linked
    t.feed(b"\x1b[1;1H"); // cursor home
    t.feed(b"\x1b[2P"); // DCH 2 -> 'L' shifts to col0
    assert_eq!(t.grid().cell(0, 0).c(), 'L');
    let link = t.link_at(0, 0).expect("link followed the delete shift");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
}

#[test]
fn link_survives_resize_reflow() {
    // A column resize reflows rows; the link map is re-keyed per column (#46).
    let mut t = Engine::new(5, 2);
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE);
    t.resize(3, 2); // column change -> reflow
    let link = t.link_at(0, 0).expect("link survives reflow");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
}

#[test]
fn hyperlink_round_trips_through_serialization() {
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed(b"hi");
    t.feed(CLOSE);
    let frame = t.frame();
    // The frame carries the URI in its own (frame-local) side-table.
    assert_eq!(frame.link_table, vec!["https://example.com".to_string()]);
    // Full round-trip: the span's per-column link indices + the link_table survive.
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded, frame);
    let span = &decoded.spans[0];
    let hcol = span
        .cells
        .iter()
        .position(|c| c.c() == 'h')
        .expect("'h' present");
    let idx = span
        .links
        .get(&hcol)
        .copied()
        .expect("decoded span keeps the link");
    assert_eq!(
        decoded.link_table[idx.get() as usize - 1],
        "https://example.com"
    );
}
