//! #490 S1 — the basis a consumer needs to keep a pulled marker index valid.
//!
//! The frame carries every live marker's absolute line in every frame, which is
//! ADR-0020's R3 violation and measured at 37–70% of an 80x24 frame at ordinary
//! OSC-133 densities. The replacement is a *pull*: the consumer asks once and keeps
//! the answer. Keeping it valid needs exactly two things, and which one applies is a
//! property of how the buffer moved:
//!
//! - **Scrollback eviction moves every marker by the same −1**, so it is one number,
//!   not M facts: `evicted_total` counts lines popped since RIS and the consumer
//!   rebases by the delta.
//! - **Everything else moves markers non-uniformly** — reflow rewrites lines outright,
//!   a region scroll rotates only the markers inside the region — so no delta can
//!   express it. `epoch` bumps and the consumer re-pulls. That is affordable precisely
//!   because those mutations are rare relative to frames; eviction, which is not rare,
//!   is the one the scalar absorbs.

use justerm_core::{Engine, MarkerKind, TermEvent, decode, encode};

/// Feed `n` lines of output, one OSC 133 prompt mark every `mark_every` lines.
fn feed_lines(e: &mut Engine, n: usize, mark_every: Option<usize>) {
    for i in 0..n {
        if let Some(k) = mark_every
            && i % k == 0
        {
            e.feed(b"\x1b]133;A\x07");
        }
        e.feed(b"out\r\n");
    }
}

#[test]
fn the_index_lists_every_live_marker_with_its_absolute_line() {
    let mut e = Engine::new(20, 5);
    feed_lines(&mut e, 8, Some(4)); // marks at lines 0 and 4

    let ix = e.marker_index();
    let lines: Vec<u32> = ix.markers.iter().map(|m| m.line).collect();
    assert_eq!(
        lines,
        vec![0, 4],
        "absolute [scrollback ++ screen] lines, in the engine's own order — that order \
         is the precedence a consumer joins decorations by (#458/#461)"
    );
    assert!(
        ix.markers.iter().all(|m| m.kind == MarkerKind::PromptStart),
        "kind rides the pull, not the frame: it is static per marker"
    );
}

#[test]
fn eviction_advances_the_basis_and_leaves_the_epoch_alone() {
    // Scrollback 4 + 5 rows: the buffer is full after 9 lines, every further line evicts.
    let mut e = Engine::with_scrollback(20, 5, 4);
    feed_lines(&mut e, 9, None);
    let before = e.marker_index();

    feed_lines(&mut e, 3, None);
    let after = e.marker_index();

    assert_eq!(
        after.evicted_total - before.evicted_total,
        3,
        "three evicted lines, three counted — this is the whole point: the shift is \
         uniform, so it is ONE number rather than M move events"
    );
    assert_eq!(
        after.epoch, before.epoch,
        "and it must NOT bump the epoch. Eviction is the frequent mutation; if it \
         forced a re-pull the design would be O(M) per output line, worse than the \
         per-frame payload it replaces"
    );
}

#[test]
fn a_marker_survives_eviction_by_the_basis_delta() {
    // Fill the buffer FIRST, then mark: a mark placed before the fill sits at line 0
    // and is evicted (and disposed) before the pull, which is a different behaviour
    // and has its own event.
    let mut e = Engine::with_scrollback(20, 5, 4);
    feed_lines(&mut e, 9, None);
    e.feed(b"\x1b]133;A\x07");
    let pulled = e.marker_index();
    let held = pulled.markers[0].line;

    feed_lines(&mut e, 2, None);
    let now = e.marker_index();
    let rebased = held - (now.evicted_total - pulled.evicted_total) as u32;

    assert_eq!(
        now.epoch, pulled.epoch,
        "no re-pull was needed, so the rebase below is the only work the consumer did"
    );
    assert_eq!(
        rebased, now.markers[0].line,
        "the consumer's arithmetic must land on the engine's own answer"
    );
}

