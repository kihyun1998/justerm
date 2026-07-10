//! Cell metrics: how the grid cell relates to the glyph box (#338).
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
//! deliberate divergence. xterm adds `Math.round(letterSpacing)` straight onto a device-px char
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
pub fn glyph_offset(cell: (u32, u32), char_px: (u32, u32)) -> (u32, u32) {
    let dx = cell.0.saturating_sub(char_px.0);
    let dy = cell.1.saturating_sub(char_px.1);
    (dx / 2, dy.div_ceil(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured ink box of `█` at `FONT_SIZE * 1` in Chromium (#328): 10 x 16 device px.
    const CHAR: (u32, u32) = (10, 16);

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
