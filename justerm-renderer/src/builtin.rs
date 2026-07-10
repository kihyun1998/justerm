//! Built-in block-element glyphs, drawn to the CELL rather than to an ink box (#359).
//!
//! `U+2580`–`U+259F` are meant to tile. A region of `█` is one solid fill; `▀▄▌▐` halve the cell
//! exactly; `▖▗▘▝` quarter it. The browser's text engine draws them as glyphs, and #338 masks every
//! glyph to its ink box — so as soon as `letterSpacing` or `lineHeight` moves the cell away from the
//! ink box, the fills stop meeting. Worse, the renderer *measures* its cell by ink-scanning `█`
//! ([`rasterizer`](crate::rasterizer)), so at `lineHeight = 1.5` the very glyph that defines the
//! cell no longer fills it.
//!
//! Both references intercept the range ahead of the font and draw it at cell size: xterm.js's
//! `CustomGlyphRasterizer` at `deviceCellWidth × deviceCellHeight`, alacritty's
//! `builtin_font::builtin_glyph` at `average_advance + offset.x` × `line_height + offset.y`. The
//! geometry below mirrors alacritty's (`builtin_font.rs:394-499`) rather than inventing its own — the
//! eighth fractions, the `round().max(1)` on every extent, and the four-quadrant decomposition.
//!
//! Coverage only. The bitmap is white with the coverage in the alpha channel, exactly what
//! [`Rasterizer::rasterize`](crate::rasterizer::Rasterizer::rasterize) returns, so the atlas upload
//! path does not care where a glyph came from. The shades are a flat alpha, as alacritty draws them
//! (`COLOR_FILL_ALPHA_STEP_*`), not a dither pattern.

/// The codepoint range this module owns. Box drawing (`U+2500`–`U+257F`) needs stroke widths, dashes,
/// diagonals and rounded corners; it is a separate slice.
pub const FIRST: u32 = 0x2580;
pub const LAST: u32 = 0x259F;

/// Block sextants: a 2x3 mosaic, 60 of the 64 six-bit combinations (#361).
pub const SEXTANT_FIRST: u32 = 0x1FB00;
pub const SEXTANT_LAST: u32 = 0x1FB3B;

/// The extra one-quarter / three-eighths / five-eighths / three-quarters / seven-eighths blocks that
/// `U+2580`-`U+259F` lacks: `1FB82`-`1FB86` measured from the top, `1FB87`-`1FB8B` from the right.
pub const EIGHTH_FIRST: u32 = 0x1FB82;
pub const EIGHTH_LAST: u32 = 0x1FB8B;

/// The six-bit mosaic mask for a sextant, or `None` outside the range.
///
/// Unicode enumerates 60 of the 64 combinations: `000000` is a space, `111111` is `█`, and `010101` /
/// `101010` would duplicate `▌` / `▐`. So the codepoint is a plain index into that filtered list, and
/// the six lists of thirty literals alacritty spells out (`builtin_font.rs:509-572`) are DERIVABLE.
/// Cross-checked against all 180 of them: zero mismatches. VTE (`minifont.cc:1682`) and kitty
/// (`decorations.c:2171`) derive it the same way, skipping at the same two masks.
///
/// Bit `i` lights cell `i+1` of `BLOCK SEXTANT-n`, left-to-right then top-to-bottom — the order
/// xterm's `sextant(0b000001)` helper uses for `BLOCK SEXTANT-1` (`CustomGlyphDefinitions.ts:465`).
pub fn sextant_mask(cp: u32) -> Option<u8> {
    if !(SEXTANT_FIRST..=SEXTANT_LAST).contains(&cp) {
        return None;
    }
    const SKIP: [u8; 2] = [0b010101, 0b101010]; // `▌` and `▐` already exist
    (1u8..=62)
        .filter(|m| !SKIP.contains(m))
        .nth((cp - SEXTANT_FIRST) as usize)
}

/// Alpha for the three shade characters, and for a solid fill. alacritty's `COLOR_FILL_ALPHA_STEP_3`
/// / `_2` / `_1` / `COLOR_FILL` (`builtin_font.rs:10-15`).
const SHADE_LIGHT: u8 = 64; // ░
const SHADE_MEDIUM: u8 = 128; // ▒
const SHADE_DARK: u8 = 192; // ▓
const SOLID: u8 = 255; // █

