//! Wide-glyph pairing for span lookups — pure, host-testable.
//!
//! A width-2 glyph occupies two cells, and every span this crate paints (`overlay`'s three
//! highlight groups and `decoration`'s rects) arrives as inclusive column ranges that know nothing
//! about that. A range whose edge lands between the two halves therefore paints one of them, and
//! the glyph is split down the middle (#454).
//!
//! **The rule: a span covering either half of a pair covers both.** It is applied where the cell is
//! painted rather than on each span's producer, because the producers do not share a layer: two are
//! `justerm-core`'s (`Term::selection_range` with an anchor on a spacer, `Term::match_spans` via the
//! #678 clamp — both now widened there as well, so the *copy* agrees with the paint), and one is a
//! decoration rect the consumer authors in its own coordinates. A rule at the paint site is the only
//! one all three pass through.
//!
//! **Every span this crate paints obeys it, the caret included** — [`cursor_cells_at`] resolves the
//! caret through [`partner_at`] for the same reason, and a caret resting on a trailing spacer moves
//! back onto the lead rather than lighting half a glyph (#454). That symmetry is load-bearing: a
//! crate that normalised every span *except* the one drawn on top of them would be the same defect
//! wearing the opposite sign.
//!
//! **Union, not one direction.** The references converge on "never paint exactly one half" and each
//! implements a different part of it: xterm.js normalises both endpoints at the selection model,
//! ghostty has a spacer ask its lead's column (so a spacer-only span paints *nothing*), alacritty
//! pulls the lead in when its spacer is covered but leaves a lead-only span bisecting. The two
//! read-site shapes are opposite halves of the union and neither is the union. What picks it is not
//! the tally but `selection_text`, which has always returned the whole glyph or none of it — so the
//! highlight and the copy cannot disagree under union, and can under either half.
//!
//! [`cursor_cells_at`]: crate::cursor::cursor_cells_at

use crate::attrs::{is_wide_lead, is_wide_spacer};

/// The column of the other half of the wide pair the cell at `(row, col)` belongs to, or `None`
/// when it is not half of one.
///
/// **Both cells must agree** — a lead is only paired with a `WIDE_CHAR_SPACER` to its right and a
/// spacer only with a `WIDE_CHAR` to its left. That is what makes the degenerate states safe rather
/// than merely unlikely: `Row::resize` narrows straight through a pair and leaves a lead standing in
/// the last column with no spacer, which ADR-0025 D4's scope records as a **legal buffer state**. A
/// stranded lead pairs with nothing and highlights alone; so does an orphaned spacer.
///
/// The pairing never crosses a row: a lead in the last column has no `col + 1` on its own row, and
/// the row is the unit a span is expressed in.
///
/// The index is widened to `u64` before the lookup for the reason [`cursor_span_at`] documents —
/// `row * cols + col` overflows a 32-bit `usize` on wasm32 for grids the packer will still accept,
/// and the overflow is invisible to the host suite (#355).
///
/// [`cursor_span_at`]: crate::cursor::cursor_span_at
pub fn partner_at(flags: &[u16], cols: u32, row: u32, col: u32) -> Option<u32> {
    let at = |c: u32| -> u16 {
        let idx = row as u64 * cols as u64 + c as u64;
        usize::try_from(idx)
            .ok()
            .and_then(|i| flags.get(i).copied())
            .unwrap_or(0)
    };
    let here = at(col);
    if is_wide_lead(here) && col + 1 < cols && is_wide_spacer(at(col + 1)) {
        return Some(col + 1);
    }
    if is_wide_spacer(here) && col > 0 && is_wide_lead(at(col - 1)) {
        return Some(col - 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{WIDE_CHAR, WIDE_CHAR_SPACER};

    /// `[narrow, lead, spacer, narrow]` on one row.
    const ROW: [u16; 4] = [0, WIDE_CHAR, WIDE_CHAR_SPACER, 0];

    #[test]
    fn each_half_of_a_pair_points_at_the_other() {
        assert_eq!(partner_at(&ROW, 4, 0, 1), Some(2), "lead → its spacer");
        assert_eq!(partner_at(&ROW, 4, 0, 2), Some(1), "spacer → its lead");
    }

    #[test]
    fn a_narrow_cell_pairs_with_nothing() {
        assert_eq!(partner_at(&ROW, 4, 0, 0), None);
        assert_eq!(partner_at(&ROW, 4, 0, 3), None, "the cell after a spacer");
    }

    #[test]
    fn a_lead_left_without_its_spacer_pairs_with_nothing() {
        // `Row::resize` narrowing through a pair — ADR-0025 D4's scope stops at reallocation, so
        // this is a legal state and not a repair site.
        assert_eq!(
            partner_at(&[0, WIDE_CHAR], 2, 0, 1),
            None,
            "at the row's end"
        );
        assert_eq!(
            partner_at(&[WIDE_CHAR, 0], 2, 0, 0),
            None,
            "with a narrow cell where the spacer should be"
        );
        assert_eq!(
            partner_at(&[WIDE_CHAR_SPACER, 0], 2, 0, 0),
            None,
            "an orphaned spacer at column 0 does not read past the row's start"
        );
    }

    #[test]
    fn the_pairing_never_crosses_a_row() {
        // Two rows of two: a lead in the last column of row 0, a spacer first in row 1. Reading
        // linearly they are adjacent; on their own rows neither has a partner.
        let grid = [0, WIDE_CHAR, WIDE_CHAR_SPACER, 0];
        assert_eq!(partner_at(&grid, 2, 0, 1), None, "row 0's trailing lead");
        assert_eq!(partner_at(&grid, 2, 1, 0), None, "row 1's leading spacer");
    }

    #[test]
    fn a_column_past_the_flags_reads_no_partner() {
        assert_eq!(partner_at(&[], 4, 0, 1), None);
        assert_eq!(partner_at(&ROW, 4, 9, 1), None, "a row past the grid");
    }
}
