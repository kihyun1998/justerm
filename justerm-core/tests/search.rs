//! #10 search — literal substring search over the grid + scrollback, returning
//! match ranges in absolute buffer coordinates. Built TDD. Smart-case: a query
//! with no uppercase matches case-insensitively. Matches cross soft-wraps and
//! skip wide-char spacers (reusing the selection coordinate model).

use justerm_core::{Engine, SelectionSpan};

/// A single-line literal match returns the inclusive range of the hit.
#[test]
fn search_finds_single_line_match() {
    let mut term = Engine::new(20, 2);
    term.feed(b"hello world");

    let matches = term.search("world");

    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert_eq!((m.start_line, m.start_col), (0, 6)); // 'w'
    assert_eq!((m.end_line, m.end_col), (0, 10)); // 'd' (inclusive)
}

/// Multiple non-overlapping matches are returned in order.
#[test]
fn search_returns_matches_in_order() {
    let mut term = Engine::new(20, 1);
    term.feed(b"ab ab ab");

    let m = term.search("ab");

    assert_eq!(m.len(), 3);
    assert_eq!((m[0].start_col, m[1].start_col, m[2].start_col), (0, 3, 6));
}

/// Smart-case: a lowercase query matches any case; a query with an uppercase
/// char is case-sensitive.
#[test]
fn search_is_smart_case() {
    let mut term = Engine::new(20, 1);
    term.feed(b"Hello hello");

    assert_eq!(term.search("hello").len(), 2); // no uppercase → case-insensitive
    let m = term.search("Hello");
    assert_eq!(m.len(), 1); // uppercase present → case-sensitive
    assert_eq!(m[0].start_col, 0);
}

/// A match in scrollback (above the screen) is found, with its absolute line.
#[test]
fn search_finds_match_in_scrollback() {
    let mut term = Engine::new(6, 2);
    term.feed(b"aaa\r\nbbb\r\nccc"); // "aaa" scrolls into history (abs line 0)

    let m = term.search("aaa");

    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 0));
}

/// A match spans a soft wrap (one logical line across two rows).
#[test]
fn search_crosses_soft_wrap() {
    let mut term = Engine::new(4, 3);
    term.feed(b"abcdef"); // "abcd"(wrap) / "ef"

    let m = term.search("cdef");

    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 2)); // 'c'
    assert_eq!((m[0].end_line, m[0].end_col), (1, 1)); // 'f' on the wrapped row
}

/// A wide glyph is one searchable char at its lead; the spacer is skipped.
#[test]
fn search_skips_wide_char_spacer() {
    let mut term = Engine::new(6, 1);
    term.feed("a한b".as_bytes()); // a(0) 한=lead(1)+spacer(2) b(3)

    let m = term.search("한");
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 1));
    assert_eq!((m[0].end_line, m[0].end_col), (0, 1)); // single cell (the lead)
}

/// No match → empty.
#[test]
fn search_no_match_is_empty() {
    let mut term = Engine::new(10, 1);
    term.feed(b"hello");

    assert!(term.search("zzz").is_empty());
    assert!(term.search("").is_empty()); // empty query never matches
}

/// `scroll_to_match` moves the viewport so a history match becomes visible.
#[test]
fn scroll_to_match_reveals_history_match() {
    let mut term = Engine::new(6, 2);
    term.feed(b"aaa\r\nbbb\r\nccc"); // "aaa" in history; bottom view shows bbb/ccc
    let m = term.search("aaa");
    assert_eq!(m.len(), 1);

    term.scroll_to_match(&m[0]);

    let top: String = term.viewport_line(0).iter().map(|c| c.c()).collect();
    assert_eq!(top.trim_end(), "aaa");
}

/// `match_spans` projects a match to inclusive viewport spans for highlighting.
#[test]
fn match_spans_project_to_viewport() {
    let mut term = Engine::new(20, 2);
    term.feed(b"hello world");
    let m = term.search("world");

    assert_eq!(
        term.match_spans(&m[0]),
        vec![SelectionSpan {
            row: 0,
            left: 6,
            right: 10
        }]
    );
}

/// `match_spans` clips: an off-screen (history) match yields no spans until the
/// viewport is scrolled to it.
#[test]
fn match_spans_clip_off_screen() {
    let mut term = Engine::new(6, 2);
    term.feed(b"aaa\r\nbbb\r\nccc"); // "aaa" in history; bottom view = bbb/ccc
    let m = term.search("aaa");

    assert!(term.match_spans(&m[0]).is_empty()); // above the viewport → clipped

    term.scroll_to_match(&m[0]);
    assert_eq!(
        term.match_spans(&m[0]),
        vec![SelectionSpan {
            row: 0,
            left: 0,
            right: 2
        }]
    );
}

