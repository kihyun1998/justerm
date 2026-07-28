//! #547 — the engine publishes a minimum width of two columns.
//!
//! A width-2 glyph cannot be represented at one column: there is no second cell for its
//! `WIDE_CHAR_SPACER`. Before this floor the crate had three different answers for that
//! width — the print path emitted a lone lead behind a leading-spacer row, reflow left the
//! pair split across two rows, and `frame()` panicked (#536) — so the same content had two
//! grid shapes depending on how it got there, and a third path crashed.
//!
//! Every reference either forbids the width or destroys the glyph, 0 of 3 supporting a lone
//! lead: alacritty `pub const MIN_COLUMNS: usize = 2` (*"A minimum of 2 is necessary to hold
//! fullwidth unicode characters"*, `alacritty_terminal/src/term/mod.rs:35-36` @ `852e971`,
//! enforced in the *app* at `alacritty/src/display/mod.rs:249`),
//! xterm.js `MINIMUM_COLS = 2, // Less than 2 can mess with wide chars`
//! (`src/common/services/BufferService.ts:13` @ `699f553`), and ghostty permits it but
//! *"wide characters are just destroyed"* (`src/terminal/PageList.zig:1783-1788` @ `e6e26e1`).
//! justerm clamps where xterm.js clamps — inside the core (`src/common`, the layer equivalent
//! of `justerm-core`) rather than in an app layer, which justerm does not have. ghostty is the
//! cautionary case for the other placement: it declares the floor downstream's job
//! (*"terminals should never be only 1-wide. We should prevent this downstream."*) and its
//! downstream floors at **1** (`renderer/size.zig:260`), so it ships the path it calls broken.
//!
//! Why the boundary is exactly two, mechanically rather than by authority: xterm.js's reflow
//! emits line lengths of only `newCols` or `newCols - 1` (the latter when a line ends in a wide
//! char), so at one column that length is zero and the loop never advances — *"Calling this
//! with a `newCols` value of `1` will lock up."* (`common/buffer/BufferReflow.ts:173`). Two is
//! the width at which a pair only just fits, and ghostty exercises it heavily (20+ `cols = 2`
//! tests in `Terminal.zig` and `PageList.zig`) with no recorded defect remaining there.
//!
//! The alternative — keep one column and take the lead alone — was tried in #533 and withdrawn
//! for losing data irreversibly. `narrowing_never_splits_a_pair_across_rows` below pins the
//! property that rules out *both* rejected options, and carries the measured loss in its docs.
//!
//! This is what makes ADR-0025 D4 (*"both halves of a pair move together, set and clear"*)
//! unconditionally satisfiable: a pair always has room.

use justerm_core::{Engine, MIN_COLUMNS};

/// The floor is two, and it is two *because* a wide glyph needs a lead and a spacer.
#[test]
fn min_columns_is_two() {
    assert_eq!(MIN_COLUMNS, 2);
}

/// Construction clamps. Note this is a stronger statement than "it already clamped to 1":
/// `Term::with_scrollback` had **no** column clamp at all, so `Engine::new(0, r)` built a
/// zero-width grid — only `resize` clamped.
#[test]
fn construction_clamps_to_the_floor() {
    assert_eq!(Engine::new(1, 3).grid().cols(), MIN_COLUMNS);
    assert_eq!(Engine::new(0, 3).grid().cols(), MIN_COLUMNS);
    assert_eq!(
        Engine::with_scrollback(1, 3, 100).grid().cols(),
        MIN_COLUMNS
    );

    // Above the floor is untouched — the clamp is a floor, not a rewrite.
    assert_eq!(Engine::new(80, 24).grid().cols(), 80);
}

/// Resize clamps at the same value, so a consumer dragging a pane narrow lands on a width
/// the engine can represent rather than one it has three answers for.
#[test]
fn resize_clamps_to_the_floor() {
    let mut term = Engine::new(8, 3);
    term.resize(1, 3);
    assert_eq!(term.grid().cols(), MIN_COLUMNS);

    term.resize(0, 3);
    assert_eq!(term.grid().cols(), MIN_COLUMNS);

    term.resize(8, 3);
    assert_eq!(term.grid().cols(), 8);
}

/// The point of the floor: at the narrowest supported width a wide glyph is still a *pair* —
/// a `WIDE_CHAR` lead followed by its spacer — not a lone lead.
#[test]
fn a_wide_glyph_is_a_whole_pair_at_the_floor() {
    let mut term = Engine::new(1, 2);
    term.feed("한".as_bytes());

    let grid = term.grid();
    let row = grid.row(0);
    assert!(row[0].is_wide(), "lead must carry WIDE_CHAR");
    assert_eq!(row[0].c(), '한');
    assert!(
        row[1].is_spacer(),
        "the pair's second half must be a spacer"
    );
}

/// Print and reflow now agree at the narrowest width, which they did not before: the print
/// path emitted `[a][b][' ' leading-spacer][한 WIDE][c][d]` and reflow emitted
/// `[a][b][한 WIDE][' ' spacer][c][d]` for the same content.
#[test]
fn print_and_reflow_agree_at_the_floor() {
    let mut printed = Engine::new(1, 8);
    printed.feed("ab한cd".as_bytes());

    let mut reflowed = Engine::new(8, 8);
    reflowed.feed("ab한cd".as_bytes());
    reflowed.resize(1, 8);

    assert_eq!(cells(&printed), cells(&reflowed));
}

