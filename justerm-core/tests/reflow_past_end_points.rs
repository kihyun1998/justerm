//! A tracked point can sit **one past the last cell** of its logical line, and reflow must carry
//! that fact instead of inventing a position inside the grid (#562).
//!
//! `grid::reflow` is handed three kinds of point — the cursor, selection anchors, and every OSC-133
//! command mark — as bare `(row, col)`. When a re-split's last row comes out **full**, "just after
//! the content" names a column that does not exist, and the three kinds do not want the same
//! answer for it:
//!
//! | kind | what "one past" means | right answer |
//! |---|---|---|
//! | cursor | the next write position | the start of the row after — a real cell |
//! | OSC-133 mark | an **exclusive** bound on the command text (`extract_lines` clips `[b, c)`) | `col == cols`, i.e. "all of this row" |
//! | selection anchor | a highlight endpoint | clamped inside the grid; UI state may not move app content |
//!
//! So `reflow` returns the honest logical answer — `col` may equal `new_cols` — and the seam in
//! `Term::resize` resolves it per kind. Collapsing the three into one grid coordinate is what made
//! a mark at the end of a line swallow the newline after it, and what folded the cursor back onto
//! the last glyph.
//!
//! Prior art covers **two** of those three rows. ghostty clamps a non-cursor tracked pin past the
//! content (`if (p.x >= cols_len) p.x = @min(p.x, self.page.size.cols - 1 - self.x)`) before it can
//! widen the row, and never clamps the cursor pin (`terminal/PageList.zig:1576-1606` @ `e6e26e1`).
//! The **mark** row is derived, not ported: ghostty's clamp lands a pin strictly *inside* the
//! destination, and it has no column-bearing semantic mark to want otherwise (`semantic_prompt` is
//! a row property). `col == cols` follows from `extract_lines` clipping `[b, c)` — the exclusive
//! end is the only value that can mean "all of this row".
//!
//! What this file deliberately does **not** fix, so the gap stays visible: the distance a point sits
//! past the content (`poff.min(line.len())` in the join) is still collapsed to "one past". Carrying
//! it needs destination rows to spill into, and spending those rows destroys content on a pane with
//! no scrollback — measured, and left to #562's open half.

use justerm_core::{Engine, SelectionType, Side};

/// `$ echo hello world` with OSC-133 B/C marks around the command, then a line of output.
fn shell_transcript(cols: usize) -> Engine {
    let mut t = Engine::new(cols, 6);
    t.feed(b"$ \x1b]133;B\x07echo hello world\x1b]133;C\x07\r\nhello world\r\n");
    t
}

fn command(t: &Engine) -> Option<String> {
    t.command_lines().first().map(|c| c.command.clone())
}

// ---------------------------------------------------------------------------
// the OSC-133 mark — "one past" is an exclusive bound and must stay one
// ---------------------------------------------------------------------------

#[test]
fn a_command_mark_ending_a_full_row_does_not_swallow_the_newline_after_it() {
    // The C mark sits just after `world`, which lands at the very end of a re-split row. Answering
    // with the *next* row's column 0 puts the bound on the first row of the NEXT logical line, so
    // `extract_lines` clips across the line break and picks up its `\n`.
    let mut t = shell_transcript(20);
    assert_eq!(command(&t).as_deref(), Some("echo hello world"), "fixture");

    t.resize(6, 6);

    assert_eq!(
        command(&t).as_deref(),
        Some("echo hello world"),
        "the command is the same text at a different width"
    );
}

#[test]
fn a_mark_past_a_full_row_is_not_truncated_by_a_resize() {
    // The other direction of the same collapse: clamping the bound back *onto* the row drops the
    // last character instead of adding one.
    let mut t = Engine::with_scrollback(8, 2, 0);
    t.feed(b"$ \x1b]133;B\x07ab\x1b]133;C\x07");
    assert_eq!(command(&t).as_deref(), Some("ab"), "fixture");

    t.resize(4, 2);

    assert_eq!(command(&t).as_deref(), Some("ab"));
}

