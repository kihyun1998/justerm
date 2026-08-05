//! #721 — the live marker population is bounded, because the stream allocates it.
//!
//! `add_command_mark` appends one marker per OSC 133 sequence with no per-line dedup,
//! and `markers_evict_oldest` only drops a marker whose line has reached absolute 0.
//! A stream that never emits a newline therefore piles every mark on the cursor's line,
//! where nothing can evict it: measured at 70 000 live markers in a 24-row buffer.
//! `Engine::feed` is an untrusted entry point (ADR-0007), so that is a defect and not a
//! configuration.
//!
//! The bound is [`MAX_MARKERS`], derived from the wire the way `MAX_COLUMNS` is — both
//! marker group counts are `u16`, so a population past `u16::MAX` wraps its declared
//! count while writing every record and `decode` returns `Ok` on garbage. Overflow
//! disposes the *oldest* marker through `TermEvent::MarkerDisposed`, the channel
//! scrollback eviction already uses.

use justerm_core::{Engine, MAX_MARKERS, MarkerId, TermEvent};

/// Feed `n` OSC 133 prompt marks with **no newline**, so every one anchors on the
/// cursor's line and nothing can evict any of them.
fn pile(e: &mut Engine, n: usize) {
    for _ in 0..n {
        e.feed(b"\x1b]133;A\x07");
    }
}

/// Every live marker's id, in the engine's own order.
fn live_ids(e: &Engine) -> Vec<MarkerId> {
    // Asks the engine since v16 removed the wire group this read (#490).
    e.marker_index().markers.iter().map(|m| m.id).collect()
}

#[test]
fn the_population_stops_at_the_cap() {
    let mut e = Engine::new(80, 24);
    pile(&mut e, MAX_MARKERS + 3);

    assert_eq!(
        live_ids(&e).len(),
        MAX_MARKERS,
        "a stream past the cap must leave exactly MAX_MARKERS live — not more (the wire \
         count wraps) and not fewer (an overflow may not empty the buffer)"
    );
}

#[test]
fn overflow_drops_the_oldest_and_keeps_the_newest() {
    let mut e = Engine::new(80, 24);
    pile(&mut e, MAX_MARKERS + 3);
    let ids = live_ids(&e);

    // Ids are handed out sequentially from 0, so "oldest" is literally the lowest.
    let first = *ids.first().expect("population is not empty");
    let last = *ids.last().expect("population is not empty");
    assert_eq!(
        first,
        MarkerId(3),
        "the three overflowing pushes must retire ids 0,1,2 — the OLDEST. Dropping the \
         newest would silently discard the mark the stream just emitted"
    );
    assert_eq!(
        last,
        MarkerId(MAX_MARKERS as u32 + 2),
        "the most recent push must be present: a full population still accepts a mark"
    );
}

#[test]
fn each_retired_marker_is_announced_exactly_once() {
    let mut e = Engine::new(80, 24);
    pile(&mut e, MAX_MARKERS);
    e.drain_events(); // baseline: nothing before the cap is reached is under test

    pile(&mut e, 3);
    let disposed: Vec<MarkerId> = e
        .drain_events()
        .into_iter()
        .filter_map(|ev| match ev {
            TermEvent::MarkerDisposed(id) => Some(id),
            _ => None,
        })
        .collect();

    assert_eq!(
        disposed,
        vec![MarkerId(0), MarkerId(1), MarkerId(2)],
        "a consumer learns of a marker's death only through this event, so an overflow \
         drop must announce exactly the ids it retired, once each, oldest first"
    );
}

#[test]
fn a_population_below_the_cap_is_untouched() {
    let mut e = Engine::new(80, 24);
    pile(&mut e, 64);
    let disposed = e
        .drain_events()
        .into_iter()
        .filter(|ev| matches!(ev, TermEvent::MarkerDisposed(_)))
        .count();

    assert_eq!(live_ids(&e).len(), 64, "nothing is dropped below the cap");
    assert_eq!(
        disposed, 0,
        "and nothing is RETIRED — the cap must not fire on an ordinary population.          Filtered rather than counting every event: since #490 a creation announces          itself too, and this assertion is about the cap, not about the channel"
    );
}