#[test]
fn a_region_scroll_that_moves_a_marker_bumps_the_epoch() {
    let mut e = Engine::new(20, 6);
    e.feed(b"\x1b[2;5r"); // DECSTBM rows 2..5
    e.feed(b"\x1b[3;1H\x1b]133;A\x07"); // a mark inside the region
    let before = e.marker_index();

    e.feed(b"\x1b[5;1H\r\n"); // scroll the region up by one

    let after = e.marker_index();
    assert_ne!(
        after.epoch, before.epoch,
        "a region scroll rotates only the markers inside the region, so no single \
         delta describes it — the consumer must re-pull"
    );
}

#[test]
fn a_region_scroll_that_moves_no_marker_does_not_bump() {
    let mut e = Engine::new(20, 6);
    e.feed(b"\x1b[2;5r");
    let before = e.marker_index();

    e.feed(b"\x1b[5;1H\r\n");

    let after = e.marker_index();
    assert_eq!(
        after.epoch, before.epoch,
        "the epoch reports that a HELD COORDINATE went stale, not that a verb ran. A \
         TUI with a scroll region and no marks would otherwise force a re-pull per \
         scrolled line, which is the cost this design exists to remove"
    );
}

#[test]
fn reflow_bumps_the_epoch() {
    let mut e = Engine::new(20, 5);
    e.feed(b"\x1b]133;A\x07");
    feed_lines(&mut e, 3, None);
    let before = e.marker_index();

    e.resize(10, 5);

    let after = e.marker_index();
    assert_ne!(
        after.epoch, before.epoch,
        "reflow rewrites marker lines outright — the one mutation no monotonic \
         coordinate survives, which is why the design re-baselines instead of \
         trying to survive it"
    );
}

#[test]
fn the_basis_rides_the_frame_header_and_round_trips() {
    let mut e = Engine::with_scrollback(20, 5, 4);
    feed_lines(&mut e, 12, None); // fill + evict
    e.feed(b"\x1b]133;A\x07");
    e.resize(10, 5); // bump the epoch

    let f = e.frame();
    let ix = e.marker_index();

    // Assert the fixture reaches the state under test before asserting anything about
    // it: at 0/0 the round-trip below would pass without carrying either field.
    assert!(
        f.evicted_total > 0 && f.marker_epoch > 0,
        "the fixture must actually evict and bump, or this test is vacuous \
         (evicted_total={}, marker_epoch={})",
        f.evicted_total,
        f.marker_epoch
    );
    assert_eq!(
        (f.evicted_total, f.marker_epoch),
        (ix.evicted_total, ix.epoch),
        "the header and the pull must report the SAME basis — a consumer compares one \
         against the other to decide whether to re-pull, so a skew between them is the \
         one thing that makes the whole scheme unusable"
    );

    let round = decode(&encode(&f)).expect("engine-produced frame decodes");
    assert_eq!(
        (round.evicted_total, round.marker_epoch),
        (f.evicted_total, f.marker_epoch),
        "and both survive the wire"
    );
}

/// #490 lens finding A — the protocol has three clauses, not two. A consumer that
/// pulls once must learn of markers born afterwards, or its index only ever shrinks.
/// The stream creates them (OSC 133), so "the consumer knows because it asked" is
/// false for the population that dominates.
#[test]
fn a_marker_born_after_the_pull_is_announced() {
    let mut e = Engine::new(20, 5);
    let pulled = e.marker_index();
    e.drain_events();

    e.feed(b"\x1b]133;A\x07");

    let created: Vec<_> = e
        .drain_events()
        .into_iter()
        .filter_map(|ev| match ev {
            TermEvent::MarkerCreated { id, line, kind } => Some((id, line, kind)),
            _ => None,
        })
        .collect();
    assert_eq!(created.len(), 1, "creation must be announced exactly once");
    let now = e.marker_index();
    assert_eq!(
        (created[0].0, created[0].1),
        (now.markers[0].id, now.markers[0].line),
        "and it must carry the same id and line a pull would have reported, so a \
         consumer can append instead of re-pulling"
    );
    assert_eq!(
        now.epoch, pulled.epoch,
        "creation must NOT bump the epoch: a bump costs an O(M) re-pull for O(1) \
         information, and at 4 marks per shell command that inverts the design"
    );
}

