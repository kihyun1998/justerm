//! #129 — the frame carries the mouse tracking mode as a *wanted-events* bitmask
//! so a frame-mode consumer (justerm-web) routes a mouse/wheel event autonomously
//! (to the app vs. local selection/scrollback) without re-implementing the VT
//! protocol→events table. The mask is the single source shared with
//! `encode_mouse`'s restriction; the wire carries the derived bits (a flag, not a
//! coordinate). Encoding never crosses — the backend encodes via `encode_mouse`.

use justerm_core::{Engine, Modifiers, MouseAction, MouseButton, MouseEvent, MouseEvents};

/// Enabling Normal mouse tracking (?1000) makes the frame report that the app
/// wants button down/up and wheel events — but not motion. The tracer: a mode
/// set via the VT stream surfaces on the frame as a routing mask.
#[test]
fn frame_carries_mouse_wanted_events_for_normal_tracking() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1000h"); // DECSET ?1000 — Normal mouse tracking

    assert_eq!(
        term.frame().mouse_events,
        MouseEvents::DOWN | MouseEvents::UP | MouseEvents::WHEEL
    );
}

/// No tracking by default — the mask is empty, so the consumer keeps every mouse
/// event local (selection / scrollback).
#[test]
fn frame_mouse_events_empty_without_tracking() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");
    assert_eq!(term.frame().mouse_events, MouseEvents::empty());
}

/// X10 (?9) reports button *press only* — crucially NOT the wheel — so a wheel
/// turn under X10 still scrolls locally. The routing-critical edge.
#[test]
fn frame_mouse_events_x10_is_down_only() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?9h");
    assert_eq!(term.frame().mouse_events, MouseEvents::DOWN);
}

/// AnyEvent (?1003) reports everything including bare motion, so even a hover
/// (no button) routes to the app.
#[test]
fn frame_mouse_events_any_includes_bare_motion() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b[?1003h");
    assert_eq!(
        term.frame().mouse_events,
        MouseEvents::DOWN
            | MouseEvents::UP
            | MouseEvents::WHEEL
            | MouseEvents::DRAG
            | MouseEvents::MOVE
    );
}

/// The single-source invariant (#129): across every (mode × action × button)
/// combination, `encode_mouse` reports an event IFF it is not a phantom
/// wheel-release AND the frame's wanted-events mask contains the event's
/// category. This is what "the wire mask and the encode gate share one source"
/// means — if `wanted_events` and `encode_mouse` ever diverge, this trips.
#[test]
fn encode_gate_matches_the_frame_mask_for_every_combination() {
    let modes: &[&[u8]] = &[
        b"",
        b"\x1b[?9h",
        b"\x1b[?1000h",
        b"\x1b[?1002h",
        b"\x1b[?1003h",
    ];
    let actions = [
        MouseAction::Press,
        MouseAction::Release,
        MouseAction::Motion,
    ];
    let buttons = [Some(MouseButton::Left), Some(MouseButton::WheelUp), None];

    for set in modes {
        for action in actions {
            for button in buttons {
                let mut t = Engine::new(80, 24);
                if !set.is_empty() {
                    t.feed(set);
                }
                let mask = t.frame().mouse_events;
                let ev = MouseEvent {
                    button,
                    action,
                    col: 5,
                    row: 5,
                    px: 0,
                    py: 0,
                    mods: Modifiers::empty(),
                };
                let is_wheel = matches!(button, Some(MouseButton::WheelUp));
                let category = match (action, button.is_some()) {
                    (MouseAction::Press, _) if is_wheel => MouseEvents::WHEEL,
                    (MouseAction::Press, _) => MouseEvents::DOWN,
                    (MouseAction::Release, _) => MouseEvents::UP,
                    (MouseAction::Motion, true) => MouseEvents::DRAG,
                    (MouseAction::Motion, false) => MouseEvents::MOVE,
                };
                let wheel_release = action == MouseAction::Release && is_wheel;
                let expected = !wheel_release && mask.contains(category);
                assert_eq!(
                    t.encode_mouse(ev).is_some(),
                    expected,
                    "set={set:?} action={action:?} button={button:?}"
                );
            }
        }
    }
}
