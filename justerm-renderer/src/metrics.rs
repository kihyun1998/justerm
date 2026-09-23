//! Cell metrics: how the grid cell relates to the glyph box (#338).
//!
//! One link in the chain ADR-0022 records: the cell comes from the face's advance (width, #962) and
//! an ink scan of its `█` (height) ([`rasterizer`](crate::rasterizer)), this module nests the glyph box inside it, and that nesting is
//! why tiling glyphs must be drawn to the *cell* ([`builtin`](crate::builtin)) instead of to their ink
//! box. `letterSpacing` and `lineHeight` grow the cell and not the glyph, so this says where inside
//! the cell the glyph sits. Both references keep the same split (ADR-0022).
//!
//! Deliberately, `letter_spacing` is taken in **CSS** pixels where both references take device
//! pixels — ADR-0023. `line_height` is a multiplier.

/// The largest a single cell may become, device px (#338) — far above any real cell, below the
/// smallest measured `MAX_TEXTURE_SIZE`. Why a finite setter value needs it:
/// `docs/map/territory/cell-geometry.md`.
pub const MAX_CELL_PX: u32 = 4096;

/// Shrink a cell until the atlas that must hold it fits the implementation's texture limit (#359).
///
/// The atlas is a 2D array texture: one padded cell wide, `glyphs_per_layer` cells tall. Over the
/// limit, `glTexStorage3D` fails silently and the terminal fills solid — so ask, then adopt, as
/// `resize_surface` does (#339; `docs/map/territory/cell-geometry.md`). The caller reports the cell it
/// actually got through `cell_height()`.
/// `bleed` is the band a slot reserves on each side of the cell for ink that leaves it, `(x, y)`
/// (ADR-0019 R1.2; vertical #791, horizontal #966). The x band is spent out of the texture's width
/// and the y band out of the per-layer height, each beside the guard band — two arguments rather
/// than one folded into `padding`, because the two bands differ in depth and each narrows only its
/// own axis.
pub fn fit_cell_to_atlas(
    cell: (u32, u32),
    padding: u32,
    bleed: (u32, u32),
    glyphs_per_layer: u32,
    max_texture_size: u32,
) -> (u32, u32) {
    let (bleed_x, bleed_y) = bleed;
    let pad2 = 2 * padding;
    let max_w = max_texture_size.saturating_sub(pad2 + 2 * bleed_x).max(1);
    // Every layer stacks `glyphs_per_layer` padded cells vertically, and each of those now carries
    // the bleed band on both sides of the cell.
    let max_h = (max_texture_size / glyphs_per_layer.max(1))
        .saturating_sub(pad2 + 2 * bleed_y)
        .max(1);
    (cell.0.min(max_w), cell.1.min(max_h))
}

/// The glyph box width in device px for a face whose reference glyph advances `advance_px` device
/// px (ADR-0022, #962): the advance floored, as xterm.js floors it (`WebglRenderer.ts:654`,
/// `Math.floor(charSizeService.width * dpr)`) and alacritty floors its own (`compute_cell_size`,
/// `display/mod.rs:1608-1615`). At least 1 and at most [`MAX_CELL_PX`]; a non-finite advance is 1.
pub fn advance_width(advance_px: f32) -> u32 {
    if !advance_px.is_finite() {
        return 1;
    }
    (advance_px.floor().clamp(1.0, MAX_CELL_PX as f32)) as u32
}

/// The device-pixel grid cell for a glyph box of `char_px`, given the consumer's policy.
///
/// `letter_spacing_css` may be negative (xterm and alacritty both allow it, and some fonts want
/// it); the cell then narrows past the glyph, which then overlaps its neighbour up to the horizontal
/// band and is cropped past it (#966) — never stretched. The
/// cell never reaches zero — alacritty floors its own at 1 (`compute_cell_size`, `.max(1.)`), and a
/// zero-width cell would make the whole grid degenerate.
///
/// `line_height` below 1 would put the cell *inside* the glyph, so it is clamped to 1 — deliberately
/// not rejected as xterm rejects it.
pub fn device_cell(
    char_px: (u32, u32),
    letter_spacing_css: f32,
    line_height: f32,
    dpr: f32,
) -> (u32, u32) {
    let dx = (letter_spacing_css * dpr).round() as i64;
    let w = (char_px.0 as i64 + dx).clamp(1, MAX_CELL_PX as i64) as u32;
    let h = (char_px.1 as f32 * line_height.max(1.0))
        .floor()
        .clamp(1.0, MAX_CELL_PX as f32) as u32;
    (w, h.max(char_px.1).min(MAX_CELL_PX.max(char_px.1)))
}

/// Where the glyph box sits inside the cell, in device px from the cell's top-left.
///
/// Centred, as xterm centres: `floor` horizontally and `round` vertically, so an odd remainder lands
/// on the right and on the top — mirrored on purpose (`docs/map/territory/cell-geometry.md`).
///
/// A cell narrower than the glyph (negative spacing) offsets by zero: the glyph starts at the cell's
/// edge and reaches into its neighbour on the far side, as far as the horizontal band (#966).
/// Underline / strikethrough thickness in device pixels, from the font SIZE — `max(1, round(font_px
/// / 15))`, where `font_px = font_size * dpr` is the em in device px — xterm.js's rule (#517), with
/// `round` rather than `floor` so a 15px em is 1. Why this rule: `docs/map/territory/cell-geometry.md`.
pub fn line_thickness(font_px: f32) -> u32 {
    ((font_px / 15.0).round() as u32).max(1)
}

/// How many dots a **dotted** underline puts in one cell (#830) — `max(1, round(cell_w / (2 *
/// thickness)))`, a **whole** number so the pattern survives a cell boundary.
///
/// xterm.js's 1:1 duty at twice the line thickness, **quantised** to a whole count per cell so the
/// cell-local shader needs no cross-cell phase — ghostty's route rather than xterm's or alacritty's
/// (`docs/map/territory/cell-compositing.md`).
///
/// The `max(1)` floor is `line_thickness`'s, for the same reason one rung up: zero dots is an
/// underline that vanished, and losing the mark entirely is a worse failure than drawing a coarse
/// one. It also makes the function total over a degenerate cell or thickness, which the renderer
/// can be handed before the first real measurement lands.
pub fn dots_per_cell(cell_w_px: u32, thickness_px: u32) -> u32 {
    let period = 2.0 * thickness_px.max(1) as f32;
    ((cell_w_px as f32 / period).round() as u32).max(1)
}