#[test]
fn a_mark_at_a_filled_last_column_records_the_whole_command() {
    // **No resize at all.** `add_command_mark` stores `cursor.col`, and the cursor that has just
    // filled the last column is held at `cols - 1` with `pending_wrap` — so the mark was written
    // one column short of where it belongs, and every command that exactly fills a row lost its
    // last character. The same "one past is not representable" gap, at the *write* site.
    let mut t = Engine::new(6, 6);
    t.feed(b"$ \x1b]133;B\x07abcd\x1b]133;C\x07");

    assert_eq!(command(&t).as_deref(), Some("abcd"));
}

#[test]
fn a_command_start_mark_past_its_rows_content_does_not_prepend_a_newline() {
    // `extract_lines` clips `[from, to)` — `to` is exclusive and absorbs a one-past column, `from`
    // is **inclusive** and cannot: it selects an empty run and, the row being hard-ended, flushes
    // that run with a `\n` the command never contained.
    //
    // The trigger is the mark sitting past its row's **content**, not past its width, so it needs
    // no resize at all: an 8-column row holding a 6-column prompt is enough. A first attempt keyed
    // the guard on the row width and fixed only the resized case — the refuting lens caught that
    // the same fixture still failed without the resize.
    let mut t = Engine::new(8, 6);
    t.feed(b"abcdef\x1b]133;B\x07\r\ngh\x1b]133;C\x07\r\n");

    assert_eq!(command(&t).as_deref(), Some("gh"), "no resize involved");

    t.resize(3, 6);

    assert_eq!(command(&t).as_deref(), Some("gh"));
}

#[test]
fn a_command_reports_the_document_line_it_is_actually_on() {
    // The command's `line` is the a11y "jump to previous command" target, and it must name the line
    // the *command text* is on — not where the prompt's B mark happened to be emitted. Both come
    // from the same start position, so normalising one and not the other made them disagree: the
    // text moved to line 1 while the reported line stayed 0.
    let mut t = Engine::new(8, 6);
    t.feed(b"abcdef\x1b]133;B\x07\r\ngh\x1b]133;C\x07\r\n");

    let before = t.command_lines().first().map(|c| c.line);
    t.resize(3, 6);

    assert_eq!(before, Some(1), "no resize involved");
    assert_eq!(t.command_lines().first().map(|c| c.line), Some(1));
}

#[test]
fn a_selection_starting_past_a_full_row_skips_to_the_next_line() {
    // The `from` normalisation above is reached by `selection_text` too, and the change is
    // deliberate rather than collateral: alacritty does the same thing in `selection.rs`
    // (`range_simple` — *"Wrap to next line when selection starts to the right of last column"* —
    // sets `start.point.column = Column(0); start.point.line += 1`). Pinned because nothing else
    // covers this path and the comment at the seam only talks about OSC-133.
    let mut t = Engine::with_scrollback(6, 3, 100);
    t.feed(b"abcdef\r\nghijkl\r\n");
    t.selection_begin(0, 5, Side::Right, SelectionType::Char);
    t.selection_extend(1, 5, Side::Right);

    assert_eq!(
        t.selection_text().as_deref(),
        Some("ghijkl"),
        "an anchor after the last cell of line 0 selects line 1, not a leading break"
    );
}

#[test]
fn a_point_on_an_absorbed_blank_line_does_not_jump_a_row() {
    // The absorbed-blank branch clamps the column, and clamping it *to* `new_cols` made the clamp
    // emit the seam's "just past a full row" signal — so a cursor parked one column further right
    // moved a whole row. A blank line has no full row to be just past.
    let mut seen = Vec::new();
    for parked in [3usize, 4, 5] {
        let mut t = Engine::new(8, 6);
        t.feed(b"ab\r\n");
        t.feed(format!("\x1b[2;{}H", parked + 1).as_bytes());
        t.resize(4, 6);
        seen.push((t.cursor().row, t.cursor().col));
    }
    assert_eq!(
        seen,
        vec![(1, 3), (1, 3), (1, 3)],
        "columns past the new width clamp to the same cell; none of them changes the row"
    );
}

