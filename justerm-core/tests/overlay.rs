//! #108 overlay — selection + search-match highlight spans projected onto the
//! viewport and carried on the frame, so a frame-mode consumer (web) can paint
//! highlights without an in-process model query. Built TDD, one behaviour per
//! test. Coordinates are viewport (re-projected in `frame()` against the scroll
//! offset, the single anchoring authority); the overlay holds positions only —
//! highlight colour is the consumer's (theme-agnostic).

use justerm_core::{
    Engine, MarkerKind, MarkerPosition, Overlay, SelectionSpan, SelectionType, Side, TermEvent,
};

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
/// consumer hands the highlight set back with `set_search_highlights`;
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

// ===========================================================================
// Active search match — the fifth overlay group (#428, #424 slice 2)
// ===========================================================================
//
// *Which* match is active is consumer policy (next/prev navigation), so the
// consumer designates an index into the set it last passed to
// `set_search_highlights`; the engine projects that member through the same
// `match_spans` mechanism into `overlay.active_match`. The active match also
// stays in `matches` — overlap is resolved downstream by the renderer's
// ranking (#424 slice 1), not by exclusion here.

/// Designating an index projects that member's spans into the active group,
/// while the full set still rides `matches` unchanged — ranking, not exclusion,
/// resolves the overlap downstream.
#[test]
fn frame_overlay_carries_active_match_spans() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello hello");

    let matches = term.search("ell"); // two hits: cols 1..=3 and 7..=9
    assert_eq!(matches.len(), 2);
    term.set_search_highlights(matches);
    term.set_active_search_highlight(Some(1));

    let overlay = term.frame().overlay;
    assert_eq!(
        overlay.active_match,
        vec![SelectionSpan {
            row: 0,
            left: 7,
            right: 9
        }]
    );
    // The active member is NOT removed from the match group.
    assert_eq!(
        overlay.matches,
        vec![
            SelectionSpan {
                row: 0,
                left: 1,
                right: 3
            },
            SelectionSpan {
                row: 0,
                left: 7,
                right: 9
            }
        ]
    );
}

/// Highlights without a designation project no active spans — the consumer has
/// not (yet) chosen a current match; `None` also clears a prior designation.
#[test]
fn frame_overlay_active_empty_without_designation() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    let matches = term.search("ell");
    term.set_search_highlights(matches);
    assert_eq!(term.frame().overlay.active_match, vec![]); // never designated

    term.set_active_search_highlight(Some(0));
    assert!(!term.frame().overlay.active_match.is_empty());
    term.set_active_search_highlight(None);
    assert_eq!(term.frame().overlay.active_match, vec![]); // cleared
}

/// Handing over a new highlight set resets the designation: a stale index into
/// a *different* set could be accidentally in range and light wrong content.
/// The consumer re-designates after every hand-over.
#[test]
fn set_search_highlights_resets_active_designation() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello hello");

    let matches = term.search("ell");
    term.set_search_highlights(matches);
    term.set_active_search_highlight(Some(0));
    assert!(!term.frame().overlay.active_match.is_empty());

    let matches = term.search("hello"); // new set (index 0 exists — would be wrong content)
    term.set_search_highlights(matches);
    assert_eq!(term.frame().overlay.active_match, vec![]);
}

/// An out-of-range index projects nothing rather than erroring — lenient like
/// an invalid regex yielding no matches (#314).
#[test]
fn frame_overlay_active_out_of_range_projects_nothing() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    let matches = term.search("ell"); // one hit
    term.set_search_highlights(matches);
    term.set_active_search_highlight(Some(5));

    assert_eq!(term.frame().overlay.active_match, vec![]);
    assert!(!term.frame().overlay.matches.is_empty()); // the set itself still rides
}

/// The active group re-projects against the scroll offset exactly like the
/// match group — same anchoring authority, same `match_spans` math.
#[test]
fn frame_overlay_active_reprojects_on_scroll() {
    let mut term = Engine::new(4, 3);
    term.feed(b"L0\r\nL1\r\nL2\r\nL3\r\nXX"); // sb=[L0,L1], screen=[L2,L3,XX]

    let matches = term.search("XX"); // abs line 4, cols 0..=1
    term.set_search_highlights(matches);
    term.set_active_search_highlight(Some(0));
    assert_eq!(
        term.frame().overlay.active_match,
        vec![SelectionSpan {
            row: 2,
            left: 0,
            right: 1
        }]
    );

    term.scroll_up(2); // viewport now abs 0..=2; "XX" (abs 4) is below it
    assert_eq!(term.frame().overlay.active_match, vec![]);
}