/// A query spanning a wide char that wrapped off the right edge matches: the
/// vacated column is a leading spacer the haystack skips, so the join is clean
/// ("abcd한", not "abcd 한"). Regression for the wide-wrap WRAPLINE/spacer fix.
#[test]
fn search_crosses_wide_char_wrap_boundary() {
    let mut term = Engine::new(5, 2);
    term.feed("abcd한".as_bytes()); // '한' can't fit col4 → wraps: "abcd" | "한"

    let m = term.search("d한");

    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 3)); // 'd'
    assert_eq!((m[0].end_line, m[0].end_col), (1, 0)); // '한' body
}

/// On the alt screen, `search()` must NOT reach into *primary* scrollback (#144):
/// those matches are unreachable (you can't scroll to them on alt), and joining a
/// primary scrollback WRAPLINE row into the alt grid corrupts the haystack at the
/// boundary. The alt buffer is separate — the analog of #113's
/// `viewport_logical_lines` guard (floor the walk at `scrollback.len()` when
/// `on_alt`; selection likewise clears on alt-swap). Regression for the unguarded
/// whole-buffer search walk.
#[test]
fn search_on_alt_does_not_cross_into_primary_scrollback() {
    let mut term = Engine::new(5, 2);
    term.feed(b"abcdefghijklmno"); // primary soft-wraps; "abcde"(WRAPLINE) evicts to scrollback
    term.feed(b"\x1b[?1049h\x1b[H"); // enter alt (separate buffer), home cursor
    term.feed(b"XY"); // alt content at row 0

    // A string that lives only in primary scrollback is unreachable on alt → no match.
    assert!(
        term.search("abcde").is_empty(),
        "primary scrollback is not searched on the alt screen"
    );
    // The cross-boundary join "abcde" + "XY" must not form a haystack → no phantom.
    assert!(
        term.search("deXY").is_empty(),
        "no primary→alt haystack corruption across the buffer boundary"
    );
    // Alt content is found at its absolute line (scrollback.len() == 1 → alt row 0 = abs 1).
    let m = term.search("XY");
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (1, 0));
    assert_eq!((m[0].end_line, m[0].end_col), (1, 1));
}

#[test]
fn search_matches_a_combining_mark_in_the_side_table() {
    // #304: a combining cluster's marks live in the row's side-table, not the codepoint column, so
    // search must include them — "e\u{0301}" (é decomposed) must be findable, not just its base 'e'.
    let mut term = Engine::new(20, 1);
    term.feed("xe\u{0301}y".as_bytes()); // 'x', é (e + combining acute), 'y'
    let m = term.search("e\u{0301}");
    assert_eq!(m.len(), 1, "the decomposed grapheme is found");
    assert_eq!(
        (m[0].start_line, m[0].start_col),
        (0, 1),
        "starts at the 'e' cell"
    );
    assert_eq!(
        (m[0].end_line, m[0].end_col),
        (0, 1),
        "the mark maps to the same cell"
    );
}

#[test]
fn search_matches_a_clustered_emoji_scalar_under_mode_2027() {
    // #304 (amplified by #295): under mode 2027 a flag clusters into one cell — base RI + the 2nd RI
    // in the side-table. Search must find the 2nd RI (with mode OFF it was its own searchable cell).
    let mut term = Engine::new(20, 1);
    term.feed(b"\x1b[?2027h");
    term.feed("\u{1F1F0}\u{1F1F7}".as_bytes()); // 🇰🇷 clustered into one wide cell
    let m = term.search("\u{1F1F7}"); // the 2nd regional indicator (🇷)
    assert_eq!(m.len(), 1, "the clustered 2nd RI is findable");
    assert_eq!(
        (m[0].start_col, m[0].end_col),
        (0, 0),
        "maps to the flag's single cell"
    );
}

#[test]
fn search_does_not_duplicate_a_match_for_a_repeated_in_cluster_scalar() {
    // #304 2-lens (Lens 1): two stacked identical marks in ONE cell's cluster ("a" + two combining
    // acutes) both live in the side-table at the same column. Searching that mark must yield ONE
    // Match at (0,0), not a duplicate per repeated hay entry.
    let mut term = Engine::new(20, 1);
    term.feed("a\u{0301}\u{0301}".as_bytes());
    let m = term.search("\u{0301}");
    assert_eq!(m.len(), 1, "one match, not one per stacked mark");
    assert_eq!((m[0].start_col, m[0].end_col), (0, 0));
}

#[test]
fn search_with_case_sensitive_option_overrides_smart_case() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(20, 1);
    term.feed(b"Hello hello");
    // Smart-case (default): a lowercase query matches both.
    assert_eq!(term.search("hello").len(), 2);
    // case_sensitive = Some(true): only the exact-case "hello" (col 6) matches.
    let opts = SearchOptions {
        case_sensitive: Some(true),
        ..Default::default()
    };
    let m = term.search_with("hello", opts);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].start_col, 6);
    // case_sensitive = Some(false): force case-insensitive even for an uppercase query.
    let ci = SearchOptions {
        case_sensitive: Some(false),
        ..Default::default()
    };
    assert_eq!(term.search_with("HELLO", ci).len(), 2);
}

