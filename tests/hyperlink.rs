//! OSC 8 hyperlink tests (#26): cells printed under an open link carry a link
//! index that resolves to the URI, survives scroll, and stops at close.
//!
//! Driven through the public API — feed OSC 8 + text, read `Cell.link` and
//! resolve via `Engine::hyperlink`. (Serialization round-trip is in its own
//! test once slice B lands.)

use justerm::{Engine, decode, encode};

const OPEN: &[u8] = b"\x1b]8;;https://example.com\x07";
const CLOSE: &[u8] = b"\x1b]8;;\x07";

#[test]
fn cells_under_open_link_carry_the_uri() {
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed(b"ab");
    t.feed(CLOSE);
    let a = *t.grid().cell(0, 0);
    let b = *t.grid().cell(0, 1);
    assert_eq!(a.c, 'a');
    let link = a.link.expect("'a' should carry a link");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
    // Both cells of one link share the same side-table index.
    assert_eq!(b.link, Some(link));
}

#[test]
fn text_before_open_and_after_close_has_no_link() {
    let mut t = Engine::new(80, 24);
    t.feed(b"x");
    t.feed(OPEN);
    t.feed(b"y");
    t.feed(CLOSE);
    t.feed(b"z");
    assert_eq!(t.grid().cell(0, 0).c, 'x');
    assert_eq!(t.grid().cell(0, 0).link, None);
    assert!(t.grid().cell(0, 1).link.is_some()); // 'y'
    assert_eq!(t.grid().cell(0, 2).c, 'z');
    assert_eq!(t.grid().cell(0, 2).link, None);
}

#[test]
fn sgr_reset_does_not_close_a_hyperlink() {
    // OSC 8 close is the only closer — an SGR reset must not drop the link.
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed(b"a\x1b[0mb"); // SGR reset between the two linked glyphs
    assert!(t.grid().cell(0, 0).link.is_some());
    assert_eq!(t.grid().cell(0, 1).link, t.grid().cell(0, 0).link);
}

#[test]
fn wide_glyph_lead_and_spacer_share_the_link() {
    let mut t = Engine::new(80, 24);
    t.feed(OPEN);
    t.feed("世".as_bytes());
    t.feed(CLOSE);
    let lead = *t.grid().cell(0, 0);
    let spacer = *t.grid().cell(0, 1);
    assert_eq!(lead.c, '世');
    assert!(lead.link.is_some());
    assert_eq!(spacer.link, lead.link);
}

#[test]
fn link_survives_scroll_into_scrollback() {
    // The index is plain Copy data on the cell, so it rides the row into
    // scrollback — the renderer can still resolve a link in history.
    let mut t = Engine::new(80, 2);
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE);
    t.feed(b"\r\nsecond\r\n"); // line-feed at the bottom evicts row 0 ('L') to scrollback
    assert!(t.scrollback_len() >= 1);
    t.scroll_up(1);
    let row0 = t.viewport_line(0);
    assert_eq!(row0[0].c, 'L');
    let link = row0[0].link.expect("link survives scroll into scrollback");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
}

#[test]
fn plain_output_carries_no_link() {
    let mut t = Engine::new(80, 24);
    t.feed(b"plain");
    assert_eq!(t.grid().cell(0, 0).link, None);
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
    // Full round-trip: cells' frame-local `link` index + the link_table survive.
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded, frame);
    let linked = decoded.spans[0]
        .cells
        .iter()
        .find(|c| c.c == 'h')
        .expect("'h' present");
    let idx = linked.link.expect("decoded cell keeps its link");
    assert_eq!(
        decoded.link_table[idx.get() as usize - 1],
        "https://example.com"
    );
}