/// A match spanning soft-wrapped rows projects *all* its spans into the active
/// group — one per visible row, like the match group (AC: wrapped rows).
#[test]
fn frame_overlay_active_spans_wrapped_match() {
    let mut term = Engine::new(4, 3);
    term.feed(b"abcdef"); // wraps: "abcd" / "ef"

    let matches = term.search("cde"); // spans the soft wrap
    term.set_search_highlights(matches);
    term.set_active_search_highlight(Some(0));

    assert_eq!(
        term.frame().overlay.active_match,
        vec![
            SelectionSpan {
                row: 0,
                left: 2,
                right: 3
            },
            SelectionSpan {
                row: 1,
                left: 0,
                right: 0
            }
        ]
    );
}

/// Invalidation kills the active projection with the set: once eviction drops
/// the highlights, a held designation has nothing valid to point at.
#[test]
fn frame_overlay_active_dies_with_invalidated_highlights() {
    let mut term = Engine::with_scrollback(10, 2, 2);
    term.feed(b"AAA\r\nBBB\r\nTARGET\r\nCCC");

    let matches = term.search("TARGET");
    term.set_search_highlights(matches);
    term.set_active_search_highlight(Some(0));
    assert!(!term.frame().overlay.active_match.is_empty());

    term.feed(b"\r\nDDD\r\nEEE\r\nFFF"); // eviction shifts absolute indices
    let overlay = term.frame().overlay;
    assert_eq!(overlay.matches, vec![]); // the set is invalidated…
    assert_eq!(overlay.active_match, vec![]); // …and the active dies with it
}

// ===========================================================================
// Active match by SPAN — decoupled from the held set (#436)
// ===========================================================================
//
// A capping backend hands over a truncated highlight set; past the cap the
// index designation has nothing to point at, so navigation painted nothing —
// a regression vs pre-#429 (selection painted regardless). xterm's model: the
// active decoration is created FROM THE FOUND RESULT, outside the capped
// highlight list (`DecorationManager.createActiveDecoration`) — so the engine
// now accepts the designation by absolute span too, independent of the set.

/// #436: a span designation projects even when the designated match is NOT in
/// the held (capped) set — the past-cap current match gets its active emphasis
/// while the match group honestly lacks it.
#[test]
fn active_match_designated_by_span_projects_outside_the_held_set() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello hello hello");

    let matches = term.search("ell"); // three hits: cols 1..=3, 7..=9, 13..=15
    assert_eq!(matches.len(), 3);
    let past_cap = matches[2]; // the backend's cap drops this one from the set…
    term.set_search_highlights(matches[..2].to_vec());
    term.set_active_search_match(Some(past_cap)); // …but designates it by span

    let overlay = term.frame().overlay;
    assert_eq!(
        overlay.active_match,
        vec![SelectionSpan {
            row: 0,
            left: 13,
            right: 15
        }]
    );
    // Honest side condition: the past-cap member has NO plain highlight — only
    // the active emphasis; the capped set still projects exactly its two.
    assert_eq!(overlay.matches.len(), 2);
    assert!(!overlay.matches.contains(&SelectionSpan {
        row: 0,
        left: 13,
        right: 15
    }));
}

/// A new hand-over resets a SPAN designation exactly like an index one — the
/// #428 contract is representation-independent (a stale span after the set
/// changed could paint content the new query never matched).
#[test]
fn set_search_highlights_resets_a_span_designation_too() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello hello");

    let matches = term.search("ell");
    let m = matches[0];
    term.set_search_highlights(matches);
    term.set_active_search_match(Some(m));
    assert!(!term.frame().overlay.active_match.is_empty());

    let matches = term.search("hello");
    term.set_search_highlights(matches); // hand-over → designation void
    assert_eq!(term.frame().overlay.active_match, vec![]);
}

/// Invalidation (eviction shifting absolute coordinates) kills a SPAN
/// designation too — a stored span would otherwise keep projecting at
/// coordinates that now hold arbitrary other text. Same funnel as the set
/// itself (`invalidate_search_highlights` covers eviction, region scroll,
/// reflow, and both alt-swap directions).
#[test]
fn a_span_designation_dies_with_invalidated_highlights() {
    let mut term = Engine::with_scrollback(10, 2, 2);
    term.feed(b"AAA\r\nBBB\r\nTARGET\r\nCCC");

    let matches = term.search("TARGET");
    let m = matches[0];
    term.set_search_highlights(matches);
    term.set_active_search_match(Some(m));
    assert!(!term.frame().overlay.active_match.is_empty());

    term.feed(b"\r\nDDD\r\nEEE\r\nFFF"); // eviction shifts absolute indices
    assert_eq!(term.frame().overlay.active_match, vec![]);
}

