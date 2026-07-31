//! #671 — a selection column past the last column must not change what is selected.
//!
//! `Term::viewport_to_abs` bounds the **row** (#660) and passes the **column** through
//! untouched, so an out-of-grid column reaches `resolve()`, where five `Side`-dependent
//! `+ 1`s and two half-clipped readers decide what happens to it. The result is not one
//! failure but a matrix, and the axis that decides it is **`Side`, not which endpoint**:
//!
//! | gesture (4-column grid `abcd`) | before | alacritty-equivalent |
//! |---|---|---|
//! | `begin(0, 80, Left)`  | `"efgh\nijkl"` — **the anchor row vanishes** | `"d\nefgh\nijkl"` |
//! | `extend(2, 80, Left)` | selects `l` — **one cell more than asked** | excludes it |
//! | `begin`/`extend` with `Side::Right` | unchanged | unchanged |
//!
//! `Side::Right` never diverges because its `+ 1` lands past the end and the readers'
//! `to.min(len)` / `right_excl > left` clip it to the same place. `Side::Left` has no
//! `+ 1` to clip, so the raw column survives into `left`, which **no reader bounds** —
//! `selection_range` clips only `right_excl` (`term/selection.rs`, the Linear arm) and
//! `selection_text`'s Block arm clips only `hi`.
//!
//! **The references make this unobservable, by three different mechanisms** — recorded
//! in `docs/agents/reference-facts.md`: alacritty clamps *both* endpoints in
//! `Selection::to_range` before any side arithmetic (and its `+ 1` then carries an
//! explicit `== columns → wrap to the next line` rule); xterm.js does not clamp, but its
//! only producer is the already-clamped `_getMouseBufferCoords` and its reader is total
//! (`while (startCol < endCol)` → `''`); ghostty's pins cannot hold an out-of-range
//! column at all. justerm is the only one where the value is representable *and*
//! observable.
//!
//! The fix bounds the column beside the row in `viewport_to_abs` — the same function,
//! the same input, the same reason. What it deliberately does **not** change is pinned
//! below: an *in-range* `Side::Right` on the last column still resolves to `from == cols`
//! and still drops that row, which is what alacritty's wrap-to-next-line produces too.

use justerm_core::{Engine, SelectionSpan, SelectionType, Side};

/// A 4×3 grid: `abcd` / `efgh` / `ijkl`. Four columns, so column 4 is the first
/// out-of-range one and no test has to reach for a large number to be off the end.
fn grid() -> Engine {
    let mut e = Engine::new(4, 3);
    e.feed(b"abcd\r\nefgh\r\nijkl");
    e
}

// ===========================================================================
// The divergence: Side::Left, where there is no `+ 1` for a reader to clip
// ===========================================================================

/// The anchor column is past the end and the side is `Left`, so `from` is the raw
/// column. `selection_range`'s Linear arm uses it as `left` **unbounded**, so
/// `right_excl > left` fails on the anchor's own row and the row is dropped — the
/// content is lost from the copy with no error anywhere.
#[test]
fn an_out_of_range_anchor_column_does_not_delete_the_anchor_row() {
    let mut term = grid();

    term.selection_begin(0, 80, Side::Left, SelectionType::Char);
    term.selection_extend(2, 3, Side::Right);

    // Clamped to the last column, `Left` includes that cell — so row 0 contributes `d`.
    assert_eq!(term.selection_text().as_deref(), Some("d\nefgh\nijkl"));
    assert_eq!(
        term.selection_range().first(),
        Some(&SelectionSpan {
            row: 0,
            left: 3,
            right: 3
        }),
        "the anchor row must still be in the projection, starting at the last column"
    );
    assert_eq!(term.selection_range().len(), 3, "no row may be dropped");
}

/// The mirror on the focus side. `Side::Left` means *exclude the cell under the
/// pointer*, so a focus past the end must resolve to excluding the last cell — not to
/// including it, which is what an unbounded `to` does once it exceeds the line length
/// and `to.min(len)` silently widens the range back to the whole row.
#[test]
fn an_out_of_range_focus_column_does_not_select_one_cell_too_many() {
    let mut term = grid();

    term.selection_begin(0, 0, Side::Left, SelectionType::Char);
    term.selection_extend(2, 80, Side::Left);

    assert_eq!(term.selection_text().as_deref(), Some("abcd\nefgh\nijk"));
}

/// Block is the second `+ 1` pair, and its two readers disagree on the unbounded
/// input: `selection_range` returns nothing while `selection_text` returns as many
/// blank lines as the block covers. Both must describe the same selection.
#[test]
fn block_range_and_text_agree_on_an_out_of_range_column() {
    let mut term = grid();

    term.selection_begin(0, 80, Side::Left, SelectionType::Block);
    term.selection_extend(2, 81, Side::Right);

    // Both columns clamp onto the last one, so the block is that single column.
    assert_eq!(term.selection_text().as_deref(), Some("d\nh\nl"));
    assert_eq!(
        term.selection_range(),
        vec![
            SelectionSpan {
                row: 0,
                left: 3,
                right: 3
            },
            SelectionSpan {
                row: 1,
                left: 3,
                right: 3
            },
            SelectionSpan {
                row: 2,
                left: 3,
                right: 3
            },
        ]
    );
}

// ===========================================================================
// Totality: the one column that panics rather than diverging
// ===========================================================================

