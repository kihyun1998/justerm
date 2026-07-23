//! #528 — the column a wide glyph could not fit into is a **soft-wrap artefact written with
//! the current pen**, not the previous occupant left in place under two extra flags.
//!
//! `write_glyph` and `relocate_cluster_wide` both vacate the last column when a width-2
//! glyph reaches it, and they must produce the same cell. All three references write a blank
//! *with the pen's attributes* there — xterm.js `setCellFromCodepoint(col, 0, 1, curAttr)`
//! (`InputHandler.ts:609-611`, and `BufferLine.ts:244-251` copies the pen's fg/bg **and** its
//! `extended` link/colour), ghostty `printCell(0, .spacer_head)` (`Terminal.zig:1410-1412`,
//! whose `printCell` stamps the cursor's hyperlink at `:1596-1612`), and alacritty's
//! `write_at_cursor(' ')` under a `LEADING_WIDE_CHAR_SPACER` template (`mod.rs:1108-1113`,
//! assigning `extra` — hyperlink + underline colour — from the template).

use justerm_core::{Color, Engine, SelectionType, Side, TermDamage, decode, encode};

const LINK_OPEN: &[u8] = b"\x1b]8;;https://example.com\x07";
const LINK_CLOSE: &[u8] = b"\x1b]8;;\x07";

#[test]
fn the_wrapped_column_keeps_no_trace_of_its_previous_occupant() {
    // 'c' sits in the last column carrying a link and an underline colour. Both are then
    // released, the cursor is put back on it, and a wide glyph is printed there.
    let mut t = Engine::new(3, 24);
    t.feed(LINK_OPEN);
    t.feed(b"\x1b[4m\x1b[58:5:1mabc");
    t.feed(LINK_CLOSE);
    t.feed(b"\x1b[59m\x1b[24m");
    t.feed(b"\x1b[1;3H");
    t.feed("\u{D55C}".as_bytes()); // 한 — width 2, cannot fit → vacate + soft wrap

    let g = t.grid();
    assert_eq!(
        g.cell(0, 2).c(),
        ' ',
        "the old glyph is gone, not merely flagged"
    );
    assert!(g.cell(0, 2).is_wrapline(), "still marked a soft wrap");
    assert_eq!(
        t.link_at(0, 2),
        None,
        "the closed hyperlink did not survive"
    );
    assert_eq!(t.underline_color_at(0, 2), Color::Default);
    assert!(
        !g.cell(0, 2)
            .flags()
            .contains(justerm_core::CellFlags::UNDERLINE),
        "the released UNDERLINE attribute did not survive either"
    );
    // The wide glyph itself wrapped to row 1.
    assert_eq!(g.cell(1, 0).c(), '\u{D55C}');
}

#[test]
fn the_wrapped_column_is_blanked_with_the_current_pen_not_a_default_cell() {
    // The reference-conformance point: a blank written with the pen, so it carries the pen's
    // background instead of punching a default-coloured notch into a coloured run.
    // The pen MOVES between writing the occupant and vacating, so "kept the old cell" and
    // "written with the pen" are distinguishable — with a matching pen the test would pass
    // against the unfixed code and assert nothing.
    let mut t = Engine::new(3, 24);
    t.feed(b"\x1b[41mabc"); // 'c' lands in the last column on red
    t.feed(b"\x1b[42m"); // pen switches to green
    t.feed(b"\x1b[1;3H");
    t.feed("\u{D55C}".as_bytes());

    assert_eq!(
        t.grid().cell(0, 2).bg(),
        Color::Indexed(2),
        "vacated column is blanked with the CURRENT pen (xterm curAttr / alacritty template), \
         not left holding the old cell's red and not reset to Default"
    );
    assert_eq!(
        t.grid().cell(0, 1).bg(),
        Color::Indexed(1),
        "the untouched neighbour still has the old run's red"
    );
}

