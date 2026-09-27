//! #967 — a consumer colours settled cells: `changed_logical_lines` reports the soft-wrap-joined
//! lines whose content changed since it last answered, and `paint_logical_line` writes a colour
//! reference into the cells of a span of one of them. The matcher (the policy) stays with the
//! consumer; the engine owns the query and the write (ADR-0017).

use justerm_core::{Cell, CellFlags, ChangedLine, Color, Engine, PaintSpan, TermDamage};

const RED: Color = Color::Indexed(1);
const BLUE: Color = Color::Indexed(4);

fn take(term: &mut Engine) -> Vec<ChangedLine> {
    term.changed_logical_lines()
}

fn line_with<'a>(lines: &'a [ChangedLine], text: &str) -> &'a ChangedLine {
    lines
        .iter()
        .find(|l| l.text == text)
        .unwrap_or_else(|| panic!("no changed line {text:?} in {lines:?}"))
}

fn fg_span(start: usize, end: usize, fg: Color) -> PaintSpan {
    PaintSpan {
        start,
        end,
        fg: Some(fg),
        ..Default::default()
    }
}

fn fgs(cells: &[Cell]) -> Vec<Color> {
    cells.iter().map(Cell::fg).collect()
}

/// Output that has been parsed is reported once, as whole logical lines; asking again with
/// nothing new in between reports nothing.
#[test]
fn new_output_is_reported_once() {
    let mut term = Engine::new(20, 4);
    term.feed(b"hello\r\nan ERROR here\r\n");

    let lines = take(&mut term);
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, vec!["hello", "an ERROR here"]);

    assert!(take(&mut term).is_empty());
}

/// A line still being written — a match that completes in a later chunk — is reported again
/// each time it changes, at the same reference.
#[test]
fn a_line_split_across_chunks_is_reported_again_when_it_completes() {
    let mut term = Engine::new(20, 4);
    term.feed(b"$ ERR");
    let first = take(&mut term);
    assert_eq!(line_with(&first, "$ ERR").at.line, 0);

    term.feed(b"OR");
    let second = take(&mut term);
    let line = line_with(&second, "$ ERROR");
    assert_eq!(line.at, first[0].at);
}

/// Soft-wrapped rows are reported as one line, however far back the wrap started.
#[test]
fn a_changed_continuation_row_reports_its_whole_logical_line() {
    let mut term = Engine::new(5, 4);
    term.feed(b"abcdefgh");
    take(&mut term);

    term.feed(b"ij");
    let lines = take(&mut term);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "abcdefghij");
    assert_eq!(lines[0].at.line, 0);
}

/// Painting changes only the named channel on only the span's cells: the attributes the
/// application set, and the other channel, are left as they were.
#[test]
fn paint_sets_only_the_named_channel_on_the_span() {
    let mut term = Engine::new(20, 2);
    term.feed(b"an \x1b[1;44mERROR\x1b[0m here");
    let lines = take(&mut term);
    let line = line_with(&lines, "an ERROR here");

    assert!(term.paint_logical_line(line.at, &line.text, &[fg_span(3, 8, RED)]));

    let row = term.grid().row(0);
    assert_eq!(
        fgs(&row[..10]),
        vec![
            Color::Default,
            Color::Default,
            Color::Default,
            RED,
            RED,
            RED,
            RED,
            RED,
            Color::Default,
            Color::Default,
        ]
    );
    for cell in &row[3..8] {
        assert!(cell.flags().contains(CellFlags::BOLD));
        assert_eq!(cell.bg(), BLUE);
    }
}

/// A span over a soft-wrapped line lands on both rows; a span over a wide glyph covers the
/// whole pair, spacer included.
#[test]
fn paint_follows_the_wrap_and_covers_a_wide_pair_whole() {
    let mut term = Engine::new(5, 3);
    term.feed("abc漢defg".as_bytes()); // row0 "abc" + 漢(3,4) wrapped; row1 "defg"
    let lines = take(&mut term);
    let line = line_with(&lines, "abc漢defg");

    // chars 3..5 = "漢d"
    assert!(term.paint_logical_line(line.at, &line.text, &[fg_span(3, 5, RED)]));

    let g = term.grid();
    assert!(g.cell(0, 3).flags().contains(CellFlags::WIDE_CHAR));
    assert_eq!(g.cell(0, 3).fg(), RED);
    assert_eq!(
        g.cell(0, 4).fg(),
        RED,
        "the spacer is painted with its lead"
    );
    assert_eq!(g.cell(1, 0).fg(), RED);
    assert_eq!(g.cell(1, 1).fg(), Color::Default);
    assert_eq!(g.cell(0, 2).fg(), Color::Default);
}

