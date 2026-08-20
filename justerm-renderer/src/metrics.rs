//! Cell metrics: how the grid cell relates to the glyph box (#338).
//!
//! One link in the chain ADR-0022 records: the cell comes from an ink scan of the font's `█`
//! ([`rasterizer`](crate::rasterizer)), this module nests the glyph box inside it, and that nesting is
//! why tiling glyphs must be drawn to the *cell* ([`builtin`](crate::builtin)) instead of to their ink
//! box. Unlike the measurement itself, this split IS the prior-art consensus — both references carry a
//! char box beside a cell box, as the paragraphs below quote.
//!
//! Until now they were the same rectangle. The rasteriser ink-scans `█` at `FONT_SIZE * dpr` and
//! that box *was* the cell, so the shader could stretch one glyph quad across one cell and be right
//! by construction. `letterSpacing` and `lineHeight` break the identity: the cell grows, the glyph
//! does not, and something has to say where inside the cell the glyph sits.
//!
//! Both references keep exactly this split. xterm.js carries `device.char.{width,height}` beside
//! `device.cell.{width,height}` and centres with `device.char.{top,left}`
//! (`WebglRenderer.ts:654-675`). Alacritty sizes the cell from the font's advance plus a user
//! `offset` and positions the glyph with a separate `glyph_offset`
//! (`display/mod.rs:1608-1615`, `config/font.rs:20-23`).
//!
//! **We take `letter_spacing` in CSS pixels; both references take device pixels.** That is a
//! deliberate divergence, adjudicated in **ADR-0023** — the rule being that a setting expressed in the
//! same space as an existing logical setting must use that space, and `font_size` is CSS px. Note
//! alacritty is not consistent with itself here: it scales `window.padding` by the scale factor
//! (`config/window.rs:123-127`) while adding `font.offset` to device-px metrics raw
//! (`display/mod.rs:1608-1615`), so its own two pixel settings speak different units. xterm adds `Math.round(letterSpacing)` straight onto a device-px char
//! width (`WebglRenderer.ts:671`, and `DomRenderer.ts:140` agrees), so the same setting is a
//! 2-CSS-px gap on a dpr-1 display and a 1-CSS-px gap on a Retina one — the text looks different
//! when you move the window. Our own `FONT_SIZE` is CSS px scaled by the DPR at rasterisation time;
//! taking spacing in device px would make the two halves of the same font description speak
//! different units. `line_height` is a multiplier, so the question does not arise.

/// The largest a single cell may become, device px (#338).
///
/// `setLetterSpacing(1e9)` is finite, so neither setter's `is_finite` check stops it, and a cell of
/// `u32::MAX` makes `resize`'s adopt-what-fits loop unsatisfiable: no allocatable buffer holds one
/// such cell, so it exhausts its passes and adopts a `size` describing a buffer WebGL never granted
/// (#339). Far above any real cell — a 16 px font ink-scans to roughly 10x16 device px at dpr 1 —
/// and below the smallest `MAX_TEXTURE_SIZE` we have measured (8192, headless SwiftShader).
pub const MAX_CELL_PX: u32 = 4096;

/// Shrink a cell until the atlas that must hold it fits the implementation's texture limit (#359).
///
/// The atlas is a 2D array texture: one padded cell wide, `glyphs_per_layer` cells tall. #338 let the
/// consumer grow the cell, and #359 tied the atlas slot to it — so `lineHeight = 16` on a 16-px glyph
/// asks for a `258 x 8256` texture, and `MAX_TEXTURE_SIZE` is 8192 under headless SwiftShader.
///
/// `glTexStorage3D` does not throw on that. It raises `GL_INVALID_VALUE`, glow does not look, and the
/// texture is left storage-less: sampling it returns `(0,0,0,1)`, i.e. **coverage 1 for every cell**.
/// The terminal fills solid with the foreground colour, and a proof drawn with `█` cannot see it.
/// Measured: at `lineHeight = 16` an `M` came back with every pixel lit.
///
/// So ask, then adopt — as `resize_surface` does with the drawing buffer (#339). The caller reports the cell
/// it actually got through `cell_height()`.
/// `bleed_y` is the band a slot reserves above *and* below the cell for ink that leaves it
/// (ADR-0019 R1.2, #791). It is spent out of the same per-layer height as the guard band, so it
/// lowers the tallest cell the atlas can hold — and it is a separate argument rather than something
/// the caller folds into `padding` because the bleed is **vertical only**: folding it in would
/// narrow the width ceiling for a band the width never reserves.
pub fn fit_cell_to_atlas(
    cell: (u32, u32),
    padding: u32,
    bleed_y: u32,
    glyphs_per_layer: u32,
    max_texture_size: u32,
) -> (u32, u32) {
    let pad2 = 2 * padding;
    let max_w = max_texture_size.saturating_sub(pad2).max(1);
    // Every layer stacks `glyphs_per_layer` padded cells vertically, and each of those now carries
    // the bleed band on both sides of the cell.
    let max_h = (max_texture_size / glyphs_per_layer.max(1))
        .saturating_sub(pad2 + 2 * bleed_y)
        .max(1);
    (cell.0.min(max_w), cell.1.min(max_h))
}