#[test]
fn a_still_open_hyperlink_and_underline_colour_reach_the_wrapped_column() {
    // Nothing is closed this time: the pen still carries the link and the colour, so the
    // blank takes them and a link run stays contiguous across the wrap. ghostty pins exactly
    // this ("Previous cell turns into spacer_head and remains hyperlinked").
    let mut t = Engine::new(3, 24);
    t.feed(LINK_OPEN);
    t.feed(b"\x1b[4m\x1b[58:5:1mab");
    t.feed("\u{D55C}".as_bytes()); // at col 2 via pending-wrap? no — 'ab' leaves the cursor at col 2

    let link = t
        .link_at(0, 2)
        .expect("the open link reached the vacated column");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
    assert_eq!(t.underline_color_at(0, 2), Color::Indexed(1));
    assert_eq!(
        t.link_at(0, 0),
        Some(link),
        "same run as the text before it"
    );
}

#[test]
fn both_vacate_paths_produce_the_same_cell() {
    // `write_glyph` (a plain wide glyph) and `relocate_cluster_wide` (a mode-2027 promotion at
    // the last column) implement the same step; one's comment says it mirrors the other.
    // Both rows are built so the vacated column HAS a previous occupant written under a
    // different pen, then the pen moves. Without that, both paths produce a default blank and
    // the equality holds while both are wrong — an agreement test has to be able to fail.
    let mut direct = Engine::new(3, 24);
    direct.feed(b"\x1b[41mabc"); // 'c' occupies the last column, on red
    direct.feed(LINK_OPEN);
    direct.feed(b"\x1b[42m\x1b[4m\x1b[58:5:1m"); // pen moves: green, underlined, colour 1
    direct.feed(b"\x1b[1;3H");
    direct.feed("\u{D55C}".as_bytes()); // wide at the last column → write_glyph vacates

    let mut relocated = Engine::new(3, 24);
    relocated.feed(b"\x1b[?2027h");
    relocated.feed(b"\x1b[41mab");
    relocated.feed("\u{25B6}".as_bytes()); // narrow base occupies the last column, on red
    relocated.feed(LINK_OPEN);
    relocated.feed(b"\x1b[42m\x1b[4m\x1b[58:5:1m"); // same pen move
    relocated.feed("\u{FE0F}".as_bytes()); // VS16 promotes → relocate_cluster_wide vacates

    assert_eq!(
        *direct.grid().cell(0, 2),
        *relocated.grid().cell(0, 2),
        "the two vacate implementations must produce an identical cell"
    );
    assert_eq!(direct.link_at(0, 2), relocated.link_at(0, 2));
    assert_eq!(
        direct.underline_color_at(0, 2),
        relocated.underline_color_at(0, 2)
    );
    // …and identical for the right reason: the pen's blank, not two matching wrong answers.
    assert_eq!(direct.grid().cell(0, 2).bg(), Color::Indexed(2));
    assert_eq!(direct.grid().cell(0, 2).c(), ' ');
    assert!(direct.link_at(0, 2).is_some());
    assert_eq!(direct.underline_color_at(0, 2), Color::Indexed(1));
}

#[test]
fn what_the_frame_carries_agrees_with_what_text_extraction_reports() {
    // The defect's user-visible half: the frame fed a renderer the old glyph while every text
    // reader reported the column as absent, so the screen showed a character you could not
    // copy, search or have announced.
    let mut t = Engine::new(4, 3);
    t.feed(b"abcd");
    t.feed(b"\x1b[1;4H");
    t.feed("\u{D55C}".as_bytes());

    let frame = t.frame();
    let span = frame
        .spans
        .iter()
        .find(|s| s.line == 0)
        .expect("row 0 damaged");
    let drawn: String = span.cells.iter().map(|c| c.c()).collect();
    assert_eq!(drawn, "abc ", "the renderer is not handed a phantom 'd'");
    assert_eq!(t.accessible_text().trim_end(), "abc한");

    // And it holds across the wire, not just in-process — a frame-mode consumer decodes the
    // same blank. NB: `decoded == frame` is deliberately not asserted. The leading-spacer
    // marker is engine-internal and never reaches `flags()` or the wire by design
    // (`cell.rs`'s `C_LEADING_SPACER`), because the consumer receives already-correct text and
    // the cell renders as the blank it is — so no frame containing a vacated column is a
    // literal fixed point, and that is the design, not a defect. What must survive is the
    // *content*: a blank, not the phantom.
    let decoded = decode(&encode(&frame)).expect("round-trips");
    let decoded_span = decoded
        .spans
        .iter()
        .find(|s| s.line == 0)
        .expect("row 0 in the decoded frame");
    let decoded_drawn: String = decoded_span.cells.iter().map(|c| c.c()).collect();
    assert_eq!(decoded_drawn, "abc ");
}