/// Both channels can be painted at once, and a span past the text's end is clipped to it.
#[test]
fn paint_sets_both_channels_and_clips_to_the_text() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ok");
    let lines = take(&mut term);
    let span = PaintSpan {
        start: 1,
        end: 50,
        fg: Some(RED),
        bg: Some(BLUE),
    };
    assert!(term.paint_logical_line(lines[0].at, "ok", &[span]));

    let g = term.grid();
    assert_eq!((g.cell(0, 1).fg(), g.cell(0, 1).bg()), (RED, BLUE));
    assert_eq!(
        (g.cell(0, 2).fg(), g.cell(0, 2).bg()),
        (Color::Default, Color::Default)
    );
}

/// The colour is in the cell: it scrolls into scrollback with the text.
#[test]
fn a_painted_colour_scrolls_into_scrollback_with_its_text() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR");
    let lines = take(&mut term);
    assert!(term.paint_logical_line(lines[0].at, "ERROR", &[fg_span(0, 5, RED)]));

    term.feed(b"\r\n1\r\n2\r\n3");
    assert_eq!(term.scrollback_len(), 2);
    term.scroll_up(2);
    let top = term.viewport_line(0);
    assert_eq!(top[0].c(), 'E');
    assert_eq!(fgs(&top[..5]), vec![RED; 5]);
}

/// The colour survives a reflow: narrowing the pane re-wraps the painted text and keeps it red.
#[test]
fn a_painted_colour_survives_reflow() {
    let mut term = Engine::new(10, 3);
    term.feed(b"xxERROR");
    let lines = take(&mut term);
    assert!(term.paint_logical_line(lines[0].at, "xxERROR", &[fg_span(2, 7, RED)]));

    term.resize(4, 3);
    let g = term.grid();
    // "xxER" / "ROR"
    assert_eq!(
        fgs(&g.row(0)[..4]),
        vec![Color::Default, Color::Default, RED, RED]
    );
    assert_eq!(fgs(&g.row(1)[..3]), vec![RED, RED, RED]);
}

/// Later output overwrites a painted cell exactly as it overwrites an SGR colour — and the
/// rewritten line is reported again, so the consumer can re-apply its rule.
#[test]
fn later_output_overwrites_the_paint_and_reports_the_line_again() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR");
    let lines = take(&mut term);
    assert!(term.paint_logical_line(lines[0].at, "ERROR", &[fg_span(0, 5, RED)]));

    term.feed(b"\rERROR");
    assert_eq!(fgs(&term.grid().row(0)[..5]), vec![Color::Default; 5]);
    let again = take(&mut term);
    assert_eq!(line_with(&again, "ERROR").at, lines[0].at);
}

/// Painting changes no text, so it reports nothing — a consumer that paints what it was told
/// about is not told about it again.
#[test]
fn painting_reports_nothing_back() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR");
    let lines = take(&mut term);
    assert!(term.paint_logical_line(lines[0].at, "ERROR", &[fg_span(0, 5, RED)]));

    assert!(take(&mut term).is_empty());
}

/// A paint whose line no longer holds the text it was matched against is refused, and changes
/// nothing: the engine will not colour text the consumer never matched.
#[test]
fn a_paint_against_rewritten_text_is_refused() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR");
    let lines = take(&mut term);

    term.feed(b"\rWARN!");
    assert!(!term.paint_logical_line(lines[0].at, "ERROR", &[fg_span(0, 5, RED)]));
    assert_eq!(fgs(&term.grid().row(0)[..5]), vec![Color::Default; 5]);
}

/// A reference that starts mid-way through a logical line names no line, and is refused.
#[test]
fn a_reference_to_a_continuation_row_is_refused() {
    let mut term = Engine::new(5, 3);
    term.feed(b"abcdefgh");
    let lines = take(&mut term);
    let mut at = lines[0].at;
    at.line += 1;
    assert!(!term.paint_logical_line(at, "fgh", &[fg_span(0, 3, RED)]));
}