/// A white RGBA bitmap of `w * h` device px with the coverage of a block element, sextant, or extra
/// eighth block in alpha — or `None` for a codepoint this module does not own.
///
/// The origin is the cell's TOP-left, matching the rasteriser's canvas and the shader's texcoord.
pub fn block_glyph(cp: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    let owned = (FIRST..=LAST).contains(&cp)
        || (SEXTANT_FIRST..=SEXTANT_LAST).contains(&cp)
        || (EIGHTH_FIRST..=EIGHTH_LAST).contains(&cp);
    if !owned || w == 0 || h == 0 {
        return None;
    }
    let mut buf = vec![0u8; (w * h * 4) as usize];

    if let Some(mask) = sextant_mask(cp) {
        // A 2x3 mosaic. The rows are EQUAL THIRDS, as alacritty divides them
        // (`builtin_font.rs:505-507`): the first two take `round(h/3)` and the last takes the
        // remainder, so the six cells tile the glyph exactly for any `h >= 3`. Below that a 2x3
        // mosaic cannot show three rows at all — the lower bands come out empty, and no pixel is ever
        // lit twice. Real cells are 16-33 device px.
        //
        // xterm divides them `3/8, 2/8, 3/8` (`CustomGlyphDefinitions.ts:888-889`), which lands the row
        // boundaries on the eighth-block grid at the cost of a thinner middle row. It is **alone** in
        // that: alacritty (`:506`), VTE (`minifont.cc:678`), wezterm (`customglyph.rs:5368`) and kitty
        // (`decorations.c:1591`) all divide into equal thirds. xterm's source states the fractions and
        // never says why; we do not guess. These are Teletext 2x3 mosaic glyphs, so a uniform mosaic
        // pixel matters more than agreeing with `▄`.
        //
        // `saturating_sub` is a deliberate hardening, not a transcription slip: alacritty computes
        // `height - 2*y_third` in `f32` with no floor, which is `-1` at `h = 1` and wraps when it
        // casts to `usize`. Do not "fix" it back.
        // `.max(1)` is the live clamp; `round(w/2) <= w` and `round(h/3) <= h` for every `w,h >= 1`,
        // so no upper clamp is needed. `fill` clips the far edge regardless.
        let xc = ((w as f32 / 2.0).round() as u32).max(1);
        let third = ((h as f32 / 3.0).round() as u32).max(1);
        let last = h.saturating_sub(2 * third);
        let rows = [(0, third), (third, third), (2 * third, last)];
        for (row, &(y, rh)) in rows.iter().enumerate() {
            for col in 0..2u32 {
                if mask >> (row * 2 + col as usize) & 1 == 1 {
                    fill(&mut buf, (w, h), (col * xc, y, xc, rh), SOLID);
                }
            }
        }
        return Some(buf);
    }

    match cp {
        // Shades and the full block: a flat coverage over the whole cell.
        0x2588 | 0x2591 | 0x2592 | 0x2593 => {
            let a = match cp {
                0x2588 => SOLID,
                0x2591 => SHADE_LIGHT,
                0x2592 => SHADE_MEDIUM,
                _ => SHADE_DARK,
            };
            fill(&mut buf, (w, h), (0, 0, w, h), a);
        }
        // Quadrants: a union of up to four half-cells.
        0x2596..=0x259F => {
            let xc = ((w as f32 / 2.0).round() as u32).max(1);
            let yc = ((h as f32 / 2.0).round() as u32).max(1);
            // Which quadrants each character lights, straight off the character names.
            let upper_left = matches!(cp, 0x2598..=0x259C);
            let upper_right = matches!(cp, 0x259B..=0x259F);
            let lower_left = matches!(cp, 0x2596 | 0x2599 | 0x259B | 0x259E | 0x259F);
            let lower_right = matches!(cp, 0x2597 | 0x2599 | 0x259A | 0x259C | 0x259F);
            if upper_left {
                fill(&mut buf, (w, h), (0, 0, xc, yc), SOLID);
            }
            if upper_right {
                fill(&mut buf, (w, h), (xc, 0, xc, yc), SOLID);
            }
            if lower_left {
                fill(&mut buf, (w, h), (0, yc, xc, yc), SOLID);
            }
            if lower_right {
                fill(&mut buf, (w, h), (xc, yc, xc, yc), SOLID);
            }
        }
        // Eighths and halves: one rectangle.
        _ => {
            let (wf, hf) = (w as f32, h as f32);
            let rect_w = match cp {
                // `1FB87`-`1FB8B`: RIGHT one quarter / three eighths / five eighths / three quarters
                // / seven eighths. alacritty folds them into the same table (`builtin_font.rs:404-410`).
                0x1FB87 => wf * 2.0 / 8.0,
                0x1FB88 => wf * 3.0 / 8.0,
                0x1FB89 => wf * 5.0 / 8.0,
                0x1FB8A => wf * 6.0 / 8.0,
                0x1FB8B => wf * 7.0 / 8.0,
                0x2589 => wf * 7.0 / 8.0,
                0x258A => wf * 6.0 / 8.0,
                0x258B => wf * 5.0 / 8.0,
                0x258C => wf * 4.0 / 8.0, // ▌ left half
                0x258D => wf * 3.0 / 8.0,
                0x258E => wf * 2.0 / 8.0,
                0x258F => wf / 8.0,
                0x2590 => wf * 4.0 / 8.0, // ▐ right half
                0x2595 => wf / 8.0,       // ▕ right one eighth
                _ => wf,
            };
            // `y_from_bottom` is where the rectangle's TOP sits, measured up from the cell's bottom —
            // which is how the lower-eighth characters are defined. alacritty flips it the same way.
            let (rect_h, y_from_bottom) = match cp {
                0x2580 => (hf * 4.0 / 8.0, hf), // ▀ upper half
                0x2581 => (hf / 8.0, hf / 8.0), // ▁ lower one eighth
                0x2582 => (hf * 2.0 / 8.0, hf * 2.0 / 8.0),
                0x2583 => (hf * 3.0 / 8.0, hf * 3.0 / 8.0),
                0x2584 => (hf * 4.0 / 8.0, hf * 4.0 / 8.0), // ▄ lower half
                0x2585 => (hf * 5.0 / 8.0, hf * 5.0 / 8.0),
                0x2586 => (hf * 6.0 / 8.0, hf * 6.0 / 8.0),
                0x2587 => (hf * 7.0 / 8.0, hf * 7.0 / 8.0),
                0x2594 => (hf / 8.0, hf), // ▔ upper one eighth
                // `1FB82`-`1FB86`: UPPER one quarter / three eighths / five eighths / three quarters
                // / seven eighths — the fractions `2580`-`2587` skips (`builtin_font.rs:426-430`).
                0x1FB82 => (hf * 2.0 / 8.0, hf),
                0x1FB83 => (hf * 3.0 / 8.0, hf),
                0x1FB84 => (hf * 5.0 / 8.0, hf),
                0x1FB85 => (hf * 6.0 / 8.0, hf),
                0x1FB86 => (hf * 7.0 / 8.0, hf),
                _ => (hf, hf),
            };
            let y = (hf - y_from_bottom).round().max(0.0) as u32;
            let rect_w = (rect_w.round().max(1.0)) as u32;
            let rect_h = (rect_h.round().max(1.0)) as u32;
            let x = match cp {
                0x2590 => (wf / 2.0) as u32,
                // Right-anchored: `▕` and the five `1FB87`-`1FB8B` (`builtin_font.rs:444`).
                0x2595 | 0x1FB87..=0x1FB8B => w.saturating_sub(rect_w),
                _ => 0,
            };
            fill(&mut buf, (w, h), (x, y, rect_w, rect_h), SOLID);
        }
    }
    Some(buf)
}

