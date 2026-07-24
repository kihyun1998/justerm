//! #538 — soft-wrap is a property of the **row**, so writing or clearing a cell must not be able
//! to destroy it.
//!
//! justerm stored it in the row's last cell (`CellFlags::WRAPLINE`, from #7), and `Cell` writes
//! and clears are whole-word operations — so ordinary typing in the last column silently split
//! the logical line, injecting a newline into copy and breaking any search across the wrap.
//!
//! Both references keep it out of a cell's reach for this reason: ghostty holds `wrap` on the
//! `Row`, xterm.js holds `isWrapped` on the `BufferLine` and makes `replaceCells` take
//! `clearWrap` as an explicit argument rather than letting a cell clear decide it.
//!
//! The tests below are written against the three ways a cell at the last column gets touched —
//! a plain write, an erase, and a structural repair — plus the one case where the wrap *should*
//! be cleared.

use justerm_core::{CellFlags, Engine};

/// `"abcdZ"` on a 4-column screen: one logical line soft-wrapped across two rows.
fn wrapped_line() -> Engine {
    let mut t = Engine::new(4, 3);
    t.feed(b"abcdZ");
    assert_eq!(t.accessible_text().trim_end(), "abcdZ", "fixture");
    assert_eq!(
        t.search("cdZ").len(),
        1,
        "fixture: searchable across the wrap"
    );
    t
}

#[test]
fn overwriting_the_last_column_keeps_the_line_joined() {
    // The most common trigger by far: no erase, no wide glyph, no repair — just typing.
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;4HQ");
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcQZ",
        "the row is still soft-wrapped, so copy gets one line"
    );
    assert_eq!(
        t.search("cQZ").len(),
        1,
        "and it is still searchable across the wrap"
    );
}

