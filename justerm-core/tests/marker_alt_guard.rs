//! #164 — `add_marker` (#118) must be alt-screen-guarded like `add_command_mark`
//! (#158). Markers anchor *primary* content; on the alt screen the current row is
//! transient alt content, so `add_marker` there would pin a primary line the user
//! never marked (it surfaces on `marker_positions` after leaving alt). xterm scopes
//! such a marker to the alt buffer + clears it on leave; justerm has one marker
//! list, so it declines (a no-op returning a dead id). Surfaced by #158's
//! fix-verification pass.

use justerm_core::Engine;

/// Calling `add_marker` on the alt screen creates NO primary marker — nothing to
/// anchor there. The bug surfaces after leaving alt: a primary-anchored marker
/// would otherwise appear on `frame().overlay.markers`.
#[test]
fn add_marker_on_alt_is_a_no_op() {
    let mut t = Engine::new(10, 5);
    t.feed(b"a\r\nb\r\nc\r\nd\r\ne");

    t.feed(b"\x1b[?1049h"); // enter alt
    let _id = t.add_marker(2); // consumer misuse: mark on the alt screen
    t.feed(b"\x1b[?1049l"); // leave alt

    assert!(
        t.frame().overlay.markers.is_empty(),
        "no primary marker should be created on the alt screen"
    );
}

/// The reserved dead id an alt `add_marker` returns is distinct from a real
/// marker's id, and `remove_marker` on it is an inert no-op that leaves the real
/// marker intact.
#[test]
fn alt_add_marker_dead_id_is_removable_no_op() {
    let mut t = Engine::new(10, 5);
    t.feed(b"hello");

    t.feed(b"\x1b[?1049h");
    let dead = t.add_marker(0); // alt no-op → dead id
    t.remove_marker(dead); // inert (no panic, no marker existed)
    t.feed(b"\x1b[?1049l");

    let live = t.add_marker(0); // primary → a real marker
    assert_ne!(dead, live, "the dead alt id must not be reused");
    assert_eq!(
        t.frame().overlay.markers.len(),
        1,
        "only the primary marker exists"
    );
}

/// The alt dead id must never alias a *real* marker id — even across a RIS
/// (`ESC c`) that resets the id counter. A real marker's id is safe on reset
/// because RIS fires `MarkerDisposed` for it first (#160); the dead id gets no
/// such event (it was never in the list), so it must come from a reserved space
/// the live counter never hands out — otherwise `remove_marker(dead)` after RIS
/// would delete the wrong marker (#164 completeness).
#[test]
fn alt_dead_id_never_aliases_a_real_marker_across_ris() {
    let mut t = Engine::new(10, 5);
    t.feed(b"hello");

    t.feed(b"\x1b[?1049h");
    let dead = t.add_marker(0); // alt no-op → reserved dead id
    t.feed(b"\x1b[?1049l");
    t.feed(b"\x1bc"); // RIS — resets the marker id counter to 0

    let live = t.add_marker(0); // real primary marker after reset
    assert_ne!(
        dead, live,
        "the dead id must not alias a real marker after RIS"
    );

    t.remove_marker(dead); // must be a no-op, not remove the real marker
    assert_eq!(
        t.frame().overlay.markers.len(),
        1,
        "the real marker survives remove_marker(dead)"
    );
}

/// A normal primary-screen `add_marker` still works (the guard is alt-only).
#[test]
fn add_marker_on_primary_still_registers() {
    let mut t = Engine::new(10, 5);
    t.feed(b"a\r\nb\r\nc");

    let _id = t.add_marker(1);

    assert_eq!(t.frame().overlay.markers.len(), 1);
}
