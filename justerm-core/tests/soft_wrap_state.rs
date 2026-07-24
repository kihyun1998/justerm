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

use justerm_core::Engine;

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
fn a_partial_erase_of_the_last_column_keeps_the_line_joined() {
    // xterm.js clears the wrap only when the *whole* line is erased; a partial erase leaves it
    // (`InputHandler.ts:1323` passes `clearWrap = (x === 0)` for EL 0).
    let mut t = wrapped_line();
    t.feed(b"\x1b[1;4H\x1b[K");
    assert_eq!(
        t.accessible_text().trim_end(),
        "abc Z",
        "the erased column is a blank *inside* the joined line, not a line break"
    );
    assert_eq!(
        t.search("abc").len(),
        1,
        "the erased tail is blank but the line is still one line"
    );
    // The join is what matters: row 1 must not read as a separate logical line.
    assert!(
        !t.accessible_text().trim_end().contains('\n'),
        "no newline injected: {:?}",
        t.accessible_text().trim_end()
    );
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
