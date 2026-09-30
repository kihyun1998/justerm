//! #1031 — with [`Engine::set_selection_carries_line_end`] on, a linear selection whose end has
//! passed the text of a finished row takes that row's `\n`, and the highlight fills the row. Off by
//! default. Each case asserts **both** observables, the text and the span, because the two answer
//! from one resolved coordinate and a rule applied to only one of them is the defect.

use justerm_core::{Engine, SelectionSpan, SelectionType, Side};

fn carrying(cols: usize, rows: usize) -> Engine {
    let mut term = Engine::new(cols, rows);
    term.set_selection_carries_line_end(true);
    term
}

fn span(row: usize, left: usize, right: usize) -> SelectionSpan {
    SelectionSpan { row, left, right }
}

fn select(term: &mut Engine, ty: SelectionType, from: (usize, usize, Side), to: (usize, usize, Side)) {
    term.selection_begin(from.0, from.1, from.2, ty);
    term.selection_extend(to.0, to.1, to.2);
}

// ===========================================================================
// The rule, on a Char drag
// ===========================================================================

#[test]
fn a_drag_ending_exactly_at_the_text_carries_nothing() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 4, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("hello"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 4)]);
}

/// The left half of the first blank cell names the text's end, not past it — `to` is exclusive.
#[test]
fn the_left_half_of_the_first_blank_cell_is_still_exactly_the_text() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 5, Side::Left));

    assert_eq!(term.selection_text().as_deref(), Some("hello"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 4)]);
}

#[test]
fn a_drag_covering_one_blank_cell_carries_the_newline_and_fills_the_row() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 5, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("hello\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 79)]);
}

#[test]
fn a_drag_far_past_the_text_carries_the_newline_once() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 60, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("hello\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 79)]);
}

/// Off by default: the same drag copies the text alone and paints what the pointer covered.
#[test]
fn off_by_default_the_same_drag_carries_nothing() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello\r\nnext");
    assert!(!term.selection_carries_line_end());
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 5, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("hello"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 5)]);
}

#[test]
fn the_last_row_of_a_multi_row_drag_carries_its_newline() {
    let mut term = carrying(80, 24);
    term.feed(b"ab\r\ncd\r\nef");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (1, 5, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("ab\ncd\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 79), span(1, 0, 79)]);
}

// ===========================================================================
// The text's end is measured in cells
// ===========================================================================

/// `가나다` is three characters and six cells. Ending on the last glyph — on either half — is the
/// text exactly.
#[test]
fn a_wide_row_selected_exactly_to_its_end_carries_nothing() {
    for end_col in [4, 5] {
        let mut term = carrying(80, 24);
        term.feed("가나다\r\nnext".as_bytes());
        select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, end_col, Side::Right));

        assert_eq!(term.selection_text().as_deref(), Some("가나다"), "end col {end_col}");
        assert_eq!(term.selection_range(), vec![span(0, 0, 5)], "end col {end_col}");
    }
}

#[test]
fn a_wide_row_selected_one_cell_past_its_end_carries_the_newline() {
    let mut term = carrying(80, 24);
    term.feed("가나다\r\nnext".as_bytes());
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 6, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("가나다\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 79)]);
}

/// Selecting `가나` of `가나다` must not read as "passed" — the row's text runs on in cells.
#[test]
fn a_wide_row_selected_part_way_carries_nothing() {
    let mut term = carrying(80, 24);
    term.feed("가나다\r\nnext".as_bytes());
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 3, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("가나"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 3)]);
}

// ===========================================================================
// Which rows are finished
// ===========================================================================

/// A row whose text reaches the right edge has no blank cell to cover, so reaching the edge is the
/// gesture there.
#[test]
fn a_full_row_carries_the_newline_when_the_drag_reaches_the_edge() {
    let mut term = carrying(10, 5);
    term.feed(b"0123456789\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 9, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("0123456789\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 9)]);
}

#[test]
fn a_full_row_stopped_one_cell_short_carries_nothing() {
    let mut term = carrying(10, 5);
    term.feed(b"0123456789\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 8, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("012345678"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 8)]);
}

/// A soft-wrapped row's end is not a line's end: its content continues on the next row.
#[test]
fn a_soft_wrapped_final_row_carries_nothing() {
    let mut term = carrying(10, 5);
    term.feed(b"0123456789abcde");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 9, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("0123456789"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 9)]);
}

/// A soft-wrapped row can still end in blanks — spaces written over its tail are padding to the
/// extent measure (#685) but leave the wrap in place — and its end is still not a line's end.
#[test]
fn a_soft_wrapped_row_ending_in_blanks_carries_nothing() {
    let mut term = carrying(10, 5);
    term.feed(b"0123456789abcde\x1b[1;7H    ");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 7, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("012345"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 7)]);
}

