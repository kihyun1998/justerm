//! #5 selection — engine-owned char/word/line/block selection, `selection_range`
//! (viewport line spans for highlight) and `selection_text` (copy across
//! scrollback). Built TDD, one behaviour per test; the risky coordinate cases
//! (cap eviction, region/RI rotate, resize reflow) are each pinned by a test so
//! "green" means the rotate is actually correct, not just correct-looking.

use justerm_core::{Engine, SelectionSpan, SelectionType, Side};

// ===========================================================================
// Char selection — the tracer bullet
// ===========================================================================

/// A char selection over one printed line copies exactly that text. Side::Left
/// at the start includes the start cell; Side::Right at the end includes the end
/// cell — so `[0,0)L .. (0,4)R` over "hello" is the whole word.
#[test]
fn char_select_one_line() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 4, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("hello"));
}

/// A char selection across a hard line-end joins with `\n` and trims each line's
/// trailing blanks. "ab" / "cd" on two rows → "ab\ncd", not "ab<76 spaces>\ncd".
#[test]
fn char_select_multi_line_trims_and_joins_with_newline() {
    let mut term = Engine::new(80, 24);
    term.feed(b"ab\r\ncd");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(1, 1, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("ab\ncd"));
}

/// A soft wrap (WRAPLINE) joins with no break — and spaces that sit at the wrap
/// boundary are real content, not trailing blanks, so they survive. "ab  " fills
/// a width-4 row and wraps into "cd"; the logical line is "ab  cd". Per-row
/// trimming would wrongly yield "abcd" — trimming is a logical-line-end concern.
#[test]
fn char_select_soft_wrap_joins_and_keeps_interior_spaces() {
    let mut term = Engine::new(4, 4);
    term.feed(b"ab  cd");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(1, 1, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("ab  cd"));
}

/// A selection can span the scrollback→screen boundary. With "L0".."L3" fed to a
/// 2-row screen, "L0"/"L1" are in scrollback and "L2"/"L3" on screen. Scrolled up
/// by one, viewport row 0 = "L1" (history) and row 1 = "L2" (screen); selecting
/// both copies "L1\nL2" — the absolute coordinate bridges the two stores.
#[test]
fn char_select_across_scrollback_boundary() {
    let mut term = Engine::new(4, 2);
    term.feed(b"L0\r\nL1\r\nL2\r\nL3");
    term.scroll_up(1);

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(1, 1, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("L1\nL2"));
}

/// The anchor side decides cell inclusion: Right at the start excludes that cell,
/// Left at the end excludes it. And a degenerate range (the two sides invert the
/// same cell) is empty, never a panic.
#[test]
fn selection_side_excludes_and_degenerate_is_empty() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    term.selection_begin(0, 0, Side::Right, SelectionType::Char);
    term.selection_extend(0, 4, Side::Left);
    assert_eq!(term.selection_text().as_deref(), Some("ell")); // 'h', 'o' excluded

    // Sides invert the same cell → empty (must not index-panic).
    term.selection_begin(0, 2, Side::Right, SelectionType::Char);
    term.selection_extend(0, 2, Side::Left);
    assert_eq!(term.selection_text().as_deref(), Some(""));
}

/// A Word selection snaps to word boundaries (double-click). Clicking inside
/// "bar" selects the whole word, stopping at the surrounding spaces.
#[test]
fn word_select_expands_to_word_boundaries() {
    let mut term = Engine::new(80, 24);
    term.feed(b"foo bar baz");

    term.selection_begin(0, 5, Side::Left, SelectionType::Word); // col 5 = 'a' in "bar"

    assert_eq!(term.selection_text().as_deref(), Some("bar"));
}

/// A Word selection follows a word across a soft wrap. "abcdef" fills a width-4
/// row ("abcd") and wraps ("ef"); double-clicking the "ef" half selects the
/// whole logical word "abcdef".
#[test]
fn word_select_crosses_soft_wrap() {
    let mut term = Engine::new(4, 4);
    term.feed(b"abcdef");

    term.selection_begin(1, 0, Side::Left, SelectionType::Word); // 'e' on the wrapped row

    assert_eq!(term.selection_text().as_deref(), Some("abcdef"));
}