/// Lines evicted off the front between the ask and the paint do not strand it: the reference
/// is rebased by how many lines left, and lands on the line it was taken from.
#[test]
fn a_paint_lands_on_its_line_across_scrollback_eviction() {
    let mut term = Engine::with_scrollback(10, 2, 3);
    term.feed(b"a\r\nb\r\nc\r\nERROR");
    let lines = take(&mut term);
    let error = line_with(&lines, "ERROR").at;

    // Six lines through two rows leave four in scrollback; the cap of three evicts "a".
    term.feed(b"\r\nd\r\ne");
    assert_eq!(term.scrollback_len(), 3);
    assert!(term.paint_logical_line(error, "ERROR", &[fg_span(0, 5, RED)]));

    term.scroll_up(1); // viewport row 0 = the last scrollback line, "ERROR"
    let row = term.viewport_line(0);
    assert_eq!(row[0].c(), 'E');
    assert_eq!(fgs(&row[..5]), vec![RED; 5]);
}

/// A reference whose line has been evicted is refused.
#[test]
fn a_paint_whose_line_was_evicted_is_refused() {
    let mut term = Engine::with_scrollback(10, 2, 1);
    term.feed(b"ERROR");
    let lines = take(&mut term);
    term.feed(b"\r\n1\r\n2\r\n3");
    assert!(!term.paint_logical_line(lines[0].at, "ERROR", &[fg_span(0, 5, RED)]));
}

/// Painting a line on screen damages its span, so the next frame carries the colour.
#[test]
fn painting_an_on_screen_line_damages_its_span() {
    let mut term = Engine::new(10, 2);
    term.feed(b"an ERROR");
    let lines = take(&mut term);
    term.reset_damage();

    assert!(term.paint_logical_line(lines[0].at, "an ERROR", &[fg_span(3, 8, RED)]));
    match term.damage() {
        TermDamage::Partial(d) => {
            assert_eq!(d.len(), 1);
            assert_eq!((d[0].line, d[0].left, d[0].right), (0, 3, 7));
        }
        TermDamage::Full => panic!("a span paint is not a full repaint"),
    }
}

/// Painting a scrollback line the user is looking at repaints the view: while scrolled up the
/// partial damage is withheld, so only a full repaint can reach it.
#[test]
fn painting_a_visible_scrollback_line_repaints_the_view() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR\r\n1\r\n2");
    let lines = take(&mut term);
    term.scroll_up(1);
    term.reset_damage();

    assert!(term.paint_logical_line(
        line_with(&lines, "ERROR").at,
        "ERROR",
        &[fg_span(0, 5, RED)]
    ));
    assert_eq!(term.damage(), TermDamage::Full);
}

/// The alt screen reports its own lines, marked as the alt screen's; a full-screen redraw
/// overwrites the paint, since the colour lives in the cell and nowhere else.
#[test]
fn the_alt_screen_is_reported_and_its_redraw_erases_the_paint() {
    let mut term = Engine::new(10, 2);
    term.feed(b"\x1b[?1049h");
    take(&mut term);
    term.feed(b"ERROR");
    let lines = take(&mut term);
    let line = line_with(&lines, "ERROR");
    assert!(line.at.alt);
    assert!(term.paint_logical_line(line.at, "ERROR", &[fg_span(0, 5, RED)]));
    assert_eq!(term.grid().cell(0, 0).fg(), RED);

    term.feed(b"\x1b[H\x1b[2JERROR");
    assert_eq!(fgs(&term.grid().row(0)[..5]), vec![Color::Default; 5]);
}

/// A primary line's paint that arrives while the alt screen is up lands on the primary line, not
/// on the alt line at the same index, and is there when the primary screen comes back.
#[test]
fn a_primary_paint_arriving_during_the_alt_screen_lands_on_the_primary() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR");
    let primary = line_with(&take(&mut term), "ERROR").at;
    assert!(!primary.alt);

    term.feed(b"\x1b[?1049h\x1b[HERROR");
    assert!(term.paint_logical_line(primary, "ERROR", &[fg_span(0, 5, RED)]));
    assert_eq!(fgs(&term.grid().row(0)[..5]), vec![Color::Default; 5]);

    term.feed(b"\x1b[?1049l");
    assert_eq!(fgs(&term.grid().row(0)[..5]), vec![RED; 5]);
}

/// A resize between the ask and the paint moves lines the paint names, so it is refused — and
/// the lines that answer covered are reported again afterwards, so no match is lost to the race.
#[test]
fn lines_answered_before_a_resize_are_reported_again_after_it() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR one\r\n2\r\n3");
    let lines = take(&mut term);
    assert!(lines.iter().any(|l| l.text == "ERROR one"));
    assert_eq!(term.scrollback_len(), 1);

    term.resize(4, 2);
    let again = take(&mut term);
    assert!(
        again.iter().any(|l| l.text == "ERROR one"),
        "the scrollback line answered before the resize is reported again: {again:?}"
    );
}

