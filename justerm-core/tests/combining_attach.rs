//! Where a combining mark attaches (#865) — the cell the last print landed in.
//!
//! Both the zero-width path (`push_combining`) and the mode-2027 join path
//! (`try_grapheme_join`) locate that cell through one helper, so they are tested
//! together here.
//!
//! The hard case is **autowrap off** (DECAWM `?7l`). With it on, a print that fills
//! the last column arms the deferred wrap and the cursor is visibly parked *on* the
//! glyph. With it off the cursor reaches the last column two ways that share every
//! cursor field — it *filled* the column, or it merely *advanced/was moved onto* it —
//! and the mark belongs to a different cell in each.
//!
//! Reference behaviour, all four read at the pins in `docs/agents/thegraph.md`:
//! a mark after a print that filled the last column attaches **under the cursor** in
//! xterm (`charproc.c:3109` — `char_was_written ? last_written_col : cur_col`),
//! alacritty (`term/mod.rs:1073`, whose `input_needs_wrap` is armed unconditionally at
//! `:1136`), xterm.js (`InputHandler.ts:625` — its `x` runs to `cols`) and ghostty's
//! mode-2027 path (`Terminal.zig:1124` — *"if we do not have wraparound, the logic is
//! trickier"*). Ghostty's plain zero-width path (`:1329`) is the sole outlier and
//! contradicts its own 2027 path.

use justerm_core::Engine;

const ACUTE: &str = "\u{301}";