pub fn glyph_offset(cell: (u32, u32), char_px: (u32, u32)) -> (u32, u32) {
    let dx = cell.0.saturating_sub(char_px.0);
    let dy = cell.1.saturating_sub(char_px.1);
    (dx / 2, dy.div_ceil(2))
}

/// The atlas slot a bake fills, and where inside it the glyph's box starts (#791).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotGeometry {
    /// The padded slot: the cell, plus the bleed band on each side, plus the guard band all round.
    pub padded: (u32, u32),
    /// Top-left of the glyph's box inside that slot, device px.
    pub draw_origin: (u32, u32),
}

/// Lay out one atlas slot: how big it is, and where the glyph goes inside it.
///
/// Three offsets stack on each axis and they belong to three different things, which is why this is
/// one function rather than three additions at the call site: the **guard band** keeps neighbouring
/// slots from bleeding into each other under sampling (#288), the **bleed band** `(x, y)` is the room
/// `I_neighbour` needs for ink that leaves the cell (ADR-0019 R1.2; y #791, x #966), and
/// `char_offset` is where the spacing policy put the glyph *within* the cell (#338).
///
/// `bleed = (0, 0)` reproduces the pre-#791 bake exactly. **No configuration is actually without a
/// band** — [`vertical_bleed`] and [`horizontal_bleed`] both floor at [`BLEED_HEADROOM_PX`] — so this
/// is the property that keeps the arithmetic checkable against the old behaviour in a test, not a
/// mode the renderer runs in.
pub fn slot_geometry(
    cell: (u32, u32),
    char_offset: (u32, u32),
    bleed: (u32, u32),
    padding: u32,
) -> SlotGeometry {
    let (bleed_x, bleed_y) = bleed;
    SlotGeometry {
        padded: (
            cell.0 + 2 * bleed_x + 2 * padding,
            cell.1 + 2 * bleed_y + 2 * padding,
        ),
        draw_origin: (
            padding + bleed_x + char_offset.0,
            padding + bleed_y + char_offset.1,
        ),
    }
}

/// Where a cell's own texcoords land inside its padded slot: `(origin, span)`, both `0..1` (#791).
///
/// This replaces the single guard-band fraction the shader used to inset by. That form assumed the
/// slot's content *was* the cell, so it could subtract the same amount from both edges; once the
/// slot carries a bleed band the two edges are no longer symmetric on the vertical axis, and a cell
/// that keeps insetting symmetrically stretches the glyph over the band instead of leaving it to the
/// neighbour it belongs to.
///
/// With `bleed_y = 0` this reduces exactly to the old inset, which is what keeps every pixel proof
/// written against it meaningful.
pub fn cell_uv(slot: SlotGeometry, cell: (u32, u32)) -> ((f32, f32), (f32, f32)) {
    let (pw, ph) = (slot.padded.0 as f32, slot.padded.1 as f32);
    // The cell's top-left inside the slot is the guard band across, and the guard band plus the
    // bleed down — which is exactly where the glyph box would start with no spacing offset.
    let x0 = (slot.padded.0 - cell.0) as f32 / 2.0;
    let y0 = (slot.padded.1 - cell.1) as f32 / 2.0;
    ((x0 / pw, y0 / ph), (cell.0 as f32 / pw, cell.1 as f32 / ph))
}

/// Band reserved beyond whatever the face's own metrics ask for, device px (#791).
///
/// Derived from nothing — it is empirical, and recorded as such. Glyphs overshoot the *declared*
/// line box, so a band sized to the line box alone is not enough: measured over ~1,570 codepoints at
/// em 24 device px, the line-box budget still clips **168** glyphs on Cascadia Mono and **263** on
/// Lucida Console. xterm.js reserves the same 4 device px in its bake canvas
/// (`TextureAtlas.ts:485`, `TMP_CANVAS_GLYPH_PADDING * 4` around a cell-height content box, SHA
/// `699f553`) and reaches ~0 clipped on the same corpus. Two independent arrivals at the same
/// number is why it is 4 rather than a rounder guess — not a derivation, and not to be read as one.
pub const BLEED_HEADROOM_PX: u32 = 4;

/// How deep a band each slot reserves above and below the cell, for this font configuration (#791).
///
/// The cell's height is the ink box of `█` (ADR-0022) and the face's own glyphs are not bounded by it, so the
/// band has to cover the gap between the two — **per face**, because that gap is a property of how
/// the designer drew one glyph and varies by several device px between faces at the same size. One
/// number serves both edges, so the larger gap wins; [`BLEED_HEADROOM_PX`] is added on top for what
/// overshoots even the declared line box.
///
/// All four arguments are device px against the baseline: the cell's own ascent and descent, and the
/// face's, which a browser reports as `fontBoundingBox{Ascent,Descent}`.
pub fn vertical_bleed(
    cell_ascent: u32,
    cell_descent: u32,
    font_ascent: u32,
    font_descent: u32,
) -> u32 {
    let above = font_ascent.saturating_sub(cell_ascent);
    let below = font_descent.saturating_sub(cell_descent);
    above.max(below) + BLEED_HEADROOM_PX
}

/// How deep a band each slot reserves left and right of the cell, for this font configuration (#966).
///
/// `block_ink_w` is the ink width of this face's `█` and `box_w` the glyph box, both device px. What
/// the block overhangs the box is room a cell sized from the ink box used to hold inside itself, so
/// reserving it here keeps every glyph that fit that cell fitting this one; [`BLEED_HEADROOM_PX`] is
/// added on top, as on the vertical axis — xterm.js draws a glyph the same 4 device px in from its bake
/// canvas's left edge (`TextureAtlas.ts:542`, `TMP_CANVAS_GLYPH_PADDING * 2`, SHA `699f553`). One
/// number serves both edges. What exceeds it is condensed at bake ([`horizontal_fit`], #792).
pub fn horizontal_bleed(block_ink_w: u32, box_w: u32) -> u32 {
    block_ink_w.saturating_sub(box_w) + BLEED_HEADROOM_PX
}

