//! Cursor geometry — pure, host-testable, device pixels.

use crate::attrs::is_wide_lead;

/// The default stroke fraction — alacritty's `cursor.thickness` (`config/cursor.rs:31`). The
/// consumer may override it per-renderer via `setCursorThickness` (#369); this is the value a
/// renderer starts with and the floor a proof compares the default stroke against.
pub const THICKNESS: f32 = 0.15;

/// Stroke thickness in device pixels: `(frac * cell_w).round().max(1)`, alacritty's rule
/// (`display/cursor.rs:25`). The cell arrives in device pixels, so this tracks both dpr and
/// font size. xterm instead uses `dpr * cursorWidth` with `cursorWidth` in CSS pixels
/// (`RectangleRenderer.ts:267`), which gives a 32px font the same hairline as a 12px one.
pub fn cursor_thickness(frac: f32, cell_w: u32) -> u32 {
    let t = (frac.max(0.0) * cell_w as f32).round();
    (t as u32).max(1)
}

/// How many cells the cursor covers, given the flags of the cell it sits on and the room left
/// to the right edge. A wide char's cursor spans its lead *and* its spacer — alacritty
/// (`display/content.rs:139`) and xterm (`cell.getWidth()`) agree.
pub fn cursor_span(flags: u16, col: u32, cols: u32) -> u32 {
    let room = cols.saturating_sub(col);
    if is_wide_lead(flags) { 2.min(room) } else { 1 }.max(1)
}

/// [`cursor_span`] for a cursor at `(col, row)` of a `cols`-wide flag grid.
///
/// The index is widened to `u64` first: `row * cols + col` overflows a 32-bit `usize` on wasm32
/// for values a cursor can legally hold, and that overflow is invisible to the host suite — #355
/// was found only when the browser panicked. `dpr::grid_px` widens for the same reason. An
/// out-of-range cursor reads no flags and spans one cell; it is made inert by the callers, which
/// only ever visit in-range cells.
pub fn cursor_span_at(flags: &[u16], cols: u32, col: u32, row: u32) -> u32 {
    let idx = row as u64 * cols as u64 + col as u64;
    let at = usize::try_from(idx)
        .ok()
        .and_then(|i| flags.get(i).copied())
        .unwrap_or(0);
    cursor_span(at, col, cols)
}

/// The cursor shapes the renderer draws. `Hidden` is not a shape — an absent cursor is
/// `Option::None`, so a blinked-off or `DECTCEM`-hidden cursor cannot be confused with a
/// visible one whose rects happen to be empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    /// The whole cell, drawn by overriding its colours — *not* a rect. Both references agree:
    /// xterm `RectangleRenderer.ts:251` returns no vertices, alacritty `display/cursor.rs:33`
    /// returns no rects, and each recolours the cell instead.
    Block,
    /// A stroke along the cell's bottom edge.
    Underline,
    /// A stroke along the cell's left edge (xterm calls it `bar`, alacritty `Beam`).
    Bar,
    /// The cell's outline — four strokes. xterm's `cursorInactiveStyle: 'outline'`,
    /// alacritty's `unfocused_hollow`.
    HollowBlock,
}

/// The wire encoding of a shape, shared by the `setCursor` boundary and the shader's
/// `u_cursor.w`. `Block` is `0` in the shader *because* it draws no stroke there.
pub fn shape_id(shape: CursorShape) -> u8 {
    match shape {
        CursorShape::Block => 0,
        CursorShape::Underline => 1,
        CursorShape::Bar => 2,
        CursorShape::HollowBlock => 3,
    }
}

/// The inverse of [`shape_id`]. `None` for an id no shape owns.
pub fn shape_from_id(id: u8) -> Option<CursorShape> {
    Some(match id {
        0 => CursorShape::Block,
        1 => CursorShape::Underline,
        2 => CursorShape::Bar,
        3 => CursorShape::HollowBlock,
        _ => return None,
    })
}

/// A resolved cursor: where it is, what shape, and the two colours a `Block` paints with.
/// The renderer is theme-agnostic, so `color`/`text_color` arrive as packed `0xRRGGBB`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cursor {
    pub col: u32,
    pub row: u32,
    pub shape: CursorShape,
    /// The cursor's own colour — a `Block`'s background, a stroke's fill.
    pub color: u32,
    /// The glyph colour under a `Block` (xterm's `cursorAccent`, alacritty's `text_color`).
    pub text_color: u32,
}

