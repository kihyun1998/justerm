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

/// The block-element codepoint range. Box drawing (`U+2500`–`U+257F`) is a sibling family with its
/// own [`box_glyph`] path (the [`BOX_ARMS`] table for straight lines, plus dashes, doubles and
/// diagonals); only its rounded corners (`256D`-`2570`) remain a later slice (#365).
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

/// A white RGBA bitmap of `w * h` device px with the coverage of a block element, sextant, extra
/// eighth block, or box-drawing glyph (delegated to [`box_glyph`]) in alpha — or `None` for a
/// codepoint this module does not own.
///
/// The origin is the cell's TOP-left, matching the rasteriser's canvas and the shader's texcoord.
pub fn block_glyph(cp: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    // Box drawing (its straight-line core, #365) is a sibling family drawn from strokes, not block
    // fractions; it owns its own codepoints and returns early.
    if let Some(g) = box_glyph(cp, w, h) {
        return Some(g);
    }
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

/// The straight-line core of box drawing: horizontal / vertical lines, corners, T-junctions and the
/// cross, in light and heavy weights, plus the mixed-weight terminals. Ranges `2500`-`2503`,
/// `250C`-`254B`, `2574`-`257F`. Each is up to four strokes — left, right, up, down — meeting at the
/// cell centre; a stroke is absent, light, or heavy. Dashes, doubles, diagonals and rounded corners
/// are the tail (still tracked on #365) and are NOT owned here.
///
/// `[left, right, up, down]` weight per codepoint: `0` no arm, `1` light, `2` heavy. Generated
/// mechanically from alacritty's four stroke-arm match arms (`builtin_font.rs:162-216`) rather than
/// hand-transcribed — copying ~200 literals invites a plausible-forever typo (#363's lesson) — and
/// re-checked against the character meaning by the tests. Ordered by codepoint for binary search.
#[rustfmt::skip]
const BOX_ARMS: [(u32, [u8; 4]); 80] = [
    (0x2500, [1, 1, 0, 0]), (0x2501, [2, 2, 0, 0]), (0x2502, [0, 0, 1, 1]), (0x2503, [0, 0, 2, 2]),
    (0x250C, [0, 1, 0, 1]), (0x250D, [0, 2, 0, 1]), (0x250E, [0, 1, 0, 2]), (0x250F, [0, 2, 0, 2]),
    (0x2510, [1, 0, 0, 1]), (0x2511, [2, 0, 0, 1]), (0x2512, [1, 0, 0, 2]), (0x2513, [2, 0, 0, 2]),
    (0x2514, [0, 1, 1, 0]), (0x2515, [0, 2, 1, 0]), (0x2516, [0, 1, 2, 0]), (0x2517, [0, 2, 2, 0]),
    (0x2518, [1, 0, 1, 0]), (0x2519, [2, 0, 1, 0]), (0x251A, [1, 0, 2, 0]), (0x251B, [2, 0, 2, 0]),
    (0x251C, [0, 1, 1, 1]), (0x251D, [0, 2, 1, 1]), (0x251E, [0, 1, 2, 1]), (0x251F, [0, 1, 1, 2]),
    (0x2520, [0, 1, 2, 2]), (0x2521, [0, 2, 2, 1]), (0x2522, [0, 2, 1, 2]), (0x2523, [0, 2, 2, 2]),
    (0x2524, [1, 0, 1, 1]), (0x2525, [2, 0, 1, 1]), (0x2526, [1, 0, 2, 1]), (0x2527, [1, 0, 1, 2]),
    (0x2528, [1, 0, 2, 2]), (0x2529, [2, 0, 2, 1]), (0x252A, [2, 0, 1, 2]), (0x252B, [2, 0, 2, 2]),
    (0x252C, [1, 1, 0, 1]), (0x252D, [2, 1, 0, 1]), (0x252E, [1, 2, 0, 1]), (0x252F, [2, 2, 0, 1]),
    (0x2530, [1, 1, 0, 2]), (0x2531, [2, 1, 0, 2]), (0x2532, [1, 2, 0, 2]), (0x2533, [2, 2, 0, 2]),
    (0x2534, [1, 1, 1, 0]), (0x2535, [2, 1, 1, 0]), (0x2536, [1, 2, 1, 0]), (0x2537, [2, 2, 1, 0]),
    (0x2538, [1, 1, 2, 0]), (0x2539, [2, 1, 2, 0]), (0x253A, [1, 2, 2, 0]), (0x253B, [2, 2, 2, 0]),
    (0x253C, [1, 1, 1, 1]), (0x253D, [2, 1, 1, 1]), (0x253E, [1, 2, 1, 1]), (0x253F, [2, 2, 1, 1]),
    (0x2540, [1, 1, 2, 1]), (0x2541, [1, 1, 1, 2]), (0x2542, [1, 1, 2, 2]), (0x2543, [2, 1, 2, 1]),
    (0x2544, [1, 2, 2, 1]), (0x2545, [2, 1, 1, 2]), (0x2546, [1, 2, 1, 2]), (0x2547, [2, 2, 2, 1]),
    (0x2548, [2, 2, 1, 2]), (0x2549, [2, 1, 2, 2]), (0x254A, [1, 2, 2, 2]), (0x254B, [2, 2, 2, 2]),
    (0x2574, [1, 0, 0, 0]), (0x2575, [0, 0, 1, 0]), (0x2576, [0, 1, 0, 0]), (0x2577, [0, 0, 0, 1]),
    (0x2578, [2, 0, 0, 0]), (0x2579, [0, 0, 2, 0]), (0x257A, [0, 2, 0, 0]), (0x257B, [0, 0, 0, 2]),
    (0x257C, [1, 2, 0, 0]), (0x257D, [0, 0, 1, 2]), (0x257E, [2, 1, 0, 0]), (0x257F, [0, 0, 2, 1]),
];

/// A white RGBA bitmap of the box-drawing glyph for `cp`, or `None` for a codepoint outside the
/// straight-line core [`BOX_ARMS`] owns (or a degenerate cell).
///
/// The stroke width is alacritty's: `max(round(cell_w / 8), 1)` device px, heavy = twice that
/// (`builtin_font.rs:53,977`). Each arm is drawn as a rectangle centred on the cell midline, its
/// thickness snapped to whole pixels — a fractional-midline 1px line would blur under the atlas's
/// texture filtering. A horizontal arm's length runs to the far edge of the vertical strokes (and
/// vice-versa), so a corner's two arms meet and a run of `─` is unbroken across the cell seam.
fn box_glyph(cp: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    if (0x2571..=0x2573).contains(&cp) {
        return box_diagonal(cp, w, h);
    }
    if let Some(g) = box_dash(cp, w, h) {
        return Some(g);
    }
    if (0x2550..=0x256C).contains(&cp) {
        return box_double(cp, w, h);
    }
    let [left, right, up, down] = BOX_ARMS
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map(|i| BOX_ARMS[i].1)?;
    if w == 0 || h == 0 {
        return None;
    }
    let stroke = ((w as f32 / 8.0).round() as u32).max(1);
    let size = |wt: u8| match wt {
        1 => stroke,
        2 => stroke * 2,
        _ => 0,
    };
    let (sh_l, sh_r, sv_u, sv_d) = (size(left), size(right), size(up), size(down));

    let x_center = w as f32 / 2.0;
    let y_center = h as f32 / 2.0;
    // The whole-pixel span of a horizontal stroke of thickness `s` centred on the vertical midline,
    // and of a vertical stroke centred on the horizontal midline. Snapping to `u32` here is what keeps
    // a 1px line off a fractional midline, where texture filtering would blur it.
    let h_bounds = |s: u32| -> (u32, u32) {
        let s = s as f32;
        (
            (y_center - s / 2.0).max(0.0) as u32,
            ((y_center + s / 2.0) as u32).min(h),
        )
    };
    let v_bounds = |s: u32| -> (u32, u32) {
        let s = s as f32;
        (
            (x_center - s / 2.0).max(0.0) as u32,
            ((x_center + s / 2.0) as u32).min(w),
        )
    };

    let (vu0, vu1) = v_bounds(sv_u);
    let (vd0, vd1) = v_bounds(sv_d);
    let (hl0, hl1) = h_bounds(sh_l);
    let (hr0, hr1) = h_bounds(sh_r);

    let mut buf = vec![0u8; (w * h * 4) as usize];
    // Each arm runs from the cell edge to the FAR side of the perpendicular strokes, so a corner's
    // two arms overlap at the centre and adjacent cells join (alacritty `builtin_font.rs:226-242`).
    //
    // The left/up arm length is the far edge of the perpendicular strokes, which collapses to
    // `floor(centre) = 0` on a 1px cell that has no perpendicular arm — leaving a left/up terminal
    // (`╴ ╸ ╵ ╹`) invisible where its right/down mirror (`╶ ╷`, sized from `w - x` / `h - y`) shows.
    // A present arm lights at least one pixel, matching the block glyphs' `.max(1)` and the sibling
    // invariant that a glyph is never blank on a 1px cell. Only w/h = 1 is affected.
    if sh_l > 0 {
        fill(
            &mut buf,
            (w, h),
            (0, hl0, vu1.max(vd1).max(1), hl1 - hl0),
            SOLID,
        );
    }
    if sh_r > 0 {
        let x = vu0.min(vd0);
        fill(&mut buf, (w, h), (x, hr0, w - x, hr1 - hr0), SOLID);
    }
    if sv_u > 0 {
        fill(
            &mut buf,
            (w, h),
            (vu0, 0, vu1 - vu0, hl1.max(hr1).max(1)),
            SOLID,
        );
    }
    if sv_d > 0 {
        let y = hl0.min(hr0);
        fill(&mut buf, (w, h), (vd0, y, vd1 - vd0, h - y), SOLID);
    }
    Some(buf)
}

/// The box-drawing diagonals `╱ ╲ ╳` (`2571`-`2573`), drawn as anti-aliased bands over
/// [`fill_polygon`] — its first consumer. alacritty draws them as Xiaolin Wu *lines* on a canvas
/// grown into the neighbouring cells for a seamless join (`builtin_font.rs:60-106`); an atlas glyph
/// cannot spill past its cell, so each band instead OVERSHOOTS its corners by half a stroke and is
/// clipped back — meeting the diagonally-adjacent cell's band at the shared corner. `╳` is the two
/// bands max-combined into one buffer, so the crossing is not double-counted.
///
/// The band is a **true perpendicular** stroke of width `stroke` at any cell aspect — a deliberate
/// divergence from alacritty, whose Wu-line loop offsets the line *vertically*, so its diagonals thin
/// to `stroke·cosθ` and read lighter than the straight box lines on a tall cell. A constant
/// perpendicular width keeps a `╱` the same visual weight as a `─` or `│`, which matters more for a
/// line-drawing family than reproducing that reference artefact.
fn box_diagonal(cp: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    let (wf, hf) = (w as f32, h as f32);
    let half = (wf / 8.0).round().max(1.0) / 2.0; // half the box-line stroke width
    let mut buf = vec![0u8; (w * h * 4) as usize];
    // A band of half-width `half` around the segment A->B, its ends pushed out along the line by
    // `half` so the corner pixel is covered and the neighbour cell's band meets it there.
    let mut band = |ax: f32, ay: f32, bx: f32, by: f32| {
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let (ux, uy) = (dx / len, dy / len); // unit along the line
        let (nx, ny) = (-uy * half, ux * half); // perpendicular, half-thickness
        let (ax, ay) = (ax - ux * half, ay - uy * half); // overshoot both ends
        let (bx, by) = (bx + ux * half, by + uy * half);
        fill_polygon(
            &mut buf,
            (w, h),
            &[
                (ax + nx, ay + ny),
                (bx + nx, by + ny),
                (bx - nx, by - ny),
                (ax - nx, ay - ny),
            ],
            SOLID,
        );
    };
    if cp == 0x2571 || cp == 0x2573 {
        band(0.0, hf, wf, 0.0); // ╱ bottom-left to top-right
    }
    if cp == 0x2572 || cp == 0x2573 {
        band(0.0, 0.0, wf, hf); // ╲ top-left to bottom-right
    }
    Some(buf)
}

/// The dashed box lines — double/triple/quadruple dash, horizontal and vertical, light and heavy
/// (`2504`-`250B`, `254C`-`254F`), or `None` for a codepoint outside that set. A line of `num_gaps+1`
/// dashes with `span/8`-px gaps, centred on the midline and clipped to the cell — alacritty's
/// `builtin_font.rs:111-152`. The dash *count* is the character's, read straight off its name
/// (DOUBLE/TRIPLE/QUADRUPLE dash), and the weight is light or heavy (2x stroke).
fn box_dash(cp: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    // (horizontal?, num_gaps, heavy?). num_gaps+1 dashes: 1 gap = double, 2 = triple, 3 = quadruple.
    let (horizontal, num_gaps, heavy) = match cp {
        0x2504 => (true, 2, false),  // ┄ triple dash light
        0x2505 => (true, 2, true),   // ┅ triple dash heavy
        0x2508 => (true, 3, false),  // ┈ quadruple dash light
        0x2509 => (true, 3, true),   // ┉ quadruple dash heavy
        0x254C => (true, 1, false),  // ╌ double dash light
        0x254D => (true, 1, true),   // ╍ double dash heavy
        0x2506 => (false, 2, false), // ┆ triple dash vertical light
        0x2507 => (false, 2, true),  // ┇
        0x250A => (false, 3, false), // ┊ quadruple dash vertical light
        0x250B => (false, 3, true),  // ┋
        0x254E => (false, 1, false), // ╎ double dash vertical light
        0x254F => (false, 1, true),  // ╏
        _ => return None,
    };
    if w == 0 || h == 0 {
        return None;
    }
    let stroke = ((w as f32 / 8.0).round() as u32).max(1) * if heavy { 2 } else { 1 };
    let span = if horizontal { w } else { h };
    // The gap is `span/8`; the dashes share the remainder. `max(1)` keeps a dash visible on a tiny
    // cell, as alacritty's `cmp::max(…, 1)` does.
    let dash_gap = (span / 8).max(1);
    let dash_len = (span.saturating_sub(dash_gap * num_gaps) / (num_gaps + 1)).max(1);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    if horizontal {
        let yc = h as f32 / 2.0;
        let (y0, y1) = (
            (yc - stroke as f32 / 2.0).max(0.0) as u32,
            ((yc + stroke as f32 / 2.0) as u32).min(h),
        );
        for gap in 0..=num_gaps {
            let x = (gap * (dash_len + dash_gap)).min(w);
            fill(&mut buf, (w, h), (x, y0, dash_len, y1 - y0), SOLID);
        }
    } else {
        let xc = w as f32 / 2.0;
        let (x0, x1) = (
            (xc - stroke as f32 / 2.0).max(0.0) as u32,
            ((xc + stroke as f32 / 2.0) as u32).min(w),
        );
        for gap in 0..=num_gaps {
            let y = (gap * (dash_len + dash_gap)).min(h);
            fill(&mut buf, (w, h), (x0, y, x1 - x0, dash_len), SOLID);
        }
    }
    Some(buf)
}

/// The double-line box components — `═ ║` and the double/single-mixed corners and junctions
/// (`2550`-`256C`), or `None` outside that range. A faithful port of alacritty's double arm
/// (`builtin_font.rs:247-348`): up to two horizontal rails (at `h_lines.0`/`.1`) and two vertical
/// rails (at `v_lines.0`/`.1`), each split into halves whose lengths are chosen per codepoint so a
/// double meets a single (`╞ ╤ …`) or a double (`╬`) without a gap. Every rail is a single stroke.
fn box_double(cp: u32, w: u32, h: u32) -> Option<Vec<u8>> {
    if !(0x2550..=0x256C).contains(&cp) || w == 0 || h == 0 {
        return None;
    }
    let stroke = ((w as f32 / 8.0).round() as u32).max(1);
    let (wf, hf) = (w as f32, h as f32);
    let (xc, yc) = (wf / 2.0, hf / 2.0);
    let s2 = stroke as f32 / 2.0;
    // alacritty's `v_line_bounds` / `h_line_bounds`: integer-truncated stroke extents, cell-clamped.
    let vb = |x: f32| -> (f32, f32) { ((x - s2).max(0.0).floor(), (x + s2).floor().min(wf)) };
    let hb = |y: f32| -> (f32, f32) { ((y - s2).max(0.0).floor(), (y + s2).floor().min(hf)) };

    // Where the two vertical rails (left, right) and two horizontal rails (top, bottom) sit. A
    // "single" arm on that axis collapses both rails onto the centre; a double offsets them by ±1.
    let v_lines = match cp {
        0x2552 | 0x2555 | 0x2558 | 0x255B | 0x255E | 0x2561 | 0x2564 | 0x2567 | 0x256A => (xc, xc),
        _ => {
            let b = vb(xc);
            ((b.0 - 1.0).max(0.0), (b.1 + 1.0).min(wf))
        }
    };
    let h_lines = match cp {
        0x2553 | 0x2556 | 0x2559 | 0x255C | 0x255F | 0x2562 | 0x2565 | 0x2568 | 0x256B => (yc, yc),
        _ => {
            let b = hb(yc);
            ((b.0 - 1.0).max(0.0), (b.1 + 1.0).min(hf))
        }
    };

    let vl = vb(v_lines.0); // left vertical rail extent
    let vr = vb(v_lines.1); // right vertical rail extent
    let ht = hb(h_lines.0); // top horizontal rail extent
    let hbo = hb(h_lines.1); // bottom horizontal rail extent

    // Left halves of the two horizontal rails (they start at x = 0).
    let (top_left_size, bot_left_size) = match cp {
        0x2550 | 0x256B => (xc, xc),
        0x2555..=0x2557 => (vr.1, vl.1),
        0x255B..=0x255D => (vl.1, vr.1),
        0x2561..=0x2563 | 0x256A | 0x256C => (vl.1, vl.1),
        0x2564..=0x2568 => (xc, vl.1),
        0x2569 => (vl.1, xc),
        _ => (0.0, 0.0),
    };
    // Right halves of the two horizontal rails (they start at these x and run to the width).
    let (top_right_x, bot_right_x, right_size) = match cp {
        0x2550 | 0x2565 | 0x256B => (xc, xc, wf),
        0x2552..=0x2554 | 0x2568 => (vl.0, vr.0, wf),
        0x2558..=0x255A => (vr.0, vl.0, wf),
        0x255E..=0x2560 | 0x256A | 0x256C => (vr.0, vr.0, wf),
        0x2564 | 0x2566 => (xc, vr.0, wf),
        0x2567 | 0x2569 => (vr.0, xc, wf),
        _ => (0.0, 0.0, 0.0),
    };
    // Top halves of the two vertical rails (they start at y = 0).
    let (left_top_size, right_top_size) = match cp {
        0x2551 | 0x256A => (yc, yc),
        0x2558..=0x255C | 0x2568 => (hbo.1, ht.1),
        0x255D => (ht.1, hbo.1),
        0x255E..=0x2560 => (yc, ht.1),
        0x2561..=0x2563 => (ht.1, yc),
        0x2567 | 0x2569 | 0x256B | 0x256C => (ht.1, ht.1),
        _ => (0.0, 0.0),
    };
    // Bottom halves of the two vertical rails (they start at these y and run to the height).
    let (left_bot_y, right_bot_y, bottom_size) = match cp {
        0x2551 | 0x256A => (yc, yc, hf),
        0x2552..=0x2554 => (ht.0, hbo.0, hf),
        0x2555..=0x2557 => (hbo.0, ht.0, hf),
        0x255E..=0x2560 => (yc, hbo.0, hf),
        0x2561..=0x2563 => (hbo.0, yc, hf),
        0x2564..=0x2566 | 0x256B | 0x256C => (hbo.0, hbo.0, hf),
        _ => (0.0, 0.0, 0.0),
    };

    let mut buf = vec![0u8; (w * h * 4) as usize];
    // A horizontal rail segment from (x, rail y) of length `size`; and a vertical one. Both mirror
    // alacritty's `draw_h_line`/`draw_v_line`: the perpendicular extent is the stroke bounds, the far
    // end is truncated then clamped to the cell.
    let draw_h = |buf: &mut [u8], x: f32, y: f32, size: f32| {
        let (y0, y1) = hb(y);
        let (x0, x1) = (x as u32, ((x + size) as u32).min(w));
        if x1 > x0 && y1 > y0 {
            fill(
                buf,
                (w, h),
                (x0, y0 as u32, x1 - x0, y1 as u32 - y0 as u32),
                SOLID,
            );
        }
    };
    let draw_v = |buf: &mut [u8], x: f32, y: f32, size: f32| {
        let (x0, x1) = vb(x);
        let (y0, y1) = (y as u32, ((y + size) as u32).min(h));
        if x1 > x0 && y1 > y0 {
            fill(
                buf,
                (w, h),
                (x0 as u32, y0, x1 as u32 - x0 as u32, y1 - y0),
                SOLID,
            );
        }
    };

    draw_h(&mut buf, 0.0, h_lines.0, top_left_size);
    draw_h(&mut buf, 0.0, h_lines.1, bot_left_size);
    draw_h(&mut buf, top_right_x, h_lines.0, right_size);
    draw_h(&mut buf, bot_right_x, h_lines.1, right_size);
    draw_v(&mut buf, v_lines.0, 0.0, left_top_size);
    draw_v(&mut buf, v_lines.1, 0.0, right_top_size);
    draw_v(&mut buf, v_lines.0, left_bot_y, bottom_size);
    draw_v(&mut buf, v_lines.1, right_bot_y, bottom_size);
    Some(buf)
}

/// Vertical sub-scanlines per output row for the polygon fill's coverage anti-aliasing. Horizontal
/// coverage is computed analytically (exact span overlap per sub-row), so only the vertical axis is
/// sampled; four sub-rows suffice at cell scale (16-33 device px) and the glyph rasterises once into
/// the atlas, so the cost never reaches a hot path.
const POLY_SS: u32 = 4;

/// Fill a simple polygon — a single closed ring of cell-local vertices, in device px with the cell's
/// TOP-left as origin — with coverage-based anti-aliasing, clipped to the cell, `alpha` in the
/// interior and scaled by coverage at the edges.
///
/// Neither reference hands us this as a primitive: alacritty anti-aliases its *diagonals* with
/// Xiaolin Wu's *line* algorithm (`builtin_font.rs:818`) and fills only rectangles, and xterm.js
/// fills polygons through Canvas2D `ctx.fill()` (`CustomGlyphRasterizer.ts:287`), which delegates the
/// coverage rule to the browser. So the area-coverage rule is ours: a scanline fill, exact in x and
/// supersampled in y (`POLY_SS` sub-rows). Vertices are `f32` so a slope need not land on a pixel
/// boundary.
///
/// The interior/exterior test is **even-odd**. It is correct for every shape this backs not because
/// those shapes avoid self-intersection — xterm draws `1FB9A`/`1FB9B` as single-ring *bowties* that
/// touch at the cell centre — but because none has two *overlapping loops of equal winding* (a
/// pentagram), the one topology where even-odd and non-zero diverge. Pass a seamless shape as ONE
/// ring (concave or self-touching is fine, as the `1FB9A` bowties are); do not split it across calls.
///
/// Coverage is **max-combined** into the alpha channel, matching alacritty's brighter-wins
/// `put_pixel` (`builtin_font.rs:807`). Max bounds overlap to 255 and is right for genuinely
/// overlapping parts (two crossing strokes), but it does **not** merge two polygons that *abut* at a
/// fractional edge in one buffer — each paints ~half of the boundary pixel and `max` keeps only one
/// half, so a seam remains. Complementary halves reassemble only across *separate* cells, where their
/// coverage sums optically over the cell boundary (the tiling #365/#366 rely on — proven by
/// `complementary_triangles_partition_the_cell_with_no_gap_or_overlap`). [`fill`], the rectangle fast
/// path, *overwrites* rather than max-combining, so mixing the two in one buffer is draw-order
/// dependent; keep them to disjoint regions.
///
/// A degenerate ring (fewer than three vertices, zero area, or entirely outside the cell) lights
/// nothing rather than panicking.
fn fill_polygon(buf: &mut [u8], size: (u32, u32), verts: &[(f32, f32)], alpha: u8) {
    let (w, h) = size;
    if w == 0 || h == 0 || verts.len() < 3 {
        return; // no area to fill
    }
    let ss = POLY_SS as f32;
    let weight = 1.0 / ss;
    // Per-row horizontal coverage, reused across rows so the fill allocates once.
    let mut cov = vec![0f32; w as usize];

    for py in 0..h {
        cov.iter_mut().for_each(|c| *c = 0.0);
        for s in 0..POLY_SS {
            // The sub-scanline's y, at the sub-row centre — a midpoint rule, which integrates a
            // linear span length exactly, so a triangle's coverage is unbiased.
            let yline = py as f32 + (s as f32 + 0.5) / ss;

            // x where each edge crosses this scanline. The half-open `<=` test counts an edge iff the
            // scanline separates its endpoints, so a vertex shared by two edges is crossed once, never
            // twice; a horizontal edge (both endpoints on the same side) is skipped.
            let mut xs: Vec<f32> = Vec::with_capacity(verts.len());
            for (i, &(x0, y0)) in verts.iter().enumerate() {
                let (x1, y1) = verts[(i + 1) % verts.len()];
                if (y0 <= yline) != (y1 <= yline) {
                    let t = (yline - y0) / (y1 - y0);
                    xs.push(x0 + t * (x1 - x0));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(f32::total_cmp);

            // Even-odd: the interior is between consecutive crossing pairs. Each span adds analytic
            // horizontal coverage — a boundary pixel gets the fraction of its width the span covers,
            // clipped to the cell.
            for pair in xs.chunks_exact(2) {
                let xa = pair[0].max(0.0);
                let xb = pair[1].min(w as f32);
                if xb <= xa {
                    continue;
                }
                let start = xa.floor() as u32;
                let end = (xb.ceil() as u32).min(w);
                for col in start..end {
                    let l = xa.max(col as f32);
                    let r = xb.min(col as f32 + 1.0);
                    if r > l {
                        cov[col as usize] += (r - l) * weight;
                    }
                }
            }
        }

        for px in 0..w {
            let c = cov[px as usize].min(1.0);
            if c <= 0.0 {
                continue;
            }
            let a = (c * alpha as f32).round() as u8;
            let i = ((py * w + px) * 4) as usize;
            if a > buf[i + 3] {
                buf[i] = 255;
                buf[i + 1] = 255;
                buf[i + 2] = 255;
                buf[i + 3] = a;
            }
        }
    }
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
    fn the_range_this_module_owns_is_the_block_elements_plus_box_drawing_core() {
        // Block elements.
        assert!(block_glyph(0x2580, 8, 8).is_some());
        assert!(block_glyph(0x259F, 8, 8).is_some());
        assert!(block_glyph(0x25A0, 8, 8).is_none());
        // Box-drawing straight-line core is now owned (#365) — the terminals at the top of the range.
        assert!(block_glyph(0x2500, 8, 8).is_some(), "─ light horizontal");
        assert!(
            block_glyph(0x257F, 8, 8).is_some(),
            "╿ mixed-weight terminal"
        );
        // Diagonals, dashes and doubles are now owned (#365 tail).
        assert!(block_glyph(0x2571, 8, 8).is_some(), "╱ diagonal");
        assert!(block_glyph(0x2573, 8, 8).is_some(), "╳ cross diagonal");
        assert!(block_glyph(0x2504, 8, 8).is_some(), "┄ triple dash");
        assert!(
            block_glyph(0x254F, 8, 8).is_some(),
            "╏ heavy double-dash vertical"
        );
        assert!(block_glyph(0x2550, 8, 8).is_some(), "═ double horizontal");
        assert!(block_glyph(0x256C, 8, 8).is_some(), "╬ double cross");
        // The rest of the box tail stays unowned: rounded corners.
        assert!(
            block_glyph(0x256D, 8, 8).is_none(),
            "╭ rounded — later slice"
        );
        assert!(block_glyph(0x41, 8, 8).is_none(), "'A' belongs to the font");
        // A degenerate cell has no pixels to fill; the caller must not be handed an empty bitmap.
        assert!(block_glyph(0x2588, 0, 8).is_none());
        assert!(
            block_glyph(0x2500, 0, 8).is_none(),
            "box on a zero cell is None too"
        );
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

    // --- polygon scanline fill primitive (#364) ---

    /// A fresh cell with one polygon painted into it, for reading alpha directly.
    fn poly(w: u32, h: u32, verts: &[(f32, f32)], alpha: u8) -> Vec<u8> {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        fill_polygon(&mut buf, (w, h), verts, alpha);
        buf
    }

    fn alpha_at(buf: &[u8], w: u32, x: u32, y: u32) -> u8 {
        buf[((y * w + x) * 4 + 3) as usize]
    }

    fn total_alpha(buf: &[u8]) -> u64 {
        buf.chunks_exact(4).map(|p| p[3] as u64).sum()
    }

    #[test]
    fn a_cell_sized_rectangle_polygon_fills_every_pixel_like_fill_does() {
        // The primitive must subsume `fill()`'s rectangle: a ring on the four cell corners covers the
        // whole cell at full alpha, white RGBA, so box drawing / wedges can be data over one path.
        let g = poly(4, 5, &[(0., 0.), (4., 0.), (4., 5.), (0., 5.)], 255);
        assert!(
            g.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
            "a cell-sized polygon is a solid white fill"
        );
    }

    #[test]
    fn a_right_triangle_covers_half_the_cell_within_tolerance() {
        // #364's named acceptance: coverage read from GEOMETRY, not from how the code computes it. The
        // upper-left right triangle has area w*h/2, so its total alpha is w*h/2*255 up to the
        // per-pixel rounding of the diagonal boundary (well under 1% of a full cell).
        let (w, h) = (16u32, 16u32);
        let g = poly(w, h, &[(0., 0.), (w as f32, 0.), (0., h as f32)], 255);
        let expect = (w * h) as i64 * 255 / 2;
        let tol = (w * h) as i64 * 255 / 100; // 1 % of a fully-lit cell
        let got = total_alpha(&g) as i64;
        assert!(
            (got - expect).abs() <= tol,
            "half-cell triangle: total alpha {got}, expected ~{expect} (±{tol})"
        );
    }

    #[test]
    fn the_diagonal_edge_carries_partial_coverage_rather_than_a_hard_step() {
        // The whole reason for the primitive over a rect table: a diagonal at cell scale must be
        // anti-aliased. At least one pixel on the hypotenuse is partially covered (0 < a < 255).
        let g = poly(16, 16, &[(0., 0.), (16., 0.), (0., 16.)], 255);
        assert!(
            g.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255),
            "the diagonal must produce partial-coverage pixels, not only 0/255"
        );
    }

    #[test]
    fn a_triangular_half_lights_the_corner_its_name_gives() {
        // "triangular upper-left half": the top-left corner pixel is inside, the bottom-right is out.
        // Asserted from the character's MEANING, never recomputed the way `fill_polygon` computes it.
        let (w, h) = (8u32, 8u32);
        let g = poly(w, h, &[(0., 0.), (w as f32, 0.), (0., h as f32)], 255);
        assert_eq!(
            alpha_at(&g, w, 0, 0),
            255,
            "top-left corner is inside the half"
        );
        assert_eq!(
            alpha_at(&g, w, w - 1, h - 1),
            0,
            "bottom-right corner is outside the half"
        );
    }

    #[test]
    fn a_concave_polygon_leaves_its_notch_empty() {
        // A "U": a rectangular slot cut from the top centre. A scanline through the slot crosses the
        // ring FOUR times, so even-odd pairing must leave the middle span dark. A convex-only fill (or
        // "inside = an odd crossing count from the left" done wrong) floods the slot — the col-4 slot
        // pixels are the ones that catch it, since they are fully inside the slot at every sub-row.
        let (w, h) = (8u32, 8u32);
        let u = &[
            (0., 0.),
            (3., 0.),
            (3., 5.),
            (5., 5.),
            (5., 0.),
            (8., 0.),
            (8., 8.),
            (0., 8.),
        ];
        let g = poly(w, h, u, 255);
        assert_eq!(alpha_at(&g, w, 4, 2), 0, "the slot is empty");
        assert_eq!(alpha_at(&g, w, 1, 2), 255, "the left arm is filled");
        assert_eq!(alpha_at(&g, w, 6, 2), 255, "the right arm is filled");
        assert_eq!(alpha_at(&g, w, 4, 6), 255, "below the slot is filled");
    }

    #[test]
    fn coverage_scales_the_requested_alpha_not_just_solid() {
        // The interior carries the requested alpha (so a shade wedge is possible), and the diagonal's
        // partial pixels are strictly below it — coverage multiplies alpha, it does not clamp to 255.
        let g = poly(16, 16, &[(0., 0.), (16., 0.), (0., 16.)], 128);
        assert!(
            g.chunks_exact(4).all(|p| p[3] <= 128),
            "no pixel exceeds the requested alpha"
        );
        assert_eq!(
            alpha_at(&g, 16, 0, 0),
            128,
            "the interior is the requested alpha"
        );
        assert!(
            g.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 128),
            "the edge is a fraction of the requested alpha"
        );
    }

    #[test]
    fn overlapping_polygons_are_max_combined_not_summed() {
        // Two solid polygons overlapping must not add: the shared region stays at the alpha, not
        // double it (which would clamp to 255). Matches alacritty's brighter-wins `put_pixel`.
        let (w, h) = (8u32, 8u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        fill_polygon(
            &mut buf,
            (w, h),
            &[(0., 0.), (8., 0.), (8., 8.), (0., 8.)],
            200,
        );
        fill_polygon(
            &mut buf,
            (w, h),
            &[(0., 0.), (4., 0.), (4., 8.), (0., 8.)],
            200,
        );
        assert!(
            buf.chunks_exact(4).all(|p| p[3] == 200),
            "the overlap is max-combined, so it stays at 200"
        );
    }

    #[test]
    fn a_polygon_is_clipped_to_the_cell_and_never_wraps() {
        // A ring poking past the right edge is clamped, not wrapped onto the next row. One that ends
        // up entirely outside lights nothing; one that straddles fills only its in-cell part.
        let (w, h) = (8u32, 8u32);
        let outside = poly(w, h, &[(10., 0.), (20., 0.), (20., 8.), (10., 8.)], 255);
        assert_eq!(
            total_alpha(&outside),
            0,
            "a polygon outside the cell lights nothing"
        );

        let straddle = poly(w, h, &[(0., 0.), (20., 0.), (20., 8.), (0., 8.)], 255);
        assert!(
            straddle.chunks_exact(4).all(|p| p[3] == 255),
            "the in-cell part of a straddling rectangle fills the whole cell, no wrap"
        );
    }

    /// Independent coverage oracle: even-odd ray-cast point-in-polygon at a dense 16x16 grid per
    /// pixel, clipped to the cell by sampling only in-cell points. A *different* algorithm from the
    /// scanline span fill (a point test, not an edge integral), so agreement is real cross-validation,
    /// not the same computation checking itself.
    fn oracle_total_alpha(w: u32, h: u32, verts: &[(f32, f32)], alpha: u8) -> f64 {
        const N: u32 = 16;
        let inside = |x: f32, y: f32| -> bool {
            let mut c = false;
            for (i, &(x0, y0)) in verts.iter().enumerate() {
                let (x1, y1) = verts[(i + 1) % verts.len()];
                if (y0 > y) != (y1 > y) {
                    let xint = x0 + (y - y0) / (y1 - y0) * (x1 - x0);
                    if x < xint {
                        c = !c;
                    }
                }
            }
            c
        };
        let mut total = 0f64;
        for py in 0..h {
            for px in 0..w {
                let mut hits = 0u32;
                for sy in 0..N {
                    for sx in 0..N {
                        let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                        let y = py as f32 + (sy as f32 + 0.5) / N as f32;
                        if inside(x, y) {
                            hits += 1;
                        }
                    }
                }
                total += hits as f64 / (N * N) as f64 * alpha as f64;
            }
        }
        total
    }

    #[test]
    fn the_fill_matches_an_independent_area_oracle() {
        // Real round-trip for a consumer-less pure primitive: the actual `fill_polygon` output is
        // cross-checked against a point-sampling oracle over several shapes — a rotated triangle, the
        // concave slot, a convex pentagon, and one straddling the edge (so clipping is exercised too).
        type PolyCase = (u32, u32, Vec<(f32, f32)>);
        let cases: &[PolyCase] = &[
            (24, 20, vec![(2., 1.), (22., 5.), (7., 19.)]),
            (
                16,
                16,
                vec![
                    (0., 0.),
                    (6., 0.),
                    (6., 10.),
                    (10., 10.),
                    (10., 0.),
                    (16., 0.),
                    (16., 16.),
                    (0., 16.),
                ],
            ),
            (
                18,
                18,
                vec![(9., 1.), (16., 7.), (13., 16.), (5., 16.), (2., 7.)],
            ),
            (12, 12, vec![(-4., -3.), (16., 2.), (6., 15.)]), // straddles top/left/right
            // A self-intersecting single-ring bowtie — xterm's `1FB9A` topology, two triangles
            // touching at the centre. Exercises the span logic on a ring the "simple polygon"
            // assumption would exclude; the oracle's point test agrees only if even-odd is right.
            (
                16,
                16,
                vec![
                    (0., 0.),
                    (8., 8.),
                    (0., 16.),
                    (16., 16.),
                    (8., 8.),
                    (16., 0.),
                ],
            ),
        ];
        for (w, h, verts) in cases {
            let mut buf = vec![0u8; (w * h * 4) as usize];
            fill_polygon(&mut buf, (*w, *h), verts, 255);
            let got = total_alpha(&buf) as f64;
            let want = oracle_total_alpha(*w, *h, verts, 255);
            // Two sampling schemes disagree only on boundary pixels; 2 % of a fully-lit cell bounds it.
            let tol = (*w * *h) as f64 * 255.0 * 0.02;
            assert!(
                (got - want).abs() <= tol,
                "{w}x{h}: fill total {got:.0}, oracle {want:.0} (±{tol:.0})"
            );
        }
    }

    #[test]
    fn complementary_triangles_partition_the_cell_with_no_gap_or_overlap() {
        // The diagonal analog of the quadrant/sextant reassembly invariant, and the one that proves
        // the fill tiles: the upper-left and lower-right triangles share the cell's diagonal, so their
        // per-pixel coverage must SUM to a full cell — no seam, no double-coverage — even where the
        // diagonal cuts a pixel and each side is only partially covered. (A single triangle pinches at
        // its vertices, so no column is solid; it is the SUM that reconstructs the block.) Checked on
        // odd cells, where the diagonal lands off the pixel grid.
        for (w, h) in [(8u32, 8u32), (9, 7), (16, 16), (5, 11)] {
            let ul = poly(w, h, &[(0., 0.), (w as f32, 0.), (0., h as f32)], 255);
            let lr = poly(
                w,
                h,
                &[(w as f32, 0.), (w as f32, h as f32), (0., h as f32)],
                255,
            );
            for y in 0..h {
                for x in 0..w {
                    let sum = alpha_at(&ul, w, x, y) as i32 + alpha_at(&lr, w, x, y) as i32;
                    // Coverages sum to 1.0 analytically. The shared diagonal's intersection x is
                    // computed from each triangle's own edge, so the two f32 results differ by up to a
                    // ULP; that plus two independent roundings leaves the sum in 255±1. A real gap or
                    // double-cover would throw a boundary pixel off by ~128, far outside this band.
                    assert!(
                        (254..=256).contains(&sum),
                        "{w}x{h} at ({x},{y}): halves sum to {sum}, not a full cell"
                    );
                }
            }
        }
    }

    #[test]
    fn tiny_and_thin_cells_fill_without_panicking() {
        // The sibling glyphs are tested down to 1x1 / 2x2 (quadrants) and 1xN / Nx1 (eighths); the
        // polygon path must survive the same degenerate cell sizes a spacing policy can produce.
        assert_eq!(
            alpha_at(
                &poly(1, 1, &[(0., 0.), (1., 0.), (1., 1.), (0., 1.)], 255),
                1,
                0,
                0
            ),
            255
        );
        // A triangle on a 2x2 cell: its top-left corner is solid and it lights something, no panic.
        let g = poly(2, 2, &[(0., 0.), (2., 0.), (0., 2.)], 255);
        assert_eq!(alpha_at(&g, 2, 0, 0), 255);
        assert!(total_alpha(&g) > 0);
        // A one-pixel-tall / one-pixel-wide cell.
        assert!(total_alpha(&poly(8, 1, &[(0., 0.), (8., 0.), (0., 1.)], 255)) > 0);
        assert!(total_alpha(&poly(1, 8, &[(0., 0.), (1., 0.), (0., 8.)], 255)) > 0);
    }

    #[test]
    fn collinear_and_duplicate_vertices_are_harmless() {
        // Vertex lists transcribed from a reference may carry a duplicated point or a redundant
        // collinear one. They must not change the filled area: a triangle with a midpoint spelled on
        // one edge, and one with a repeated vertex, match the clean triangle byte for byte.
        let clean = poly(16, 16, &[(0., 0.), (16., 0.), (0., 16.)], 255);
        let collinear = poly(16, 16, &[(0., 0.), (8., 0.), (16., 0.), (0., 16.)], 255);
        let duplicate = poly(16, 16, &[(0., 0.), (0., 0.), (16., 0.), (0., 16.)], 255);
        assert_eq!(collinear, clean, "a collinear midpoint changes nothing");
        assert_eq!(duplicate, clean, "a duplicated vertex changes nothing");
    }

    #[test]
    fn abutting_polygons_in_one_buffer_seam_by_design() {
        // Pinning the documented limitation so it is visible, not a latent surprise: two complementary
        // triangles drawn into the SAME buffer do NOT reassemble — max-combine keeps only one ~half of
        // each shared diagonal pixel, so the seam pixels land near half alpha, not 255. The seamless
        // path is a single ring (or separate cells); this test exists to prove the seam is a known,
        // deliberate consequence of max-combine, not an accident.
        let (w, h) = (16u32, 16u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        fill_polygon(
            &mut buf,
            (w, h),
            &[(0., 0.), (w as f32, 0.), (0., h as f32)],
            255,
        );
        fill_polygon(
            &mut buf,
            (w, h),
            &[(w as f32, 0.), (w as f32, h as f32), (0., h as f32)],
            255,
        );
        // A pixel the diagonal bisects (x + y == w) is left at partial alpha, well under solid.
        let seam = alpha_at(&buf, w, (w / 2) - 1, h / 2);
        assert!(
            (64..200).contains(&seam),
            "the shared diagonal seams to partial alpha ({seam}), not a full 255"
        );
        // Off the diagonal, both interiors are solid — the seam is confined to the shared edge.
        assert_eq!(alpha_at(&buf, w, 0, 0), 255);
        assert_eq!(alpha_at(&buf, w, w - 1, h - 1), 255);
    }

    #[test]
    fn a_degenerate_polygon_lights_nothing_rather_than_panicking() {
        // Fewer than three vertices, or a zero-size cell: no area to fill, and the caller must never
        // be handed a panic or a stray pixel.
        let mut empty = vec![0u8; 0];
        fill_polygon(&mut empty, (0, 8), &[(0., 0.), (1., 1.), (2., 0.)], 255);

        let two = poly(8, 8, &[(1., 1.), (6., 6.)], 255);
        assert_eq!(total_alpha(&two), 0, "a two-vertex ring has no area");

        let none = poly(8, 8, &[], 255);
        assert_eq!(total_alpha(&none), 0, "an empty ring has no area");
    }

    // --- box drawing straight-line core (#365) ---

    fn box_g(cp: u32, w: u32, h: u32) -> Vec<u8> {
        block_glyph(cp, w, h).expect("owned box codepoint")
    }
    fn col_lit(g: &[u8], w: u32, h: u32, x: u32) -> bool {
        (0..h).any(|y| g[((y * w + x) * 4 + 3) as usize] > 0)
    }
    fn row_lit(g: &[u8], w: u32, _h: u32, y: u32) -> bool {
        // `_h` keeps the call sites symmetric with `col_lit`; a row scan only needs the width.
        (0..w).any(|x| g[((y * w + x) * 4 + 3) as usize] > 0)
    }

    #[test]
    fn every_box_arm_reaches_exactly_the_edge_its_name_gives() {
        // The join guarantee, read from each character's MEANING (its `[left,right,up,down]` arms),
        // not from how the code draws it: an arm exists IFF that cell edge has lit pixels. So a run of
        // `─` is unbroken across the seam (both left and right edges lit), and a corner reaches only
        // its two neighbours. Checked across all 80 owned codepoints on a comfortable cell.
        let (w, h) = (16u32, 16u32);
        for &(cp, [l, r, u, d]) in BOX_ARMS.iter() {
            let g = box_g(cp, w, h);
            assert_eq!(col_lit(&g, w, h, 0), l > 0, "{cp:#06X}: left edge vs L={l}");
            assert_eq!(
                col_lit(&g, w, h, w - 1),
                r > 0,
                "{cp:#06X}: right edge vs R={r}"
            );
            assert_eq!(row_lit(&g, w, h, 0), u > 0, "{cp:#06X}: top edge vs U={u}");
            assert_eq!(
                row_lit(&g, w, h, h - 1),
                d > 0,
                "{cp:#06X}: bottom edge vs D={d}"
            );
        }
    }

    #[test]
    fn a_light_horizontal_is_a_centred_bar_clear_of_top_and_bottom() {
        // `─`: a horizontal bar at mid-height spanning the whole width, with the top and bottom of the
        // cell empty (so it does not smear into the rows above/below).
        let (w, h) = (16u32, 16u32);
        let g = box_g(0x2500, w, h);
        assert!(row_lit(&g, w, h, h / 2), "the mid row is lit");
        assert!(
            !row_lit(&g, w, h, 0) && !row_lit(&g, w, h, h - 1),
            "top/bottom clear"
        );
        // The lit mid row runs edge to edge.
        assert!(
            (0..w).all(|x| g[(((h / 2) * w + x) * 4 + 3) as usize] > 0),
            "full width"
        );
    }

    #[test]
    fn heavy_strokes_are_twice_the_light_stroke() {
        // `┃` (heavy vertical) is twice as thick as `│` (light). Count lit columns across the mid row.
        let (w, h) = (16u32, 16u32);
        let light = box_g(0x2502, w, h);
        let heavy = box_g(0x2503, w, h);
        let thickness = |g: &[u8]| {
            (0..w)
                .filter(|&x| g[(((h / 2) * w + x) * 4 + 3) as usize] > 0)
                .count()
        };
        assert_eq!(thickness(&heavy), 2 * thickness(&light), "heavy = 2x light");
        assert!(thickness(&light) >= 1);
    }

    #[test]
    fn a_mixed_weight_terminal_is_thicker_on_its_heavy_side() {
        // `╼` = light left, heavy right (`257C` = L1 R2). The right arm's bar is thicker than the
        // left's — proving the per-arm weight is honoured, not a single stroke for the whole glyph.
        let (w, h) = (16u32, 16u32);
        let g = box_g(0x257C, w, h);
        let bar = |x: u32| {
            (0..h)
                .filter(|&y| g[((y * w + x) * 4 + 3) as usize] > 0)
                .count()
        };
        assert!(bar(1) >= 1 && bar(w - 2) >= 1, "both arms present");
        assert!(
            bar(w - 2) > bar(1),
            "the heavy right arm is thicker than the light left arm"
        );
    }

    #[test]
    fn box_drawing_stays_text_presentation_not_emoji() {
        // #365 must not perturb emoji classification: box drawing is text presentation, so the emoji
        // gate never fires for it (the range is nowhere near the 1F000+ plane it keys on).
        for s in ["─", "│", "┼", "╋", "╿", "╱", "╲", "╳", "┄", "╍", "═", "╬"]
        {
            assert!(
                !crate::emoji::is_emoji_text(s, false),
                "{s} is not emoji (narrow)"
            );
            assert!(
                !crate::emoji::is_emoji_text(s, true),
                "{s} is not emoji (wide)"
            );
        }
    }

    /// Number of 4-connected lit components in a glyph bitmap.
    fn lit_components(g: &[u8], w: u32, h: u32) -> u32 {
        let lit = |x: u32, y: u32| g[((y * w + x) * 4 + 3) as usize] > 0;
        let mut seen = vec![false; (w * h) as usize];
        let mut count = 0u32;
        for sy in 0..h {
            for sx in 0..w {
                if !lit(sx, sy) || seen[(sy * w + sx) as usize] {
                    continue;
                }
                count += 1;
                let mut stack = vec![(sx, sy)];
                while let Some((x, y)) = stack.pop() {
                    let i = (y * w + x) as usize;
                    if seen[i] || !lit(x, y) {
                        continue;
                    }
                    seen[i] = true;
                    if x > 0 {
                        stack.push((x - 1, y));
                    }
                    if x + 1 < w {
                        stack.push((x + 1, y));
                    }
                    if y > 0 {
                        stack.push((x, y - 1));
                    }
                    if y + 1 < h {
                        stack.push((x, y + 1));
                    }
                }
            }
        }
        count
    }

    #[test]
    fn a_glyphs_arms_meet_in_one_connected_shape() {
        // The meeting rule (each arm runs to the far side of the perpendicular strokes) exists so a
        // corner or junction is a SINGLE connected shape — an arm that stopped at the midline would
        // leave its stub as a second component. Every multi-arm glyph must be one piece.
        let (w, h) = (16u32, 16u32);
        // Corners, T-junctions, the cross, all-heavy, plus the mixed-weight junctions and terminals
        // (2540-254A, 257D-257F) — the asymmetric ones where a swapped arm weight is most plausible.
        for cp in [
            0x250C, 0x2510, 0x2514, 0x2518, 0x251C, 0x2524, 0x252C, 0x2534, 0x253C, 0x254B, 0x257C,
            0x2540, 0x2541, 0x2542, 0x2543, 0x2545, 0x254A, 0x257D, 0x257E, 0x257F,
        ] {
            assert_eq!(
                lit_components(&box_g(cp, w, h), w, h),
                1,
                "{cp:#06X} is one connected shape"
            );
        }
    }

    #[test]
    fn a_terminal_lights_at_least_one_pixel_even_on_a_one_pixel_cell() {
        // `╴ ╸ ╵ ╹` (single-arm terminals) are sized from the far edge of the perpendicular strokes,
        // which collapses to 0 on a 1px cross-axis cell where their `╶ ╷` mirrors — sized from
        // `w - x` / `h - y` — stay lit. They must not vanish, matching the block glyphs' `.max(1)`.
        assert!(
            col_lit(&box_g(0x2574, 1, 8), 1, 8, 0),
            "╴ left terminal on a 1px-wide cell"
        );
        assert!(
            col_lit(&box_g(0x2578, 1, 8), 1, 8, 0),
            "╸ heavy left terminal on a 1px-wide cell"
        );
        assert!(
            row_lit(&box_g(0x2575, 8, 1), 8, 1, 0),
            "╵ up terminal on a 1px-tall cell"
        );
        assert!(
            row_lit(&box_g(0x2579, 8, 1), 8, 1, 0),
            "╹ heavy up terminal on a 1px-tall cell"
        );
        // The mirrors were already fine, but pin them so the two stay symmetric.
        assert!(col_lit(&box_g(0x2576, 1, 8), 1, 8, 0), "╶ right terminal");
        assert!(row_lit(&box_g(0x2577, 8, 1), 8, 1, 0), "╷ down terminal");
    }

    #[test]
    fn a_mixed_weight_junction_is_thicker_on_its_heavy_arm() {
        // The join test only checks an edge is lit (any weight), so it cannot catch a light↔heavy
        // swap on a junction. These assert the heavy arm is visibly thicker than its light opposite,
        // so a swapped weight in BOX_ARMS reddens rather than shipping a plausible-forever wrong glyph.
        let (w, h) = (16u32, 16u32);
        let cols_at_row = |g: &[u8], y: u32| {
            (0..w)
                .filter(|&x| g[((y * w + x) * 4 + 3) as usize] > 0)
                .count()
        };
        let rows_at_col = |g: &[u8], x: u32| {
            (0..h)
                .filter(|&y| g[((y * w + x) * 4 + 3) as usize] > 0)
                .count()
        };

        // `╁` 2541 = down HEAVY, up light: the vertical bar is thicker below the centre than above.
        let g = box_g(0x2541, w, h);
        assert!(
            cols_at_row(&g, h - 3) > cols_at_row(&g, 2),
            "╁ heavier below"
        );
        // `╿` 257F = up HEAVY, down light: thicker above.
        let g = box_g(0x257F, w, h);
        assert!(
            cols_at_row(&g, 2) > cols_at_row(&g, h - 3),
            "╿ heavier above"
        );
        // `┭` 252D = left HEAVY, right light: the horizontal bar is thicker left of centre than right.
        let g = box_g(0x252D, w, h);
        assert!(
            rows_at_col(&g, 2) > rows_at_col(&g, w - 3),
            "┭ heavier on the left"
        );
    }

    #[test]
    fn a_forward_slash_runs_from_bottom_left_to_top_right() {
        // `╱` 2571 is the anti-diagonal: lit at the two corners it touches, dark at the two it misses.
        let (w, h) = (16u32, 16u32);
        let g = box_g(0x2571, w, h);
        assert!(alpha_at(&g, w, 0, h - 1) > 0, "bottom-left corner lit");
        assert!(alpha_at(&g, w, w - 1, 0) > 0, "top-right corner lit");
        assert_eq!(alpha_at(&g, w, 0, 0), 0, "top-left corner dark");
        assert_eq!(alpha_at(&g, w, w - 1, h - 1), 0, "bottom-right corner dark");
    }

    #[test]
    fn a_backslash_runs_from_top_left_to_bottom_right() {
        // `╲` 2572 is the main diagonal — the mirror of `╱`.
        let (w, h) = (16u32, 16u32);
        let g = box_g(0x2572, w, h);
        assert!(alpha_at(&g, w, 0, 0) > 0, "top-left corner lit");
        assert!(alpha_at(&g, w, w - 1, h - 1) > 0, "bottom-right corner lit");
        assert_eq!(alpha_at(&g, w, 0, h - 1), 0, "bottom-left corner dark");
        assert_eq!(alpha_at(&g, w, w - 1, 0), 0, "top-right corner dark");
    }

    #[test]
    fn a_cross_lights_both_diagonals_and_their_meeting_point() {
        // `╳` 2573 is both bands, max-combined: all four corners and the centre are lit.
        let (w, h) = (16u32, 16u32);
        let g = box_g(0x2573, w, h);
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert!(alpha_at(&g, w, x, y) > 0, "corner ({x},{y}) lit");
        }
        assert!(alpha_at(&g, w, w / 2, h / 2) > 0, "the crossing is lit");
        // The two bands are max-combined into one buffer (fill_polygon's rule, proven not to
        // double-count by `overlapping_polygons_are_max_combined_not_summed`); here the crossing is
        // simply lit, drawn by both.
    }

    #[test]
    fn a_diagonal_is_anti_aliased() {
        // The whole reason it rides fill_polygon rather than a stair-stepped rect run: its edges carry
        // partial coverage.
        let g = box_g(0x2571, 16, 16);
        assert!(
            g.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255),
            "the diagonal band has partial-coverage edge pixels"
        );
    }

    #[test]
    fn a_diagonal_band_is_about_one_box_stroke_wide() {
        // Pins the band thickness — the diagonals' analog of `heavy_strokes_are_twice_the_light_stroke`
        // for the straight lines. A band of width `stroke` over length `~sqrt(w^2+h^2)` sums to about
        // `len * stroke * 255` of coverage; halving or doubling the `/ 2.0` half-width in box_diagonal
        // moves the total far outside this tolerance (the corner present/absent tests do not — their
        // margins are too generous to feel a thickness change).
        for (w, h) in [(32u32, 32u32), (24, 40)] {
            let g = box_g(0x2571, w, h);
            let stroke = (w as f32 / 8.0).round().max(1.0);
            let len = ((w * w + h * h) as f32).sqrt();
            let expect = len * stroke * 255.0;
            let got = total_alpha(&g) as f32;
            assert!(
                (got - expect).abs() < expect * 0.30,
                "{w}x{h}: diagonal ink {got:.0}, expected ~{expect:.0} for a {stroke}px band"
            );
        }
    }

    #[test]
    fn diagonals_join_at_the_shared_corner_across_cells() {
        // Two `╱` cells stacked lower-left→upper-right share a corner: the lower cell's top-right and
        // the upper cell's bottom-left are the same physical pixel, so BOTH must be lit for the line
        // to be unbroken. (Each cell rasterises independently; the overshoot is what reaches the
        // corner.) Asserted on several cell sizes, since the band's slope changes with the aspect.
        for (w, h) in [(16u32, 16u32), (10, 20), (20, 10), (7, 7)] {
            let g = box_g(0x2571, w, h);
            assert!(
                alpha_at(&g, w, w - 1, 0) > 0,
                "{w}x{h}: top-right corner reaches the seam"
            );
            assert!(
                alpha_at(&g, w, 0, h - 1) > 0,
                "{w}x{h}: bottom-left corner reaches the seam"
            );
        }
    }

    /// The number of contiguous lit runs along a row (for a horizontal dash) or column (vertical).
    fn lit_runs_h(g: &[u8], w: u32, y: u32) -> u32 {
        let mut runs = 0;
        let mut prev = false;
        for x in 0..w {
            let lit = g[((y * w + x) * 4 + 3) as usize] > 0;
            if lit && !prev {
                runs += 1;
            }
            prev = lit;
        }
        runs
    }
    fn lit_runs_v(g: &[u8], w: u32, h: u32, x: u32) -> u32 {
        let mut runs = 0;
        let mut prev = false;
        for y in 0..h {
            let lit = g[((y * w + x) * 4 + 3) as usize] > 0;
            if lit && !prev {
                runs += 1;
            }
            prev = lit;
        }
        runs
    }

    /// The dash count = the run count on the busiest row/column (the one the dash line sits on;
    /// which exact row/column depends on the stroke's pixel-snapping, so scan rather than assume).
    fn dash_count_h(g: &[u8], w: u32, h: u32) -> u32 {
        (0..h).map(|y| lit_runs_h(g, w, y)).max().unwrap_or(0)
    }
    fn dash_count_v(g: &[u8], w: u32, h: u32) -> u32 {
        (0..w).map(|x| lit_runs_v(g, w, h, x)).max().unwrap_or(0)
    }

    #[test]
    fn a_dash_has_the_number_of_segments_its_name_gives() {
        // Double/triple/quadruple counted as contiguous runs — for ALL twelve dashes, light AND
        // heavy, both axes, so a swapped `num_gaps` (e.g. the heavy triple `┅` drawn as a quadruple)
        // reddens. Read from the character's meaning, on a long cell where the gaps are unambiguous.
        let (w, h) = (32u32, 8u32);
        for (cp, n) in [
            (0x254Cu32, 2u32),
            (0x254D, 2),
            (0x2504, 3),
            (0x2505, 3),
            (0x2508, 4),
            (0x2509, 4),
        ] {
            assert_eq!(
                dash_count_h(&box_g(cp, w, h), w, h),
                n,
                "{cp:#06X} horizontal dash count"
            );
        }
        let (w, h) = (8u32, 32u32);
        for (cp, n) in [
            (0x254Eu32, 2u32),
            (0x254F, 2),
            (0x2506, 3),
            (0x2507, 3),
            (0x250A, 4),
            (0x250B, 4),
        ] {
            assert_eq!(
                dash_count_v(&box_g(cp, w, h), w, h),
                n,
                "{cp:#06X} vertical dash count"
            );
        }
    }

    #[test]
    fn a_dash_is_centred_on_the_midline_and_clear_of_the_edges() {
        // A horizontal dash sits at mid-height with the top and bottom rows empty (it is a broken
        // line, not a fill).
        let (w, h) = (32u32, 8u32);
        let g = box_g(0x2504, w, h);
        assert!(row_lit(&g, w, h, h / 2), "mid row has dashes");
        assert!(
            !row_lit(&g, w, h, 0) && !row_lit(&g, w, h, h - 1),
            "top/bottom clear"
        );
    }

    #[test]
    fn heavy_dashes_are_thicker_than_light_dashes() {
        // Every heavy dash is twice the stroke of its light sibling — all six pairs, both axes — so a
        // flipped `heavy` flag on any one reddens. Thickness measured across the first dash.
        let (wf, hf) = (32u32, 8u32); // horizontal: thickness is the lit rows at x=1
        let h_thick = |cp| {
            (0..hf)
                .filter(|&y| box_g(cp, wf, hf)[((y * wf + 1) * 4 + 3) as usize] > 0)
                .count()
        };
        for (light, heavy) in [(0x2504, 0x2505), (0x2508, 0x2509), (0x254C, 0x254D)] {
            assert_eq!(
                h_thick(heavy),
                2 * h_thick(light),
                "{heavy:#06X} = 2x {light:#06X}"
            );
        }
        let (wv, hv) = (8u32, 32u32); // vertical: thickness is the lit cols at y=1
        let v_thick = |cp| {
            (0..wv)
                .filter(|&x| box_g(cp, wv, hv)[((wv + x) * 4 + 3) as usize] > 0)
                .count()
        };
        for (light, heavy) in [(0x2506, 0x2507), (0x250A, 0x250B), (0x254E, 0x254F)] {
            assert_eq!(
                v_thick(heavy),
                2 * v_thick(light),
                "{heavy:#06X} = 2x {light:#06X}"
            );
        }
    }

    #[test]
    fn a_double_line_is_two_parallel_rails_spanning_the_cell() {
        // `═` is two horizontal rails with a gap: a vertical cut crosses two runs, and each rail spans
        // the full width. `║` is the transpose.
        let (w, h) = (16u32, 16u32);
        let g = box_g(0x2550, w, h);
        assert_eq!(dash_count_v(&g, w, h), 2, "═ is two horizontal rails");
        assert!(
            (0..h).any(|y| (0..w).all(|x| g[((y * w + x) * 4 + 3) as usize] > 0)),
            "═ has a full-width rail"
        );
        let g = box_g(0x2551, w, h);
        assert_eq!(dash_count_h(&g, w, h), 2, "║ is two vertical rails");
        assert!(
            (0..w).any(|x| (0..h).all(|y| g[((y * w + x) * 4 + 3) as usize] > 0)),
            "║ has a full-height rail"
        );
    }

    #[test]
    fn every_double_arm_has_the_single_or_double_rail_count_its_name_gives() {
        // The strong double invariant, read from each character's name: the number of rails crossing
        // each edge — 0 absent, 1 SINGLE arm, 2 DOUBLE arm. This pins the single-vs-double distinction
        // (the thing that makes a double a double) for every arm of every codepoint; presence alone
        // could not tell `╒`'s single down from `╓`'s double down. `[left, right, up, down]`.
        // Rail count at an edge = the run count on that edge line: a horizontal double crosses the
        // left/right edge as two vertical runs, a single as one.
        #[rustfmt::skip]
        let cases: &[(u32, [u32; 4])] = &[
            (0x2550, [2, 2, 0, 0]), (0x2551, [0, 0, 2, 2]),          // ═ ║
            (0x2552, [0, 2, 0, 1]), (0x2553, [0, 1, 0, 2]), (0x2554, [0, 2, 0, 2]), // ╒ ╓ ╔
            (0x2555, [2, 0, 0, 1]), (0x2556, [1, 0, 0, 2]), (0x2557, [2, 0, 0, 2]), // ╕ ╖ ╗
            (0x2558, [0, 2, 1, 0]), (0x2559, [0, 1, 2, 0]), (0x255A, [0, 2, 2, 0]), // ╘ ╙ ╚
            (0x255B, [2, 0, 1, 0]), (0x255C, [1, 0, 2, 0]), (0x255D, [2, 0, 2, 0]), // ╛ ╜ ╝
            (0x255E, [0, 2, 1, 1]), (0x255F, [0, 1, 2, 2]), (0x2560, [0, 2, 2, 2]), // ╞ ╟ ╠
            (0x2561, [2, 0, 1, 1]), (0x2562, [1, 0, 2, 2]), (0x2563, [2, 0, 2, 2]), // ╡ ╢ ╣
            (0x2564, [2, 2, 0, 1]), (0x2565, [1, 1, 0, 2]), (0x2566, [2, 2, 0, 2]), // ╤ ╥ ╦
            (0x2567, [2, 2, 1, 0]), (0x2568, [1, 1, 2, 0]), (0x2569, [2, 2, 2, 0]), // ╧ ╨ ╩
            (0x256A, [2, 2, 1, 1]), (0x256B, [1, 1, 2, 2]), (0x256C, [2, 2, 2, 2]), // ╪ ╫ ╬
        ];
        let (w, h) = (16u32, 16u32);
        for &(cp, [l, r, u, d]) in cases {
            let g = box_g(cp, w, h);
            assert_eq!(lit_runs_v(&g, w, h, 0), l, "{cp:#06X} left rails");
            assert_eq!(lit_runs_v(&g, w, h, w - 1), r, "{cp:#06X} right rails");
            assert_eq!(lit_runs_h(&g, w, 0), u, "{cp:#06X} top rails");
            assert_eq!(lit_runs_h(&g, w, h - 1), d, "{cp:#06X} bottom rails");
        }
    }

    #[test]
    fn a_double_arm_shows_two_rails_at_its_edge_a_single_arm_one() {
        // What makes a double a double: `═` crosses its left/right edge as TWO rails, where a single
        // `─` crosses as one. `╞`'s right (double) edge shows two, its vertical (single) rail one.
        let (w, h) = (20u32, 16u32);
        assert_eq!(
            lit_runs_v(&box_g(0x2550, w, h), w, h, 0),
            2,
            "═ left edge = two rails"
        );
        assert_eq!(
            lit_runs_v(&box_g(0x2500, w, h), w, h, 0),
            1,
            "─ left edge = one rail"
        );
        // `╞` 255E: right edge is a double (two rails), and it has a single vertical (one rail on a
        // horizontal cut clear of the branch).
        let g = box_g(0x255E, w, h);
        assert_eq!(lit_runs_v(&g, w, h, w - 1), 2, "╞ right edge = two rails");
        assert_eq!(
            lit_runs_h(&g, w, 0),
            1,
            "╞ top: the single vertical crosses as one rail"
        );
    }

    #[test]
    fn tiny_cells_draw_box_glyphs_without_panicking() {
        // Whatever a spacing policy produces, a box glyph must survive a 1x1 / 2x2 cell — the
        // diagonals over fill_polygon included (a sub-pixel band must not panic or overflow the buf) —
        // AND never come out blank: every one of these crosses the cell, so it lights at least one
        // pixel (the invariant the block glyphs and the 1px-terminal fix hold).
        for (w, h) in [(1u32, 1u32), (2, 2), (1, 8), (8, 1)] {
            for cp in [
                0x2500u32, 0x2502, 0x253C, 0x254B, 0x257F, 0x2571, 0x2572, 0x2573, 0x2504, 0x2506,
                0x254F, 0x2550, 0x2551, 0x2554, 0x256C,
            ] {
                let g = block_glyph(cp, w, h).expect("owned");
                assert_eq!(g.len(), (w * h * 4) as usize);
                assert!(
                    total_alpha(&g) > 0,
                    "{cp:#06X} on {w}x{h} must not be blank"
                );
            }
        }
    }
}