/// How far a glyph's measured ink may exceed its box before the bake condenses it, device px (#792).
///
/// Empirical, and recorded as such — the same shape as [`BLEED_HEADROOM_PX`]. The bake decides from
/// `measureText`'s `actualBoundingBox{Left,Right}`, and that is **not** the alpha >= 128 window the
/// rasteriser and every pixel proof use: measured in this repo, the metric agrees on one edge and
/// runs one antialiased row long on the other. A threshold of zero would therefore condense glyphs a
/// pixel scan says already fit, on the metric's own bias rather than on anything the reader sees.
/// One pixel is that bias; ink lost within it is one antialiased column at the box edge.
pub const FIT_TOLERANCE_PX: f32 = 1.0;

/// How a bake makes a glyph fit its box on the horizontal axis (#792).
///
/// Applied as `translate(box_origin, ..); scale(scale_x, 1); fill_text(text, pen_offset, ..)`, so
/// `pen_offset` is in **pre-scale** device px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HorizontalFit {
    /// Horizontal scale for the draw. `1.0` leaves the glyph alone; below 1 condenses it.
    pub scale_x: f32,
    /// Device px to draw at, relative to the box origin, before `scale_x` applies.
    pub pen_offset: f32,
}

/// Fit a glyph's ink into `box_w`, condensing it only on the axis that is over budget (#792).
///
/// The vertical axis is deliberately absent. Since #791 a slot carries a bleed band above and below
/// the cell and the receiving cell reads it back (ADR-0019 R1.2), so vertical ink already has
/// somewhere to go; horizontal ink has only the band [`horizontal_bleed`] sizes (#966), a few pixels
/// deep, because the Canvas API exposes no face-level horizontal counterpart to
/// `fontBoundingBox{Ascent,Descent}` for a deeper one to be sized from. Scaling uniformly
/// would spend budget this renderer already owns and partially undo #791's recovery — so the glyph
/// is condensed, never shrunk, and its height is untouched by construction rather than by a check.
///
/// Both inputs are `measureText`'s, in device px against the pen: `ink_left` is how far the ink
/// reaches **left** of the pen (negative when the ink starts to its right) and `ink_right` how far
/// right. The window is `[-bleed, box_w + bleed)`: the box, plus the horizontal band on each side
/// that a neighbour reads back (#966). With `bleed = 0` it is the box alone, the pre-#966 window.
///
/// `box_w` is the **glyph box**, never the cell. [`device_cell`] documents that a negative
/// `letter_spacing` narrows the cell past the glyph, *which then overlaps its neighbour rather than
/// being stretched*: a predicate keyed on the cell would read that consumer policy as a font fact and
/// condense every glyph on the grid.
///
/// Two treatments, and which one applies is a property of the ink rather than a mode:
///
/// - ink **wider** than the window is condensed to exactly the window and aligned to its left edge;
/// - ink that **fits** and sits outside is moved in by the least that puts it inside — never
///   further, since a glyph's side bearing is the font's and a period is not meant to sit on the
///   cell's left edge.
pub fn horizontal_fit(ink_left: f32, ink_right: f32, box_w: u32, bleed: u32) -> HorizontalFit {
    let identity = HorizontalFit {
        scale_x: 1.0,
        pen_offset: 0.0,
    };
    let (box_w, bleed) = (box_w as f32, bleed as f32);
    let window = box_w + 2.0 * bleed;
    let ink_w = ink_left + ink_right;
    // A blank grapheme measures no ink, and a font that reports something worse than that must not
    // reach the division below.
    if !ink_w.is_finite() || ink_w <= 0.0 {
        return identity;
    }
    if ink_w > window + FIT_TOLERANCE_PX {
        // Condensed to exactly the window, then aligned to its left edge at `-bleed`. The offset is
        // pre-scale, so the edge is divided back out of the scale.
        let scale_x = window / ink_w;
        return HorizontalFit {
            scale_x,
            pen_offset: ink_left - bleed / scale_x,
        };
    }
    let over_left = (ink_left - bleed).max(0.0);
    let over_right = (ink_right - box_w - bleed).max(0.0);
    let pen_offset = if over_left > FIT_TOLERANCE_PX {
        over_left
    } else if over_right > FIT_TOLERANCE_PX {
        -over_right
    } else {
        0.0
    };
    HorizontalFit {
        scale_x: 1.0,
        pen_offset,
    }
}

/// Which adjacent cell an `I_neighbour` contribution came from (ADR-0019 R1.2).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjacent {
    Above,
    Below,
}

/// The slot content rows a receiver row reads: its own, and at most one neighbour's.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InkRows {
    /// Row of the receiver's OWN slot content — offset past the top bleed band.
    pub own: u32,
    /// The adjacent cell whose overflow reaches this row, and the row of ITS slot content.
    pub neighbour: Option<(Adjacent, u32)>,
}