/// The device-pixel grid cell for a glyph box of `char_px`, given the consumer's policy.
///
/// `letter_spacing_css` may be negative (xterm and alacritty both allow it, and some fonts want
/// it); the cell then narrows past the glyph, which the shader crops rather than stretching. The
/// cell never reaches zero — alacritty floors its own at 1 (`compute_cell_size`, `.max(1.)`), and a
/// zero-width cell would make the whole grid degenerate.
///
/// `line_height` below 1 would put the cell *inside* the glyph. xterm rejects the option outright
/// (`OptionsService.ts:182-186`, "cannot be less than 1"); we clamp, because a renderer that throws
/// from a setter is a worse contract than one that reports the metrics it adopted.
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
/// **Centring is xterm's choice, not both references'.** alacritty baseline-anchors instead
/// (`glyph_cache.rs:256`, `glyph.top -= metrics.descent`, no halving) and leaves the vertical
/// placement to the user's `glyph_offset`. We follow xterm because it is the closer analogue — a
/// browser rasteriser feeding a GPU cell atlas — and because a terminal that grows its line height
/// wants the extra room split, not all of it above the text.
///
/// The halves are split the way xterm splits them: horizontally `floor` (`char.left =
/// Math.floor(letterSpacing / 2)`), vertically `round` (`char.top = Math.round((cell.height -
/// char.height) / 2)`). With an odd remainder the extra pixel lands on the right and on the top.
/// Arbitrary-looking, and mirrored on purpose — the alternative is to invent a different arbitrary.
///
/// A cell narrower than the glyph (negative spacing) offsets by zero: the glyph starts at the cell's
/// edge and is cropped on the far side.
/// Underline / strikethrough thickness in device pixels, from the font SIZE — `max(1, round(font_px
/// / 15))`, where `font_px = font_size * dpr` is the em in device px. This is xterm.js's rule
/// (`addon-webgl/src/TextureAtlas.ts`, `Math.max(1, Math.floor(fontSize * dpr / 15))`), used because a
/// Canvas renderer has no font file and therefore no `underline_thickness` metric the native
/// terminals read (#517; `TextMetrics` exposes only box + baseline, measured). It replaces beamterm's
/// `0.05 * glyph_box` (#267), which was ~2x too heavy — measured 11.3% of the cell against xterm's
/// 6.2%, and box-relative so `lineHeight`/font-family could distort it. `round`, not `floor`, so a
/// 15px em rounds to 1 rather than truncating to 0 before the `max`.
pub fn line_thickness(font_px: f32) -> u32 {
    ((font_px / 15.0).round() as u32).max(1)
}

pub fn glyph_offset(cell: (u32, u32), char_px: (u32, u32)) -> (u32, u32) {
    let dx = cell.0.saturating_sub(char_px.0);
    let dy = cell.1.saturating_sub(char_px.1);
    (dx / 2, dy.div_ceil(2))
}

/// The atlas slot a bake fills, and where inside it the glyph's box starts (#791).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotGeometry {
    /// The padded slot: the cell, plus the bleed band above and below, plus the guard band all round.
    pub padded: (u32, u32),
    /// Top-left of the glyph's box inside that slot, device px.
    pub draw_origin: (u32, u32),
}