/// The first screen row as text, marks included — this is what `selection_text`,
/// `search` and the wire all read, so it is the surface the defect is visible on.
fn row0(t: &Engine) -> String {
    t.accessible_text()
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

#[test]
fn autowrap_off_a_mark_after_a_filled_row_attaches_to_the_last_column() {
    // AC 1. `abc` exactly fills a 3-column row with `?7l`, pinning the cursor on `c`.
    let mut t = Engine::new(3, 1);
    t.feed(b"\x1b[?7labc");
    t.feed(ACUTE.as_bytes());
    assert_eq!(
        row0(&t),
        format!("abc{ACUTE}"),
        "the mark belongs to `c`, the glyph the print just wrote"
    );
}

#[test]
fn autowrap_off_a_mark_after_a_cursor_move_attaches_before_the_cursor() {
    // AC 2 — the case a naive widening breaks. Nothing was printed since the move, so
    // the cursor is *at* a cell rather than pinned on one, and the mark takes the cell
    // before it. Unchanged from before #865, and deliberately so: xterm and ghostty
    // put it under the cursor instead, alacritty and xterm.js put it here, and the
    // 2-2 split is not this change's to settle.
    let mut t = Engine::new(3, 1);
    t.feed(b"\x1b[?7labc\x1b[1;3H");
    t.feed(ACUTE.as_bytes());
    assert_eq!(
        row0(&t),
        format!("ab{ACUTE}c"),
        "a bare cursor move leaves the mark on the column before the cursor"
    );
}

#[test]
fn autowrap_off_the_mode_2027_join_shares_the_attach_point() {
    // AC 3. The join path locates its cluster through the same helper, so it moves
    // with it — asserted through a mark that joins rather than one that merely
    // attaches.
    let mut t = Engine::new(3, 1);
    t.feed(b"\x1b[?2027h\x1b[?7labc");
    t.feed(ACUTE.as_bytes());
    assert_eq!(
        row0(&t),
        format!("abc{ACUTE}"),
        "mode 2027 joins into the same cell the zero-width path attaches to"
    );
}

#[test]
fn autowrap_off_a_mark_after_a_filled_row_ending_in_a_wide_glyph_reaches_its_lead() {
    // AC 4, `?7l` half. A wide glyph at columns 1-2 fills the row and pins the cursor
    // on its *spacer*; the mark belongs to the lead at column 1, never the spacer.
    let mut t = Engine::new(3, 1);
    t.feed("\x1b[?7la\u{4e00}".as_bytes()); // 'a', then a width-2 CJK glyph
    t.feed(ACUTE.as_bytes());
    assert_eq!(
        row0(&t),
        format!("a\u{4e00}{ACUTE}"),
        "the mark rides the wide lead, not its spacer"
    );
}

#[test]
fn autowrap_on_a_mark_after_a_filled_row_ending_in_a_wide_glyph_reaches_its_lead() {
    // AC 4, deferred-wrap half — the same shape with the wrap armed.
    let mut t = Engine::new(3, 2);
    t.feed("a\u{4e00}".as_bytes());
    t.feed(ACUTE.as_bytes());
    assert_eq!(
        row0(&t),
        format!("a\u{4e00}{ACUTE}"),
        "the mark rides the wide lead under a deferred wrap too"
    );
}

#[test]
fn autowrap_on_a_mark_after_a_filled_row_attaches_to_the_last_column() {
    // Control. The deferred wrap makes the pin visible, so this path was always right;
    // it is here so a regression cannot be read as "the edge case was always broken".
    let mut t = Engine::new(3, 2);
    t.feed(b"abc");
    t.feed(ACUTE.as_bytes());
    assert_eq!(row0(&t), format!("abc{ACUTE}"));
}

#[test]
fn a_mark_mid_row_attaches_to_the_glyph_before_the_cursor() {
    // Control. Away from the right margin nothing is ambiguous, with the mode either
    // way — the ordinary path must not move.
    for prefix in [&b""[..], &b"\x1b[?7l"[..]] {
        let mut t = Engine::new(5, 1);
        t.feed(prefix);
        t.feed(b"abc");
        t.feed(ACUTE.as_bytes());
        assert_eq!(row0(&t), format!("abc{ACUTE}"), "prefix {prefix:?}");
    }
}

#[test]
fn autowrap_off_a_second_mark_stacks_on_the_same_pinned_cell() {
    // The attach point survives its own output: a mark is itself a print, so the
    // second one must find the first one's cell rather than falling back a column.
    let mut t = Engine::new(3, 1);
    t.feed(b"\x1b[?7labc");
    t.feed(ACUTE.as_bytes());
    t.feed("\u{308}".as_bytes()); // combining diaeresis
    assert_eq!(row0(&t), format!("abc{ACUTE}\u{308}"));
}

#[test]
fn a_restored_deferred_wrap_still_attaches_under_the_cursor() {
    // The guard on collapsing the two readings into one. DECSC / move / DECRC restores
    // the *park* while every escape in between clears the record of where the last
    // print wrote — so a helper that consulted only that record would step a column
    // left here. Both readings are load-bearing; neither subsumes the other.
    let mut t = Engine::new(3, 2);
    t.feed(b"abc"); // fills the row, arms the deferred wrap
    t.feed(b"\x1b7"); // DECSC
    t.feed(b"\x1b[2;1H"); // move away
    t.feed(b"\x1b8"); // DECRC — the park comes back, the print record does not
    t.feed(ACUTE.as_bytes());
    assert_eq!(
        row0(&t),
        format!("abc{ACUTE}"),
        "a restored park still names the cell under the cursor"
    );
}

#[test]
fn a_mark_after_a_relocated_cluster_follows_it_rather_than_its_vacated_column() {
    // The window where "the cursor is standing on what the print wrote" and "wherever
    // the print wrote" come apart, and the reason the first is asked rather than the
    // second.
    //
    // Under mode 2027 a narrow base joined by VS16 grows to width 2. At the last
    // column the pair cannot fit, so `relocate_cluster_wide` vacates it and re-places
    // the cluster on the next row — and on a one-row grid that "next row" is reached by
    // *scrolling*, so the cluster lands back on row 0 at column 0 while the record of
    // where the print wrote still names the far column it came from.
    //
    // A helper that trusted that record alone would attach the next mark to a column
    // the cluster no longer occupies. Reached by proptest (`robustness.rs`) before it
    // was reached by hand.
    let mut t = Engine::new(6, 1);
    t.feed(b"\x1b[?2027h");
    t.feed("abcde".as_bytes()); // fill columns 0-4, base lands on column 5
    t.feed("\u{25B6}".as_bytes()); // ▶ , width 1, at column 5
    t.feed("\u{FE0F}".as_bytes()); // VS16 → width 2 → cannot fit → relocated to column 0

    // The window itself, asserted before the behaviour inside it: the cluster really did
    // move to column 0, so this test cannot pass by never entering the state.
    assert_eq!(
        t.grid().cell(0, 0).c(),
        '\u{25B6}',
        "precondition: the promoted cluster relocated to column 0"
    );
    assert!(t.grid().cell(0, 1).is_wide_spacer(), "precondition: its spacer");

    t.feed("\u{301}".as_bytes()); // a further mark, arriving after the relocation
    // The relocation soft-wraps, so the logical line is the vacated half followed by
    // the cluster's new home — the mark has to land on the second half.
    assert_eq!(
        t.accessible_text(),
        format!("abcde\u{25B6}\u{FE0F}{ACUTE}"),
        "the mark follows the cluster to column 0, not the column it was vacated from"
    );
}