/// `Side::Right` adds one to the column before anything bounds it, so the single
/// value `usize::MAX` overflows instead of resolving. Debug builds panic; a library
/// panic crosses into the consumer's process, which is the premise #660 was fixed on.
/// `Word` and `Line` ignore the side and were never affected.
#[test]
fn the_largest_column_resolves_instead_of_overflowing() {
    for ty in [SelectionType::Char, SelectionType::Block] {
        let mut term = grid();
        term.selection_begin(0, 0, Side::Left, ty);
        term.selection_extend(2, usize::MAX, Side::Right);

        // The assertion is that these two calls return at all; the values are the
        // ordinary whole-grid selection the clamp resolves them to.
        assert_eq!(term.selection_text().as_deref(), Some("abcd\nefgh\nijkl"));
        assert_eq!(term.selection_range().len(), 3);
    }
}

/// The `from` side of the same arithmetic — `usize::MAX` with `Side::Right` as the
/// *anchor*, which `ordered()` may place at either end depending on the focus.
#[test]
fn the_largest_column_resolves_as_an_anchor_too() {
    let mut term = grid();
    term.selection_begin(2, usize::MAX, Side::Right, SelectionType::Char);
    term.selection_extend(0, 0, Side::Left);

    assert_eq!(term.selection_text().as_deref(), Some("abcd\nefgh\nijkl"));
}

// ===========================================================================
// Control: what must NOT change
// ===========================================================================

/// `Side::Right` on an out-of-range column already resolved the way the references
/// do, because its `+ 1` was clipped by the readers. Pinned so the clamp is not
/// credited with a change it does not make — and so a later refactor that moves the
/// bound cannot quietly alter this half.
#[test]
fn side_right_out_of_range_is_unchanged() {
    let mut anchor = grid();
    anchor.selection_begin(0, 80, Side::Right, SelectionType::Char);
    anchor.selection_extend(2, 3, Side::Right);
    assert_eq!(anchor.selection_text().as_deref(), Some("efgh\nijkl"));

    let mut focus = grid();
    focus.selection_begin(0, 0, Side::Left, SelectionType::Char);
    focus.selection_extend(2, 80, Side::Right);
    assert_eq!(focus.selection_text().as_deref(), Some("abcd\nefgh\nijkl"));
}

/// The in-range case that also produces `from == cols`: anchoring at the **right**
/// edge of the last cell starts the selection after that row, so dropping the row is
/// correct and stays. alacritty reaches the same outcome by moving the start to
/// `(line + 1, column 0)` explicitly (`selection.rs`, `range_simple`); justerm reaches
/// it by the reader's `right_excl > left` test. Same answer, and this pins that the
/// clamp does not disturb it.
#[test]
fn an_in_range_right_edge_anchor_still_starts_on_the_next_row() {
    let mut term = grid();

    term.selection_begin(0, 3, Side::Right, SelectionType::Char);
    term.selection_extend(2, 3, Side::Right);

    assert_eq!(term.selection_text().as_deref(), Some("efgh\nijkl"));
    assert_eq!(term.selection_range().len(), 2);
}

/// The clamp must bound against the **grid**, not against the line's content, or a
/// selection past the end of a *short* line would stop at that line's last character
/// instead of the grid's last column. `Line` selection already resolves `to` as
/// `grid.cols()`, so the grid is the coordinate space the type works in.
#[test]
fn the_bound_is_the_grid_width_not_the_line_length() {
    let mut term = Engine::new(4, 3);
    term.feed(b"ab\r\ncdef"); // row 0 holds two characters, the grid is four wide

    term.selection_begin(0, 80, Side::Left, SelectionType::Block);
    term.selection_extend(1, 80, Side::Right);

    // Column 3 on both rows: blank on row 0 (trimmed away), `f` on row 1.
    assert_eq!(term.selection_text().as_deref(), Some("\nf"));
}

// ===========================================================================
// Step 4 — the same gesture on bytes a real application produced
// ===========================================================================

/// Recorded on a real PTY (the #660 fixtures): vim and htop drawing an 80×24 alt
/// screen. A synthetic 4×3 grid proves the arithmetic; this proves the arithmetic is
/// about the thing the engine actually holds after a real application has drawn into
/// it — full-width rows, wide glyphs, SGR runs and all.
///
/// The gesture is the one a drag past the right edge produces: an anchor column past
/// the last column with `Side::Left`. Before the fix the anchor's row was absent from
/// both the projection and the copy, on real content.
fn a_real_screen_keeps_its_anchor_row(capture: &[u8], label: &str) {
    let mut t = Engine::new(80, 24);
    t.feed(capture);
    assert_eq!(t.grid().cols(), 80, "fixture: {label} is 80 columns wide");

    // The row the anchor sits on, selected in range, is the comparand.
    let mut inrange = Engine::new(80, 24);
    inrange.feed(capture);
    inrange.selection_begin(3, 79, Side::Left, SelectionType::Char);
    inrange.selection_extend(5, 79, Side::Right);
    let expected = inrange.selection_text();

    t.selection_begin(3, 200, Side::Left, SelectionType::Char); // past the right edge
    t.selection_extend(5, 79, Side::Right);

    assert_eq!(
        t.selection_text(),
        expected,
        "{label}: an out-of-range anchor column must resolve to the last column, not \
         delete its own row"
    );
    assert_eq!(
        t.selection_range().first().map(|s| s.row),
        Some(3),
        "{label}: the anchor row must still be in the projection"
    );
}

#[test]
fn a_real_vim_screen_keeps_its_anchor_row() {
    a_real_screen_keeps_its_anchor_row(include_bytes!("fixtures/alt_resize_vim.pre.raw"), "vim");
}

#[test]
fn a_real_htop_screen_keeps_its_anchor_row() {
    a_real_screen_keeps_its_anchor_row(include_bytes!("fixtures/alt_resize_htop.pre.raw"), "htop");
}