/// Lay out one atlas slot: how big it is, and where the glyph goes inside it.
///
/// Three offsets stack on the vertical axis and they belong to three different things, which is why
/// this is one function rather than three additions at the call site: the **guard band** keeps
/// neighbouring slots from bleeding into each other under sampling (#288), the **bleed band** is the
/// room `I_neighbour` needs for ink that leaves the cell (ADR-0019 R1.2), and `char_offset` is where
/// the spacing policy put the glyph *within* the cell (#338). Only the last is horizontal too — the
/// bleed is vertical for now, so `padded.0` grows by the guard band alone.
///
/// `bleed_y = 0` reproduces the pre-#791 bake exactly, which is what lets the band be adopted one
/// font configuration at a time.
pub fn slot_geometry(
    cell: (u32, u32),
    char_offset: (u32, u32),
    bleed_y: u32,
    padding: u32,
) -> SlotGeometry {
    SlotGeometry {
        padded: (cell.0 + 2 * padding, cell.1 + 2 * bleed_y + 2 * padding),
        draw_origin: (padding + char_offset.0, padding + bleed_y + char_offset.1),
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
/// The cell is the ink box of `█` (ADR-0022) and the face's own glyphs are not bounded by it, so the
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

/// Which adjacent cell an `I_neighbour` contribution came from (ADR-0019 R1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjacent {
    Above,
    Below,
}

/// The slot content rows a receiver row reads: its own, and at most one neighbour's.
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

    /// The measured ink box of `█` at `FONT_SIZE * 1` in Chromium (#328): 10 x 16 device px.
    const CHAR: (u32, u32) = (10, 16);

    #[test]
    fn the_cell_maps_to_its_own_band_of_the_slot_not_to_the_whole_slot() {
        // The shader's half of the same contract. A taller slot must NOT stretch into the cell —
        // the cell's texcoords have to land on the middle band and leave the bleed to whoever owns
        // the ink in it. Worked by hand: a 12x24 cell with bleed 4 and guard 1 is a 14x34 slot, so
        // the cell occupies y 5..=28 of 34 and x 1..=12 of 14.
        let (o, s) = cell_uv(slot_geometry((12, 24), (0, 0), 4, 1), (12, 24));
        assert_eq!(o, (1.0 / 14.0, 5.0 / 34.0));
        assert_eq!(s, (12.0 / 14.0, 24.0 / 34.0));

        // Bleed 0 reproduces the guard-band inset the shader has always applied, which is the
        // assertion that keeps every existing pixel proof meaningful across this change.
        let (o0, s0) = cell_uv(slot_geometry((12, 24), (0, 0), 0, 1), (12, 24));
        assert_eq!(o0, (1.0 / 14.0, 1.0 / 26.0));
        assert_eq!(s0, (12.0 / 14.0, 24.0 / 26.0));
    }

    #[test]
    fn the_bake_reserves_the_band_around_the_cell_and_pushes_the_glyph_past_it() {
        // Worked from the slot layout by hand. A padded slot is the cell, plus the bleed band on the
        // top and bottom edges, plus the guard band on all four; the glyph's box then starts past
        // the guard band, past the bleed, and past wherever the spacing policy put it in the cell.
        let cell = (12, 24);

        let g = slot_geometry(cell, (0, 0), 4, 1);
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
        let spaced = slot_geometry(cell, (2, 3), 4, 1);
        assert_eq!(
            spaced.padded,
            (14, 34),
            "the spacing moved the glyph, not the slot"
        );
        assert_eq!(spaced.draw_origin, (3, 8));

        // Bleed 0 must reproduce today's bake exactly — this is the assertion that lets the band be
        // switched on per configuration without every other configuration moving.
        let none = slot_geometry(cell, (2, 3), 0, 1);
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
        assert_eq!(fit_cell_to_atlas((10, 246), 1, 4, 32, 8192), (10, 246));
        assert_eq!(fit_cell_to_atlas((10, 247), 1, 4, 32, 8192), (10, 246));
        assert_eq!(fit_cell_to_atlas((10, 4096), 1, 4, 32, 8192), (10, 246));
        // The width budget is untouched — the bleed is vertical only (#791's scope), so a cell that
        // fits today still fits. This is the half that would silently regress if the bleed were
        // folded into `padding` at the call site instead of being its own argument.
        assert_eq!(fit_cell_to_atlas((8190, 16), 1, 4, 32, 8192), (8190, 16));
        assert_eq!(fit_cell_to_atlas((9000, 16), 1, 4, 32, 8192), (8190, 16));
        // And bleed 0 is exactly today's ceiling: the limit moves only when the feature is on.
        assert_eq!(fit_cell_to_atlas((10, 4096), 1, 0, 32, 8192), (10, 254));
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
        assert_eq!(fit_cell_to_atlas((10, 254), 1, 0, 32, 8192), (10, 254));
        assert_eq!(fit_cell_to_atlas((10, 255), 1, 0, 32, 8192), (10, 254));
        assert_eq!(fit_cell_to_atlas((10, 4096), 1, 0, 32, 8192), (10, 254));
        // The width is bounded by the texture directly, not by the layer stack.
        assert_eq!(fit_cell_to_atlas((8190, 16), 1, 0, 32, 8192), (8190, 16));
        assert_eq!(fit_cell_to_atlas((9000, 16), 1, 0, 32, 8192), (8190, 16));
        // A real GPU's 16384 doubles both.
        assert_eq!(fit_cell_to_atlas((10, 4096), 1, 0, 32, 16384), (10, 510));
        // Degenerate limits never produce a zero cell.
        assert_eq!(fit_cell_to_atlas((10, 20), 1, 0, 32, 1), (1, 1));
    }

    #[test]
    fn a_negative_spacing_narrows_the_cell_and_never_reaches_zero() {
        // Both references allow it (alacritty's `offset.x` is an `i8`; xterm validates only
        // `lineHeight`). alacritty floors the cell at 1 px; so do we. The glyph then starts at the
        // cell edge and the shader crops it — it is never stretched.
        assert_eq!(device_cell(CHAR, -2.0, 1.0, 1.0).0, 8);
        assert_eq!(glyph_offset((8, 16), CHAR), (0, 0));
        assert_eq!(device_cell(CHAR, -100.0, 1.0, 1.0).0, 1);
        assert_eq!(device_cell(CHAR, -100.0, 1.0, 2.0).0, 1);
    }
}