/// Lines evicted between two answers shift the record of what changed with them, so nothing
/// that changed before an eviction goes unreported.
#[test]
fn a_change_before_an_eviction_is_still_reported() {
    let mut term = Engine::with_scrollback(10, 2, 1);
    term.feed(b"x\r\ny\r\nz");
    take(&mut term);

    // ERROR is written, then one more line evicts a scrollback line under it.
    term.feed(b"\r\nERROR\r\nq");
    let texts: Vec<String> = take(&mut term).into_iter().map(|l| l.text).collect();
    assert!(texts.contains(&"ERROR".to_string()), "{texts:?}");
}

/// An alt-screen paint is checked against the alt buffer's own start: a wrapped primary
/// scrollback row above it does not make its first row read as a continuation.
#[test]
fn an_alt_line_under_a_wrapped_scrollback_row_can_be_painted() {
    let mut term = Engine::new(5, 2);
    term.feed(b"abcdefgh\r\nz"); // scrollback: "abcde", soft-wrapped into a row now on screen
    term.feed(b"\x1b[?1049h\x1b[HERR");
    let lines = take(&mut term);
    let line = line_with(&lines, "ERR");
    assert!(line.at.alt);
    assert!(term.paint_logical_line(line.at, "ERR", &[fg_span(0, 3, RED)]));
    assert_eq!(term.grid().cell(0, 0).fg(), RED);
}

/// A full-screen scroll on the alt screen moves every line on it with no text changed; the
/// moved lines are reported again.
#[test]
fn lines_moved_by_an_alt_screen_scroll_are_reported_again() {
    let mut term = Engine::new(10, 3);
    term.feed(b"\x1b[?1049h\x1b[2;1HERROR\x1b[3;1H");
    let lines = take(&mut term);
    let error = line_with(&lines, "ERROR").at;

    term.feed(b"\n");
    assert_eq!(term.grid().cell(0, 0).c(), 'E');
    assert!(!term.paint_logical_line(error, "ERROR", &[fg_span(0, 5, RED)]));
    let moved = line_with(&take(&mut term), "ERROR").at;
    assert!(term.paint_logical_line(moved, "ERROR", &[fg_span(0, 5, RED)]));
}

/// A primary line that changed before the alt screen opened, and that a resize on the alt screen
/// reflowed into scrollback, is still reported once the primary screen is back.
#[test]
fn a_primary_change_reflowed_while_on_the_alt_screen_is_still_reported() {
    let mut term = Engine::new(5, 3);
    term.feed(b"1\r\n2\r\nabcdefghij\r\n");
    take(&mut term);

    term.feed(b"ERROR\r\nx\x1b[?1049h");
    term.resize(10, 1); // the primary joins "abcdefghij" and keeps only "x" on screen
    term.feed(b"\x1b[?1049l");
    assert_eq!(term.grid().cell(0, 0).c(), 'x');

    let texts: Vec<String> = take(&mut term).into_iter().map(|l| l.text).collect();
    assert!(texts.contains(&"ERROR".to_string()), "{texts:?}");
}

/// A resize that re-fits the alt screen moves its lines; they are reported again, at a
/// reference that paints them.
#[test]
fn lines_moved_by_an_alt_screen_resize_are_reported_again() {
    let mut term = Engine::new(10, 3);
    term.feed(b"\x1b[?1049h\x1b[3;1HERROR");
    take(&mut term);

    term.resize(10, 2);
    assert_eq!(term.grid().cell(1, 0).c(), 'E');
    let moved = line_with(&take(&mut term), "ERROR").at;
    assert!(term.paint_logical_line(moved, "ERROR", &[fg_span(0, 5, RED)]));
    assert_eq!(term.grid().cell(1, 0).fg(), RED);
}

/// A line inserted at the top of the screen cuts the soft wrap from scrollback into it; the
/// scrollback line, now ending where it did not before, is reported again.
#[test]
fn a_scrollback_line_whose_wrap_was_cut_is_reported_again() {
    let mut term = Engine::new(5, 3);
    // Rows: 1, abcde (wraps), fgh, x, y — scrollback holds "1" and "abcde".
    term.feed(b"1\r\nabcdefgh\r\nx\r\ny");
    assert_eq!(term.scrollback_len(), 2);
    take(&mut term);

    term.feed(b"\x1b[H\x1b[L"); // IL at row 0
    let texts: Vec<String> = take(&mut term).into_iter().map(|l| l.text).collect();
    assert!(texts.contains(&"abcde".to_string()), "{texts:?}");
}

