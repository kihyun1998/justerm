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
    assert_eq!(
        t.link_at(0, 0).as_ref().map(|h| h.uri()),
        Some("https://example.com"),
        "'a' should carry the link",
    );
    // Both cells of one link resolve to the same URI — and since #628 they share the
    // same allocation rather than an index into a table, which `grid.rs`'s
    // `ext_attrs_round_trip_from_one_column_to_another` pins by pointer identity.
    assert_eq!(
        t.link_at(0, 1).as_ref().map(|h| h.uri()),
        Some("https://example.com")
    );
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
    assert_eq!(
        t.viewport_link_at(0, 0).as_ref().map(|h| h.uri()),
        Some("https://example.com"),
        "link survives scroll into scrollback",
    );
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
    assert_eq!(
        t.link_at(0, 2).as_ref().map(|h| h.uri()),
        Some("https://example.com"),
        "link followed the insert shift"
    );
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
    assert_eq!(
        t.link_at(0, 0).as_ref().map(|h| h.uri()),
        Some("https://example.com"),
        "link followed the delete shift"
    );
}

#[test]
fn link_survives_resize_reflow() {
    // A column resize reflows rows; the link map is re-keyed per column (#46).
    let mut t = Engine::new(5, 2);
    t.feed(OPEN);
    t.feed(b"L");
    t.feed(CLOSE);
    t.resize(3, 2); // column change -> reflow
    assert_eq!(
        t.link_at(0, 0).as_ref().map(|h| h.uri()),
        Some("https://example.com"),
        "link survives reflow"
    );
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

/// A URI carrying an unencoded `;` survives whole (#650).
///
/// `OSC 8 ; params ; URI` is `;`-separated, so vte hands the handler the URI in pieces; reading
/// `params[2]` alone kept the first piece and dropped the rest, silently. xterm.js special-cases
/// exactly this — it splits on the **first** `;` only, *"to support unencoded semi-colons in the
/// URIs"* (`InputHandler.ts:3106`) — and nothing is lost at the parser, so this is a rejoin.
///
/// Reachable without anything exotic: `?a=1;b=2` is a legal query string, and `;` is a legal
/// filename byte, so `ls --hyperlink=auto` can emit one inside a `file://` URI.
#[test]
fn a_uri_keeps_its_unencoded_semicolons() {
    let mut t = Engine::new(60, 2);
    t.feed(b"\x1b]8;;https://example.com/p?a=1;b=2\x07X\x1b]8;;\x07");
    assert_eq!(
        t.link_at(0, 0).map(|h| h.uri().to_owned()).as_deref(),
        Some("https://example.com/p?a=1;b=2"),
        "the tail after the first ';' belongs to the URI, not to another parameter",
    );
}

/// The URI field and the `id=` field are independent: a semicolon-carrying URI still groups, and
/// grouping still keys on the *whole* URI (#635 + #650 together).
///
/// Worth its own test because the two fixes touch the same handler: rejoining the tail changes what
/// the group key is built from, so a rejoin that dropped the tail *after* keying would group two
/// different URIs that happen to share a prefix.
#[test]
fn a_semicolon_uri_still_groups_by_id_on_its_whole_value() {
    let mut t = Engine::new(60, 4);
    t.feed(b"\x1b]8;id=q;https://x/p?a=1;b=2\x07A\x1b]8;;\x07\r\n");
    t.feed(b"\x1b]8;id=q;https://x/p?a=1;b=2\x07B\x1b]8;;\x07");
    assert_eq!(
        t.link_at(0, 0).map(|h| h.uri().to_owned()).as_deref(),
        Some("https://x/p?a=1;b=2"),
        "the id= parse must not have eaten part of the URI",
    );
    assert_eq!(
        t.frame().link_table.len(),
        1,
        "same id, same whole URI — one link",
    );

    // …and a URI differing only *after* the first ';' is a different link. This is the assertion a
    // key built from the truncated value would fail.
    let mut u = Engine::new(60, 4);
    u.feed(b"\x1b]8;id=q;https://x/p?a=1;b=2\x07A\x1b]8;;\x07\r\n");
    u.feed(b"\x1b]8;id=q;https://x/p?a=1;b=9\x07B\x1b]8;;\x07");
    assert_eq!(
        u.frame().link_table.len(),
        2,
        "the group key sees the whole URI, so these are two links",
    );
}

/// The rejoin must not swallow the close. `OSC 8 ; ;` arrives as `["8", "", ""]`, whose rejoined
/// tail is the empty string — still a close (#650).
#[test]
fn an_empty_uri_still_closes_after_the_rejoin() {
    let mut t = Engine::new(60, 2);
    t.feed(b"\x1b]8;;https://example.com/a;b\x07X\x1b]8;;\x07Y");
    assert!(t.link_at(0, 0).is_some(), "X is inside the link");
    assert!(
        t.link_at(0, 1).is_none(),
        "Y is after the close — a rejoin that produced a non-empty URI would leave it open",
    );
}

/// A percent-encoded `%3B` is passed through untouched — the engine never decodes a URI.
///
/// Kept as a test rather than a comment because it is the control that pinned #650's cause to the
/// *raw* `;`, and because it forecloses a future "fix" that starts decoding: `Hyperlink::uri` hands
/// the target over exactly as declared, and whether it is openable is consumer policy (ADR-0017).
#[test]
fn a_percent_encoded_semicolon_is_not_decoded() {
    let mut t = Engine::new(60, 2);
    t.feed(b"\x1b]8;;https://example.com/a%3Bb=c\x07X\x1b]8;;\x07");
    assert_eq!(
        t.link_at(0, 0).map(|h| h.uri().to_owned()).as_deref(),
        Some("https://example.com/a%3Bb=c"),
    );
}

/// An `id=`-grouped link reaches the consumer as **one** link across two lines (#635).
///
/// This is the assertion that matches the user-visible symptom, and it is deliberately
/// made after `encode`/`decode` rather than on the `Frame`: a consumer never sees an
/// engine-side allocation, it sees a frame-local index into `link_table`, and grouping
/// cells by that index is literally what `justerm-web/src/links.ts` does. So "hovering
/// one half of a wrapped link highlights the other half" is true exactly when the two
/// rows' indices are equal here — an in-crate `Arc` identity check cannot say that.
#[test]
fn an_id_grouped_link_round_trips_as_one_link_across_two_lines() {
    let mut t = Engine::new(20, 4);
    // The case `id=` exists for: one logical link the application had to emit as two runs.
    t.feed(b"\x1b]8;id=grp;https://example.com/split\x07first\x1b]8;;\x07\r\n");
    t.feed(b"\x1b]8;id=grp;https://example.com/split\x07second\x1b]8;;\x07");

    let frame = t.frame();
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded, frame, "round-trip is lossless");

    // One entry, because one link — the whole point. Two entries is the pre-#635 defect,
    // and it is what the consumer would group by.
    assert_eq!(
        decoded.link_table,
        vec!["https://example.com/split".to_string()],
        "the grouped link ships once, not once per run",
    );

    // Both runs point at it. Collected across every span so this does not depend on how
    // the damage happened to be split into spans.
    let mut indices: Vec<u32> = decoded
        .spans
        .iter()
        .flat_map(|s| s.links.values().map(|i| i.get()))
        .collect();
    indices.dedup();
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(
        indices,
        vec![1],
        "every linked cell on both lines carries the same link index",
    );

    // And the halves really are on different lines — otherwise the assertion above would
    // be satisfied by one run and prove nothing about grouping.
    let lines: std::collections::BTreeSet<u16> = decoded
        .spans
        .iter()
        .filter(|s| !s.links.is_empty())
        .map(|s| s.line)
        .collect();
    assert_eq!(lines.len(), 2, "the two runs are on two separate lines");
}
