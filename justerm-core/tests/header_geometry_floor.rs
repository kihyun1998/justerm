//! #663 — a frame's *own* declared geometry is read against the floor the engine holds.
//!
//! `decode` validated every part of a frame against the header (#582) and never validated
//! the header itself, so a frame declaring `cols: 0` or `cols: 1` decoded `Ok` — a screen
//! `justerm-core` defines as impossible and clamps away at every entry point
//! (`MIN_COLUMNS = 2`, #547; `rows` clamped to 1 by the same constructor).
//!
//! **Why 2 is not an arbitrary number**, which is the whole reason this is worth a check:
//! two independent implementations arrive at the same floor from the same cause. xterm.js
//! clamps identically — `MINIMUM_COLS = 2`, commented *"Less than 2 can mess with wide
//! chars"* (`common/services/BufferService.ts:13` @ `699f553`), applied on every resize
//! (`common/CoreTerminal.ts:192`), with `MINIMUM_ROWS = 1`. justerm enforces the same pair
//! because a wide glyph needs a `WIDE_CHAR` lead *and* its `WIDE_CHAR_SPACER`, which is
//! ADR-0025 D4's stated precondition. A check for it therefore imports no policy.
//!
//! **The ceiling is deliberately not the symmetric half of this, and the asymmetry is
//! recorded so it does not get "completed" later.** xterm.js has no maximum at all, and
//! the one layer that could know one — `justerm-renderer` — refuses to guess it, asking
//! the GL implementation and adopting what it grants (*"Do not try to predict the
//! limit"*). Any ceiling here would be the only arbitrary constant in the stack. It also
//! needs none: `cols` and `rows` are read as `u16`, and `MAX_COLUMNS`/`MAX_ROWS` are
//! `u16::MAX` for exactly that representational reason, so the upper bound is satisfied by
//! the field's own width. `65535 × 65535` is a legal engine geometry and decodes below.
//!
//! **This is new strictness on a published contract, and it is a different argument from
//! #582's** — which is why the two were not folded together. #582 rejects a coordinate a
//! consumer *writes through* (`cell-mirror.ts` keeps the viewport as one flat array, so an
//! out-of-frame column silently overwrites the next row). Nothing of that kind is reachable
//! here: after #582 a `cols: 1` frame can carry no span at all except `left == right == 0`,
//! and a `cols: 0` frame can carry none, so the degenerate geometry allocates *less*, not
//! more. The grade is honest — inert today. What the check buys is `decode`'s stated
//! contract (ADR-0008): it rejects what no frame could contain, and a 1-column frame is
//! exactly that.
//!
//! **No `WIRE_VERSION` bump**, on #582's own grounds: the byte layout is unchanged, and a
//! bump would make every published v14 decoder reject byte-identical frames.

use justerm_core::{DecodeError, Engine, Frame, FrameKind, MIN_COLUMNS, decode, encode};

/// Header offsets (`serialize.rs`): MAGIC(2) · VERSION(1) · has_scroll(1) · kind(1) ·
/// cols(2) · rows(2). Mirrors `span_bounds.rs` — named rather than inlined so a header
/// change breaks one place.
const COLS_AT: usize = 5;
const ROWS_AT: usize = 7;

/// A frame carrying no payload at all, so the only thing under test is the header. Built
/// by hand rather than by the engine on purpose: the engine cannot *produce* a geometry
/// below the floor, which is the producer-side test at the bottom of this file.
fn bare(cols: u16, rows: u16) -> Frame {
    Frame {
        cols,
        rows,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: justerm_core::CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        evicted_total: 0,
        marker_epoch: 0,
        marker_count: 0,
        mouse_events: Default::default(),
        alt_screen: false,
        scroll: None,
        spans: vec![],
        link_table: vec![],
        overlay: Default::default(),
    }
}

#[test]
fn a_frame_declaring_fewer_than_two_columns_is_rejected() {
    for cols in [0u16, 1] {
        assert_eq!(
            decode(&encode(&bare(cols, 24))),
            Err(DecodeError::BadGeometry),
            "cols = {cols} is below MIN_COLUMNS, a width the engine clamps away at every \
             entry point — no frame it produces can declare it"
        );
    }
}