// ===========================================================================
// Top-anchored SUB-REGION scroll — anchors below the bottom margin (#449)
// ===========================================================================
//
// A top-anchored sub-region scroll (`scroll_top == 0`, `scroll_bottom <
// rows-1`, primary — e.g. DECSTBM with a bottom status line) ACCRUES the
// evicted top row to scrollback (ADR-0009; alacritty's `Grid::scroll_up` does
// the same for `region.start == 0`, swapping the fixed bottom lines back).
// Rows below the margin stay fixed on screen, but scrollback grew — so their
// concatenated ABSOLUTE index shifts +1 while their content does not move.
// Anchors must follow the content: selection/markers re-anchor +1 (what
// alacritty gets free from screen-relative coordinates), query-derived search
// highlights invalidate (the drop-not-reanchor policy, #108).
//
// Scene: rows=4, DECSTBM [1..3] → scroll_top=0, scroll_bottom=2; "TARGET"
// lives on the fixed bottom row (grid row 3); an LF at the region's bottom
// row triggers one accrual sub-region scroll.

/// Search highlights (and the active designation) below the margin invalidate
/// on a sub-region scroll — before #449 they stale-painted one row off.
#[test]
fn sub_region_scroll_invalidates_highlights_below_the_margin() {
    let mut term = Engine::new(20, 4);
    term.feed(b"\x1b[1;3r\x1b[4;1HTARGET");

    let matches = term.search("TARGET");
    let m = matches[0];
    term.set_search_highlights(matches);
    term.set_active_search_match(Some(m));
    assert_eq!(
        term.frame().overlay.matches,
        vec![SelectionSpan {
            row: 3,
            left: 0,
            right: 5
        }]
    );

    term.feed(b"\x1b[3;1H\n"); // LF at the region bottom → accrual sub-region scroll
    let overlay = term.frame().overlay;
    assert_eq!(overlay.matches, vec![], "invalidated, not stale-shifted");
    assert_eq!(overlay.active_match, vec![], "the designation dies with it");
}

/// A selection on the fixed bottom row keeps tracking its CONTENT — the
/// viewport row must not move (the content did not move on screen).
#[test]
fn sub_region_scroll_keeps_a_selection_below_the_margin_anchored() {
    let mut term = Engine::new(20, 4);
    term.feed(b"\x1b[1;3r\x1b[4;1HTARGET");

    term.selection_begin(3, 0, Side::Left, SelectionType::Char);
    term.selection_extend(3, 5, Side::Right);
    assert_eq!(term.selection_text().as_deref(), Some("TARGET"));

    term.feed(b"\x1b[3;1H\n");
    assert_eq!(
        term.selection_range(),
        vec![SelectionSpan {
            row: 3,
            left: 0,
            right: 5
        }],
        "the selection stays on the fixed row"
    );
    assert_eq!(
        term.selection_text().as_deref(),
        Some("TARGET"),
        "…and still extracts the same content"
    );
}

/// A marker on the fixed bottom row keeps tracking its content the same way.
#[test]
fn sub_region_scroll_keeps_a_marker_below_the_margin_anchored() {
    let mut term = Engine::new(20, 4);
    term.feed(b"\x1b[1;3r\x1b[4;1HTARGET");

    let id = term.add_marker(3);
    term.feed(b"\x1b[3;1H\n");

    let positions = term.frame().overlay.markers;
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].id, id);
    assert_eq!(positions[0].row, 3, "the marker stays on the fixed row");
}

/// At the scrollback cap the accrual (+1 below the margin) and the eviction
/// (−1 everywhere) compose to net zero for the fixed rows — pins the ordering.
#[test]
fn sub_region_scroll_at_the_cap_composes_the_shifts() {
    let mut term = Engine::with_scrollback(20, 4, 1); // cap = 1 line
    term.feed(b"\x1b[1;3r\x1b[4;1HTARGET");
    term.feed(b"\x1b[3;1H\n"); // fills scrollback to its cap
    let id = term.add_marker(3);
    term.selection_begin(3, 0, Side::Left, SelectionType::Char);
    term.selection_extend(3, 5, Side::Right);

    term.feed(b"\x1b[3;1H\n"); // at cap: accrue (+1) then evict (−1)

    assert_eq!(
        term.selection_range(),
        vec![SelectionSpan {
            row: 3,
            left: 0,
            right: 5
        }]
    );
    assert_eq!(term.selection_text().as_deref(), Some("TARGET"));
    let positions = term.frame().overlay.markers;
    assert_eq!(
        (positions.len(), positions[0].id, positions[0].row),
        (1, id, 3)
    );
}