/// #490 lens finding B — RIS is the most destructive buffer mutation there is, and
/// the documented re-pull trigger did not fire for it.
#[test]
fn ris_is_visible_to_a_holder_of_the_basis() {
    let mut e = Engine::with_scrollback(20, 5, 4);
    for _ in 0..30 {
        e.feed(b"x\r\n");
    }
    e.feed(b"\x1b]133;A\x07");
    let held = e.marker_index();
    assert!(
        held.evicted_total > 0,
        "the fixture must have evicted, or the reset below proves nothing"
    );

    e.feed(b"\x1bc"); // RIS

    let now = e.marker_index();
    assert_ne!(
        now.epoch, held.epoch,
        "RIS rebuilds the buffer and reissues marker ids. If the epoch resets to the \
         value a quiet session already holds (0), the one signal a consumer is told to \
         watch never fires for the mutation that invalidates everything"
    );
}

/// #490 lens finding C — `justerm-web`'s fit loop re-asserts the size every frame, and
/// `ResizePort` states no idempotency guarantee, so a no-op resize is a real call.
#[test]
fn a_resize_that_changes_nothing_does_not_bump() {
    let mut e = Engine::new(20, 5);
    e.feed(b"\x1b]133;A\x07");
    let before = e.marker_index();

    for _ in 0..100 {
        e.resize(20, 5);
    }

    assert_eq!(
        e.marker_index().epoch,
        before.epoch,
        "100 no-op resizes must cost nothing — otherwise a consumer that re-asserts \
         its size per frame re-pulls the whole index per frame, which is the cost this \
         design exists to remove"
    );
}

// The v15 oracle test lived here — it rebased a pull and compared it against the frame's
// own `marker_lines`. v16 removed that group, so the comparand is gone and the test with
// it; what it proved (the rebase arithmetic lands on the engine's own answer) is still
// covered by `a_marker_survives_eviction_by_the_basis_delta` above, which asks the engine
// twice instead of asking the wire. `the_frame_reports_how_many_markers_are_live` is what
// replaces it as a standing cross-check, one dimension weaker on purpose: a count cannot
// verify a line, only that the consumer still holds the right number of them.

/// #490 v16 — the two marker groups leave the frame, and a **count** takes their place.
///
/// The count is not a shrunken group: it is the safety net for a consumer that wired the
/// pull but not the events. Comparing it against the size of a held index detects a
/// desync the events would otherwise have to be trusted for, so a mis-wired host
/// self-heals instead of painting decorations on lines it no longer owns.
#[test]
fn the_frame_reports_how_many_markers_are_live() {
    let mut e = Engine::new(20, 5);
    feed_lines(&mut e, 8, Some(2)); // several marks

    let f = e.frame();
    let ix = e.marker_index();
    assert!(
        ix.markers.len() > 1,
        "the fixture must hold several markers, or the equality below is vacuous"
    );
    assert_eq!(
        f.marker_count as usize,
        ix.markers.len(),
        "the header's count must be the population the pull reports — a consumer compares \
         the two to notice it has drifted"
    );
    assert_eq!(
        decode(&encode(&f)).expect("round-trip").marker_count,
        f.marker_count,
        "and it survives the wire"
    );
}

/// The count follows the *active* buffer, like the pull it is checked against — an
/// absolute index means a different thing on each screen.
#[test]
fn the_count_follows_the_active_buffer() {
    let mut e = Engine::new(20, 5);
    feed_lines(&mut e, 4, Some(2));
    let primary = e.frame().marker_count;
    assert!(primary > 0);

    e.feed(b"\x1b[?1049h"); // to the alt screen, which has its own (empty) population

    assert_eq!(
        e.frame().marker_count,
        0,
        "the alt screen reports its own population, not the primary's — otherwise a \
         consumer on alt would read a desync that is not there"
    );
}
