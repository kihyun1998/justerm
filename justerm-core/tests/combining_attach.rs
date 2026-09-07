//! Where a combining mark attaches (#865) — the cell the last print landed in.
//!
//! Both the zero-width path (`push_combining`) and the mode-2027 join path
//! (`try_grapheme_join`) locate that cell through one helper, so they are tested
//! together here.
//!
//! The hard case **was** autowrap off (DECAWM `?7l`), and the reason it was hard has
//! since been removed. A print that fills the last column arms the deferred wrap and
//! the cursor is parked *on* the glyph; until #869 that arm folded the mode in, so with
//! `?7l` the cursor reached the last column two ways that shared every cursor field —
//! it *filled* the column, or it merely *advanced/was moved onto* it — and the mark
//! belongs to a different cell in each. #865 told them apart with `Term::repeat_anchor`;
//! #869 made the arm unconditional and that workaround was measured dead and removed.
//!
//! **These tests are kept exactly as they were**, and they still redden if the arm is
//! re-conditioned — which is the point: they now pin the *behaviour* against whichever
//! mechanism supplies it, rather than the mechanism they were written against.
//!
//! Reference behaviour, all four read at the pins in `docs/agents/thegraph.md`:
//! a mark after a print that filled the last column attaches **under the cursor** in
//! xterm (`charproc.c:3109` — `char_was_written ? last_written_col : cur_col`),
//! alacritty (`term/mod.rs:1073`, whose `input_needs_wrap` is armed unconditionally at
//! `:1136`), xterm.js (`InputHandler.ts:625` — its `x` runs to `cols`) and ghostty's
//! mode-2027 path (`Terminal.zig:1124` — *"if we do not have wraparound, the logic is
//! trickier"*). That makes it **3 of 4 against this engine, not 4 of 4**: mode 2027
//! carries no `.default = true` in ghostty either (`modes.zig:283`), so ghostty's
//! shipped answer is its plain zero-width path (`:1329`), which reads
//! `wraparound and pending_wrap` and therefore had exactly the defect fixed here.
//! Ghostty's two paths contradict each other; the 2027 one is the one that agrees
//! with the other three.

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
    // before it. Unchanged from before #865, and deliberately so. The references do not
    // agree and none of them agrees with two others: xterm puts it under the cursor,
    // alacritty and ghostty put it here, and xterm.js puts it in a cell of its own
    // (the intervening CSI zeroes its join state). This is the plurality's answer and
    // reopening it is not this change's job.
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
    // Written as the guard on collapsing #865's two readings into one: DECSC / move /
    // DECRC restores the *park* while every escape in between clears the record of where
    // the last print wrote, so a helper consulting only that record stepped a column left
    // here. There is one reading again since #869, so this no longer separates two
    // mechanisms — it pins that a restored park is still a park, which is
    // `Term::settle_restored_wrap`'s contract and nothing else asserts.
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
    // Under mode 2027 a narrow base joined by VS16 grows to width 2. At the last column
    // the pair cannot fit, so `relocate_cluster_wide` vacates that column and re-places
    // the cluster on the next row — on a one-row grid, reached by *scrolling*, so it
    // lands back on row 0 at column 0.
    //
    // **The width sweep is kept, and what it buys has changed.** It was written because
    // #865's anchor branch keyed on the anchor naming the cursor's own column, so only
    // the narrow grids made it fire — at 6 columns the vacated column is 5, the branch
    // never fired, and the case was a vacuous window that passed whatever the helper did
    // (the refuting pass measured what that missed: 3 columns dropped the mark onto a
    // blank cell, 2 columns lost it entirely). That branch is gone since #869. The sweep
    // now guards the thing that outlived it — `try_grapheme_join` anchoring on where a
    // relocated cluster *landed* rather than where it was joined, which `REP` still
    // depends on — and the narrow widths remain the ones where a wrong anchor collides
    // with a live cell instead of a harmless one.
    for cols in [2usize, 3, 4, 6] {
        let filler = "abcdefgh"[..cols - 1].to_string();
        let mut t = Engine::new(cols, 1);
        t.feed(b"\x1b[?2027h");
        t.feed(filler.as_bytes()); // base lands on the last column
        t.feed("\u{25B6}".as_bytes()); // a width-1 base
        t.feed("\u{FE0F}".as_bytes()); // VS16 -> width 2 -> cannot fit -> relocated

        // The window itself, asserted before the behaviour inside it, so a build that
        // stops relocating reports that rather than passing quietly.
        assert_eq!(
            t.grid().cell(0, 0).c(),
            '\u{25B6}',
            "precondition at {cols} columns: the cluster relocated to column 0"
        );
        assert!(
            t.grid().cell(0, 1).is_wide_spacer(),
            "precondition at {cols} columns: its spacer"
        );

        t.feed("\u{301}".as_bytes()); // a further mark, arriving after the relocation
        // The relocation soft-wraps, so the logical line is the vacated half followed by
        // the cluster's new home — the mark has to land on the second half.
        assert_eq!(
            t.accessible_text(),
            format!("{filler}\u{25B6}\u{FE0F}{ACUTE}"),
            "at {cols} columns the mark follows the cluster, not its vacated column"
        );
    }
}

#[test]
fn a_base_less_mark_at_column_zero_is_not_a_cluster_the_join_may_extend() {
    // `push_combining` anchors a mark that opens a row at column 0 through its own
    // `unwrap_or(0)`; the join path declines that case instead, and the two differ
    // deliberately. `cursor_cluster_col`'s `col == 0 -> None` arm is what keeps them
    // apart, and it records no reason of its own — this test is the record.
    //
    // Reached by the completeness pass on #865, where a helper branch briefly answered
    // `Some(0)` here and reconciled them by accident; that branch is gone since #869,
    // and the decision it threatened is still only stated here.
    let mut t = Engine::new(6, 2);
    t.feed(b"\x1b[?2027h");
    t.feed("\u{25B6}".as_bytes());
    t.feed(b"\r"); // back to column 0; the anchor is cleared by the C0
    t.feed("\u{FE0F}\u{200D}".as_bytes()); // a base-less mark, then a joining scalar
    assert!(
        !t.grid().cell(0, 0).is_wide(),
        "the join declined, so nothing promoted column 0 to a wide pair"
    );
}