/// A Line selection takes whole lines regardless of the click column, trimmed.
#[test]
fn line_select_takes_whole_lines() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello world");

    term.selection_begin(0, 3, Side::Left, SelectionType::Line);

    assert_eq!(term.selection_text().as_deref(), Some("hello world"));
}

/// A Block selection is rectangular: the same column range on each row, joined
/// by newlines, each row trimmed. Columns 1..=2 over "abcd"/"efgh" → "bc\nfg".
#[test]
fn block_select_is_rectangular() {
    let mut term = Engine::new(80, 24);
    term.feed(b"abcd\r\nefgh");

    term.selection_begin(0, 1, Side::Left, SelectionType::Block);
    term.selection_extend(1, 2, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("bc\nfg"));
}

/// A wide glyph occupies two cells (lead + spacer); copying a selection over it
/// emits the glyph once — the WIDE_CHAR_SPACER cell is skipped, not turned into a
/// stray blank.
#[test]
fn selection_skips_wide_char_spacer() {
    let mut term = Engine::new(80, 24);
    term.feed("한국".as_bytes()); // two width-2 glyphs → cols 0..=3

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 3, Side::Right); // through both glyphs incl. spacers

    assert_eq!(term.selection_text().as_deref(), Some("한국"));
}

// ===========================================================================
// selection_range — viewport line spans for highlight (option (a))
// ===========================================================================

/// `selection_range` reports one span per visible row, columns inclusive.
#[test]
fn selection_range_reports_viewport_spans() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 4, Side::Right);

    assert_eq!(
        term.selection_range(),
        vec![SelectionSpan {
            row: 0,
            left: 0,
            right: 4
        }]
    );
}

/// While scrolled, `selection_range` maps absolute lines to viewport rows and
/// drops the parts scrolled off-screen. A Line selection over abs lines 2..=4 on
/// a 3-row screen: at the bottom it fills rows 0..2; scrolled up by one it shifts
/// to rows 1..2 and the third line (now below the viewport) is dropped.
#[test]
fn selection_range_clips_to_viewport_when_scrolled() {
    let mut term = Engine::new(4, 3);
    term.feed(b"L0\r\nL1\r\nL2\r\nL3\r\nL4"); // sb=[L0,L1], screen=[L2,L3,L4]

    term.selection_begin(0, 0, Side::Left, SelectionType::Line); // abs 2
    term.selection_extend(2, 0, Side::Left); // abs 4

    assert_eq!(
        term.selection_range(),
        vec![
            SelectionSpan {
                row: 0,
                left: 0,
                right: 3
            },
            SelectionSpan {
                row: 1,
                left: 0,
                right: 3
            },
            SelectionSpan {
                row: 2,
                left: 0,
                right: 3
            },
        ]
    );

    term.scroll_up(1); // viewport now abs 1..=3; selection abs 2..=4
    assert_eq!(
        term.selection_range(),
        vec![
            SelectionSpan {
                row: 1,
                left: 0,
                right: 3
            }, // abs 2
            SelectionSpan {
                row: 2,
                left: 0,
                right: 3
            }, // abs 3
               // abs 4 is now below the viewport → dropped
        ]
    );
}

// ===========================================================================
// Coordinate stability — the risky absolute-coordinate cases
// ===========================================================================