// ---------------------------------------------------------------------------
// the cursor — "one past" is the next write position
// ---------------------------------------------------------------------------

#[test]
fn the_cursor_past_a_full_last_row_writes_after_it() {
    // #562 symptom 3. The mapping already produced the right row; a defensive clamp against
    // `out.len()` — a row count `reflow` does not own, the caller's fit does — pulled it back onto
    // the last glyph and the next byte destroyed the `f`.
    let mut t = Engine::new(8, 10);
    t.feed(b"abcdef");

    t.resize(3, 10);
    t.feed(b"X");

    assert_eq!(t.accessible_text().trim_end(), "abcdef\nX");
}

#[test]
fn the_same_when_the_resize_grows_instead_of_shrinking() {
    // The issue frames this as a narrowing problem. It is not: widening re-splits too, and the
    // clamp fires the same way.
    let mut t = Engine::new(6, 6);
    t.feed(b"abcdefgh");

    t.resize(8, 6);
    t.feed(b"Z");

    assert_eq!(t.accessible_text().trim_end(), "abcdefgh\nZ");
}

#[test]
fn the_cursor_on_a_trailing_blank_line_keeps_its_line() {
    // #562 symptom 2. Trailing blank *lines* are absorbed by the join, and a point on one collapsed
    // onto the last content row — so the next byte overwrote the content instead of following it.
    // The row it names is one the caller's row-fit creates; `reflow` emits nothing extra.
    let mut t = Engine::new(6, 10);
    t.feed(b"ab\r\n");

    t.resize(3, 10);
    t.feed(b"Z");

    assert_eq!(t.accessible_text().trim_end(), "ab\nZ");
}

// ---------------------------------------------------------------------------
// controls — things this change must NOT do
// ---------------------------------------------------------------------------

#[test]
fn a_bce_tail_still_costs_no_row_and_gains_no_wrap_flag() {
    // #530's invariant, asserted on the *structure* rather than on a count of non-default cells.
    // The existing pin (`reflow_trim::a_bce_tail_does_not_cost_a_row_on_reflow`) counts rows holding
    // a non-default cell, and a row padded for a tracked point holds `Cell::default()` — so it stays
    // green while the line really does spend a second row. Measured: an earlier design passed that
    // pin with `r0` wrapped and `r1` blank.
    let mut t = Engine::new(8, 4);
    t.feed(b"abcde\x1b[1;6H\x1b[41m\x1b[K");

    t.resize(5, 4);

    assert_eq!(t.accessible_text().trim_end(), "abcde");
    assert!(
        !t.grid().is_row_wrapped(0),
        "the BCE tail must not make row 0 claim a continuation"
    );
    assert_eq!(
        t.scrollback_len(),
        0,
        "and must not push anything to history"
    );
}

#[test]
fn a_hard_line_break_survives_the_resize() {
    // Constraint 2. Recovering "past the end" by arming `pending_wrap` glues a hard-ended row to the
    // next one and deletes a break the application emitted. `pending_wrap` means *continue this
    // logical line*, and the cursor reaching a line's end does not mean that — in justerm the flag
    // is carried beside the reflow, so a `col == cols` coming back out of it is a cursor parked past
    // content by CUP, never a deferred wrap.
    let mut t = Engine::new(8, 4);
    t.feed(b"abcdef\r\n$ \x1b[1;7H\x1b[K");

    t.resize(3, 4);
    t.feed(b"Z");

    assert!(
        t.accessible_text().contains('\n'),
        "the break between the two lines must still be there, got {:?}",
        t.accessible_text()
    );
}