/// The default cursor-contrast threshold — alacritty's `MIN_CURSOR_CONTRAST` (`content.rs:22`). A
/// cursor whose colour has WCAG contrast below this against the cell it sits on is inverted to the
/// terminal's default fg/bg so it stays visible (#368). Injected via `set_cursor_contrast`; a
/// consumer sets `1.0` — the floor of the contrast range — to turn the guard off (xterm's default).
pub const DEFAULT_CURSOR_CONTRAST: f32 = 1.5;

/// The colours a cursor is actually painted with, after the visibility guard (#368). If `color`
/// contrasts with the cell's **resolved** background (`cell_bg`, normalised `0.0..=1.0`) below
/// `threshold`, the cursor is invisible where it landed, so it inverts to the terminal's default
/// fg/bg (`default_fg`/`default_bg`) — exactly alacritty's fallback to `primary.foreground/background`.
/// Otherwise the consumer's `(color, text_color)` are honoured verbatim.
///
/// The decision needs the cell's *resolved* RGB, which only the renderer has (ADR-0017), so the
/// mechanism is here and the number is injected. justerm always takes an explicit cursor colour (it
/// has no cell-reference cursor), so unlike alacritty — which only guards a cell-reference cursor and
/// tests `cell.fg` vs `cell.bg` — this checks the cursor colour itself against the cell background.
pub fn guarded_cursor_colors(
    color: u32,
    text_color: u32,
    cell_bg: [f32; 3],
    default_fg: u32,
    default_bg: u32,
    threshold: f32,
) -> (u32, u32) {
    if crate::color::contrast(crate::color::gl_rgb(color), cell_bg) < threshold {
        (default_fg, default_bg)
    } else {
        (color, text_color)
    }
}

/// Does the cell `(col, row)` lie under the cursor's `span`-wide box?
///
/// The fragment shader mirrors this test, so it is pinned here rather than only in GLSL. Beware
/// `col >= c.col && col < c.col + span`: the sum overflows `u32` at the far edge and only `&&`'s
/// short-circuit kept that unreachable. Subtraction cannot wrap.
pub fn covers(cursor: &Cursor, span: u32, col: u32, row: u32) -> bool {
    row == cursor.row && col.checked_sub(cursor.col).is_some_and(|d| d < span)
}