/// When the scrollback cap evicts the oldest line, every absolute index shifts
/// down by one — the selection anchors must follow so they keep pointing at the
/// same content. With a 2-line cap, selecting "L2" then pushing one more line
/// (which evicts "L0") must still copy "L2", not the line that slid into its old
/// absolute slot.
#[test]
fn selection_follows_cap_eviction() {
    let mut term = Engine::with_scrollback(4, 2, 2); // cap = 2 scrollback lines
    term.feed(b"L0\r\nL1\r\nL2\r\nL3"); // sb=[L0,L1], screen=[L2,L3]

    // Select "L2" (screen row 0, abs 2) as a whole line.
    term.selection_begin(0, 0, Side::Left, SelectionType::Line);
    assert_eq!(term.selection_text().as_deref(), Some("L2"));

    // One more line: scroll pushes "L2" to history and the cap evicts "L0",
    // shifting all absolute indices down by one.
    term.feed(b"\r\nL4"); // sb=[L1,L2], screen=[L3,L4]

    assert_eq!(term.selection_text().as_deref(), Some("L2"));
}

/// A scroll-region scroll (top margin > 0) moves content *within* the screen, so
/// absolute indices in the region shift — unlike a top-anchored scroll, which is
/// absorbed by scrollback growth. The selection must rotate with the content.
/// Region rows 2..=4, screen "A/B/C/D"; select "C"; a region scroll slides "C"
/// up one row, and the selection still copies "C".
#[test]
fn selection_rotates_with_region_scroll() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[2;4r"); // DECSTBM: region rows 2..4 (0-based 1..=3), homes cursor
    term.feed(b"A\r\nB\r\nC\r\nD"); // rows 0=A,1=B,2=C,3=D

    term.selection_begin(2, 0, Side::Left, SelectionType::Line); // "C" (abs 2)
    assert_eq!(term.selection_text().as_deref(), Some("C"));

    term.feed(b"\r\n"); // line-feed at the bottom margin → region scrolls up

    assert_eq!(term.selection_text().as_deref(), Some("C"));
}

/// Reverse index (RI) at the top margin scrolls the region *down*; the selection
/// rotates the other way. Same region; select "C"; RI slides it down one row.
#[test]
fn selection_rotates_with_reverse_index() {
    let mut term = Engine::new(4, 4);
    term.feed(b"\x1b[2;4r");
    term.feed(b"A\r\nB\r\nC\r\nD"); // rows 0=A,1=B,2=C,3=D
    term.feed(b"\x1b[2;1H"); // cursor to the region top (0-based row 1)

    term.selection_begin(2, 0, Side::Left, SelectionType::Line); // "C" (abs 2)
    assert_eq!(term.selection_text().as_deref(), Some("C"));

    term.feed(b"\x1bM"); // RI at top margin → region scrolls down

    assert_eq!(term.selection_text().as_deref(), Some("C"));
}

/// A column resize reflows soft-wrapped lines, moving content's absolute
/// coordinates — the selection anchors must reflow with it. "abcdef" on one
/// width-6 row, fully selected, then narrowed to width 3 (wrapping into
/// "abc"/"def"): the selection still copies "abcdef".
#[test]
fn selection_survives_reflow_on_resize() {
    let mut term = Engine::new(6, 4);
    term.feed(b"abcdef"); // row 0 = "abcdef"

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 5, Side::Right);
    assert_eq!(term.selection_text().as_deref(), Some("abcdef"));

    term.resize(3, 4); // narrow → "abc"(wrap)/"def"

    assert_eq!(term.selection_text().as_deref(), Some("abcdef"));
}

/// The selection belongs to the primary screen; switching to/from the alt screen
/// clears it (it can't meaningfully survive a screen swap).
#[test]
fn selection_clears_on_alt_screen_switch() {
    let mut term = Engine::new(80, 24);
    term.feed(b"hello");

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 4, Side::Right);
    assert_eq!(term.selection_text().as_deref(), Some("hello"));

    term.feed(b"\x1b[?1049h"); // enter alt screen
    assert_eq!(term.selection_text(), None);

    // A selection made on the alt screen is dropped when leaving it.
    term.feed(b"\x1b[Hworld"); // home the cursor (alt enter doesn't), then print
    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(0, 4, Side::Right);
    assert_eq!(term.selection_text().as_deref(), Some("world"));

    term.feed(b"\x1b[?1049l"); // leave alt screen
    assert_eq!(term.selection_text(), None);
}