#[test]
fn vacating_the_column_records_damage_for_it() {
    // ADR-0003: every mutation site records damage. The cell's contents change, so a
    // frame-mode consumer must be told, or it keeps painting the old glyph.
    let mut t = Engine::new(4, 3);
    t.feed(b"abcd");
    t.feed(b"\x1b[1;4H");
    t.reset_damage(); // baseline immediately before the vacate
    t.feed("\u{D55C}".as_bytes());

    match t.damage() {
        TermDamage::Partial(lines) => {
            let row0 = lines
                .iter()
                .find(|l| l.line == 0)
                .expect("row 0 (the vacated column) is damaged");
            assert!(
                row0.left <= 3 && row0.right >= 3,
                "the vacated column 3 is inside the damaged span, got {}..={}",
                row0.left,
                row0.right
            );
        }
        other => panic!("expected partial damage, got {other:?}"),
    }
}

// ---- the vacate must not damage what it overwrites ---------------------------
//
// Writing the column (rather than flagging it) means the vacate is an overwrite like any
// other, and inherits every obligation the other overwrite sites carry.

#[test]
fn vacating_over_a_wide_spacer_repairs_its_orphaned_lead() {
    // The vacated column may be the SPACER of a wide glyph at cols-2. Blanking it destroys
    // the spacer marker, and every repair path keys off `is_wide_spacer()` — so an orphaned
    // lead left here can never be repaired by any later write, ECH, EL, ICH or DCH.
    let mut t = Engine::new(3, 24);
    t.feed("a\u{D55C}".as_bytes()); // a | 한 lead@1 | spacer@2
    assert!(t.grid().cell(1 - 1, 1).is_wide() && t.grid().cell(0, 2).is_wide_spacer());
    t.feed(b"\x1b[1;3H");
    t.feed("\u{AC00}".as_bytes()); // 가 at the last column → vacate(0,2) + wrap

    assert!(
        !t.grid().cell(0, 1).is_wide(),
        "the lead whose spacer was overwritten must be cleared, not left orphaned"
    );
    // …and the row still reads as its 3 columns, not 4 characters.
    t.feed(b"\x1b[1;3HX");
    assert_eq!(
        (0..3).map(|c| t.grid().cell(0, c).c()).collect::<String>(),
        "a X"
    );
    assert_eq!(t.accessible_text().trim_end(), "a X\n\u{AC00}");
}

#[test]
fn no_column_is_vacated_when_the_wrap_cannot_happen() {
    // `wrapline()` does not always advance: below a DECSTBM region on the last row, `linefeed`
    // neither scrolls nor moves. Blanking the column then destroys a visible glyph for a wrap
    // that never occurs.
    let mut t = Engine::new(4, 4);
    t.feed(b"1234\r\n5678\r\n9abc\r\ndefg");
    t.feed(b"\x1b[1;2r"); // DECSTBM rows 1-2 → the cursor ends up below the region
    t.feed(b"\x1b[4;4H"); // last row, last column
    t.feed("\u{D55C}".as_bytes());

    assert_eq!(
        t.grid().cell(3, 3).c(),
        'g',
        "no wrap happened, so nothing may be vacated"
    );
}

#[test]
fn a_word_selection_still_crosses_the_wrap_artefact() {
    // The artefact represents no column of text — the extractors drop it entirely — so a word
    // walk must pass through it rather than treat its blank as a boundary. Making the column
    // reliably blank is what exposes this.
    let mut t = Engine::new(3, 24);
    t.feed(b"abZ");
    t.feed(b"\x1b[1;3H");
    t.feed("\u{D55C}".as_bytes()); // vacates (0,2); 한 wraps to (1,0)
    t.selection_begin(0, 0, Side::Left, SelectionType::Word);
    assert_eq!(
        t.selection_text().as_deref(),
        Some("ab\u{D55C}"),
        "double-clicking a word that wraps because of a wide char selects the whole word"
    );
}

