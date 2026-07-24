//! The constructor must honour the row floor `resize` already enforces.
//!
//! This is **not** the mirror of `min_columns.rs`, and the difference is the whole point.
//! `MIN_COLUMNS = 2` was a *contract change* (#547): one column used to be a supported size and
//! is not any more, so `resize(1, r)` silently returns two and every consumer had to be told.
//! The row floor changes no contract — `Term::resize` has always done `rows.max(1)`, and the
//! comment beside it states the rule outright: *"A terminal is never 0-tall"*. The constructor
//! simply never enforced it, while carrying the identical `scroll_bottom: rows - 1` expression,
//! so `Engine::new(cols, 0)` panicked with a subtract overflow before reaching any of it.
//!
//! No `MIN_ROWS` constant is published, deliberately. `MIN_COLUMNS` earned publication because
//! it is surprising (ask for one, get two) and its reason is non-obvious (a width-2 glyph needs
//! a lead *and* a spacer); a floor of one row is neither. alacritty publishes both
//! (`MIN_COLUMNS`, `MIN_SCREEN_LINES`) because its *app* reads them across a crate boundary —
//! justerm clamps internally, so nothing outside needs the value. xterm.js's `MINIMUM_ROWS = 1`
//! is likewise internal to `common/services/BufferService.ts`, applied in its constructor
//! (`:42`) exactly as here, and ghostty rejects a zero dimension outright
//! (`Terminal.zig:3721`, `error.InvalidValue`). All three floor or reject; none of them panics.

use justerm_core::Engine;

/// A zero-row screen is clamped, not fatal. The panic this replaces was a subtract overflow in
/// `Term::with_scrollback`, so it fired during construction — before a caller could observe
/// anything about the engine it asked for.
#[test]
fn construction_floors_rows_at_one() {
    assert_eq!(Engine::new(80, 0).grid().rows(), 1);
    assert_eq!(Engine::with_scrollback(80, 0, 100).grid().rows(), 1);

    // Above the floor is untouched.
    assert_eq!(Engine::new(80, 24).grid().rows(), 24);
}

/// Both dimensions degenerate at once — the case that reaches the column clamp and the row
/// clamp in the same call.
#[test]
fn a_fully_degenerate_screen_is_usable() {
    let mut term = Engine::new(0, 0);
    assert_eq!(term.grid().cols(), justerm_core::MIN_COLUMNS);
    assert_eq!(term.grid().rows(), 1);

    // And it survives ordinary input rather than merely existing.
    term.feed("한".as_bytes());
    assert!(term.grid().row(0)[0].is_wide());
    assert_eq!(term.frame().rows, 1);
}

/// The constructor now agrees with `resize`, which is the whole claim: the same screen shape
/// results whether a zero row count arrives at construction or at resize.
#[test]
fn the_constructor_and_resize_floor_rows_alike() {
    let built = Engine::new(8, 0);

    let mut resized = Engine::new(8, 5);
    resized.resize(8, 0);

    assert_eq!(built.grid().rows(), resized.grid().rows());
}