// ===========================================================================
// Markers — stable line anchors for decorations (#118)
// ===========================================================================

/// Registering a marker at a viewport row makes it ride the frame's overlay as
/// a `(id, row)` position the consumer can anchor a decoration to. The tracer:
/// add_marker returns a handle, frame() projects its line back to the viewport.
#[test]
fn frame_overlay_carries_marker_position() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    let id = term.add_marker(0); // mark viewport row 0

    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }]
    );
}

/// A marker re-anchors when cap eviction shifts the absolute indices down: it
/// keeps pointing at its content line, not the line that slid into its old slot.
/// Mark "L2"; one more line evicts "L0" and shifts everything down one; scrolled
/// to where "L2" now sits, the marker is still on it.
#[test]
fn frame_overlay_marker_reanchors_on_eviction() {
    let mut term = Engine::with_scrollback(4, 2, 2); // cap = 2 scrollback lines
    term.feed(b"L0\r\nL1\r\nL2\r\nL3"); // sb=[L0,L1], screen=[L2,L3]

    let id = term.add_marker(0); // "L2" (viewport row 0, abs 2)
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }]
    );

    term.feed(b"\r\nL4"); // sb=[L1,L2], screen=[L3,L4]; "L0" evicted, indices -1
    term.scroll_up(1); // viewport abs 1..=2 = [L2, L3]; "L2" at row 0
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }] // followed "L2", not stuck on abs 2
    );
}

/// When the marked line itself is evicted, the marker is disposed — dropped from
/// the frame and announced via `TermEvent::MarkerDisposed` so the consumer can
/// remove its decoration (it cannot otherwise tell "gone" from "scrolled away").
#[test]
fn frame_overlay_marker_disposed_when_its_line_is_evicted() {
    let mut term = Engine::with_scrollback(4, 2, 2); // cap = 2
    term.feed(b"L0\r\nL1\r\nL2\r\nL3"); // sb=[L0,L1], screen=[L2,L3]
    term.scroll_up(2); // viewport abs 0..=1 = [L0, L1]

    let id = term.add_marker(0); // "L0" (abs 0) — the oldest line
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }]
    );
    term.drain_events(); // clear unrelated events

    term.feed(b"\r\nL4"); // pushes a line; cap evicts "L0" — the marked line
    assert!(
        term.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "disposal must be announced"
    );
    assert_eq!(term.frame().overlay.markers, vec![]); // and dropped from the frame
}

/// A scroll-region scroll moves content within the screen, so a marker inside
/// the region rotates with it (like the selection). Region rows 2..=4, "A/B/C/D";
/// mark "C"; a region scroll slides "C" up one row and the marker follows.
#[test]
fn frame_overlay_marker_rotates_with_region_scroll() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[2;4r"); // DECSTBM: region rows 2..4 (0-based 1..=3)
    term.feed(b"A\r\nB\r\nC\r\nD"); // rows 0=A,1=B,2=C,3=D

    let id = term.add_marker(2); // "C" (abs 2)
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 2,
            kind: MarkerKind::Plain
        }]
    );

    term.feed(b"\r\n"); // line-feed at the bottom margin → region scrolls up
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 1,
            kind: MarkerKind::Plain
        }] // "C" now at row 1
    );
}

/// A marker on the line that scrolls out of the region is disposed (the content
/// is gone), announced like any disposal. Region rows 2..=4; mark "B" at the
/// region top; a region scroll-up drops it.
#[test]
fn frame_overlay_marker_disposed_on_region_scroll_out() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[2;4r"); // region 0-based 1..=3
    term.feed(b"A\r\nB\r\nC\r\nD");

    let id = term.add_marker(1); // "B" at the region top (abs 1)
    term.drain_events();

    term.feed(b"\r\n"); // region scrolls up → "B" leaves the region
    assert!(term.drain_events().contains(&TermEvent::MarkerDisposed(id)));
    assert_eq!(term.frame().overlay.markers, vec![]);
}