#[test]
fn erasing_rightward_ends_the_wrap_at_any_column() {
    // `EL 0` destroys this row's tail, so it can no longer be continuing — at ANY column, not
    // only when it happens to start at 0.
    //
    // This test asserted the opposite when it was written, on a misreading of xterm.js: that
    // reference's `clearWrap` flag targets `isWrapped`, which marks the row that *continues* the
    // previous one — the opposite link from justerm's. Real xterm ends `ClearRight` with
    // `LineClrWrapped(ld)` unconditionally (`util.c:1871`, comment: *"with the right part
    // cleared, we can't be wrapping"*), and ghostty calls `cursorResetWrap()` in
    // `eraseLine(.right)`. Both were read directly.
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;3H\x1b[K"); // erase from a MID column, not from 0
    assert_eq!(
        t.accessible_text().trim_end(),
        "ab
Z",
        "the erased row stops continuing into the next"
    );
    assert!(!t.grid().is_row_wrapped(0));
}

#[test]
fn erasing_leftward_leaves_the_wrap_alone() {
    // `EL 1`'s mirror: the tail survives, so what it flowed into still follows it. Both
    // references agree here (xterm's `ClearLeft` has no `LineClrWrapped`).
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;3H\x1b[1K");
    assert!(
        t.grid().is_row_wrapped(0),
        "erasing to the left cannot end a wrap"
    );
    assert_eq!(t.accessible_text().trim_end(), "   dZ"); // EL 1 erases through the cursor column
}

#[test]
fn a_structural_repair_at_the_last_column_keeps_the_line_joined() {
    // Overwriting a wide glyph's lead frees its spacer at the last column (#530's `free_cell`).
    let mut t = Engine::new(6, 3);
    t.feed("abcd\u{D55C}".as_bytes()); // 한 at columns 4-5, filling the row
    t.feed(b"Z"); // wraps → row 0 is soft-wrapped
    assert_eq!(t.accessible_text().trim_end(), "abcd\u{D55C}Z", "fixture");
    t.feed(b"\x1b[1;5HX"); // overwrite 한's lead → frees the spacer at the last column
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcdX Z",
        "the freed spacer is a blank inside the joined line"
    );
}

#[test]
fn erasing_the_whole_line_does_clear_the_wrap() {
    // The one case where clearing it is correct — and the rule xterm.js actually implements:
    // `clearWrap` is true for EL 2 and for EL 0 with the cursor at column 0.
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;1H\x1b[2K"); // EL 2 — erase the entire line
    assert_eq!(
        t.accessible_text().trim_end(),
        "\nZ",
        "row 0 is empty and no longer continues into row 1 — the wrap IS cleared here"
    );
    // Row 1's content is its own logical line now.
    assert_eq!(t.search("Z").len(), 1);
}

#[test]
fn the_wrap_survives_a_cell_write_at_the_last_column_across_reflow() {
    // The end-to-end consequence: a resize re-splits by logical line, so a lost wrap changes the
    // resulting geometry, not just the text.
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;4HQ");
    t.resize(8, 3);
    assert_eq!(
        t.accessible_text().trim_end(),
        "abcQZ",
        "widening rejoins the line into one row"
    );
}

// ---- a blanked row must not inherit a wrap ------------------------------------------------
//
// Moving the flag onto `Row` created a new hazard the cell encoding did not have: the paths
// that blank a row blank its *cells*, while the `Row` itself is rotated/reused — so the flag
// rides along into a row the engine has just declared empty.

#[test]
fn a_row_blanked_by_a_region_scroll_is_not_wrapped() {
    let mut t = Engine::new(4, 3);
    t.feed(b"abcdZ"); // row 0 wraps into row 1
    t.feed(b"\x1b[3;1HQ");
    assert!(t.grid().is_row_wrapped(0), "fixture");
    t.feed(b"\x1b[1S"); // SU 1 — rows rotate up, the bottom is blanked
    assert!(
        (0..3).all(|r| !t.grid().is_row_wrapped(r)),
        "no blanked row claims to continue: {:?}",
        (0..3)
            .map(|r| t.grid().is_row_wrapped(r))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_row_blanked_by_an_alt_screen_scroll_is_not_wrapped() {
    let mut t = Engine::new(4, 3);
    t.feed(b"\x1b[?1049h");
    t.feed(b"abcdZ");
    t.feed(b"\x1b[3;1H\r\n\r\n"); // scroll the alt screen twice
    t.feed(b"PQ");
    assert_eq!(
        t.accessible_text().trim_end().replace('\n', "|"),
        "||PQ",
        "three separate lines — a blanked row must not merge with the one below it"
    );
}

#[test]
fn re_entering_the_alt_screen_starts_unwrapped() {
    // The alt grid persists across `?1049` cycles, so a stale wrap survives into a screen the
    // application believes it just cleared.
    let mut t = Engine::new(4, 3);
    t.feed(b"\x1b[?1049h");
    t.feed(b"abcdZ"); // wraps
    t.feed(b"\x1b[?1049l\x1b[?1049h"); // leave and re-enter
    t.feed(b"ab\r\ncd"); // two independent HARD lines
    assert_eq!(
        t.accessible_text().trim_end(),
        "ab\ncd",
        "two hard lines stay two lines on a freshly entered alt screen"
    );
}

#[test]
fn ed0_from_mid_row_ends_the_wrap_it_erased_through() {
    // `CSI J` from a mid-row cursor erases this row's tail *and every row below*, so the row
    // cannot continue into anything. justerm flags the row that wraps INTO the next, where
    // xterm.js flags the continuation row — so where xterm clears the erased row's own flag,
    // justerm must clear the flag of the row *above* the erased range. For ED 0 that is the
    // cursor's row, which the partial-erase rule deliberately leaves alone.
    let mut t = Engine::new(4, 3);
    t.feed(b"abcdZ");
    t.feed(b"\x1b[1;3H\x1b[J"); // ED 0 from column 2
    t.feed(b"\x1b[2;1HQR"); // redraw on the next row — the ordinary prompt-redraw shape
    assert_eq!(
        t.accessible_text().trim_end(),
        "ab\nQR",
        "the erased row no longer continues into the redrawn one"
    );
}

// ---- the two guards a mutation pass found unprotected -------------------------------------

#[test]
fn the_wire_still_carries_the_wrap_on_the_last_cell() {
    // This is the claim that let the storage move without a format change, and it was resting on
    // one unguarded line: `frame()` derives the bit back onto a span's last cell. Deleting that
    // line left the whole workspace green.
    let mut t = Engine::new(4, 3);
    t.feed(b"abcdZ"); // row 0 wraps
    let frame = t.frame();
    let span = frame
        .spans
        .iter()
        .find(|s| s.line == 0)
        .expect("row 0 is damaged");
    assert!(
        span.cells
            .last()
            .unwrap()
            .flags()
            .contains(CellFlags::WRAPLINE),
        "a wrapped row still ships WRAPLINE on its last cell"
    );
    // …and it is genuinely *derived*: the live cell does not carry it.
    assert!(
        !t.grid().cell(0, 3).flags().contains(CellFlags::WRAPLINE),
        "the live cell never carries it — that is the point of the move"
    );
    // Right reason: a hard-ended row must not ship it.
    let mut hard = Engine::new(4, 3);
    hard.feed(b"ab\r\nc");
    let hf = hard.frame();
    let hs = hf.spans.iter().find(|s| s.line == 0).expect("row 0");
    assert!(
        !hs.cells
            .last()
            .unwrap()
            .flags()
            .contains(CellFlags::WRAPLINE)
    );
}

#[test]
fn a_recycled_row_buffer_does_not_carry_a_wrap_into_its_next_life() {
    // `Row::clear` resets `wrapped` for the buffer `scroll_up_recycle` hands back. With a
    // scrollback cap the evicted row IS the recycled buffer, so a wrapped row's flag would
    // otherwise reappear on a fresh screen row several lines later.
    let mut t = Engine::with_scrollback(4, 2, 1);
    t.feed(b"abcdZ"); // row 0 wraps; it will be evicted first
    // Check after EVERY line. The recycled buffer surfaces for only a step or two before being
    // overwritten again, so a single look at the end misses the leak entirely — measured.
    for i in 0..8 {
        t.feed(format!("\r\nL{i}").as_bytes());
        assert!(
            (0..2).all(|r| !t.grid().is_row_wrapped(r)),
            "after line {i}, a screen row inherited the evicted row's wrap: {:?}",
            (0..2)
                .map(|r| t.grid().is_row_wrapped(r))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn ech_ends_the_wrap() {
    // ECH destroys content from the cursor rightward, so the row stops continuing — at any
    // column and for any count. xterm routes ECH through the same `ClearRight` as `EL 0`
    // (`util.c:1961` → `:1871`); ghostty calls `cursorResetWrap()` in `eraseChars`.
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;2H\x1b[1X"); // erase ONE cell, mid-row
    assert!(
        !t.grid().is_row_wrapped(0),
        "ECH ends the wrap even for a single cell in the middle"
    );
    assert_eq!(t.accessible_text().trim_end(), "a cd\nZ");
}

#[test]
fn dch_ends_the_wrap() {
    // DCH pulls the tail left and blanks the far end — ghostty says it outright, "Our row's
    // soft-wrap is always reset".
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;2H\x1b[1P");
    assert!(!t.grid().is_row_wrapped(0), "DCH ends the wrap");
    assert_eq!(t.accessible_text().trim_end(), "acd\nZ");
}

#[test]
fn ich_leaves_the_wrap_alone() {
    // The negative control: inserting blanks does not destroy the tail's meaning — it pushes it
    // right. All references agree ICH does not touch the flag.
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;2H\x1b[1@");
    assert!(t.grid().is_row_wrapped(0), "ICH must not end the wrap");
}