#[test]
fn search_with_whole_word_bounds_the_match() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(30, 1);
    term.feed(b"cat category scattered cat.");
    // Literal "cat" matches inside "category" and "scattered" too.
    assert_eq!(term.search("cat").len(), 4);
    // whole_word: only the standalone "cat" tokens (col 0, and col 23 before '.').
    let opts = SearchOptions {
        whole_word: true,
        ..Default::default()
    };
    let m = term.search_with("cat", opts);
    assert_eq!(m.len(), 2, "only standalone 'cat'");
    assert_eq!((m[0].start_col, m[1].start_col), (0, 23));
}

#[test]
fn search_with_regex_matches_a_pattern() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(30, 1);
    term.feed(b"err 42 err 7 warn 99");
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    let m = term.search_with(r"\d+", opts); // digit runs: 42, 7, 99
    assert_eq!(m.len(), 3);
    assert_eq!((m[0].start_col, m[0].end_col), (4, 5)); // "42"
    assert_eq!((m[1].start_col, m[1].end_col), (11, 11)); // "7"
    assert_eq!((m[2].start_col, m[2].end_col), (18, 19)); // "99"
}

#[test]
fn search_with_invalid_regex_returns_no_matches() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(20, 1);
    term.feed(b"abc");
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    assert_eq!(term.search_with("(unclosed", opts).len(), 0);
}

#[test]
fn search_with_regex_respects_smart_case_and_override() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(20, 1);
    term.feed("Err err ERR".as_bytes());
    // Smart-case regex (lowercase pattern) → all 3.
    let re = SearchOptions {
        regex: true,
        ..Default::default()
    };
    assert_eq!(term.search_with("err", re).len(), 3);
    // Case-sensitive override → only the lowercase "err" (col 4).
    let cs = SearchOptions {
        regex: true,
        case_sensitive: Some(true),
        ..Default::default()
    };
    let m = term.search_with("err", cs);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].start_col, 4);
}

#[test]
fn search_with_regex_maps_multibyte_columns_correctly() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(20, 1);
    // 'é' is 2 UTF-8 bytes but ONE column; the byte→char→column mapping must not drift.
    term.feed("café 42".as_bytes());
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    let m = term.search_with(r"\d+", opts);
    assert_eq!(m.len(), 1);
    assert_eq!(
        (m[0].start_col, m[0].end_col),
        (5, 6),
        "42 sits at cols 5-6 after 'café '"
    );
}

#[test]
fn search_with_regex_matches_across_a_soft_wrap() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(4, 2); // 4 cols → "abcd" wraps
    term.feed(b"abcdef");
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    let m = term.search_with("cde", opts); // spans the soft-wrap (c,d on row0; e on row1)
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].start_line, m[0].start_col), (0, 2));
    assert_eq!((m[0].end_line, m[0].end_col), (1, 0));
}

#[test]
fn search_with_whole_word_treats_a_combining_mark_as_part_of_the_word() {
    use justerm_core::SearchOptions;
    // #314 2-lens (Lens 1): "cat" + a combining acute on 't' + "s" renders ONE word "catś".
    // Whole-word "cat" must NOT match (it's only a prefix) — a combining mark is not a boundary.
    let mut term = Engine::new(20, 1);
    term.feed("cat\u{0301}s".as_bytes());
    let opts = SearchOptions {
        whole_word: true,
        ..Default::default()
    };
    assert_eq!(
        term.search_with("cat", opts).len(),
        0,
        "cat is a prefix of catś, not whole"
    );
    // A left-edge mark is symmetric: "s´cat" → 'cat' is a suffix, not whole.
    let mut t2 = Engine::new(20, 1);
    t2.feed("s\u{0301}cat".as_bytes());
    assert_eq!(t2.search_with("cat", opts).len(), 0);
    // Control: a real word boundary (space) still matches.
    let mut t3 = Engine::new(20, 1);
    t3.feed("a cat s".as_bytes());
    assert_eq!(t3.search_with("cat", opts).len(), 1);
}

#[test]
fn search_with_regex_ignores_trailing_blank_padding() {
    use justerm_core::SearchOptions;
    let mut term = Engine::new(20, 1); // "hi" then 18 blank padding cols
    term.feed(b"hi");
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    // `$` anchors to the visible end, not after the padding.
    assert_eq!(
        term.search_with("hi$", opts).len(),
        1,
        "$ matches after 'hi', not padding"
    );
    // Greedy `.*` stops at the visible text end (col 1), not the padded col 19.
    let g = term.search_with("h.*", opts);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].end_col, 1, "greedy stops at visible text, not padding");
}