#[test]
fn a_pane_with_no_history_does_not_spend_a_row_on_a_tracked_point() {
    // Constraint 3, re-aimed. It used to be an **alt-screen** test, because the alt screen is the
    // pane with no history to absorb a displaced row. Since #567 the alt screen does not reflow at
    // all, so that fixture re-splits nothing and the test could no longer fail — the constraint did
    // not go away with it, though: any pane whose scrollback limit is 0 destroys content instead of
    // scrolling, and that is now expressed directly.
    //
    // Keep this tight. The first version of the alt test had 22 short lines in 24 rows resized
    // 80->40 — nothing re-split and two rows of slack, so a one-row cost was unobservable and it
    // stayed green under exactly the design it names.
    //
    // Losing `aaaaaa` is not the defect: 3 lines x 2 rows into a 4-row pane with nowhere to put the
    // overflow cannot fit, and that is ordinary truncation. What is asserted is that the *tracked
    // point* costs nothing on top of it.
    let mut t = Engine::with_scrollback(6, 4, 0);
    t.feed(b"aaaaaa\r\nbbbbbb\r\ncccccc\r\n");

    t.resize(3, 4);

    assert_eq!(
        t.accessible_text().trim_end(),
        "bbbbbb\ncccccc",
        "materialising the absorbed blank row would cost one more line than truncation alone"
    );
}

#[test]
fn a_mark_on_an_absorbed_blank_line_stays_inside_the_buffer() {
    // The bound on a tracked line did not disappear with #559's clamp — it **moved**, from `reflow`
    // (which bounded against a row count it does not own) to `reflow_pane` (which knows the final
    // geometry). It is load-bearing, and this is the input that reaches it: the content grows to
    // fill the screen at the new width, the marks sit on the trailing blank line the join absorbs,
    // and the scrollback cap of 0 leaves no history to hold the overflow.
    //
    // Measured with the bound removed: `index out of bounds: the len is 2 but the index is 2` in
    // `Grid::row` — a panic crossing into the consumer's process (#536's class). Only the cursor is
    // clamped on the way in (`Cursor::set_point`); marks and selection anchors are written back raw.
    let mut t = Engine::with_scrollback(6, 2, 0);
    t.feed(b"aaaaaa\r\n");
    t.feed(b"\x1b]133;B\x07\x1b]133;C\x07");
    assert_eq!(
        t.command_marks().len(),
        2,
        "fixture: both marks on the blank line"
    );

    t.resize(3, 2);

    let buffer_lines = t.scrollback_len() + 2;
    let lines: Vec<usize> = t.command_marks().iter().map(|m| m.1).collect();
    assert!(
        lines.iter().all(|&l| l < buffer_lines),
        "a mark may not name a line the buffer does not have: {lines:?} in {buffer_lines} lines"
    );
    // The read path over those marks must survive too — the panic above was raised by this call.
    let _ = t.command_lines();
}

#[test]
fn a_selection_does_not_move_the_apps_content() {
    // What this pins is the **second** half of "UI state may not move app content": no design that
    // widens or materialises for a tracked point may make a selected buffer lay out differently
    // from an unselected one. It is the fixture #562 records as the discriminating one — line 1
    // must be non-empty ahead of the blank so the trailing-blank trim actually runs in both arms.
    //
    // It does **not** pin the `.min(cols - 1)` on the anchor column, and an earlier comment here
    // claimed it did. Measured: removing that clamp leaves this test green and reddens
    // `reflow_point_mapping::a_point_past_a_full_last_row_of_the_last_line_does_not_name_a_row_that_does_not_exist`
    // instead — the #559 test, which is where that guard is pinned.
    let mut t = Engine::with_scrollback(6, 3, 100);
    t.feed(b"aaaaaa\r\nbbbbbb\r\n");
    t.feed(b"\x1b[1;1H");
    t.selection_begin(2, 0, Side::Left, SelectionType::Char);
    t.selection_extend(2, 0, Side::Right);

    let mut plain = Engine::with_scrollback(6, 3, 100);
    plain.feed(b"aaaaaa\r\nbbbbbb\r\n");
    plain.feed(b"\x1b[1;1H");

    t.resize(3, 3);
    plain.resize(3, 3);

    assert_eq!(
        t.scrollback_len(),
        plain.scrollback_len(),
        "a selection must not move the app's content"
    );
    assert_eq!(t.accessible_text(), plain.accessible_text());
}