/// The buffer's last row has nothing below it, so nothing has finished it yet.
#[test]
fn the_buffers_last_row_carries_nothing() {
    let mut term = carrying(80, 3);
    term.feed(b"a\r\nb\r\nc");
    select(&mut term, SelectionType::Char, (2, 0, Side::Left), (2, 40, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("c"));
    assert_eq!(term.selection_range(), vec![span(2, 0, 40)]);
}

/// The decided rule has no cursor-row guard (#1031, the maintainer's call): a prompt row with
/// blank rows below it is finished like any other.
#[test]
fn the_cursor_row_is_finished_when_a_row_exists_below_it() {
    let mut term = carrying(80, 24);
    term.feed(b"$ ls");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 10, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("$ ls\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 79)]);
}

// ===========================================================================
// A selection with no text stays empty
// ===========================================================================

/// #914: a bare click selects nothing, wherever it lands.
#[test]
fn a_bare_click_past_the_text_selects_nothing() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    term.selection_begin(0, 10, Side::Left, SelectionType::Char);

    assert_eq!(term.selection_text().as_deref(), Some(""));
    assert_eq!(term.selection_range(), vec![]);
}

/// A press and release inside one blank cell that crosses its midpoint covers padding only; it
/// must not copy a lone `\n`.
#[test]
fn a_drag_over_padding_only_selects_nothing() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 10, Side::Left), (0, 10, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some(""));
    assert_eq!(term.selection_range(), vec![]);
}

/// A drag that starts in a row's padding begins on the next row: the row it started on
/// contributed no text, and so no ending either.
#[test]
fn a_drag_starting_in_the_padding_begins_on_the_next_row() {
    let mut term = carrying(80, 24);
    term.feed(b"ab\r\ncd\r\nef");
    select(&mut term, SelectionType::Char, (0, 5, Side::Left), (1, 1, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("cd"));
    assert_eq!(term.selection_range(), vec![span(1, 0, 1)]);
}

/// The first blank cell is already padding: a drag that starts there covers no text.
#[test]
fn a_drag_starting_on_the_first_blank_cell_covers_padding_only() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 5, Side::Left), (0, 7, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some(""));
    assert_eq!(term.selection_range(), vec![]);
}

#[test]
fn a_double_click_on_padding_selects_nothing() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    term.selection_begin(0, 20, Side::Left, SelectionType::Word);

    assert_eq!(term.selection_text().as_deref(), Some(""));
    assert_eq!(term.selection_range(), vec![]);
}

// ===========================================================================
// The other selection types
// ===========================================================================

/// A line selection always reaches the edge, so a finished line carries its `\n` (#1031).
#[test]
fn a_line_selection_of_a_finished_row_carries_the_newline() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    term.selection_begin(0, 2, Side::Left, SelectionType::Line);

    assert_eq!(term.selection_text().as_deref(), Some("hello\n"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 79)]);
}

#[test]
fn a_word_selection_ending_on_the_text_carries_nothing() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    term.selection_begin(0, 2, Side::Left, SelectionType::Word);

    assert_eq!(term.selection_text().as_deref(), Some("hello"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 4)]);
}

/// A rectangle joins nothing, so it has no line ending to carry.
#[test]
fn a_block_selection_past_the_text_carries_nothing() {
    let mut term = carrying(80, 24);
    term.feed(b"ab\r\ncd\r\nef");
    select(&mut term, SelectionType::Block, (0, 0, Side::Left), (1, 5, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("ab\ncd"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 5), span(1, 0, 5)]);
}

// ===========================================================================
// The setting
// ===========================================================================

/// Consumer policy, like the word separators: RIS resets the terminal, not the embedder's choice.
#[test]
fn the_setting_survives_a_full_reset() {
    let mut term = carrying(80, 24);
    term.feed(b"\x1bc");
    assert!(term.selection_carries_line_end());
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 5, Side::Right));

    assert_eq!(term.selection_text().as_deref(), Some("hello\n"));
}

#[test]
fn turning_it_off_restores_the_plain_run() {
    let mut term = carrying(80, 24);
    term.feed(b"hello\r\nnext");
    select(&mut term, SelectionType::Char, (0, 0, Side::Left), (0, 5, Side::Right));
    term.set_selection_carries_line_end(false);

    assert_eq!(term.selection_text().as_deref(), Some("hello"));
    assert_eq!(term.selection_range(), vec![span(0, 0, 5)]);
}
