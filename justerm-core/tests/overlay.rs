//! #108 overlay — selection + search-match highlight spans projected onto the
//! viewport and carried on the frame, so a frame-mode consumer (web) can paint
//! highlights without an in-process model query. Built TDD, one behaviour per
//! test. Coordinates are viewport (re-projected in `frame()` against the scroll
//! offset, the single anchoring authority); the overlay holds positions only —
//! highlight colour is the consumer's (theme-agnostic).

use justerm_core::{Engine, Overlay, SelectionSpan, SelectionType, Side};

// ===========================================================================
// Selection rides the frame — the tracer bullet
// ===========================================================================

/// A live selection appears on the frame's overlay as the same viewport spans
/// `selection_range` reports — `frame()` projects the engine-owned selection so
/// the wire consumer paints it. "hello", cols 0..=4 selected → one span row 0.
#[test]
fn frame_overlay_carries_selection_spans() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 4, Side::Right);

    assert_eq!(
        term.frame().overlay.selection,
        vec![SelectionSpan {
            row: 0,
            left: 0,
            right: 4
        }]
    );
}

// ===========================================================================
// Search highlights ride the frame
// ===========================================================================

/// Search matches are not engine-owned (the consumer holds the `Vec<Match>` for
/// next/prev navigation), so the engine cannot project them on its own. The
/// consumer hands the active highlight set back with `set_search_highlights`;
/// `frame()` then projects each to viewport spans. Searching "ell" in "hello"
/// and highlighting it → one match span on row 0, cols 1..=3.
#[test]
fn frame_overlay_carries_search_highlight_spans() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    let matches = term.search("ell");
    term.set_search_highlights(matches);

    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 0,
            left: 1,
            right: 3
        }]
    );
}

// ===========================================================================
// Default / clear — the empty overlay
// ===========================================================================

/// With nothing selected and no highlights set, the overlay is empty on both
/// axes — the append-only zero case the wire section must also round-trip.
#[test]
fn frame_overlay_is_empty_without_interaction() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    assert_eq!(term.frame().overlay, Overlay::default());
}

/// Clearing the selection and emptying the highlight set drops them from the
/// overlay — the consumer dismissing a selection / closing the search box.
#[test]
fn frame_overlay_drops_cleared_selection_and_highlights() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 4, Side::Right);
    let matches = term.search("ell");
    term.set_search_highlights(matches);
    assert_ne!(term.frame().overlay, Overlay::default()); // populated

    term.selection_clear();
    term.set_search_highlights(vec![]);
    assert_eq!(term.frame().overlay, Overlay::default()); // both gone
}

// ===========================================================================
// Scroll re-projection — the anchoring authority (why overlay rides the frame)
// ===========================================================================

/// The overlay is re-projected against the scroll offset by `frame()` itself, so
/// scrolling shifts the highlight rows and drops the parts now off-screen — the
/// single anchoring authority that justifies carrying overlay on the frame
/// rather than a side channel. A match on abs line 4 (bottom screen of a 3-row
/// view) sits at row 2; scrolling up by two pushes it below the viewport, so the
/// next frame's overlay carries no match span.
#[test]
fn frame_overlay_reprojects_on_scroll() {
    let mut term = Engine::new(4, 3);
    term.feed(b"L0\r\nL1\r\nL2\r\nL3\r\nXX"); // sb=[L0,L1], screen=[L2,L3,XX]

    let matches = term.search("XX"); // abs line 4, cols 0..=1
    term.set_search_highlights(matches);
    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 2,
            left: 0,
            right: 1
        }]
    );

    term.scroll_up(2); // viewport now abs 0..=2; "XX" (abs 4) is below it
    assert_eq!(term.frame().overlay.matches, vec![]);
}

// ===========================================================================
// Search-highlight invalidation — query-derived data, not user-authored
// ===========================================================================
//
// Unlike the selection (a user-drawn range the engine re-anchors through buffer
// mutation), search matches are a *derived result set*: when output evicts lines
// or a reflow re-wraps them, the correct set changes (new matches appear, old
// ones move/merge), so the stored set is no longer authoritative. The engine
// holds matches, not the query, so it cannot re-derive — it invalidates instead,
// and the consumer re-searches (xterm/alacritty re-search on output). Pure
// scrollback scrolling does NOT shift absolute coordinates, so it must preserve.

/// Output that evicts scrollback shifts every absolute index, so the stored
/// match set is stale — the engine drops it rather than highlight wrong content.
#[test]
fn frame_overlay_highlights_invalidate_on_eviction() {
    let mut term = Engine::with_scrollback(10, 2, 2); // cap = 2 scrollback lines
    term.feed(b"AAA\r\nBBB\r\nTARGET\r\nCCC"); // sb=[AAA,BBB], screen=[TARGET,CCC]

    let matches = term.search("TARGET");
    term.set_search_highlights(matches);
    assert!(!term.frame().overlay.matches.is_empty()); // lit on screen

    // New output scrolls "TARGET" into history and the cap evicts it; absolute
    // indices shift, so the held match no longer points at "TARGET".
    term.feed(b"\r\nDDD\r\nEEE\r\nFFF");
    assert_eq!(term.frame().overlay.matches, vec![]); // invalidated, not stale
}

/// A column resize reflows soft-wrapped lines, changing match coordinates (and
/// possibly the match set), so the held highlights are invalidated.
#[test]
fn frame_overlay_highlights_invalidate_on_reflow() {
    let mut term = Engine::new(6, 4);
    term.feed(b"abcdef"); // one width-6 row

    let matches = term.search("cd");
    term.set_search_highlights(matches);
    assert!(!term.frame().overlay.matches.is_empty());

    term.resize(3, 4); // reflow → "abc"/"def"
    assert_eq!(term.frame().overlay.matches, vec![]);
}

/// Pure scrollback scrolling keeps absolute coordinates valid, so the held
/// highlights survive — the "search, then scroll through the hits" UX stays lit.
#[test]
fn frame_overlay_highlights_survive_pure_scroll() {
    let mut term = Engine::new(4, 2);
    term.feed(b"L0\r\nHIT\r\nL2\r\nL3"); // sb=[L0,HIT], screen=[L2,L3]

    let matches = term.search("HIT"); // abs line 1
    term.set_search_highlights(matches);
    assert_eq!(term.frame().overlay.matches, vec![]); // off-screen at bottom

    term.scroll_up(1); // viewport abs 1..=2 → "HIT" (abs 1) visible at row 0
    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 0,
            left: 0,
            right: 2
        }]
    );
}

/// A screen swap invalidates the highlights: the matches index the primary
/// buffer, so projecting them onto the alt grid would paint stale content.
/// Like the selection, they cannot survive the swap.
#[test]
fn frame_overlay_highlights_invalidate_on_alt_screen_switch() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    let matches = term.search("ell");
    term.set_search_highlights(matches);
    assert!(!term.frame().overlay.matches.is_empty());

    term.feed(b"\x1b[?1049h"); // enter alt screen
    assert_eq!(term.frame().overlay.matches, vec![]);
}