/// A wide lead left in the last column with no spacer beside it — the alt screen re-fits rather
/// than re-wraps, so a narrowing cuts the pair — is painted alone.
#[test]
fn a_wide_lead_cut_from_its_spacer_is_painted_alone() {
    let mut term = Engine::new(10, 3);
    term.feed("\x1b[?1049haaaaaaaa中".as_bytes());
    take(&mut term);
    term.resize(9, 3);
    assert!(term.grid().cell(0, 8).flags().contains(CellFlags::WIDE_CHAR));

    let lines = take(&mut term);
    let line = line_with(&lines, "aaaaaaaa中");
    assert!(term.paint_logical_line(line.at, &line.text, &[fg_span(8, 9, RED)]));
    assert_eq!(term.grid().cell(0, 8).fg(), RED);
}

/// When the cap evicts the first row of a soft-wrapped line, the rows left behind are a line of
/// their own, with other text; it is reported.
#[test]
fn the_rest_of_a_line_whose_start_was_evicted_is_reported() {
    let mut term = Engine::with_scrollback(5, 2, 1);
    term.feed(b"ERROR0123456789"); // ERROR / 01234 / 56789, one logical line
    take(&mut term);

    term.feed(b"\r\nz"); // the cap evicts "ERROR"; "01234" + "56789" remain, wrapped
    let texts: Vec<String> = take(&mut term).into_iter().map(|l| l.text).collect();
    assert!(texts.contains(&"0123456789".to_string()), "{texts:?}");
}

/// A resize re-reports every line any answer since the previous resize covered, not only the
/// latest answer's — paints from several answers can be in flight at once.
#[test]
fn a_resize_re_reports_lines_from_every_answer_since_the_last_one() {
    let mut term = Engine::with_scrollback(10, 2, 100);
    term.feed(b"aaaaaaaaaaaa\r\nERROR\r\nb\r\nc");
    let first = take(&mut term);
    let error = line_with(&first, "ERROR").at;
    term.feed(b"\r\nd");
    take(&mut term);

    term.resize(20, 2);
    assert!(!term.paint_logical_line(error, "ERROR", &[fg_span(0, 5, RED)]));
    let again = take(&mut term);
    assert!(again.iter().any(|l| l.text == "ERROR"), "{again:?}");
}

/// A paint for an alt-screen line arriving after the alt screen closed is refused: the buffer it
/// names is gone from view and is cleared on the next entry.
#[test]
fn an_alt_paint_after_the_alt_screen_closed_is_refused() {
    let mut term = Engine::new(10, 2);
    term.feed(b"ERROR\x1b[?1049h\x1b[HERROR");
    let line = line_with(&take(&mut term), "ERROR").at;
    assert!(line.alt);
    term.feed(b"\x1b[?1049l");
    assert!(!term.paint_logical_line(line, "ERROR", &[fg_span(0, 5, RED)]));
    assert_eq!(fgs(&term.grid().row(0)[..5]), vec![Color::Default; 5]);
}

/// A region scroll that does not feed scrollback moves the lines inside it to other absolute
/// lines, so a paint taken before it is refused — and the moved lines are reported again.
#[test]
fn lines_moved_by_a_region_scroll_are_reported_again() {
    let mut term = Engine::new(10, 4);
    // Region rows 2..=4 (1-based); ERROR on its middle row, cursor on its bottom one.
    term.feed(b"top\x1b[2;4r\x1b[3;1HERROR\x1b[4;1H");
    let lines = take(&mut term);
    let error = line_with(&lines, "ERROR").at;

    term.feed(b"\n"); // LF at the bottom margin scrolls the region up: ERROR moves up a row
    assert_eq!(term.grid().cell(1, 0).c(), 'E');
    assert!(!term.paint_logical_line(error, "ERROR", &[fg_span(0, 5, RED)]));
    let again = take(&mut term);
    let moved = line_with(&again, "ERROR").at;
    assert!(term.paint_logical_line(moved, "ERROR", &[fg_span(0, 5, RED)]));
    assert_eq!(term.grid().cell(1, 0).fg(), RED);
}
