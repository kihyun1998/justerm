//! #829 — a curly underline reaches the screen, SGR 4:3 end to end (the core half).
//!
//! The **style** is the storage and `UnderlineStyle::None` is a member of it; there is no
//! second boolean saying whether the cell is underlined. `CellFlags::UNDERLINE` survives as a
//! *derived* view bit so every existing consumer keeps working, and the style setter is its
//! only writer — so "is it underlined" and "which style" cannot disagree.
//!
//! **Why that shape, rather than a flag plus a qualifier.** Three of the four references let the
//! two disagree and each one measurably pays for it: alacritty's display layer inserts a plain
//! `UNDERLINE` over an existing curly bit and its renderer draws *both* rects
//! (`alacritty/src/display/mod.rs:867-870` against `renderer/rects.rs:180-198`); xterm.js has two
//! readers that resolve the conflict in **opposite** directions, pinned by a test
//! (`AttributeData.ts:125-129` against `:39-44`, `BufferLine.test.ts:124-145`); xterm leaves both
//! bits set after `CSI 4m; CSI 21m` and lets each consumer pick a resolution (`html.c:208-216`
//! against `svg.c:271`). ghostty is the one where it is not representable — `underline` is an
//! `enum(u3)` with `none` among its members and there is no `underline: bool`
//! (`src/terminal/style.zig:29-40`), and that is the shape taken here.
//!
//! ## What these assertions can and cannot observe
//!
//! Stated because a test that cannot fail reads as coverage while proving nothing. Every row was
//! run as a mutation, not reasoned.
//!
//! | Mutation | Does this file redden? |
//! |---|---|
//! | `flags()` stops deriving `UNDERLINE` from the style | **yes** — four cases, both parser and type |
//! | derive `UNDERLINE` but drop the style bits on store | **yes** — every `underline_style()` assertion |
//! | let `CellFlags::UNDERLINE` with no style stay style-less | **yes** — `a_bare_underline_flag_normalises_to_single` |
//! | leave the style bits out of `flags()` (storage right, view wrong) | **yes** — `the_style_survives_an_encode_decode_round_trip` |
//! | forget the style in the vacated wide-wrap column | **yes** — `the_column_a_wrapped_wide_glyph_vacates_carries_the_style` |
//! | `SGR 24` clears `UNDERLINE` but leaves the style | **yes** — `sgr_24_clears_the_style_not_just_the_flag` |
//!
//! One row was **wrong in the first draft and the battery caught it**: a derived `FG_UNDERLINE`
//! copy in the fg word reddened nothing, because nothing reads it — the wire carries
//! `encode_color(cell.fg())` and `flags()` derives from the style. The copy was removed rather
//! than the row rewritten; see `flag_words`.
//!
//! What this file cannot observe: whether the renderer draws a *visibly* curled line. That is the
//! browser proof's job, and no host assertion substitutes for it.

use justerm_core::{Cell, CellFlags, Color, Engine, UnderlineStyle};

fn style_at(e: &Engine, row: usize, col: usize) -> UnderlineStyle {
    e.viewport_line(row)[col].underline_style()
}

fn flags_at(e: &Engine, row: usize, col: usize) -> CellFlags {
    e.viewport_line(row)[col].flags()
}

#[test]
fn sgr_4_3_sets_a_curly_underline_on_subsequent_cells() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA");
    assert_eq!(style_at(&e, 0, 0), UnderlineStyle::Curly);
}

#[test]
fn plain_sgr_4_is_a_single_underline_and_still_sets_the_flag() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4mA");
    assert_eq!(style_at(&e, 0, 0), UnderlineStyle::Single);
    assert!(
        flags_at(&e, 0, 0).contains(CellFlags::UNDERLINE),
        "an existing consumer reading UNDERLINE must be unaffected",
    );
}

#[test]
fn a_curly_cell_also_reports_the_derived_underline_flag() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA");
    assert!(
        flags_at(&e, 0, 0).contains(CellFlags::UNDERLINE),
        "UNDERLINE is derived from the style, so a curl is underlined",
    );
}

#[test]
fn sgr_24_clears_the_style_not_just_the_flag() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA\x1b[24mB");
    assert_eq!(style_at(&e, 0, 1), UnderlineStyle::None);
    assert!(!flags_at(&e, 0, 1).contains(CellFlags::UNDERLINE));
}

#[test]
fn sgr_0_clears_the_style() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA\x1b[0mB");
    assert_eq!(style_at(&e, 0, 1), UnderlineStyle::None);
    assert!(!flags_at(&e, 0, 1).contains(CellFlags::UNDERLINE));
}