/// A column resize reflows soft-wrapped lines, moving content's coordinates —
/// the marker reflows with its content (like the selection anchor). "abcdefXY"
/// soft-wraps at width 6 into "abcdef"/"XY"; mark "XY" (row 1). Narrowed to
/// width 3 the logical line becomes "abc"/"def"/"XY", so "XY" is now row 2 and
/// the marker followed it.
#[test]
fn frame_overlay_marker_reflows_on_resize() {
    let mut term = Engine::new(6, 4);
    term.feed(b"abcdefXY"); // row 0 = "abcdef" (WRAPLINE), row 1 = "XY"

    let id = term.add_marker(1); // "XY" wrapped row (abs 1)
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 1,
            kind: MarkerKind::Plain
        }]
    );

    term.resize(3, 4); // reflow → "abc"/"def"/"XY"
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 2,
            kind: MarkerKind::Plain
        }] // "XY" followed to row 2
    );
}

/// A resize *while on the alt screen* still reflows the (inactive) primary
/// markers, since they anchor primary content — so on return to the primary the
/// marker sits at its reflowed row. Same "abcdefXY" case, resized under alt.
#[test]
fn frame_overlay_marker_reflows_during_alt_excursion() {
    let mut term = Engine::new(6, 4);
    term.feed(b"abcdefXY"); // primary: row0="abcdef"(wrap), row1="XY"
    let id = term.add_marker(1); // "XY" (abs 1)

    term.feed(b"\x1b[?1049h"); // enter alt
    assert_eq!(term.frame().overlay.markers, vec![]); // dormant on alt
    term.resize(3, 4); // resize WHILE on alt → primary reflows underneath
    term.feed(b"\x1b[?1049l"); // leave alt → primary

    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 2,
            kind: MarkerKind::Plain
        }] // reflowed to row 2 despite the excursion
    );
}

/// A marker SURVIVES an alt-screen excursion — unlike the selection (cleared)
/// and search highlights (invalidated). It anchors primary content, which is
/// frozen while the alt screen is up, so it is merely dormant (absent from alt
/// frames, no disposal) and reappears on return to the primary.
#[test]
fn frame_overlay_marker_survives_alt_screen_excursion() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    let id = term.add_marker(0);
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }]
    );
    term.drain_events();

    term.feed(b"\x1b[?1049h"); // enter alt screen
    assert_eq!(term.frame().overlay.markers, vec![]); // dormant, not emitted
    assert!(
        !term.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "an alt excursion must not dispose the marker"
    );

    term.feed(b"\x1b[?1049l"); // leave alt screen → primary
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }] // back, still anchored
    );
}

/// `remove_marker` disposes a marker explicitly: it drops from the frame and
/// fires `MarkerDisposed` (one disposal channel regardless of cause — eviction
/// or explicit removal — like xterm's `dispose()` always firing onDispose).
#[test]
fn remove_marker_disposes_and_drops_it() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");
    let id = term.add_marker(0);
    term.drain_events();

    term.remove_marker(id);

    assert!(term.drain_events().contains(&TermEvent::MarkerDisposed(id)));
    assert_eq!(term.frame().overlay.markers, vec![]);
}

/// A marker scrolled off-screen is omitted from the frame but stays ALIVE — no
/// disposal — so it reappears when scrolled back. Off-screen (absent) must be
/// distinguishable from disposed (gone): only the latter fires an event.
#[test]
fn frame_overlay_offscreen_marker_stays_alive() {
    let mut term = Engine::new(4, 2);
    term.feed(b"MK\r\nL1"); // screen=[MK,L1], abs MK=0

    let id = term.add_marker(0); // "MK" (abs 0)
    term.drain_events();

    term.feed(b"\r\nL2\r\nL3"); // MK scrolls into scrollback (not evicted), off-screen
    assert_eq!(term.frame().overlay.markers, vec![]); // omitted while off-screen
    assert!(
        !term.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "scrolling off-screen must not dispose"
    );

    term.scroll_up(2); // bring "MK" back into view
    assert_eq!(
        term.frame().overlay.markers,
        vec![MarkerPosition {
            id,
            row: 0,
            kind: MarkerKind::Plain
        }] // alive, reappears
    );
}

/// RIS (full reset) wipes the buffer, so every marker's line is gone — each is
/// disposed and announced. Without the event the consumer would leak decorations
/// (and the id counter resets, so new markers could collide with stale ones).
#[test]
fn frame_overlay_markers_disposed_on_full_reset() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");
    let id = term.add_marker(0);
    term.drain_events();

    term.feed(b"\x1bc"); // RIS — full reset to power-on state

    assert!(
        term.drain_events().contains(&TermEvent::MarkerDisposed(id)),
        "RIS must announce marker disposal"
    );
    assert_eq!(term.frame().overlay.markers, vec![]);
}
