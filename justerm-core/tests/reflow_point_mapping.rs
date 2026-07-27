//! #549 — reflow maps a tracked point by dividing its offset by `new_cols`, which assumes every
//! emitted row holds exactly `new_cols` content cells. It does not.
//!
//! The re-split loop deliberately emits a **short** row whenever the row would end on a
//! `WIDE_CHAR` lead (`grid.rs`, the `take -= 1` guard): the pair must not be split, so the lead
//! moves to the next row and the vacated column becomes the wrap artefact. That guard is correct
//! and stays. What was wrong is the arithmetic that ran *after* the loop and re-derived a fact the
//! loop already owned — `new_points[pi] = (start + off / new_cols, off % new_cols)`.
//!
//! Every wide glyph that lands on a re-split boundary therefore shifted the true position of
//! everything after it by one, and the errors accumulate: two short rows before a point put it two
//! cells off, which is enough to cross into a neighbouring row and land on an unrelated glyph.
//!
//! ADR-0025 files this as a **D1 read-side** violation — the loop owns each segment's real extent
//! (`[i, i + take)`), so the mapping reads the owner instead of recomputing it. All three
//! references decide the position where the real extent is known, and none divides an offset by the
//! new width:
//!
//! - **xterm.js precomputes the same array** — `reflowSmallerGetNewLineLengths`
//!   (`common/buffer/BufferReflow.ts:179` @ `699f553`), documented as *"pre-compute the wrapping
//!   points since wide characters may need to be wrapped onto the following line"*, yielding
//!   `newCols` or `newCols - 1` per row. That is the closest prior art for this design.
//! - **ghostty** moves a tracked pin by assignment from the write cursor's live position inside its
//!   reflow loop (`terminal/PageList.zig:1650-1659` @ `e6e26e1`); its `tracked_pins` is the closest
//!   analogue of `points` — anchors *and* marks, not just the cursor.
//! - **alacritty** `alacritty_terminal/src/grid/resize.rs:169-188` @ `852e971` — `if i ==
//!   cursor_buffer_line && reflow` adjusts the cursor while that line is being processed, against
//!   `num_wrapped`, the count of cells actually wrapped.
//!
//! Everything riding `points` is affected: selection anchors, the cursor, and the OSC-133 command
//! columns. Points at offset 0 of a logical line are immune, so this bites *intra-line* offsets.

use justerm_core::{Engine, SelectionType, Side, decode, encode};

/// Select exactly one cell and read it back.
fn select_cell(t: &mut Engine, row: usize, col: usize) {
    t.selection_begin(row, col, Side::Left, SelectionType::Char);
    t.selection_extend(row, col, Side::Right);
}

// ---- the drift ------------------------------------------------------------------------------

#[test]
fn a_wide_glyph_on_the_boundary_does_not_drift_a_selection() {
    // `"ab한cd"` on 6 columns; re-split at 3 puts 한's lead at the boundary, so the first row is
    // short. Everything after it was reported one cell early.
    let mut t = Engine::new(6, 10);
    t.feed("ab한cd\r\nZZZZ".as_bytes());
    select_cell(&mut t, 0, 5);
    assert_eq!(t.selection_text().as_deref(), Some("d"), "fixture");

    t.resize(3, 10);

    assert_eq!(t.selection_text().as_deref(), Some("d"));
}

#[test]
fn the_same_shape_without_a_wide_glyph_is_the_control() {
    // Identical in every respect except that the boundary glyph is narrow, so no row is short.
    // This is what makes the test above a statement about the wide pair and not about resize.
    let mut t = Engine::new(6, 10);
    t.feed(b"abxcd\r\nZZZZ");
    select_cell(&mut t, 0, 4);
    assert_eq!(t.selection_text().as_deref(), Some("d"), "fixture");

    t.resize(3, 10);

    assert_eq!(t.selection_text().as_deref(), Some("d"));
}

#[test]
fn the_drift_accumulates_across_several_short_rows() {
    // `"ab한cde한f"` re-split at 3 emits TWO short rows, so the old arithmetic put a point after
    // both of them two cells early — far enough to land on an unrelated glyph rather than merely
    // one column off. Cells: a b 한(2,3) c d e 한(7,8) f.
    let mut t = Engine::new(12, 10);
    t.feed("ab한cde한f".as_bytes());
    select_cell(&mut t, 0, 9);
    assert_eq!(t.selection_text().as_deref(), Some("f"), "fixture");

    t.resize(3, 10);

    assert_eq!(
        t.selection_text().as_deref(),
        Some("f"),
        "two short rows before the point, so the old mapping was two cells off"
    );
}

#[test]
fn the_cursor_rides_the_same_mapping() {
    let mut t = Engine::new(6, 10);
    t.feed("ab한cd".as_bytes());
    assert_eq!((t.cursor().row, t.cursor().col), (0, 5), "fixture");

    t.resize(3, 10);

    // Rows after the re-split: "ab"+artefact | 한 c | d.
    //
    // `(2, 1)`, not `(2, 0)`: the fixture fills all six columns, so the cursor is parked on `d`
    // with the wrap **deferred** — it is logically one past `d`, and `d` lands at `(2, 0)`. This
    // assertion originally read `(2, 0)` because `resize` used to drop `pending_wrap`, which put
    // the cursor *on* `d` and let the next byte overwrite it. Both this expectation and that
    // reset moved in the same change; the drift this test is about is unchanged either way, and
    // `a_wide_glyph_on_the_boundary_does_not_drift_a_selection` pins it without the cursor.
    assert_eq!((t.cursor().row, t.cursor().col), (2, 1));
}