#[test]
fn a_promoted_cluster_does_not_relocate_when_the_wrap_cannot_happen() {
    // The mirror of `no_column_is_vacated_when_the_wrap_cannot_happen`, for the promotion path.
    // `relocate_cluster_wide` moves the cluster to `(cursor.row, 0..=1)` *after* `wrapline()` —
    // so when the wrap cannot advance, "the next row" is THIS row and the relocation lands on
    // top of live content at columns 0-1. Reasoning only about the vacated source column misses
    // that entirely: the destination is the damage.
    let mut t = Engine::new(4, 4);
    t.feed(b"\x1b[?2027h\x1b[1;2r\x1b[4;1H"); // DECSTBM rows 1-2 → the cursor is below the region
    t.feed("abc\u{25B6}".as_bytes()); // ▶ lands narrow in the last column
    t.feed("\u{FE0F}".as_bytes()); // VS16 promotes → would relocate, but nothing can wrap

    assert_eq!(
        (0..4).map(|c| t.grid().cell(3, c).c()).collect::<String>(),
        "abc\u{25B6}",
        "with nowhere to wrap the cluster stays narrow in place — as it already does for a \
         1-column screen and for DECAWM off"
    );
    assert!(
        !t.grid().cell(3, 3).is_leading_spacer() && !t.grid().cell(3, 3).is_wrapline(),
        "and no wrap artefact is left behind for a wrap that never happened"
    );
}

#[test]
fn repairing_an_orphaned_lead_damages_it() {
    // ADR-0003 again, for the *other* cell `vacate_for_wrap` mutates. Resetting the orphaned
    // lead changes its contents, so a frame-mode consumer that is not told keeps painting the
    // destroyed glyph — a ghost beside the correctly-repainted blank. ghostty pins the same
    // thing ("print over wide spacer tail" asserts dirty on the repaired lead, not the written
    // cell).
    let mut t = Engine::new(4, 4);
    t.feed("ab\u{D55C}".as_bytes()); // 한 wide at columns 2-3
    t.feed(b"\x1b[1;4H"); // onto its spacer
    t.reset_damage();
    t.feed("\u{AC00}".as_bytes()); // 가 at the last column → vacate over the spacer

    assert_eq!(
        t.grid().cell(0, 2).c(),
        ' ',
        "the orphaned lead was cleared"
    );
    match t.damage() {
        TermDamage::Partial(lines) => {
            let row0 = lines
                .iter()
                .find(|l| l.line == 0)
                .expect("row 0 is damaged");
            assert!(
                row0.left <= 2,
                "the repaired lead at column 2 must be inside the damaged span, got {}..={}",
                row0.left,
                row0.right
            );
        }
        other => panic!("expected partial damage, got {other:?}"),
    }
}

#[test]
fn a_migrated_leading_spacer_marker_does_not_join_two_words() {
    // The artefact is by definition the LAST column of a soft-wrapped row. DCH shifts whole
    // cells left, marker included, so the marker can end up mid-row where it no longer
    // describes anything (a pre-existing gap in the shift paths). Treating a leading spacer as
    // transparent *wherever* it sits then silently joins two visually separate words in the
    // clipboard — so the walk must key on the position, not the marker alone.
    let mut t = Engine::new(6, 4);
    t.feed(b"abcde");
    t.feed("\u{D55C}".as_bytes()); // wide at the last column → vacate(0,5) + wrap
    t.feed(b"\x1b[1;1H\x1b[2P"); // DCH 2 — the marker migrates to column 3
    t.feed(b"\x1b[1;5HZ"); // a separate word after it
    assert_eq!(
        (0..6).map(|c| t.grid().cell(0, c).c()).collect::<String>(),
        "cde Z "
    );

    t.selection_begin(0, 0, Side::Left, SelectionType::Word);
    assert_eq!(
        t.selection_text().as_deref(),
        Some("cde"),
        "a blank in the middle of a row ends the word, whatever marker it happens to carry"
    );
}