/// A rectangle in device pixels, relative to the top-left of the cursor's *first* cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The strokes that draw `shape` over a `span`-cell-wide box. `Block` draws none.
///
/// Mirrors alacritty `display/cursor.rs` (`beam`, `underline`, `hollow`) in device pixels.
/// Rects with no area are dropped rather than emitted degenerate: alacritty computes them in
/// `f32` and lets a too-short cell produce a negative height, which is invisible there and
/// would wrap to `u32::MAX` here.
pub fn cursor_rects(shape: CursorShape, cell: (u32, u32), span: u32, thickness: u32) -> Vec<Rect> {
    let (w, h) = (cell.0.saturating_mul(span.max(1)), cell.1);
    // A stroke can never be thicker than the box it outlines.
    let t = thickness.min(w).min(h);
    let mut out = Vec::with_capacity(4);
    let mut push = |x, y, rw, rh| {
        if rw > 0 && rh > 0 {
            out.push(Rect { x, y, w: rw, h: rh });
        }
    };
    match shape {
        // Drawn as a colour override on the cell, not as geometry.
        CursorShape::Block => {}
        // `beam(x, y, thickness, height)` — never widened by the span, and its width has nothing
        // to do with the cell's height, so it is clamped only by the cell it sits in.
        CursorShape::Bar => push(0, 0, thickness.min(cell.0), h),
        // `underline(y + height - thickness, width, thickness)`.
        CursorShape::Underline => push(0, h - t, w, t),
        // `hollow()` — horizontals full width, verticals only the gap between them.
        CursorShape::HollowBlock => {
            push(0, 0, w, t);
            push(0, h - t, w, t);
            let gap = h.saturating_sub(2 * t);
            push(0, t, t, gap);
            push(w - t, t, t, gap);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{BOLD, WIDE_CHAR, WIDE_CHAR_SPACER};
    use crate::color::gl_rgb;

    const CELL: (u32, u32) = (10, 20);

    const DEF_FG: u32 = 0xFF_FF_FF;
    const DEF_BG: u32 = 0x1E_1E_2E;

    #[test]
    fn a_cursor_that_matches_its_cell_inverts_to_stay_visible() {
        // A block whose colour equals the cell it sits on has contrast 1.0 (< 1.5) and would be
        // invisible — the guard inverts it to the default fg/bg, alacritty's fallback.
        let cell_bg = gl_rgb(0x1E_1E_2E);
        let (color, text) = guarded_cursor_colors(
            0x1E_1E_2E, // cursor colour == cell bg
            0xAB_CD_EF,
            cell_bg,
            DEF_FG,
            DEF_BG,
            DEFAULT_CURSOR_CONTRAST,
        );
        assert_eq!(
            (color, text),
            (DEF_FG, DEF_BG),
            "invisible cursor rescued to defaults"
        );
    }

    #[test]
    fn a_high_contrast_cursor_is_left_exactly_as_the_consumer_set_it() {
        // White on a near-black cell is far above 1.5, so the consumer's colours pass through
        // untouched — the guard must not override a perfectly visible cursor.
        let cell_bg = gl_rgb(0x1E_1E_2E);
        let (color, text) = guarded_cursor_colors(
            0xFF_FF_FF,
            0x00_00_00,
            cell_bg,
            DEF_FG,
            DEF_BG,
            DEFAULT_CURSOR_CONTRAST,
        );
        assert_eq!(
            (color, text),
            (0xFF_FF_FF, 0x00_00_00),
            "visible cursor untouched"
        );
    }

    #[test]
    fn a_threshold_of_one_disables_the_guard_entirely() {
        // Contrast is always >= 1.0, so `< 1.0` is never true: threshold 1.0 is the off switch
        // (xterm's `minimumContrastRatio` default). Even a cursor equal to its cell passes through.
        let cell_bg = gl_rgb(0x40_40_40);
        let (color, text) =
            guarded_cursor_colors(0x40_40_40, 0x80_80_80, cell_bg, DEF_FG, DEF_BG, 1.0);
        assert_eq!(
            (color, text),
            (0x40_40_40, 0x80_80_80),
            "guard off at threshold 1.0"
        );
    }

    #[test]
    fn the_guard_fires_exactly_at_the_threshold_boundary() {
        // A pair whose contrast straddles 1.5 must invert below and pass at/above — pins that the
        // comparison is `< threshold`, not `<=`, and uses the real contrast value.
        let cell_bg = gl_rgb(0x00_00_00);
        // grey 0x282828 on black: contrast ~1.43 (< 1.5) → inverts.
        let below = guarded_cursor_colors(0x28_28_28, 0x11, cell_bg, DEF_FG, DEF_BG, 1.5);
        assert_eq!(below.0, DEF_FG, "just below threshold inverts");
        // grey 0x303030 on black: contrast ~1.59 (>= 1.5) → passes.
        let above = guarded_cursor_colors(0x30_30_30, 0x11, cell_bg, DEF_FG, DEF_BG, 1.5);
        assert_eq!(above.0, 0x30_30_30, "just above threshold passes");
    }

    /// alacritty `display/cursor.rs:25` — `(thickness * width).round().max(1.)`, with
    /// `thickness` defaulting to `0.15` of the cell width (`config/cursor.rs:31`). The cell
    /// is already in device pixels here, so this scales with dpr *and* font size.
    #[test]
    fn the_stroke_is_a_rounded_fraction_of_the_cell_width() {
        assert_eq!(cursor_thickness(0.15, 16), 2); // 2.4 -> 2
        assert_eq!(cursor_thickness(0.15, 10), 2); // 1.5 -> 2 (round half away from zero)
        assert_eq!(cursor_thickness(0.15, 8), 1); // 1.2 -> 1
    }

    /// The `.max(1.)` floor: a cell too narrow for the fraction still gets a visible stroke.
    /// Without it a 3px cell yields `round(0.45) == 0` and the cursor vanishes.
    #[test]
    fn a_narrow_cell_still_gets_one_pixel_of_stroke() {
        assert_eq!(cursor_thickness(0.15, 3), 1); // 0.45 -> 0 -> floored to 1
        assert_eq!(cursor_thickness(0.15, 1), 1);
        assert_eq!(cursor_thickness(0.0, 100), 1); // a zero fraction is still visible
    }

    /// A consumer-injected fraction other than the default scales the stroke proportionally (#369).
    /// These are the numbers the browser proof (`demo/cursor.html`) holds `setCursorThickness`
    /// against, pinned here host-side so the two cannot silently disagree. The setter clamps the
    /// fraction to `[0, 1]` (alacritty's `Percentage`), so `1.0` is the widest a stroke can be — a
    /// full-cell-width stroke, which `cursor_rects`/the shader then cap at the box they outline.
    #[test]
    fn a_non_default_fraction_scales_the_stroke() {
        assert_eq!(cursor_thickness(0.5, 16), 8); // 8.0
        assert_eq!(cursor_thickness(0.5, 10), 5); // 5.0
        assert_eq!(cursor_thickness(1.0, 10), 10); // the clamp ceiling: a full-cell stroke
        assert_ne!(
            cursor_thickness(0.5, 16),
            cursor_thickness(THICKNESS, 16),
            "a non-default fraction must differ from 0.15, or the proof proves nothing"
        );
    }

    /// A wide char occupies a lead cell plus a spacer; the cursor covers both, so a CJK glyph
    /// under a block cursor is not half-lit. alacritty `display/content.rs:139`
    /// (`Flags::WIDE_CHAR -> NonZeroU32::new(2)`), xterm `WebglRenderer.ts:541` (`cell.getWidth()`).
    #[test]
    fn the_cursor_spans_both_halves_of_a_wide_char() {
        assert_eq!(cursor_span(WIDE_CHAR, 0, 10), 2);
        assert_eq!(cursor_span(0, 0, 10), 1);
        assert_eq!(
            cursor_span(BOLD, 0, 10),
            1,
            "an unrelated flag changes nothing"
        );
    }

    /// A spacer is the *right* half; a cursor resting on one must not stretch a second cell to
    /// the right. Neither reference handles this — xterm would read `getWidth() == 0` and drop
    /// the override entirely. One cell is the honest answer.
    #[test]
    fn a_cursor_on_the_spacer_half_covers_only_that_cell() {
        assert_eq!(cursor_span(WIDE_CHAR_SPACER, 1, 10), 1);
    }

    /// A wide lead in the last column has no spacer to cover. The span is clamped to the grid
    /// rather than running a rect off the right edge.
    #[test]
    fn the_span_is_clamped_to_the_right_edge() {
        assert_eq!(cursor_span(WIDE_CHAR, 9, 10), 1);
        assert_eq!(cursor_span(WIDE_CHAR, 8, 10), 2);
    }

    fn at(col: u32, row: u32) -> Cursor {
        Cursor {
            col,
            row,
            shape: CursorShape::Block,
            color: 0xFF_00_00,
            text_color: 0,
        }
    }

    /// The box test the fragment shader mirrors. A one-cell cursor claims one cell; a wide char's
    /// claims two, so a CJK glyph under a block is not half-lit.
    #[test]
    fn the_cursor_covers_exactly_its_span_of_cells_on_its_own_row() {
        let c = at(2, 1);
        assert!(covers(&c, 1, 2, 1));
        assert!(
            !covers(&c, 1, 3, 1),
            "one cell does not reach its neighbour"
        );
        assert!(covers(&c, 2, 3, 1), "a wide char's cursor does");
        assert!(!covers(&c, 2, 4, 1));
        assert!(!covers(&c, 1, 1, 1), "nor the cell to its left");
        assert!(!covers(&c, 2, 2, 0), "nor any other row");
    }

    /// A cursor parked past the right edge must not wrap into cell 0 via `col - c.col`. It also
    /// must not overflow `c.col + span`, which is why the test is a subtraction. `justerm-core`
    /// never emits such a cursor — it parks pending-wrap on the last column (`term.rs:2573`) —
    /// but the renderer takes `u32` from any consumer.
    #[test]
    fn a_cursor_beyond_the_far_edge_covers_nothing_and_never_wraps() {
        let c = at(u32::MAX, 0);
        for col in [0, 1, 2, u32::MAX - 1] {
            assert!(!covers(&c, 2, col, 0), "col {col}");
        }
        assert!(
            covers(&c, 1, u32::MAX, 0),
            "it does still cover its own cell"
        );
    }

    /// One encoding, two boundaries — the `setCursor` argument and the shader's `u_cursor.w`.
    /// If they ever disagree a bar would render as an underline, silently.
    #[test]
    fn every_shape_survives_the_wire_encoding_and_no_other_id_decodes() {
        for shape in [
            CursorShape::Block,
            CursorShape::Underline,
            CursorShape::Bar,
            CursorShape::HollowBlock,
        ] {
            assert_eq!(shape_from_id(shape_id(shape)), Some(shape));
        }
        for id in 4..=u8::MAX {
            assert_eq!(shape_from_id(id), None, "id {id} decodes to a shape");
        }
    }

    /// The whole point of the block shape: it is drawn by recolouring the cell, so it
    /// contributes no geometry. xterm `RectangleRenderer.ts:251`, alacritty `cursor.rs:33`.
    #[test]
    fn a_block_cursor_draws_no_strokes() {
        assert!(cursor_rects(CursorShape::Block, CELL, 1, 2).is_empty());
        assert!(cursor_rects(CursorShape::Block, CELL, 2, 2).is_empty());
    }

    /// A bar hugs the *left* edge and runs the cell's full height. It does not widen with the
    /// span — a caret sits before one character, not across a wide glyph (alacritty `beam()`
    /// takes `height` but never `width`).
    #[test]
    fn a_bar_is_a_left_edge_stroke_of_full_height() {
        let r = cursor_rects(CursorShape::Bar, CELL, 1, 2);
        assert_eq!(
            r,
            vec![Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 20
            }]
        );
        let wide = cursor_rects(CursorShape::Bar, CELL, 2, 2);
        assert_eq!(wide, r, "a wide char's bar stays one stroke on the left");
    }

    /// An underline hugs the *bottom* edge and does widen with the span — alacritty
    /// `underline()` is given `width` after `width *= self.width()`.
    #[test]
    fn an_underline_is_a_bottom_edge_stroke_that_spans_the_glyph() {
        assert_eq!(
            cursor_rects(CursorShape::Underline, CELL, 1, 2),
            vec![Rect {
                x: 0,
                y: 18,
                w: 10,
                h: 2
            }]
        );
        assert_eq!(
            cursor_rects(CursorShape::Underline, CELL, 2, 2),
            vec![Rect {
                x: 0,
                y: 18,
                w: 20,
                h: 2
            }]
        );
    }

    /// A hollow block is four strokes: the horizontals run the full width, the verticals fill
    /// only the gap between them so the corners are not drawn twice (alacritty `hollow()`).
    #[test]
    fn a_hollow_block_outlines_the_cell_without_overdrawing_its_corners() {
        let r = cursor_rects(CursorShape::HollowBlock, CELL, 1, 2);
        assert_eq!(r.len(), 4);
        assert!(
            r.contains(&Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 2
            }),
            "top"
        );
        assert!(
            r.contains(&Rect {
                x: 0,
                y: 18,
                w: 10,
                h: 2
            }),
            "bottom"
        );
        assert!(
            r.contains(&Rect {
                x: 0,
                y: 2,
                w: 2,
                h: 16
            }),
            "left"
        );
        assert!(
            r.contains(&Rect {
                x: 8,
                y: 2,
                w: 2,
                h: 16
            }),
            "right"
        );
    }

    /// The right stroke of a wide char's outline sits at the far edge of the *pair*.
    #[test]
    fn a_wide_hollow_block_outlines_both_cells() {
        let r = cursor_rects(CursorShape::HollowBlock, CELL, 2, 2);
        assert!(
            r.contains(&Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 2
            }),
            "top spans the pair"
        );
        assert!(
            r.contains(&Rect {
                x: 18,
                y: 2,
                w: 2,
                h: 16
            }),
            "right at the pair's edge"
        );
    }

    /// A cell shorter than two strokes has no room for the verticals. alacritty lets the height
    /// go negative in `f32`; in `u32` that wraps, so the rect must be dropped instead. Nothing
    /// with zero area is ever emitted, for any shape or size.
    #[test]
    fn a_cell_too_small_for_the_outline_drops_the_degenerate_strokes() {
        let r = cursor_rects(CursorShape::HollowBlock, (10, 3), 1, 2);
        assert_eq!(r.len(), 2, "only the horizontals survive a 3px-tall cell");
        for shape in [
            CursorShape::Bar,
            CursorShape::Underline,
            CursorShape::HollowBlock,
        ] {
            for cell in [(0, 0), (1, 1), (10, 3), (3, 10), (2, 2)] {
                for span in [1, 2] {
                    for t in [1, 2, 5] {
                        for rect in cursor_rects(shape, cell, span, t) {
                            assert!(
                                rect.w > 0 && rect.h > 0,
                                "{shape:?} {cell:?} span={span} t={t} emitted {rect:?}"
                            );
                            assert!(
                                rect.x + rect.w <= cell.0 * span && rect.y + rect.h <= cell.1,
                                "{shape:?} {cell:?} span={span} t={t} escaped the box: {rect:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