#[test]
fn an_osc133_command_mark_rides_the_same_mapping() {
    // The command columns are tracked points too, and a drifted mark sends a consumer's
    // "jump to previous command" to the wrong row.
    let mut t = Engine::new(6, 10);
    t.feed(b"\x1b]133;A\x07");
    t.feed("ab한cd".as_bytes());
    t.feed(b"\x1b]133;B\x07");
    let before = t.command_marks();
    assert_eq!(before.len(), 2, "fixture: prompt-start and command-start");

    t.resize(3, 10);

    let after = t.command_marks();
    assert_eq!(
        after[0].1, 0,
        "the prompt start is at offset 0, so it never drifts"
    );
    assert_eq!(
        after[1].1, 2,
        "the command start was at the cursor, which now sits on the third row"
    );
}

#[test]
fn the_drift_reaches_a_frame_mode_consumer_through_the_wire() {
    // The surface a real consumer actually sees. penterm resizes the engine in lockstep with its
    // PTY (`src-tauri/src/pty/manager.rs:1018`) and ships `encode(&engine.frame())` to its
    // webview, whose header carries `cursor_row` / `cursor_col` — so this defect renders the
    // caret on the wrong cell after a window resize over wrapped CJK, with no error anywhere.
    // Asserted through the encode→decode round-trip (ADR-0005) rather than through `cursor()`,
    // because the wire is what crosses the boundary.
    let mut t = Engine::new(6, 10);
    t.feed("ab한cd".as_bytes());

    t.resize(3, 10);

    let frame = decode(&encode(&t.frame())).expect("round-trip");
    // One past `d`, for the reason spelled out in `the_cursor_rides_the_same_mapping`.
    assert_eq!((frame.cursor_row, frame.cursor_col), (2, 1));
}

// ---- side conditions: what must NOT change ---------------------------------------------------

#[test]
fn a_point_past_a_partial_last_row_stays_on_it() {
    // Offset == line length with room left on the row: the point belongs just after the content,
    // on the same row. `"abcde"` at 3 columns is "abc" | "de", and the cursor sits at column 2.
    let mut t = Engine::new(6, 10);
    t.feed(b"abcde");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 5), "fixture");

    t.resize(3, 10);

    assert_eq!((t.cursor().row, t.cursor().col), (1, 2));
}

#[test]
fn a_point_past_a_full_last_row_moves_to_the_next_row() {
    // The mirror: offset == line length with the last row *full*, so there is no column left and
    // the point belongs at the start of the row after it. Pinned because the loop-based mapping
    // has no segment containing that offset and must not clamp it back onto the full row.
    let mut t = Engine::new(8, 10);
    t.feed(b"abcdef\r\nQQ");
    t.feed(b"\x1b[1;7H");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 6), "fixture");

    t.resize(3, 10);

    assert_eq!((t.cursor().row, t.cursor().col), (2, 0));
}

#[test]
fn a_point_past_a_full_last_row_of_the_last_line_does_not_name_a_row_that_does_not_exist() {
    // The mirror of the test above at the buffer's end, where `(last_row + 1, 0)` has no row to
    // land on. `Grid::row` indexes `lines` directly and a selection anchor is written back
    // unclamped, so this was an index-out-of-bounds **panic** — a library crashing its consumer,
    // the #536 class.
    //
    // It is pinned here rather than merely fixed because this change would otherwise have *widened*
    // it: the old arithmetic went out of range only when `line.len() % new_cols == 0`, which a
    // short row makes impossible, so a line with a wide pair on the boundary — this change's whole
    // subject — was safe before. Caught by the refuting lens, not by me.
    let mut t = Engine::new(12, 2);
    t.feed("ab한cdef".as_bytes()); // 8 cells; at 3 columns: "ab"+artefact | 한 c | d e f
    t.selection_begin(0, 8, Side::Left, SelectionType::Char);
    t.selection_extend(0, 8, Side::Right);

    t.resize(3, 2);

    let spans = t.selection_range();
    assert_eq!(spans.len(), 1);
    assert!(
        spans[0].row < t.grid().rows(),
        "the anchor must name a row that exists"
    );
}

#[test]
fn a_wide_glyph_that_does_not_land_on_the_boundary_changes_nothing() {
    // The negative control for the guard itself: `"ab한cd"` re-split at **4** fits the pair
    // without a short row, so the mapping had nothing to get wrong and must stay put.
    let mut t = Engine::new(6, 10);
    t.feed("ab한cd".as_bytes());

    t.resize(4, 10);

    // Rows: "ab한" | "cd". One past `d` at `(1, 1)` is `(1, 2)` — again the deferred wrap, not the
    // mapping, which is exactly this test's point: no short row is emitted at width 4.
    assert_eq!((t.cursor().row, t.cursor().col), (1, 2));
    assert_eq!(t.accessible_text().trim_end(), "ab한cd");
}

#[test]
fn a_point_on_an_empty_logical_line_stays_at_its_start() {
    // The `line.is_empty()` branch emits one blank row and runs no segment loop at all, so the
    // mapping has no segment to read and must fall back to that row.
    //
    // The empty line is deliberately **not** the first one: `"AAAA"` re-splits into two rows at
    // three columns, so the blank row lands at index 2 and the answer `(start, 0)` is
    // distinguishable from a bare `(0, 0)`. An earlier version of this test put the cursor on the
    // first line, where `start == 0` made the two indistinguishable and the test stayed green with
    // the branch broken — caught by mutating it.
    let mut t = Engine::new(6, 10);
    t.feed(b"AAAA\r\n\r\nQQ");
    t.feed(b"\x1b[2;1H"); // the empty line
    assert_eq!((t.cursor().row, t.cursor().col), (1, 0), "fixture");

    t.resize(3, 10);

    assert_eq!((t.cursor().row, t.cursor().col), (2, 0));
}
