//! Pure glyph-bitmap helpers (host-testable; the GL upload is browser-only).

/// Split a double-width glyph bitmap (`2*cell_w × cell_h`, RGBA, row-major) into its two
/// `cell_w × cell_h` halves: the left half (columns `0..cell_w`) uploads to the lead cell's
/// slot, the right half (columns `cell_w..2*cell_w`) to the spacer's slot (`slot+1`), so the
/// two grid cells together show the whole wide glyph. (No padding yet — #280 adds the guard
/// band, which will split the *content* region like beamterm's `split_double_width_glyph`.)
pub fn split_wide_bitmap(rgba: &[u8], cell_w: u32, cell_h: u32) -> (Vec<u8>, Vec<u8>) {
    let dst_stride = cell_w as usize * 4;
    let src_stride = dst_stride * 2;
    let mut left = vec![0u8; dst_stride * cell_h as usize];
    let mut right = vec![0u8; dst_stride * cell_h as usize];
    for row in 0..cell_h as usize {
        let (s, d) = (row * src_stride, row * dst_stride);
        if s + src_stride <= rgba.len() {
            left[d..d + dst_stride].copy_from_slice(&rgba[s..s + dst_stride]);
            right[d..d + dst_stride].copy_from_slice(&rgba[s + dst_stride..s + src_stride]);
        }
    }
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_wide_bitmap_into_left_and_right_halves() {
        // 2×1 cell → a 4px-wide × 1-row source; each pixel is a distinct RGBA.
        let cell_w = 2;
        let px = |r| [r, 0u8, 0, 255];
        let src: Vec<u8> = [px(1), px(2), px(3), px(4)].concat();

        let (left, right) = split_wide_bitmap(&src, cell_w, 1);

        // Left = columns 0..2 (pixels 1,2); right = columns 2..4 (pixels 3,4).
        assert_eq!(left, [px(1), px(2)].concat());
        assert_eq!(right, [px(3), px(4)].concat());
    }

    #[test]
    fn splits_each_row_independently() {
        // 1×2 cell → a 2px-wide × 2-row source; check row 1's split too.
        let cell_w = 1;
        let px = |r| [r, 0u8, 0, 255];
        // row0: [10][11]  row1: [20][21]
        let src: Vec<u8> = [px(10), px(11), px(20), px(21)].concat();

        let (left, right) = split_wide_bitmap(&src, cell_w, 2);

        assert_eq!(left, [px(10), px(20)].concat()); // left col of each row
        assert_eq!(right, [px(11), px(21)].concat()); // right col of each row
    }
}