/// Where the ink on receiver row `y` comes from, given the cell height and the bake's bleed (#791).
///
/// A slot's content is `cell_h + 2*bleed` rows — the cell's box with a band above and below for ink
/// that leaves it. A receiver's own box is always the middle band; its outer `bleed` rows *also*
/// carry whatever the adjacent cell spilled toward them, which is the [`Adjacent`] half.
#[cfg(test)]
pub fn ink_rows(y: u32, cell_h: u32, bleed: u32) -> InkRows {
    let own = bleed + y;
    let neighbour = if bleed == 0 {
        None
    } else if y < bleed {
        Some((Adjacent::Above, bleed + cell_h + y))
    } else if y >= cell_h.saturating_sub(bleed) {
        Some((Adjacent::Below, y - cell_h.saturating_sub(bleed)))
    } else {
        None
    };
    InkRows { own, neighbour }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the ink lands after a fit is applied, as `(left, right)` device px from the box
    /// origin — the reader's view of it, which is what every assertion below is written against.
    fn ink_after(fit: HorizontalFit, ink_left: f32, ink_right: f32) -> (f32, f32) {
        let left = (-ink_left + fit.pen_offset) * fit.scale_x;
        let right = (ink_right + fit.pen_offset) * fit.scale_x;
        (left, right)
    }

    #[test]
    fn the_glyph_box_width_is_the_advance_floored_not_the_ink() {
        // #962's measurement: Consolas at 18.9 device px advances 10.39 while its `█` inks 12 wide
        // in a WebView2 app. xterm.js takes 10 there, so this does.
        assert_eq!(advance_width(10.39), 10);
        // A whole advance stays whole; a fraction just short of the next pixel still floors.
        assert_eq!(advance_width(10.0), 10);
        assert_eq!(advance_width(10.99), 10);
        // Degenerate advances cannot produce a zero-width cell or an unbounded one.
        assert_eq!(advance_width(0.4), 1);
        assert_eq!(advance_width(f32::NAN), 1);
        assert_eq!(advance_width(-3.0), 1);
        assert_eq!(advance_width(1e9), MAX_CELL_PX);
    }

    #[test]
    fn a_glyph_inside_its_box_is_left_exactly_alone() {
        // Ink from the pen to 8 px in a 12 px box, and a glyph with a natural left side bearing:
        // neither is touched, and the second is the side condition — "fits" must not mean
        // "left-align", or every period moves to the cell's edge.
        assert_eq!(
            horizontal_fit(0.0, 8.0, 12, 0),
            HorizontalFit {
                scale_x: 1.0,
                pen_offset: 0.0
            }
        );
        assert_eq!(
            horizontal_fit(-3.0, 9.0, 12, 0),
            HorizontalFit {
                scale_x: 1.0,
                pen_offset: 0.0
            }
        );
    }

    #[test]
    fn ink_wider_than_the_box_is_condensed_to_exactly_the_box() {
        // `Ǆ` on the demo face, measured 2026-08-21: 32 device px of ink in a 12 px box.
        let fit = horizontal_fit(0.0, 32.0, 12, 0);
        assert_eq!(fit.scale_x, 12.0 / 32.0);
        let (l, r) = ink_after(fit, 0.0, 32.0);
        assert_eq!((l, r), (0.0, 12.0));
    }

    #[test]
    fn a_condensed_glyph_keeps_its_bearing_out_of_the_box() {
        // Same total ink, but 2 px of it left of the pen. The condensed ink must still start at the
        // box origin — an offset applied before the scale, not after.
        let fit = horizontal_fit(2.0, 30.0, 12, 0);
        assert_eq!(fit.scale_x, 12.0 / 32.0);
        let (l, r) = ink_after(fit, 2.0, 30.0);
        assert_eq!((l, r), (0.0, 12.0));
    }

    #[test]
    fn ink_that_fits_but_sits_outside_is_moved_in_rather_than_condensed() {
        // `ᾷ` on the demo face: ink exactly as wide as the cell, sitting 2 px to its right. Scaling
        // is a no-op on this class — 59 to 177 codepoints per face — so it must translate, and the
        // scale must stay 1 or the glyph shrinks for no reason.
        let fit = horizontal_fit(-2.0, 14.0, 12, 0);
        assert_eq!(fit.scale_x, 1.0);
        assert_eq!(ink_after(fit, -2.0, 14.0), (0.0, 12.0));
    }

    #[test]
    fn ink_reaching_left_of_the_pen_is_moved_in_by_exactly_what_it_overhangs() {
        let fit = horizontal_fit(3.0, 5.0, 12, 0);
        assert_eq!(fit.scale_x, 1.0);
        assert_eq!(ink_after(fit, 3.0, 5.0), (0.0, 8.0));
    }

    #[test]
    fn ink_within_the_tolerance_is_not_condensed_on_the_metric_s_own_bias() {
        // One px past the box is the measurement's known bias against the alpha scan, not ink the
        // reader loses. Condensing here would fire on glyphs a pixel scan says already fit.
        let fit = horizontal_fit(0.0, 13.0, 12, 0);
        assert_eq!(
            fit,
            HorizontalFit {
                scale_x: 1.0,
                pen_offset: 0.0
            }
        );
        // ... and one px beyond the tolerance is condensed, so the threshold is a threshold.
        assert!(horizontal_fit(0.0, 14.0, 12, 0).scale_x < 1.0);
    }

    #[test]
    fn ink_inside_the_band_is_left_exactly_alone() {
        // #966: Consolas at 18.9 device px draws `@` about 12 wide over a 10 px advance box. With a
        // 6 px band on each side that ink has a home in the neighbour, so the bake must not touch it.
        let identity = HorizontalFit {
            scale_x: 1.0,
            pen_offset: 0.0,
        };
        assert_eq!(horizontal_fit(1.0, 11.0, 10, 6), identity);
        // The same ink with no band is condensed, which is the pre-#966 behaviour.
        assert!(horizontal_fit(1.0, 11.0, 10, 0).scale_x < 1.0);
    }

    #[test]
    fn ink_wider_than_box_and_band_is_condensed_to_exactly_that_window() {
        // 30 px of ink in a 10 px box with 6 px either side: the window is 22 px, from -6 to 16.
        let fit = horizontal_fit(2.0, 28.0, 10, 6);
        assert_eq!(fit.scale_x, 22.0 / 30.0);
        let (l, r) = ink_after(fit, 2.0, 28.0);
        assert!(
            (l + 6.0).abs() < 1e-4 && (r - 16.0).abs() < 1e-4,
            "{l}..{r}"
        );
    }

    #[test]
    fn ink_that_fits_the_window_but_sits_past_the_band_is_moved_in_to_its_edge() {
        // 16 px of ink starting 3 px right of the pen reaches 19, 3 past the band's edge at 16.
        let fit = horizontal_fit(-3.0, 19.0, 10, 6);
        assert_eq!(fit.scale_x, 1.0);
        assert_eq!(ink_after(fit, -3.0, 19.0), (0.0, 16.0));
    }

    #[test]
    fn a_blank_glyph_produces_no_scale_at_all() {
        // A space measures no ink; the fit must not divide by it.
        let fit = horizontal_fit(0.0, 0.0, 12, 0);
        assert_eq!(
            fit,
            HorizontalFit {
                scale_x: 1.0,
                pen_offset: 0.0
            }
        );
        assert!(fit.scale_x.is_finite());
    }

    /// The measured ink box of `█` at `FONT_SIZE * 1` in Chromium (#328): 10 x 16 device px.
    const CHAR: (u32, u32) = (10, 16);

    #[test]
    fn the_cell_maps_to_its_own_band_of_the_slot_not_to_the_whole_slot() {
        // The shader's half of the same contract. A taller slot must NOT stretch into the cell —
        // the cell's texcoords have to land on the middle band and leave the bleed to whoever owns
        // the ink in it. Worked by hand: a 12x24 cell with bleed 4 and guard 1 is a 14x34 slot, so
        // the cell occupies y 5..=28 of 34 and x 1..=12 of 14.
        let (o, s) = cell_uv(slot_geometry((12, 24), (0, 0), (0, 4), 1), (12, 24));
        assert_eq!(o, (1.0 / 14.0, 5.0 / 34.0));
        assert_eq!(s, (12.0 / 14.0, 24.0 / 34.0));

        // Bleed 0 reproduces the guard-band inset the shader has always applied, which is the
        // assertion that keeps every existing pixel proof meaningful across this change.
        let (o0, s0) = cell_uv(slot_geometry((12, 24), (0, 0), (0, 0), 1), (12, 24));
        assert_eq!(o0, (1.0 / 14.0, 1.0 / 26.0));
        assert_eq!(s0, (12.0 / 14.0, 24.0 / 26.0));
    }

    #[test]
    fn the_bake_reserves_the_band_around_the_cell_and_pushes_the_glyph_past_it() {
        // Worked from the slot layout by hand. A padded slot is the cell, plus the bleed band on the
        // top and bottom edges, plus the guard band on all four; the glyph's box then starts past
        // the guard band, past the bleed, and past wherever the spacing policy put it in the cell.
        let cell = (12, 24);

        let g = slot_geometry(cell, (0, 0), (0, 4), 1);
        assert_eq!(
            g.padded,
            (14, 34),
            "12+2 wide, 24 + 2*4 bleed + 2*1 guard tall"
        );
        assert_eq!(
            g.draw_origin,
            (1, 5),
            "1 guard across; 1 guard + 4 bleed down"
        );

        // `letterSpacing` / `lineHeight` move the glyph inside the cell (#338); the band is outside
        // it, so the two offsets simply add.
        let spaced = slot_geometry(cell, (2, 3), (0, 4), 1);
        assert_eq!(
            spaced.padded,
            (14, 34),
            "the spacing moved the glyph, not the slot"
        );
        assert_eq!(spaced.draw_origin, (3, 8));

        // Bleed 0 must reproduce today's bake exactly — this is the assertion that lets the band be
        // switched on per configuration without every other configuration moving.
        let none = slot_geometry(cell, (2, 3), (0, 0), 1);
        assert_eq!(none.padded, (14, 26));
        assert_eq!(none.draw_origin, (3, 4));
    }

    #[test]
    fn the_bleed_is_derived_per_font_from_what_that_face_overshoots_its_own_cell() {
        // Measured 2026-08-20 in Chromium at em 24 device px (#791): the cell is the ink box of
        // `█`, the line box is `fontBoundingBox{Ascent,Descent}`, and the gap between them differs
        // per face. A constant band would over-reserve on the tight faces and under-reserve on the
        // loose one — which is the whole argument for deriving it.
        //
        //                        cell asc/desc   line box asc/desc   gap   + headroom
        // Consolas                  22 / 5           22 / 6           1        5
        // Cascadia Mono             22 / 5           22 / 6           1        5
        // Courier New               20 / 6           20 / 7           1        5
        // Lucida Console            19 / 4           19 / 5           1        5
        // DejaVu Sans Mono          19 / 5           22 / 6           3        7
        assert_eq!(vertical_bleed(22, 5, 22, 6), 5, "Consolas");
        assert_eq!(vertical_bleed(20, 6, 20, 7), 5, "Courier New");
        assert_eq!(vertical_bleed(19, 4, 19, 5), 5, "Lucida Console");
        assert_eq!(
            vertical_bleed(19, 5, 22, 6),
            7,
            "DejaVu Sans Mono — 3 px of ascent to recover"
        );

        // The band is one number used on BOTH sides, so the larger of the two gaps wins.
        assert_eq!(
            vertical_bleed(19, 5, 22, 12),
            11,
            "a deep descender drives it instead"
        );

        // A face whose ink box already reaches its line box still gets the headroom, because glyphs
        // overshoot the *declared* line box too: measured, the line-box budget alone still clips 168
        // glyphs on Cascadia Mono and 263 on Lucida Console, and only the headroom takes those to ~0.
        assert_eq!(
            vertical_bleed(22, 6, 22, 6),
            4,
            "no gap — the headroom is the whole band"
        );
    }

    #[test]
    fn a_vertical_bleed_eats_the_layer_height_budget_and_leaves_the_width_alone() {
        // Continued by hand from the arithmetic the #359 test below pins: one layer gets
        // 8192/32 = 256 rows, and a slot spends `2*padding + 2*bleed` of them before the cell.
        // At padding 1, bleed 4 that is 256 - 2 - 8 = 246.
        assert_eq!(fit_cell_to_atlas((10, 246), 1, (0, 4), 32, 8192), (10, 246));
        assert_eq!(fit_cell_to_atlas((10, 247), 1, (0, 4), 32, 8192), (10, 246));
        assert_eq!(
            fit_cell_to_atlas((10, 4096), 1, (0, 4), 32, 8192),
            (10, 246)
        );
        // With no horizontal band the width budget is untouched, so a cell that fits today still
        // fits. This is the half that would silently regress if the bleed were
        // folded into `padding` at the call site instead of being its own argument.
        assert_eq!(
            fit_cell_to_atlas((8190, 16), 1, (0, 4), 32, 8192),
            (8190, 16)
        );
        assert_eq!(
            fit_cell_to_atlas((9000, 16), 1, (0, 4), 32, 8192),
            (8190, 16)
        );
        // And bleed 0 is exactly today's ceiling: the limit moves only when the feature is on.
        assert_eq!(
            fit_cell_to_atlas((10, 4096), 1, (0, 0), 32, 8192),
            (10, 254)
        );
    }

    #[test]
    fn the_horizontal_bleed_holds_what_the_block_glyph_overhangs_its_box_plus_the_headroom() {
        // #966, measured 2026-09-22 in headless Chromium: Consolas at 18.9 device px inks `█` 12 wide
        // over a 10.39 advance, so an advance-floored box of 10 leaves 2 px of it outside — which a
        // cell sized from the ink box used to hold inside itself.
        assert_eq!(horizontal_bleed(12, 10), 6, "2 px overhang + 4 px headroom");
        // A box that IS the ink box (every configuration before #962) keeps only the headroom.
        assert_eq!(horizontal_bleed(14, 14), 4);
        // An ink box narrower than the box owes nothing, and does not underflow.
        assert_eq!(horizontal_bleed(9, 10), 4);
    }

    #[test]
    fn a_horizontal_bleed_eats_the_width_budget_as_the_vertical_one_eats_the_height() {
        // 8192 wide, 1 guard band and 6 bleed on each side: 8192 - 2 - 12 = 8178 for the cell.
        assert_eq!(
            fit_cell_to_atlas((9000, 16), 1, (6, 4), 32, 8192),
            (8178, 16)
        );
        assert_eq!(
            fit_cell_to_atlas((8178, 16), 1, (6, 4), 32, 8192),
            (8178, 16)
        );
        // The height budget is the vertical band's alone.
        assert_eq!(
            fit_cell_to_atlas((10, 4096), 1, (6, 4), 32, 8192),
            (10, 246)
        );
    }

    #[test]
    fn a_slot_carries_the_horizontal_band_on_both_sides_and_the_glyph_starts_past_it() {
        let g = slot_geometry((10, 24), (0, 0), (6, 4), 1);
        assert_eq!(g.padded, (10 + 2 * 6 + 2, 24 + 2 * 4 + 2));
        assert_eq!(g.draw_origin, (1 + 6, 1 + 4));
        // The spacing offset still stacks on top, on both axes.
        let spaced = slot_geometry((12, 24), (1, 3), (6, 4), 1);
        assert_eq!(spaced.draw_origin, (1 + 6 + 1, 1 + 4 + 3));
        // And the cell's own texcoords start past the guard band and the band, on both axes.
        let (o, s) = cell_uv(g, (10, 24));
        assert_eq!((o.0 * 24.0, s.0 * 24.0), (7.0, 10.0));
    }

    #[test]
    fn the_shaders_one_cell_step_across_is_the_same_mapping_as_the_column_arithmetic() {
        // The x twin of the row test below: `inner.x ± u_cell_uv.z` must name the column the
        // band arithmetic does. Left neighbour = `Above` (the lower index), right = `Below`.
        let (cell_w, bleed, pad) = (10u32, 6u32, 1u32);
        let slot = slot_geometry((cell_w, 24), (0, 0), (bleed, 4), pad);
        let (origin, span) = cell_uv(slot, (cell_w, 24));
        let pw = slot.padded.0 as f32;
        let mut seen = 0;
        for x in 0..cell_w {
            let cols = ink_rows(x, cell_w, bleed);
            let Some((which, src_col)) = cols.neighbour else {
                continue;
            };
            seen += 1;
            let u_own = origin.0 + (x as f32 + 0.5) / cell_w as f32 * span.0;
            let u_shader = match which {
                Adjacent::Above => u_own + span.0,
                Adjacent::Below => u_own - span.0,
            };
            let col_from_shader = (u_shader * pw - pad as f32).floor() as u32;
            assert_eq!(col_from_shader, src_col, "x={x} {which:?}");
        }
        // A band wider than half the cell reaches every column from one side or the other.
        assert_eq!(seen, cell_w);
    }

    #[test]
    fn the_shaders_one_cell_step_is_the_same_mapping_as_the_row_arithmetic() {
        // The fragment does not compute rows — it samples its own texcoord shifted by exactly one
        // CELL (`inner.y ± u_cell_uv.w`), which is far simpler than the row form and therefore
        // worth checking rather than trusting. Both must name the same slot row.
        let (cell_h, bleed, pad) = (24u32, 4u32, 1u32);
        let slot = slot_geometry((12, cell_h), (0, 0), (0, bleed), pad);
        let (origin, span) = cell_uv(slot, (12, cell_h));
        let ph = slot.padded.1 as f32;

        for y in [0u32, 1, 3, 20, 22, 23] {
            let rows = ink_rows(y, cell_h, bleed);
            let Some((which, src_row)) = rows.neighbour else {
                continue;
            };
            // The shader's own-sample v, then one cell up or down.
            let v_own = origin.1 + (y as f32 + 0.5) / cell_h as f32 * span.1;
            let v_shader = match which {
                Adjacent::Above => v_own + span.1,
                Adjacent::Below => v_own - span.1,
            };
            // `ink_rows` counts CONTENT rows; the slot's rows start after the guard band.
            let row_from_shader = (v_shader * ph - pad as f32).floor() as u32;
            assert_eq!(
                row_from_shader, src_row,
                "y={y} {which:?}: the shader's one-cell step and the row arithmetic disagree"
            );
        }
    }

    #[test]
    fn a_neighbours_overflow_lands_on_the_receiver_edge_nearest_it() {
        // Worked by hand from the slot layout, not re-derived from the code. A slot's CONTENT is
        // `cell_h + 2*bleed` rows: `[0, bleed)` holds ink that rose above the cell, `[bleed,
        // bleed+cell_h)` is the cell's own box, `[bleed+cell_h, ..)` holds ink that fell below it.
        // With cell_h 24 and bleed 4 that is 32 rows: above-overflow 0..=3, own box 4..=27,
        // below-overflow 28..=31.
        let (h, b) = (24, 4);

        // The receiver's TOP edge shows the cell ABOVE's below-overflow, in order.
        assert_eq!(ink_rows(0, h, b).neighbour, Some((Adjacent::Above, 28)));
        assert_eq!(ink_rows(3, h, b).neighbour, Some((Adjacent::Above, 31)));
        // The receiver's BOTTOM edge shows the cell BELOW's above-overflow, in order.
        assert_eq!(ink_rows(20, h, b).neighbour, Some((Adjacent::Below, 0)));
        assert_eq!(ink_rows(23, h, b).neighbour, Some((Adjacent::Below, 3)));
        // The interior belongs to nobody else.
        assert_eq!(ink_rows(4, h, b).neighbour, None);
        assert_eq!(ink_rows(19, h, b).neighbour, None);

        // ...and every row still reads its OWN ink from its own slot, offset past the top bleed.
        assert_eq!(ink_rows(0, h, b).own, 4);
        assert_eq!(ink_rows(23, h, b).own, 27);
    }

    #[test]
    fn line_thickness_is_the_xterm_font_size_formula() {
        // xterm.js: `max(1, floor(fontSize*dpr/15))`. We `round`, which agrees on all the whole-px
        // cases below and only differs by rounding a fractional em UP toward 1 rather than down.
        assert_eq!(line_thickness(16.0 * 1.0), 1); // 16px @ dpr 1  → 1.07 → 1
        assert_eq!(line_thickness(16.0 * 2.0), 2); // 16px @ dpr 2  → 2.13 → 2
        assert_eq!(line_thickness(32.0 * 2.0), 4); // 32px @ dpr 2  → 4.27 → 4
        assert_eq!(line_thickness(48.0 * 2.0), 6); // 48px @ dpr 2  → 6.4  → 6, vs beamterm's 11
    }

    #[test]
    fn line_thickness_never_vanishes() {
        // A tiny em must still draw a 1px line — the `max(1)` floor, the reason a small underline is
        // visible at all (#515's floor, now the metric's floor).
        assert_eq!(line_thickness(1.0), 1);
        assert_eq!(line_thickness(7.0), 1); // 7/15 = 0.47 → rounds to 0 → floored to 1
    }

    #[test]
    fn the_defaults_reproduce_the_cell_the_rasteriser_measured() {
        // #338 AC: `letterSpacing = 0`, `lineHeight = 1` must change nothing. This is the property
        // that lets the option land without re-baselining every other proof.
        assert_eq!(device_cell(CHAR, 0.0, 1.0, 1.0), CHAR);
        assert_eq!(device_cell(CHAR, 0.0, 1.0, 2.0), CHAR);
        assert_eq!(glyph_offset(CHAR, CHAR), (0, 0));
    }

    #[test]
    fn letter_spacing_is_css_px_so_the_same_setting_looks_the_same_at_every_density() {
        // 1 CSS px of spacing is 1 device px at dpr 1 and 2 at dpr 2 — the gap the reader sees is
        // the same. xterm's `char.width + Math.round(letterSpacing)` would add 1 device px at both,
        // i.e. half the gap on a Retina display.
        assert_eq!(device_cell(CHAR, 1.0, 1.0, 1.0).0, 11);
        assert_eq!(device_cell((20, 32), 1.0, 1.0, 2.0).0, 22);
        // And it rounds, so a fractional DPR still lands on the device grid.
        assert_eq!(device_cell(CHAR, 1.0, 1.0, 1.1).0, 11); // round(1.1) == 1
        assert_eq!(device_cell(CHAR, 2.0, 1.0, 1.1).0, 12); // round(2.2) == 2
    }

    #[test]
    fn line_height_multiplies_the_glyph_height_and_floors_like_xterm() {
        // `cell.height = Math.floor(char.height * lineHeight)` (WebglRenderer.ts:664).
        assert_eq!(device_cell(CHAR, 0.0, 1.5, 1.0).1, 24); // floor(16 * 1.5)
        assert_eq!(device_cell(CHAR, 0.0, 1.2, 1.0).1, 19); // floor(19.2)
        // It never shrinks the cell below the glyph: xterm relies on `lineHeight >= 1` for this,
        // and rejects anything less. We clamp instead of throwing from a setter.
        assert_eq!(device_cell(CHAR, 0.0, 0.5, 1.0).1, 16);
        assert_eq!(device_cell(CHAR, 0.0, 0.0, 1.0).1, 16);
    }

    #[test]
    fn the_glyph_is_centred_in_a_taller_or_wider_cell() {
        // 1.5 line height on a 16 px glyph gives a 24 px cell: 8 px of slack, 4 above, 4 below.
        let cell = device_cell(CHAR, 0.0, 1.5, 1.0);
        assert_eq!(cell, (10, 24));
        assert_eq!(glyph_offset(cell, CHAR), (0, 4));
        // 2 CSS px of spacing gives a 12 px cell: 1 px each side.
        let cell = device_cell(CHAR, 2.0, 1.0, 1.0);
        assert_eq!(glyph_offset(cell, CHAR), (1, 0));
    }

    #[test]
    fn an_odd_remainder_lands_on_the_right_and_on_the_top_exactly_as_xterm_splits_it() {
        // 3 device px of slack. xterm: `char.left = floor(3/2) = 1` (so 2 px on the right) and
        // `char.top = round(3/2) = 2` (so 1 px below). Not symmetric, and not ours to re-invent.
        assert_eq!(glyph_offset((13, 19), CHAR), (1, 2));
    }

    #[test]
    fn a_finite_but_absurd_policy_cannot_make_a_cell_no_buffer_could_hold() {
        // #338, found by the sibling lens. `NaN`/`Inf` are rejected in the setters, but `1e9` is
        // finite: the cell became `u32::MAX`, `resize`'s adopt-what-fits loop could never satisfy
        // `bw >= dw` (a single cell exceeds any allocatable buffer), and it exhausted its four
        // passes and adopted a `size` larger than the buffer WebGL actually granted — quietly
        // breaking the #339 invariant that `size` describes a buffer that exists.
        assert_eq!(device_cell(CHAR, 1e9, 1.0, 1.0).0, MAX_CELL_PX);
        assert_eq!(device_cell(CHAR, 0.0, 1e9, 1.0).1, MAX_CELL_PX);
        assert_eq!(device_cell(CHAR, 1e9, 1e9, 4.0), (MAX_CELL_PX, MAX_CELL_PX));
        // The ceiling is far above any real cell (a 16 px font ink-scans to ~10x16 device px at
        // dpr 1) and far below the smallest MAX_TEXTURE_SIZE we have measured (8192, SwiftShader).
        // A `const` assertion, so moving the bound out of that window fails the build, not a run.
        const { assert!(MAX_CELL_PX > 1000 && MAX_CELL_PX < 8192) };
    }

    #[test]
    fn a_cell_the_atlas_texture_cannot_hold_is_shrunk_rather_than_silently_breaking_it() {
        // #359. The atlas is `padded_w` wide and `padded_h * 32` tall. Headless SwiftShader reports
        // MAX_TEXTURE_SIZE 8192, so the tallest padded cell is 8192/32 = 256, i.e. a 254-px cell at
        // PADDING 1. Measured: at 258 the texture has no storage, sampling returns alpha 1, and every
        // glyph renders as a solid block — `M` came back fully lit.
        //
        // **This is the arithmetic at `bleed_y = 0`, which #791 made a case the renderer no longer
        // runs** — `vertical_bleed` floors at its headroom, so production always spends a band here.
        // The pin is kept in this form because it is the one the #359 measurement was taken against;
        // the ceiling that actually ships is the bleed-4 case pinned above it.
        assert_eq!(fit_cell_to_atlas((10, 254), 1, (0, 0), 32, 8192), (10, 254));
        assert_eq!(fit_cell_to_atlas((10, 255), 1, (0, 0), 32, 8192), (10, 254));
        assert_eq!(
            fit_cell_to_atlas((10, 4096), 1, (0, 0), 32, 8192),
            (10, 254)
        );
        // The width is bounded by the texture directly, not by the layer stack.
        assert_eq!(
            fit_cell_to_atlas((8190, 16), 1, (0, 0), 32, 8192),
            (8190, 16)
        );
        assert_eq!(
            fit_cell_to_atlas((9000, 16), 1, (0, 0), 32, 8192),
            (8190, 16)
        );
        // A real GPU's 16384 doubles both.
        assert_eq!(
            fit_cell_to_atlas((10, 4096), 1, (0, 0), 32, 16384),
            (10, 510)
        );
        // Degenerate limits never produce a zero cell.
        assert_eq!(fit_cell_to_atlas((10, 20), 1, (0, 0), 32, 1), (1, 1));
    }

    #[test]
    fn a_negative_spacing_narrows_the_cell_and_never_reaches_zero() {
        // Both references allow it (alacritty's `offset.x` is an `i8`; xterm validates only
        // `lineHeight`). alacritty floors the cell at 1 px; so do we. The glyph then starts at the
        // cell edge and overlaps its neighbour up to the band (#966) — it is never stretched.
        assert_eq!(device_cell(CHAR, -2.0, 1.0, 1.0).0, 8);
        assert_eq!(glyph_offset((8, 16), CHAR), (0, 0));
        assert_eq!(device_cell(CHAR, -100.0, 1.0, 1.0).0, 1);
        assert_eq!(device_cell(CHAR, -100.0, 1.0, 2.0).0, 1);
    }
}