#[test]
fn a_frame_declaring_no_rows_is_rejected() {
    assert_eq!(
        decode(&encode(&bare(80, 0))),
        Err(DecodeError::BadGeometry),
        "the row floor is 1 — `Term::with_scrollback` clamps `rows` to it, and xterm.js's \
         MINIMUM_ROWS is the same number. A 0-row terminal is not a small screen, it is \
         not a screen"
    );
}

/// The control that keeps the two floors from drifting upward. Written as the boundary
/// pair rather than as "a normal frame decodes", because a guard written `<=` instead of
/// `<` passes every ordinary geometry and fails only here.
#[test]
fn the_floor_is_exactly_the_engines_own_floor() {
    assert_eq!(
        MIN_COLUMNS, 2,
        "the check is pinned to the engine's constant"
    );
    let smallest = bare(MIN_COLUMNS as u16, 1);
    assert_eq!(
        decode(&encode(&smallest)),
        Ok(smallest),
        "2×1 is the narrowest screen the engine can hold, so it is a frame it can produce"
    );
}

/// The ceiling is not this issue's half, and this pins that it was not quietly added.
#[test]
fn the_upper_end_is_left_alone_because_the_field_already_bounds_it() {
    let widest = bare(u16::MAX, u16::MAX);
    assert_eq!(
        decode(&encode(&widest)),
        Ok(widest),
        "MAX_COLUMNS/MAX_ROWS are u16::MAX because the header field is u16 — the ceiling \
         is representational and needs no check. A rejection here would be the only \
         arbitrary constant in the stack"
    );
}

/// The realistic shape of the defect: a frame that was legitimate when produced, whose
/// *declared* geometry is shrunk in transit. `decode` reads bytes a consumer hands back
/// over its own transport (ADR-0008; `tests/robustness.rs` names them
/// attacker-influenced), so this is input, not an internal inconsistency.
#[test]
fn shrinking_a_real_frames_declared_geometry_below_the_floor_is_rejected() {
    let mut e = Engine::new(9, 2);
    e.feed(b"abcdefghi");
    let frame = e.frame();
    let clean = encode(&frame);
    assert_eq!(decode(&clean), Ok(frame), "unpatched, it round-trips");

    let mut narrow = clean.clone();
    narrow[COLS_AT..COLS_AT + 2].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode(&narrow),
        Err(DecodeError::BadGeometry),
        "the header is judged before its payload is: this frame's span is *also* out of \
         bounds for a 1-column grid, and reporting BadSpan would name the consequence \
         instead of the cause"
    );

    let mut short = clean;
    short[ROWS_AT..ROWS_AT + 2].copy_from_slice(&0u16.to_le_bytes());
    assert_eq!(
        decode(&short),
        Err(DecodeError::BadGeometry),
        "same on the row axis, and same reason for the variant"
    );
}

/// The producer half of the safety claim, made standing rather than measured once and
/// deleted. The engine is *asked* for every geometry at and below the floor; the frames it
/// answers with must all survive their own decoder, or this change has narrowed the
/// contract onto something justerm itself emits.
///
/// The material matters: a wide glyph is what the floor exists for, so the stream feeds
/// one at every width. `Engine::new(1, ..)` used to be a supported size (#547 changed
/// that), which is exactly why the clamp — not the request — is what the wire sees.
#[test]
fn no_geometry_this_engine_produces_is_below_the_floor() {
    let mut frames = 0usize;
    for cols in 0usize..=4 {
        for rows in 0usize..=3 {
            let mut e = Engine::new(cols, rows);
            e.feed("ab한cd\r\nef".as_bytes());
            let frame = e.frame();
            assert!(
                frame.cols >= MIN_COLUMNS as u16 && frame.rows >= 1,
                "Engine::new({cols}, {rows}) produced a frame declaring \
                 {}x{} — below the floor its own decoder now enforces",
                frame.cols,
                frame.rows
            );
            assert!(
                decode(&encode(&frame)).is_ok(),
                "Engine::new({cols}, {rows}) produced a frame its own decoder rejects"
            );
            frames += 1;
        }
    }
    assert_eq!(
        frames, 20,
        "the sweep is the evidence; it must not silently shrink"
    );
}
