//! #621 — the engine publishes a maximum grid size, because the wire cannot carry a
//! larger one.
//!
//! The mirror of [`min_columns`]/[`min_rows`]: those floor a screen too small to hold a
//! wide glyph, this ceilings one too large to *describe*. The frame header stores `cols`
//! and `rows` as `u16` each (`serialize.rs`), so a grid wider than `u16::MAX` is accepted
//! by the engine and then silently misreported — measured before the clamp,
//! `Engine::new(70_000, 2)` produced a grid of 70 000 columns whose frame declared
//! `cols = 4464` and decoded `Ok`, so a consumer laid out 4464 columns of a 70 000-column
//! screen with no error anywhere.
//!
//! The floor and the ceiling answer *different* questions and that is why the constant is
//! not symmetric with `MIN_COLUMNS`: the floor is a semantic requirement (a wide glyph
//! needs two cells) derived from the references, while the ceiling is a representational
//! one (the header field is 16 bits) derived from this repo's own wire. No reference
//! bounds a grid this way because none of them serializes one — `docs/map/territory/wire-format.md`
//! records that the format choice has no prior art to compare against.
//!
//! The value is deliberately far above any real terminal: a 4K display at a very small
//! font is ~550 columns, and `u16::MAX` is two orders of magnitude past that. This clamp
//! is a representability backstop, not a policy about how big a terminal may be.

use justerm_core::{Engine, MAX_COLUMNS, MAX_ROWS, decode, encode};

#[test]
fn the_ceiling_is_the_widest_grid_the_frame_header_can_describe() {
    // The contract in one line: the clamp exists because `frame.cols` is a u16.
    assert_eq!(MAX_COLUMNS, u16::MAX as usize);
    assert_eq!(MAX_ROWS, u16::MAX as usize);
}

#[test]
fn a_grid_wider_than_the_header_is_clamped_rather_than_misreported() {
    let e = Engine::new(70_000, 2);
    assert_eq!(
        e.grid().cols(),
        MAX_COLUMNS,
        "the grid itself is clamped, not just the frame's view of it"
    );
    let frame = e.frame();
    assert_eq!(
        frame.cols as usize,
        e.grid().cols(),
        "the header describes the grid that exists — before #621 this was 4464 for a 70000-column grid"
    );
    let decoded = decode(&encode(&frame)).expect("round-trips");
    assert_eq!(decoded.cols, frame.cols);
}

#[test]
fn a_grid_taller_than_the_header_is_clamped_too() {
    let e = Engine::new(80, 70_000);
    assert_eq!(e.grid().rows(), MAX_ROWS);
    let frame = e.frame();
    assert_eq!(frame.rows as usize, e.grid().rows());
    assert_eq!(
        decode(&encode(&frame)).expect("round-trips").rows,
        frame.rows
    );
}

#[test]
fn resize_clamps_on_the_same_terms_as_the_constructor() {
    // Both entry points, or the ceiling holds only until the first resize — the same
    // gap `Engine::new` had against `resize`'s row floor before #547.
    //
    // One axis at a time, and that is the contract rather than a convenience: the clamp
    // bounds each axis, *not their product*. `resize(70_000, 70_000)` asks for
    // MAX_COLUMNS * MAX_ROWS ≈ 4.3e9 cells — at 12 bytes per `Cell`, ~51 GB for `grid`
    // and the same again for `alt_grid`, which the OS kills rather than serves (measured:
    // SIGKILL, not a failure). That is not a hole the ceiling was meant to close: it is a
    // *representability* backstop (the header field is 16 bits), never a promise that
    // every representable size can be allocated. Pushing the axes separately proves
    // exactly what the constant claims and nothing it does not.
    let mut wide = Engine::new(80, 24);
    wide.resize(70_000, 24);
    assert_eq!(wide.grid().cols(), MAX_COLUMNS);
    assert_eq!(wide.frame().cols as usize, MAX_COLUMNS);

    let mut tall = Engine::new(80, 24);
    tall.resize(80, 70_000);
    assert_eq!(tall.grid().rows(), MAX_ROWS);
    assert_eq!(tall.frame().rows as usize, MAX_ROWS);
}

#[test]
fn an_ordinary_grid_is_untouched_by_the_ceiling() {
    // The negative half: the clamp must not be reachable by anything real. A 4K display
    // at a very small font is ~550 columns; assert a generous ordinary size passes through
    // unchanged, so a future narrowing of the constant cannot silently start biting.
    let e = Engine::new(1000, 500);
    assert_eq!(e.grid().cols(), 1000);
    assert_eq!(e.grid().rows(), 500);
    let frame = e.frame();
    assert_eq!(frame.cols, 1000);
    assert_eq!(frame.rows, 500);
}
