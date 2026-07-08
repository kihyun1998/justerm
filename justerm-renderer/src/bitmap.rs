//! Pure glyph-bitmap helpers (host-testable; the GL upload is browser-only).

/// The tight bounding box of a glyph's ink within a bitmap, in pixel coordinates (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InkBounds {
    pub min_x: u32,
    pub max_x: u32,
    pub min_y: u32,
    pub max_y: u32,
}

/// The physical (content) cell derived from a reference glyph's ink box, plus the baseline
/// `ascent` (pixels the ink rose above the draw position). The atlas cell adds the guard band
/// ([`PADDING`]) around this; the on-screen grid cell is this physical size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
    pub ascent: f32,
}

/// Derive the physical cell metrics from a reference glyph's ink `bounds` measured at
/// `draw_offset` (the pixel offset the glyph was drawn at). Width/height are the inclusive ink
/// span; `ascent` is how far the ink rose above the draw point (`draw_offset - min_y`).
pub fn cell_metrics(bounds: InkBounds, draw_offset: f32) -> CellMetrics {
    CellMetrics {
        width: bounds.max_x - bounds.min_x + 1,
        height: bounds.max_y - bounds.min_y + 1,
        ascent: draw_offset - bounds.min_y as f32,
    }
}

/// Scan an RGBA bitmap (`w`×`h`, row-major) for the tight bounding box of pixels whose alpha
/// is `>= alpha_threshold`. Returns `None` when nothing meets the threshold (a blank glyph).
/// This is the basis of ink-scan cell metrics (#288): measuring the cell from the `█` glyph's
/// real pixel bounds is more accurate than `fontBoundingBox`, which has rounding/box-gap
/// issues (mirrors beamterm `canvas_rasterizer::measure_cell_metrics`).
pub fn ink_bounds(pixels: &[u8], w: u32, h: u32, alpha_threshold: u8) -> Option<InkBounds> {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (w, 0u32, h, 0u32);
    let mut found = false;
    for y in 0..h {
        for x in 0..w {
            let alpha = pixels[((y * w + x) * 4 + 3) as usize];
            if alpha >= alpha_threshold {
                found = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    found.then_some(InkBounds {
        min_x,
        max_x,
        min_y,
        max_y,
    })
}

/// Minimum alpha for a pixel to count when detecting colour — skips near-transparent
/// anti-aliasing fringe that could carry stray channel noise.
const COLOR_ALPHA_MIN: u8 = 16;
/// Minimum channel spread (max − min of R/G/B) for a pixel to read as coloured. The rasteriser
/// fills text in white, and the canvas's grayscale AA sets `R = G = B` exactly, so any real
/// spread signals a colour glyph; the small floor absorbs stray rounding.
const COLOR_SPREAD_MIN: u8 = 8;

/// Whether an RGBA bitmap (row-major) contains colour — a colour emoji the browser drew in its
/// own palette (COLR/CBDT/SVG), versus a text glyph the rasteriser drew in white (grayscale,
/// with coverage in the alpha channel only). True if any sufficiently-opaque pixel's R/G/B are
/// not near-equal. This is **font ground-truth** emoji detection (#284): it classifies by what
/// the font actually rendered, so a text-presentation glyph (e.g. `✂` with no VS16) that the
/// font draws monochrome is correctly treated as text — not colour-sampled like a unicode
/// range check would mis-do.
pub fn is_color_bitmap(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|px| {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        if a < COLOR_ALPHA_MIN {
            return false;
        }
        let hi = r.max(g).max(b);
        let lo = r.min(g).min(b);
        hi - lo >= COLOR_SPREAD_MIN
    })
}

/// The transparent guard band (in pixels) around every atlas glyph cell: the atlas cell is the
/// physical cell grown by `2*PADDING` in each dimension, with the glyph drawn inset. The band
/// stops adjacent glyph bands bleeding under `NEAREST` sampling and gives over-tall / fallback
/// glyphs room not to clip (mirrors beamterm `FontAtlasData::PADDING`). #288.
pub const PADDING: u32 = 1;

/// Split a padded double-width glyph into its two `cell_w × cell_h` (padded) halves. `src` is
/// `src_w × cell_h` (RGBA, row-major) with a [`PADDING`] guard band only on its *outer* edges;
/// each output half becomes `[outer padding][half content][inner padding]`, the inner (centre-
/// join) padding left transparent so the two halves keep a guard band on every side. The left
/// half uploads to the lead cell's slot, the right to the spacer's (`slot+1`). Mirrors beamterm
/// `split_double_width_glyph`.
pub fn split_wide_bitmap(src: &[u8], src_w: u32, cell_w: u32, cell_h: u32) -> (Vec<u8>, Vec<u8>) {
    let padding = PADDING as usize;
    let (cell_w, src_w) = (cell_w as usize, src_w as usize);
    let content_w = cell_w.saturating_sub(2 * padding);
    let src_stride = src_w * 4;
    let dst_stride = cell_w * 4;
    let mut left = vec![0u8; dst_stride * cell_h as usize];
    let mut right = vec![0u8; dst_stride * cell_h as usize];

    let src_content_start = padding;
    let src_content_width = src_w.saturating_sub(2 * padding);
    let left_content_width = src_content_width / 2;
    let right_content_width = src_content_width - left_content_width;

    let copy = |dst: &mut [u8], d_col: usize, src: &[u8], s_col: usize, s_row: usize| {
        let s = s_row * src_stride + s_col * 4;
        let d = s_row * dst_stride + d_col * 4;
        if s + 4 <= src.len() && d + 4 <= dst.len() {
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    };

    for row in 0..cell_h as usize {
        // left half: [outer padding][left content]  (inner padding stays transparent)
        for col in 0..padding {
            copy(&mut left, col, src, col, row);
        }
        for col in 0..left_content_width.min(content_w) {
            copy(&mut left, padding + col, src, src_content_start + col, row);
        }
        // right half: [left content...][right content]  then [outer padding] on the far edge
        for col in 0..right_content_width.min(content_w) {
            copy(
                &mut right,
                padding + col,
                src,
                src_content_start + left_content_width + col,
                row,
            );
        }
        for col in 0..padding {
            copy(
                &mut right,
                cell_w - padding + col,
                src,
                src_w - padding + col,
                row,
            );
        }
    }
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `w`×`h` RGBA bitmap (opaque white) from a list of inked `(x, y, alpha)` pixels.
    fn bitmap(w: u32, h: u32, ink: &[(u32, u32, u8)]) -> Vec<u8> {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for &(x, y, a) in ink {
            let i = ((y * w + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&[255, 255, 255, a]);
        }
        buf
    }

    /// A `w`×`h` RGBA bitmap from a list of `(x, y, [r,g,b,a])` pixels (rest transparent).
    fn rgba(w: u32, h: u32, px: &[(u32, u32, [u8; 4])]) -> Vec<u8> {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for &(x, y, c) in px {
            let i = ((y * w + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&c);
        }
        buf
    }

    #[test]
    fn is_color_bitmap_false_for_a_grayscale_text_glyph() {
        // A white text glyph: R=G=B=255, coverage in alpha. No pixel is coloured.
        let buf = bitmap(3, 3, &[(0, 0, 255), (1, 1, 128), (2, 2, 40)]);
        assert!(!is_color_bitmap(&buf));
        // Pure black/gray coverage is also not colour.
        let gray = rgba(2, 1, &[(0, 0, [90, 90, 90, 255])]);
        assert!(!is_color_bitmap(&gray));
    }

    #[test]
    fn is_color_bitmap_true_for_a_coloured_emoji_pixel() {
        // One opaque red-ish pixel (spread 255 ≥ 8) among grayscale → colour emoji.
        let buf = rgba(
            3,
            1,
            &[(0, 0, [200, 200, 200, 255]), (1, 0, [220, 40, 40, 255])],
        );
        assert!(is_color_bitmap(&buf));
    }

    #[test]
    fn is_color_bitmap_ignores_near_transparent_and_low_spread_pixels() {
        // A coloured pixel but almost transparent (alpha < 16) → ignored (AA fringe).
        let faint = rgba(1, 1, &[(0, 0, [255, 0, 0, 8])]);
        assert!(!is_color_bitmap(&faint));
        // Opaque but tiny channel spread (< 8, rounding noise) → not colour.
        let noise = rgba(1, 1, &[(0, 0, [200, 204, 199, 255])]);
        assert!(!is_color_bitmap(&noise));
    }

    #[test]
    fn ink_bounds_finds_a_single_pixel() {
        // One opaque pixel at (1, 1) in a 3×3 buffer → a 1×1 box at (1,1).
        let buf = bitmap(3, 3, &[(1, 1, 255)]);
        assert_eq!(
            ink_bounds(&buf, 3, 3, 128),
            Some(InkBounds {
                min_x: 1,
                max_x: 1,
                min_y: 1,
                max_y: 1,
            })
        );
    }

    #[test]
    fn ink_bounds_is_none_for_a_blank_bitmap() {
        // Nothing at/above the threshold → no ink box.
        let buf = bitmap(4, 4, &[]);
        assert_eq!(ink_bounds(&buf, 4, 4, 128), None);
        // A pixel below the threshold is also ignored.
        let dim = bitmap(4, 4, &[(2, 2, 100)]);
        assert_eq!(ink_bounds(&dim, 4, 4, 128), None);
    }

    #[test]
    fn ink_bounds_spans_all_inked_pixels() {
        // Inked pixels at (1,0), (3,1), (2,2) → box x:1..=3, y:0..=2. The sub-threshold pixel
        // at (0,3) must not widen the box.
        let buf = bitmap(4, 4, &[(1, 0, 200), (3, 1, 255), (2, 2, 128), (0, 3, 50)]);
        assert_eq!(
            ink_bounds(&buf, 4, 4, 128),
            Some(InkBounds {
                min_x: 1,
                max_x: 3,
                min_y: 0,
                max_y: 2,
            })
        );
    }

    #[test]
    fn cell_metrics_derive_span_and_ascent_from_ink_bounds() {
        // Ink box x:2..=9 (width 8), y:3..=20 (height 18), drawn at offset 16 → the ink rose
        // 16-3 = 13 px above the draw point. Values worked by hand from the beamterm formula.
        let m = cell_metrics(
            InkBounds {
                min_x: 2,
                max_x: 9,
                min_y: 3,
                max_y: 20,
            },
            16.0,
        );
        assert_eq!(m.width, 8);
        assert_eq!(m.height, 18);
        assert_eq!(m.ascent, 13.0);
    }

    #[test]
    fn split_keeps_the_outer_guard_band_and_leaves_the_centre_join_transparent() {
        // PADDING = 1. Padded single cell = 3px wide (content_w = 1). A source wide glyph is
        // src_w = 2*cell_w - 2*PADDING = 4px: [L-pad][c0][c1][R-pad], one row. Worked by hand
        // from beamterm split_double_width_glyph:
        //   left  = [L-pad][c0][transparent]   (outer pad + left content + inner pad)
        //   right = [transparent][c1][R-pad]   (inner pad + right content + outer pad)
        let px = |a| [255u8, 255, 255, a]; // white, distinct alpha marker
        let zero = [0u8, 0, 0, 0];
        let src: Vec<u8> = [px(11), px(22), px(33), px(44)].concat(); // 4px, 1 row

        let (left, right) = split_wide_bitmap(&src, 4, 3, 1);

        assert_eq!(
            left,
            [px(11), px(22), zero].concat(),
            "left: outer-pad, content, inner-pad"
        );
        assert_eq!(
            right,
            [zero, px(33), px(44)].concat(),
            "right: inner-pad, content, outer-pad"
        );
    }

    #[test]
    fn split_handles_each_row_independently() {
        // Same 4→(3,3) layout, two rows, to prove per-row striding.
        let px = |a| [255u8, 255, 255, a];
        let zero = [0u8, 0, 0, 0];
        // row0: 11 22 33 44   row1: 51 52 53 54
        let src: Vec<u8> = [
            px(11),
            px(22),
            px(33),
            px(44), //
            px(51),
            px(52),
            px(53),
            px(54),
        ]
        .concat();

        let (left, right) = split_wide_bitmap(&src, 4, 3, 2);

        assert_eq!(left, [px(11), px(22), zero, px(51), px(52), zero].concat());
        assert_eq!(right, [zero, px(33), px(44), zero, px(53), px(54)].concat());
    }
}