#[test]
fn sgr_4_0_turns_the_underline_off() {
    // `4:0` is an explicit off in every reference that implements the sub-parameter form
    // (xterm.js `InputHandler.ts:2492-2495`, ghostty `sgr.zig`, vte `ansi.rs:1838`).
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA\x1b[4:0mB");
    assert_eq!(style_at(&e, 0, 1), UnderlineStyle::None);
    assert!(!flags_at(&e, 0, 1).contains(CellFlags::UNDERLINE));
}

#[test]
fn a_bare_underline_flag_normalises_to_single() {
    // The single-owner property, asserted at the type rather than through the parser: a cell
    // built with UNDERLINE and no style is a *single* underline, because "underlined with no
    // style" is not a state this model has. This is also what keeps `Cell`'s packing canonical,
    // which its `Eq` is a bitwise compare because of.
    let c = Cell::from_parts('A', Color::Default, Color::Default, CellFlags::UNDERLINE);
    assert_eq!(c.underline_style(), UnderlineStyle::Single);
    assert!(c.flags().contains(CellFlags::UNDERLINE));
}

#[test]
fn a_style_with_no_underline_flag_still_reads_as_underlined() {
    // The other direction of the same property: the style is authoritative, so a cell built from
    // style bits alone reports UNDERLINE. Neither input can produce a disagreeing cell.
    let mut f = CellFlags::empty();
    f.set_underline_style(UnderlineStyle::Dotted);
    let c = Cell::from_parts('A', Color::Default, Color::Default, f);
    assert_eq!(c.underline_style(), UnderlineStyle::Dotted);
    assert!(c.flags().contains(CellFlags::UNDERLINE));
}

#[test]
fn the_style_composes_with_sgr_58_without_disturbing_it() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3m\x1b[58:2::255:0:0mA");
    assert_eq!(style_at(&e, 0, 0), UnderlineStyle::Curly);
    assert_eq!(e.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
}

#[test]
fn setting_the_colour_does_not_disturb_the_style_and_vice_versa() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[58:2::0:255:0m\x1b[4:3mA");
    assert_eq!(style_at(&e, 0, 0), UnderlineStyle::Curly);
    assert_eq!(e.underline_color_at(0, 0), Color::Rgb(0, 255, 0));
}

#[test]
fn the_style_survives_scrolling_into_scrollback_and_back() {
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA\r\n\x1b[mB\r\nC\r\nD\r\nE");
    e.scroll_up(4);
    let found = (0..3).any(|r| style_at(&e, r, 0) == UnderlineStyle::Curly);
    assert!(found, "the curl must come back out of scrollback");
}

#[test]
fn the_style_survives_a_reflow_across_a_resize() {
    let mut e = Engine::new(6, 3);
    e.feed(b"\x1b[4:3mABCDEFGH");
    e.resize(4, 3);
    let mut seen = 0;
    for r in 0..3 {
        for c in 0..4 {
            if style_at(&e, r, c) == UnderlineStyle::Curly {
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 8, "every reflowed cell keeps its curl");
}

#[test]
fn the_column_a_wrapped_wide_glyph_vacates_carries_the_style() {
    // HS-1, from `architecture.md` §"Hidden VT state": the column a width-2 glyph vacates when it
    // wraps is a blank *written with the current pen*, carrying the pen's fg/bg and its armed
    // underline colour. Once the pen carries a style, that blank owes it too — a `reset()` to a
    // default cell would punch an unstyled notch into a styled run. Not named in #829's AC.
    let mut e = Engine::new(4, 3);
    e.feed("\u{1b}[4:3mabc\u{ac00}".as_bytes());
    assert_eq!(
        style_at(&e, 0, 3),
        UnderlineStyle::Curly,
        "the vacated column is written from the pen, so it carries the style",
    );
}

#[test]
fn the_style_survives_an_encode_decode_round_trip() {
    use justerm_core::{decode, encode};
    let mut e = Engine::new(20, 3);
    e.feed(b"\x1b[4:3mA\x1b[4:4mB\x1b[4:5mC\x1b[21mD");
    let f = e.frame();
    let bytes = encode(&f);
    let back = decode(&bytes).expect("a frame this engine produced must decode");
    assert_eq!(back, f, "the fixed point #531 broke for the colour axis");
}

#[test]
fn every_style_round_trips_through_the_packed_cell() {
    for s in [
        UnderlineStyle::None,
        UnderlineStyle::Single,
        UnderlineStyle::Double,
        UnderlineStyle::Curly,
        UnderlineStyle::Dotted,
        UnderlineStyle::Dashed,
    ] {
        let mut f = CellFlags::empty();
        f.set_underline_style(s);
        let c = Cell::from_parts('x', Color::Indexed(3), Color::Rgb(1, 2, 3), f);
        assert_eq!(c.underline_style(), s, "{s:?} must survive the packing");
        assert_eq!(c.c(), 'x', "{s:?} must not disturb the codepoint");
        assert_eq!(c.fg(), Color::Indexed(3), "{s:?} must not disturb fg");
    }
}

#[test]
fn removing_the_underline_flag_clears_the_style_rather_than_rewriting_it() {
    // A completeness pass found this: `remove_flags`/`insert_flags` set the packed words with
    // `|=` / `&= !`, and the style is a 3-BIT FIELD, so a bitwise clear of `UNDERLINE` — which
    // `flag_words` normalises to `Single` = 1 — subtracts bit 26 from `Curly` (3) and leaves
    // `Double` (2). The method's own contract says "clear the given flags (leaving the others as
    // they are)"; silently converting one underline style into a different one is not that.
    //
    // Zero in-repo call sites pass an underline flag today — all eight pass only wide/wrap
    // markers — but `Cell` is re-exported and published, and `remove_flags(UNDERLINE)` is exactly
    // the verb a consumer reaches for to un-underline a cell.
    let mut f = CellFlags::empty();
    f.set_underline_style(UnderlineStyle::Curly);
    let mut c = Cell::from_parts('x', Color::Default, Color::Default, f);

    c.remove_flags(CellFlags::UNDERLINE);
    assert_eq!(
        c.underline_style(),
        UnderlineStyle::None,
        "clearing the underline must clear the style, not turn a curl into a double",
    );
    assert!(!c.flags().contains(CellFlags::UNDERLINE));
}

#[test]
fn inserting_a_style_replaces_the_field_rather_than_or_ing_into_it() {
    // The other direction of the same defect: `Single` (1) OR `Dotted` (4) is `Dashed` (5).
    let mut f = CellFlags::empty();
    f.set_underline_style(UnderlineStyle::Single);
    let mut c = Cell::from_parts('x', Color::Default, Color::Default, f);

    let mut add = CellFlags::empty();
    add.set_underline_style(UnderlineStyle::Dotted);
    c.insert_flags(add);
    assert_eq!(c.underline_style(), UnderlineStyle::Dotted);
}

#[test]
fn inserting_an_unrelated_flag_leaves_the_style_alone() {
    // The guard against over-correcting: a caller adding a wide-char marker must not disturb the
    // style, which is what every in-repo call site actually does.
    let mut f = CellFlags::empty();
    f.set_underline_style(UnderlineStyle::Curly);
    let mut c = Cell::from_parts('x', Color::Default, Color::Default, f);
    c.insert_flags(CellFlags::WIDE_CHAR);
    assert_eq!(c.underline_style(), UnderlineStyle::Curly);
    c.remove_flags(CellFlags::WIDE_CHAR);
    assert_eq!(c.underline_style(), UnderlineStyle::Curly);
}

#[test]
fn a_cluster_promoted_to_wide_gives_its_spacer_the_leads_style() {
    // ADR-0025 D4: "any path that moves, SYNTHESISES or frees one cell of a width-2 glyph carries
    // the whole pair". `promote_cluster_to_wide` synthesises the spacer from `pen.cell(' ')`, and
    // its own comment already says why that is not enough — "the pen may have moved on since the
    // base was printed" — which is exactly why it re-attaches the lead's extended attrs. The
    // underline style rides the packed cell rather than a side map, so it needs the same care:
    // taking it from the pen paints the pair's left half curled and its right half bare.
    //
    // A refuting pass found this. It is pre-existing for the plain UNDERLINE flag, but #829 is
    // what claims the wide-pair rider carries the style for free, so the claim is made true here
    // rather than retracted.
    let mut e = Engine::new(10, 2);
    e.feed(b"\x1b[?2027h"); // grapheme clustering on
    e.feed("\x1b[4:3m\u{1F1F0}".as_bytes()); // curly regional indicator, narrow so far
    e.feed(b"\x1b[24m"); // the pen moves on: underline off
    e.feed("\u{1F1F7}".as_bytes()); // the joiner promotes the cluster to width 2

    assert_eq!(
        style_at(&e, 0, 0),
        UnderlineStyle::Curly,
        "the lead keeps its style"
    );
    assert_eq!(
        style_at(&e, 0, 1),
        UnderlineStyle::Curly,
        "and so must the spacer — it is the lead's second half, not a fresh pen cell",
    );
}