/// Paint an axis-aligned rectangle of coverage, clipped to the bitmap (alacritty's `draw_rect`
/// clamps the far edge the same way, so a rounded-up extent never wraps onto the next row).
fn fill(buf: &mut [u8], size: (u32, u32), rect: (u32, u32, u32, u32), alpha: u8) {
    let ((w, h), (x, y, rw, rh)) = (size, rect);
    let x_end = (x + rw).min(w);
    let y_end = (y + rh).min(h);
    for row in y..y_end {
        for col in x..x_end {
            let i = ((row * w + col) * 4) as usize;
            buf[i] = 255;
            buf[i + 1] = 255;
            buf[i + 2] = 255;
            buf[i + 3] = alpha;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render into a picture: `#` = full coverage, `+` = partial, `.` = none. Read by eye against
    /// what the CHARACTER means, never recomputed the way `block_glyph` computes it.
    fn picture(cp: u32, w: u32, h: u32) -> Vec<String> {
        let buf = block_glyph(cp, w, h).expect("owned codepoint");
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| match buf[((y * w + x) * 4 + 3) as usize] {
                        0 => '.',
                        255 => '#',
                        _ => '+',
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_sextant_mask_is_derived_not_transcribed() {
        // 60 codepoints, 64 combinations, four omitted: `000000` (space), `111111` (`█`), and the two
        // that would duplicate `▌` / `▐`. Cross-checked against every one of alacritty's 180 literals.
        assert_eq!(sextant_mask(0x1FB00), Some(0b000001)); // BLOCK SEXTANT-1  (top-left)
        assert_eq!(sextant_mask(0x1FB01), Some(0b000010)); // BLOCK SEXTANT-2  (top-right)
        assert_eq!(sextant_mask(0x1FB02), Some(0b000011)); // BLOCK SEXTANT-12 (upper third)
        assert_eq!(sextant_mask(0x1FB03), Some(0b000100)); // BLOCK SEXTANT-3  (middle-left)
        // Past the first omission: `1FB14`'s index is 20, and mask 21 (`▌`) was skipped.
        assert_eq!(sextant_mask(0x1FB14), Some(0b010110));
        // Past the second: `1FB28`'s index is 40, and mask 42 (`▐`) was skipped.
        assert_eq!(sextant_mask(0x1FB28), Some(0b101011));
        assert_eq!(sextant_mask(0x1FB3B), Some(0b111110)); // the last one
        assert_eq!(sextant_mask(0x1FB3C), None);
        assert_eq!(sextant_mask(0x2588), None);
        // The two omitted masks never appear, and every mask appears once.
        let all: Vec<u8> = (SEXTANT_FIRST..=SEXTANT_LAST)
            .map(|c| sextant_mask(c).unwrap())
            .collect();
        assert_eq!(all.len(), 60);
        assert!(!all.contains(&0b010101) && !all.contains(&0b101010));
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 60, "no mask is used twice");
        // Each of the six cells is lit by exactly half of them — alacritty's six lists have 30 each.
        for bit in 0..6 {
            assert_eq!(
                all.iter().filter(|m| *m >> bit & 1 == 1).count(),
                30,
                "bit {bit}"
            );
        }
    }

    #[test]
    fn the_range_this_module_owns_is_exactly_the_block_elements() {
        assert!(
            block_glyph(0x257F, 8, 8).is_none(),
            "box drawing is a later slice"
        );
        assert!(block_glyph(0x2580, 8, 8).is_some());
        assert!(block_glyph(0x259F, 8, 8).is_some());
        assert!(block_glyph(0x25A0, 8, 8).is_none());
        assert!(block_glyph(0x41, 8, 8).is_none(), "'A' belongs to the font");
        // A degenerate cell has no pixels to fill; the caller must not be handed an empty bitmap.
        assert!(block_glyph(0x2588, 0, 8).is_none());
    }

    #[test]
    fn the_full_block_fills_every_pixel_of_the_cell() {
        // This is the whole point of #359: `█` defines the cell, so it must cover it. At
        // `lineHeight = 1.5` the font's `█` covers only its ink box and leaves a seam.
        assert_eq!(picture(0x2588, 4, 6), ["####"; 6]);
        // ...at any cell the spacing policy produces, including a very tall one.
        assert_eq!(picture(0x2588, 2, 3), ["##", "##", "##"]);
    }

    #[test]
    fn the_halves_meet_exactly_in_the_middle() {
        // `▀` upper half, `▄` lower half: together they must be the full block, with no row shared
        // and none missed.
        assert_eq!(picture(0x2580, 4, 4), ["####", "####", "....", "...."]);
        assert_eq!(picture(0x2584, 4, 4), ["....", "....", "####", "####"]);
        // `▌` left half, `▐` right half — same, along x.
        assert_eq!(picture(0x258C, 4, 2), ["##..", "##.."]);
        assert_eq!(picture(0x2590, 4, 2), ["..##", "..##"]);
    }

    #[test]
    fn the_eighths_count_from_the_edge_the_character_names() {
        // `▁` LOWER ONE EIGHTH BLOCK: one row of eight, at the BOTTOM.
        assert_eq!(picture(0x2581, 2, 8).last().unwrap(), "##");
        assert_eq!(picture(0x2581, 2, 8)[0], "..");
        // `▔` UPPER ONE EIGHTH BLOCK: one row, at the TOP.
        assert_eq!(picture(0x2594, 2, 8)[0], "##");
        assert_eq!(picture(0x2594, 2, 8).last().unwrap(), "..");
        // `▇` LOWER SEVEN EIGHTHS: everything but the top row.
        assert_eq!(picture(0x2587, 2, 8)[0], "..");
        assert_eq!(picture(0x2587, 2, 8)[1], "##");
        // `▏` LEFT ONE EIGHTH: one column, at the LEFT. `▕` RIGHT ONE EIGHTH: one column, right.
        assert_eq!(picture(0x258F, 8, 1)[0], "#.......");
        assert_eq!(picture(0x2595, 8, 1)[0], ".......#");
    }

    #[test]
    fn the_quadrants_are_named_by_the_corners_they_light() {
        // `▖` QUADRANT LOWER LEFT.
        assert_eq!(picture(0x2596, 2, 2), ["..", "#."]);
        // `▝` QUADRANT UPPER RIGHT.
        assert_eq!(picture(0x259D, 2, 2), [".#", ".."]);
        // `▚` QUADRANT UPPER LEFT AND LOWER RIGHT — the diagonal pair.
        assert_eq!(picture(0x259A, 2, 2), ["#.", ".#"]);
        // `▙` UPPER LEFT AND LOWER LEFT AND LOWER RIGHT: everything but the upper right.
        assert_eq!(picture(0x2599, 2, 2), ["#.", "##"]);
        // `▟` UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT.
        assert_eq!(picture(0x259F, 2, 2), [".#", "##"]);
    }

    #[test]
    fn the_four_quadrants_of_a_cell_reassemble_into_the_full_block() {
        // Independent of any hand-drawn picture: whatever the rounding does, `▘▝▖▗` must cover the
        // cell exactly once between them, or a quadrant-drawn TUI border leaves a seam. Checked on an
        // odd cell, where the centre rounds and the four rectangles are NOT the same size.
        for (w, h) in [(2u32, 2u32), (3, 3), (5, 2), (2, 5), (9, 7)] {
            let mut union = vec![0u8; (w * h) as usize];
            for cp in [0x2598, 0x259D, 0x2596, 0x2597] {
                let g = block_glyph(cp, w, h).unwrap();
                for (i, u) in union.iter_mut().enumerate() {
                    let a = g[i * 4 + 3];
                    assert!(a == 0 || a == 255, "a quadrant is solid or absent");
                    *u += u8::from(a == 255);
                }
            }
            assert!(
                union.iter().all(|&n| n == 1),
                "{w}x{h}: every pixel lit by exactly one quadrant, got {union:?}"
            );
        }
    }

    #[test]
    fn a_sextant_lights_the_cells_of_the_2x3_mosaic_its_name_gives() {
        // Rows are equal thirds; on a 2x3 bitmap each mosaic cell is one pixel.
        assert_eq!(picture(0x1FB00, 2, 3), ["#.", "..", ".."]); // SEXTANT-1  upper left
        assert_eq!(picture(0x1FB01, 2, 3), [".#", "..", ".."]); // SEXTANT-2  upper right
        assert_eq!(picture(0x1FB02, 2, 3), ["##", "..", ".."]); // SEXTANT-12 upper third
        assert_eq!(picture(0x1FB03, 2, 3), ["..", "#.", ".."]); // SEXTANT-3  middle left
        assert_eq!(picture(0x1FB3B, 2, 3), [".#", "##", "##"]); // SEXTANT-23456
        // `1FB0F` is SEXTANT-5 — the lower left. The mask derivation puts it there without a table.
        assert_eq!(picture(0x1FB0F, 2, 3), ["..", "..", "#."]);
    }

    #[test]
    fn the_six_sextant_cells_reassemble_into_the_full_block() {
        // The same invariant the quadrants keep, and the one that catches a rounding seam: the six
        // single-cell sextants (masks 1, 2, 4, 8, 16, 32) must cover every pixel exactly once. Sizes
        // where `h/3` and `w/2` both round, and where they do not.
        // Masks 1, 2, 4, 8, 16, 32 sit at indices 0, 1, 3, 7, 15, 30 of the filtered enumeration —
        // 30, not 31, because mask 21 (`▌`) is skipped before it. The `count_ones` guard below is
        // what caught me writing `1FB06` (mask 7, three cells) for mask 8.
        const SINGLE: [u32; 6] = [0x1FB00, 0x1FB01, 0x1FB03, 0x1FB07, 0x1FB0F, 0x1FB1E];
        for cp in SINGLE {
            assert_eq!(
                sextant_mask(cp).unwrap().count_ones(),
                1,
                "{cp:#x} is a single cell"
            );
        }
        for (w, h) in [(2u32, 3u32), (3, 3), (8, 16), (9, 17), (5, 4), (10, 7)] {
            let mut union = vec![0u8; (w * h) as usize];
            for cp in SINGLE {
                let g = block_glyph(cp, w, h).unwrap();
                for (i, u) in union.iter_mut().enumerate() {
                    *u += u8::from(g[i * 4 + 3] == 255);
                }
            }
            assert!(
                union.iter().all(|&n| n == 1),
                "{w}x{h}: every pixel lit by exactly one sextant cell, got {union:?}"
            );
        }
    }

    #[test]
    fn the_extra_eighth_blocks_measure_from_the_edge_their_names_give() {
        // `U+2580`-`U+259F` has no 2/8, 3/8, 5/8, 6/8 or 7/8 block measured from the TOP, nor from the
        // RIGHT. `1FB82`-`1FB8B` fill both gaps, and both references draw them.
        assert_eq!(picture(0x1FB82, 1, 8)[0], "#"); // UPPER ONE QUARTER: two rows, from the top
        assert_eq!(picture(0x1FB82, 1, 8)[1], "#");
        assert_eq!(picture(0x1FB82, 1, 8)[2], ".");
        assert_eq!(picture(0x1FB86, 1, 8)[6], "#"); // UPPER SEVEN EIGHTHS: all but the last row
        assert_eq!(picture(0x1FB86, 1, 8)[7], ".");
        // RIGHT ONE QUARTER: two columns, from the RIGHT.
        assert_eq!(picture(0x1FB87, 8, 1)[0], "......##");
        // RIGHT SEVEN EIGHTHS: all but the first column.
        assert_eq!(picture(0x1FB8B, 8, 1)[0], ".#######");
        // And they tile with their `2580`-`259F` complements: `▂` (lower 2/8) + `1FB86` (upper 7/8)
        // overlap by one row on an 8-row cell, but `1FB82` (upper 2/8) + `▆` (lower 6/8) meet exactly.
        let upper = block_glyph(0x1FB82, 1, 8).unwrap();
        let lower = block_glyph(0x2586, 1, 8).unwrap();
        for row in 0..8 {
            let a = upper[row * 4 + 3] == 255;
            let b = lower[row * 4 + 3] == 255;
            assert!(a ^ b, "row {row}: exactly one of `1FB82` / `▆` lights it");
        }
    }

    #[test]
    fn the_shades_are_a_flat_coverage_not_a_dither() {
        // alacritty fills the cell with a constant alpha (`COLOR_FILL_ALPHA_STEP_*`); the terminal's
        // foreground colour then shows through at that strength. A dither would moire against the
        // pixel grid at fractional DPRs.
        let alpha = |cp: u32| block_glyph(cp, 3, 3).unwrap()[3];
        assert_eq!(alpha(0x2591), 64); // ░
        assert_eq!(alpha(0x2592), 128); // ▒
        assert_eq!(alpha(0x2593), 192); // ▓
        assert_eq!(alpha(0x2588), 255); // █
        // Flat: every pixel carries the same alpha.
        let buf = block_glyph(0x2592, 3, 3).unwrap();
        assert!(buf.chunks_exact(4).all(|px| px[3] == 128));
    }

    #[test]
    fn a_rounded_up_extent_is_clipped_rather_than_wrapping_onto_the_next_row() {
        // An odd cell makes `w/2` fractional. `▐`'s rectangle rounds UP to 2 while its origin
        // truncates to 1, so it would run one pixel past the right edge and reappear on the left of
        // the next row. alacritty clamps the far edge for the same reason (`draw_rect`).
        assert_eq!(picture(0x2590, 3, 2), [".##", ".##"]);
        // And the quadrant centres are rounded, so they never leave an unlit seam down the middle.
        assert_eq!(picture(0x2588, 3, 3), ["###", "###", "###"]);
        // On an odd cell the centre rounds UP: `round(3/2) = 2`, so the upper half is two rows and
        // the lower one. `▀` splits the same way (`round(1.5) = 2` rows), so halves and quadrants
        // agree — which is the only thing that matters, since they must tile with each other.
        assert_eq!(picture(0x2580, 3, 3), ["###", "###", "..."]);
        assert_eq!(picture(0x259F, 3, 3), ["..#", "..#", "###"]);
    }
}