#[cfg(test)]
mod dotted_tests {
    use super::dots_per_cell;

    #[test]
    fn a_dot_and_its_gap_are_each_one_line_thickness() {
        // xterm.js draws dotted as `setLineDash([lineWidth, lineWidth])` — a 1:1 duty at a period of
        // twice the line thickness (`addon-webgl/src/TextureAtlas.ts`). That is the target this
        // quantises toward, so where the cell divides evenly the two agree exactly.
        assert_eq!(dots_per_cell(8, 1), 4); // 8px cell, 1px line -> 1px dot, 1px gap
        assert_eq!(dots_per_cell(8, 2), 2); // 8px cell, 2px line -> 2px dot, 2px gap
        assert_eq!(dots_per_cell(16, 2), 4);
        assert_eq!(dots_per_cell(20, 2), 5);
    }

    #[test]
    fn the_count_is_whole_so_the_pattern_survives_a_cell_boundary() {
        // The reason this function exists at all. The fragment shader's x is CELL-LOCAL, so a period
        // that is not a whole number of cells restarts the dots at every boundary — an uneven gap the
        // eye reads as a defect, and one NO pixel assertion inside a single cell can observe.
        //
        // Two discharges exist in the corpus. xterm.js and alacritty both carry a cross-cell PHASE
        // (`computeNextVariantOffset`; and alacritty's `cellEven`, which inverts the pattern every two
        // cells because "if the cellWidth is odd, the cell will start and end with a dot, creating a
        // dash"). ghostty instead QUANTISES the count (`src/font/sprite/draw/special.zig:107-121`
        // — a three-way clamp, not the bare `ceil` an earlier version of this comment quoted in two
        // places), which is what is taken here: a whole count makes
        // the period cell-periodic by construction, so the phase term #829 deleted as mathematically
        // inert stays deleted rather than being resurrected for one mark.
        //
        // Any odd cell width is the case that decides it — alacritty's comment names exactly this one.
        for w in [7u32, 9, 11, 13, 15, 17, 19, 21] {
            let n = dots_per_cell(w, 1);
            assert!(n >= 1, "{w}px cell produced {n} dots");
        }
        // ...and the property itself: the count is an integer, so `fract(x * n)` is continuous across
        // the boundary for every cell in the row. Stated as the arithmetic a caller can check.
        assert_eq!(dots_per_cell(9, 1), 5); // 9/2 = 4.5, rounded away from zero
        assert_eq!(dots_per_cell(7, 1), 4); // 7/2 = 3.5
    }

    #[test]
    fn a_cell_too_small_for_a_pattern_still_gets_one_dot() {
        // The mirror of `line_thickness_never_vanishes`. Zero dots is an underline that vanishes,
        // which is the failure the fallback rule exists to prevent one layer up — losing the mark
        // entirely is worse than drawing a coarse one.
        assert_eq!(dots_per_cell(3, 2), 1); // 3/4 = 0.75 -> rounds to 1, not 0
        assert_eq!(dots_per_cell(1, 8), 1);
        assert_eq!(dots_per_cell(0, 1), 1); // a degenerate cell must not divide by anything
        // A degenerate THICKNESS floors the thickness, not the count — the two clamps are
        // independent and collapsing both to one dot would be the wrong answer for a cell that is
        // perfectly capable of holding a pattern. This assertion said `1` in the first draft and
        // the implementation disagreed with it; the implementation was right.
        assert_eq!(dots_per_cell(8, 0), 4);
    }
}