/// #536's reproduction, through the public API: a width-2 glyph on the narrowest screen the
/// engine accepts used to record an out-of-range damage span and panic in `frame()`. The floor
/// makes that reproduction unreachable; the clamp itself landed separately, as #536's own scope
/// (`damage_span` now asserts its span in debug and clamps it in release, so no caller computing
/// `col + width` can store an out-of-range bound). Both halves are needed and neither subsumes the
/// other: the floor removes this input, the clamp removes the class.
#[test]
fn frame_does_not_panic_at_the_floor_after_a_wide_glyph() {
    let mut term = Engine::new(1, 3);
    term.feed(b"A");
    term.feed(b"\x1b[1;1H");
    term.feed("한".as_bytes());

    let frame = term.frame();
    assert_eq!(frame.cols, MIN_COLUMNS as u16);
}

/// Narrowing never *splits* a pair across rows. This is the property that ruled out the two
/// rejected options: reflow at one column strands the `WIDE_CHAR_SPACER` at column 0 of the
/// row below a live lead, and the withdrawn #533 attempt to keep one column by taking the lead
/// alone lost the spacer irreversibly — after which `write_glyph`'s orphan repair
/// (`term.rs:2875-2877`) freed the wrong cell, turning `"ab한cd"` into `"abX d"` on the next
/// overwrite. A pair that never splits has neither failure available to it.
///
/// The round trip below is the *consequence*, not the discriminator: it passed before the
/// floor too, because master leaves the pair split rather than dropping half of it.
#[test]
fn narrowing_never_splits_a_pair_across_rows() {
    let mut term = Engine::new(8, 8);
    term.feed("ab한cd".as_bytes());
    term.resize(1, 8);

    let (row, col) = wide_lead(&term).expect("the wide lead survives the narrowing");
    let grid = term.grid();
    assert!(
        col + 1 < grid.cols(),
        "lead at ({row},{col}) has no column for its spacer — the pair is split across rows"
    );
    assert!(grid.row(row)[col + 1].is_spacer());

    term.resize(8, 8);
    term.feed(b"\x1b[1;3HX");
    assert_eq!(row_text(&term, 0).trim_end(), "abX cd");
}

/// A `WIDE_CHAR` lead **can** stand in the last column, and a relocation that lands beside one
/// at the floor must not index past the row (#529).
///
/// The floor guarantees a pair *fits*; it does not guarantee every path keeps one together. The
/// alt screen resizes but does not reflow (#567), so `Row::resize` truncates: 한 at columns 1-2
/// of a 4-column alt screen narrowed to 2 loses its spacer and leaves a lead in the last column.
/// A mode-2027 promotion then relocating onto that row asks `cell(nr, 1).is_wide()` — true — and
/// without the width bound reads `(nr, 2)` on a 2-column grid: `index out of bounds: the len is
/// 2 but the index is 2`, in a library, inside a consumer's process.
///
/// The lead-less lead itself is **not** repaired here and is not this issue's scope; this test
/// pins only that the relocation survives meeting one. Both halves were measured: with the bound
/// removed this input panics, and with it restored the whole workspace is green.
#[test]
fn a_relocation_beside_a_truncated_wide_lead_does_not_index_past_the_row() {
    let mut term = Engine::new(4, 3);
    term.feed(b"\x1b[?1049h"); // alt: resizes, does not reflow
    term.feed(b"\x1b[2;2H");
    term.feed("한".as_bytes()); // pair at (1,1)-(1,2)
    term.resize(2, 3); // truncation strands the lead in the last column
    assert!(
        term.grid().cell(1, 1).is_wide(),
        "precondition: the truncated lead really does sit in the last column"
    );

    term.feed(b"\x1b[?2027h");
    term.feed(b"\x1b[1;2H");
    term.feed("\u{25B6}".as_bytes()); // ▶ — narrow, at the last column
    term.feed("\u{FE0F}".as_bytes()); // VS16 → promote → no room → relocate onto row 1

    let grid = term.grid();
    assert_eq!(grid.cell(1, 0).c(), '\u{25B6}', "the cluster relocated");
    assert!(grid.cell(1, 0).is_wide());
    assert!(grid.cell(1, 1).is_spacer(), "and its own spacer is intact");
}

/// The `(row, col)` of the first `WIDE_CHAR` lead in the grid.
fn wide_lead(term: &Engine) -> Option<(usize, usize)> {
    let grid = term.grid();
    (0..grid.rows()).find_map(|r| {
        let row = grid.row(r);
        (0..grid.cols()).find(|&c| row[c].is_wide()).map(|c| (r, c))
    })
}

fn cells(term: &Engine) -> Vec<(char, bool, bool)> {
    let grid = term.grid();
    (0..grid.rows())
        .flat_map(|r| {
            let row = grid.row(r);
            (0..grid.cols())
                .map(|c| (row[c].c(), row[c].is_wide(), row[c].is_spacer()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn row_text(term: &Engine, row: usize) -> String {
    let grid = term.grid();
    let row = grid.row(row);
    (0..grid.cols())
        .filter(|&c| !row[c].is_spacer())
        .map(|c| row[c].c())
        .collect()
}
