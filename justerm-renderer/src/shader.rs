//! The WebGL2 shader sources the renderer links into its one program, and the compile-time
//! check that the packer's neighbour offsets are laid out the way the fragment stage reads them.

use crate::frame::{
    NEIGHBOUR_DN, NEIGHBOUR_DN_FG, NEIGHBOUR_LT, NEIGHBOUR_LT_FG, NEIGHBOUR_RT, NEIGHBOUR_RT_FG,
    NEIGHBOUR_UP, NEIGHBOUR_UP_FG,
};

// The shader reads each group of four neighbour floats as one `vec4` (up, down, left, right), so
// the packer's offsets must stay consecutive in that order.
const _: () = assert!(
    NEIGHBOUR_DN == NEIGHBOUR_UP + 1
        && NEIGHBOUR_LT == NEIGHBOUR_UP + 2
        && NEIGHBOUR_RT == NEIGHBOUR_UP + 3
        && NEIGHBOUR_DN_FG == NEIGHBOUR_UP_FG + 1
        && NEIGHBOUR_LT_FG == NEIGHBOUR_UP_FG + 2
        && NEIGHBOUR_RT_FG == NEIGHBOUR_UP_FG + 3
);

pub(crate) const VERT_SRC: &str = r#"#version 300 es
layout(location = 0) in vec2 a_pos;    // unit-quad corner (0..1) = local glyph texcoord
layout(location = 1) in vec2 a_cell;   // instance: (col, row)
layout(location = 2) in vec3 a_bg;     // instance: background rgb
layout(location = 3) in vec3 a_fg;     // instance: foreground rgb
layout(location = 4) in float a_glyph; // instance: atlas slot index
// instance: the underline's and the strikethrough's inks, packed 0xRRGGBB one per float
// (#513, #525).
layout(location = 5) in float a_underline_fg;
layout(location = 6) in float a_strike_fg;
// instance: 1.0 iff this cell's bg is the pristine default backdrop — provenance, packed by
// the Rust side (#455).
layout(location = 7) in float a_bg_default;
// instance: `I_neighbour` (ADR-0019 R1.2) — the glyph field of the cell above, below, left and
// right, and the ink each draws in, in that order; BLANK_SLOT where the packer withdrew it.
layout(location = 8) in vec4 a_glyph_nb;
layout(location = 9) in vec4 a_nb_fg;
uniform mat4 u_projection;
uniform vec2 u_cell_size;   // the GRID cell in device px
out vec3 v_bg;
out vec3 v_fg;
flat out vec3 v_underline_fg;
flat out vec3 v_strike_fg;
flat out float v_bg_default;
flat out uint v_glyph;
flat out vec2 v_cell;
out vec2 v_tex;
flat out uvec4 v_glyph_nb; // up, down, left, right
flat out uvec4 v_nb_fg;    // their inks, still packed 0xRRGGBB
void main() {
    vec2 origin = a_cell * u_cell_size;
    vec2 pos = floor(origin + a_pos * u_cell_size + 0.5); // pixel-snapped
    gl_Position = u_projection * vec4(pos, 0.0, 1.0);
    v_bg = a_bg;
    v_fg = a_fg;
    // Unpack once per instance rather than per fragment.
    uint ul = uint(a_underline_fg);
    v_underline_fg = vec3(float((ul >> 16u) & 255u), float((ul >> 8u) & 255u), float(ul & 255u)) / 255.0;
    uint st = uint(a_strike_fg);
    v_strike_fg = vec3(float((st >> 16u) & 255u), float((st >> 8u) & 255u), float(st & 255u)) / 255.0;
    v_bg_default = a_bg_default;
    v_glyph = uint(a_glyph);
    v_cell = a_cell;
    // Cell-local: the atlas slot is the padded cell (#359), so the bitmap carries the glyph at its
    // offset.
    v_tex = a_pos;
    v_glyph_nb = uvec4(a_glyph_nb);
    v_nb_fg = uvec4(a_nb_fg);
}
"#;

pub(crate) const FRAG_SRC: &str = r#"#version 300 es
precision mediump float;
uniform mediump sampler2DArray u_atlas;
// Where a cell sits inside its padded atlas slot: (origin.xy, span.xy), all 0..1 (#288, #791).
uniform vec4 u_cell_uv;
uniform float u_bg_alpha;    // background cell opacity: 0 = transparent, 1 = opaque (#298)
// `u_cell_size` is declared in both stages, so `highp` to match the vertex stage's default.
uniform highp vec2 u_cell_size;   // the grid cell in device px
uniform highp vec2 u_char_size;   // the glyph box inside it (#338) — decorations only
uniform highp vec2 u_char_offset; // where that box starts
uniform highp float u_line_thickness; // underline/strikethrough thickness in device px (#517)
// Dots per cell for a dotted underline (#830), a whole number from `metrics::dots_per_cell`.
uniform highp float u_dots_per_cell;
// The cursor (#270): (col, row, span, shape); shape 0 = none, else `shape_id + 1`
// (1 block, 2 underline, 3 bar, 4 hollow block).
uniform highp vec4 u_cursor;
uniform vec3 u_cursor_color;
uniform vec3 u_cursor_text_color;       // the glyph colour under a block (xterm's cursorAccent)
uniform highp float u_cursor_thickness; // stroke width in device px
in vec3 v_bg;
in vec3 v_fg;
flat in vec3 v_underline_fg;
flat in vec3 v_strike_fg;
flat in float v_bg_default; // 1.0 = the default backdrop (#455/#298)
flat in uint v_glyph;
flat in vec2 v_cell;
in vec2 v_tex;
flat in uvec4 v_glyph_nb; // up, down, left, right
flat in uvec4 v_nb_fg;    // their inks, packed 0xRRGGBB
// How deep a band of this cell's left/right (x) and top/bottom (y) edges may receive a
// neighbour's ink, device px (#966, #791).
uniform vec2 u_bleed_px;
// Per-channel (LCD) text coverage (#961): 0 for a grayscale configuration, else the exponent dark ink
// raises the slot's light mask to (`lcd::fit_dark_gamma`).
uniform float u_lcd_gamma;
out vec4 FragColor;
// Per-channel coverage from a subpixel slot's light mask for ink `ink` (#961): the mask as-is
// where that channel of the ink is at or above 0.75, raised to `u_lcd_gamma` below it.
vec3 lcd_cov(vec3 mask, vec3 ink) {
    return mix(pow(mask, vec3(u_lcd_gamma)), mask, step(vec3(0.75), ink));
}
// Coverage of a horizontal band at glyph-box centre `c`, `thick_px` device px thick in a glyph
// box `char_h` device px tall: full on the pixel rows it covers, a half-pixel ramp at each edge,
// the centre clamped inside the box (#515, #517).
float hline(float gy, float c, float thick_px, float char_h) {
    float th = max(thick_px, 1.0) / max(char_h, 1.0); // device-px thickness, in gy units
    float top = clamp(c - th * 0.5, 0.0, 1.0 - th);   // centre-clamp: stay in the cell
    float aa = 0.5 * fwidth(gy);
    return clamp((gy - (top - aa)) / max(aa, 1e-5), 0.0, 1.0)
         * clamp(((top + th + aa) - gy) / max(aa, 1e-5), 0.0, 1.0);
}
// Coverage of an x-axis gate: 1 inside `[lo, hi]`, with a ramp `aa` at both edges (#830); `aa`
// is the caller's, taken from the unwrapped coordinate.
float xgate(float t, float lo, float hi, float aa) {
    return clamp((t - (lo - aa)) / max(aa, 1e-5), 0.0, 1.0)
         * clamp(((hi + aa) - t) / max(aa, 1e-5), 0.0, 1.0);
}
// Which cell of the cursor's `span`-wide box is this, or -1 for a fragment outside it? Mirrors
// `cursor::covers`.
float cursor_dx() {
    if (int(u_cursor.w) == 0) return -1.0;
    if (abs(v_cell.y - u_cursor.y) > 0.5) return -1.0;
    float dx = v_cell.x - u_cursor.x;
    return (dx < -0.5 || dx > u_cursor.z - 0.5) ? -1.0 : dx;
}
// Is the cell at grid `cell` (col, row) painted by a block cursor? (#791, #966)
bool block_cursor_at(vec2 cell) {
    if (int(u_cursor.w) != 1) return false;
    if (abs(cell.y - u_cursor.y) > 0.5) return false;
    float dx = cell.x - u_cursor.x;
    return !(dx < -0.5 || dx > u_cursor.z - 0.5);
}
// The texel of atlas slot `glyph` (its low 13 bits) at slot-local texcoord `uv`, explicit LOD so it
// may be fetched outside uniform control flow. The same nudge off the texel edge as the cell's own.
vec4 slot_texel(uint glyph, vec2 uv) {
    uint slot = glyph & 0x1FFFu;
    return textureLod(u_atlas,
        vec3(uv.x + 0.001, (float(slot & 31u) + uv.y + 0.001) / 32.0, float(slot >> 5u)), 0.0);
}
// A neighbour's packed `0xRRGGBB` ink, as the colour it draws in.
vec3 unpack_rgb(uint c) {
    return vec3(float((c >> 16u) & 255u), float((c >> 8u) & 255u), float(c & 255u)) / 255.0;
}
// Does this fragment fall on a cursor stroke? Mirrors `cursor::cursor_rects` in device pixels,
// hard-edged; a block draws no stroke.
float stroke_coverage(float dx) {
    int shape = int(u_cursor.w);
    if (dx < 0.0 || shape < 2) return 0.0;
    vec2 p = v_tex * u_cell_size;                 // device px inside THIS cell
    float bx = dx * u_cell_size.x + p.x;          // device px inside the cursor's box
    float box_w = u_cursor.z * u_cell_size.x;
    float h = u_cell_size.y;
    // The same clamp `cursor_rects` applies: a stroke is never thicker than the box it outlines.
    float t = min(u_cursor_thickness, min(box_w, h));
    if (shape == 2) return p.y >= h - t ? 1.0 : 0.0;                          // underline
    // A bar's width is clamped by its own cell, not by the cell's height.
    if (shape == 3) return bx < min(u_cursor_thickness, u_cell_size.x) ? 1.0 : 0.0;
    return (p.y < t || p.y >= h - t || bx < t || bx >= box_w - t) ? 1.0 : 0.0; // hollow
}
void main() {
    // The glyph field: slot (bits 0..12), underline (13), strikethrough (14), colour emoji
    // (15, #284), ink class (16, #712), underline style (17..19, #829).
    uint slot = v_glyph & 0x1FFFu;
    uint layer = slot >> 5u;   // 32 glyphs stack vertically per layer
    uint band = slot & 31u;
    // The cell-local texcoord, inset into the padded slot's content region.
    vec2 inner = u_cell_uv.xy + v_tex * u_cell_uv.zw;
    // Nudged off the exact texel edge so NEAREST cannot round to a neighbour.
    vec3 tc = vec3(inner.x + 0.001, (float(band) + inner.y + 0.001) / 32.0, float(layer));
    vec4 texel = texture(u_atlas, tc);
    float coverage = texel.a;

    // ── I_neighbour (ADR-0019 R1.2 / rule 6, #791, #966): the adjacent cells' spilled ink ──────
    // Sampled unconditionally and masked, not branched.
    vec2 px = v_tex * u_cell_size;
    vec2 armed = step(vec2(0.5), u_bleed_px);
    // ...and the block-cursor half of rule 5's withdrawal, which the packer could not apply.
    bool me = block_cursor_at(v_cell);
    vec4 same = vec4(
        me == block_cursor_at(v_cell - vec2(0.0, 1.0)) ? 1.0 : 0.0,
        me == block_cursor_at(v_cell + vec2(0.0, 1.0)) ? 1.0 : 0.0,
        me == block_cursor_at(v_cell - vec2(1.0, 0.0)) ? 1.0 : 0.0,
        me == block_cursor_at(v_cell + vec2(1.0, 0.0)) ? 1.0 : 0.0);
    // up, down, left, right — the order of `v_glyph_nb`.
    vec4 gate = same * vec4(
        step(px.y, u_bleed_px.y) * armed.y,
        step(u_cell_size.y - u_bleed_px.y, px.y) * armed.y,
        step(px.x, u_bleed_px.x) * armed.x,
        step(u_cell_size.x - u_bleed_px.x, px.x) * armed.x);

    vec4 tex_up = slot_texel(v_glyph_nb.x, inner + vec2(0.0, u_cell_uv.w));
    vec4 tex_dn = slot_texel(v_glyph_nb.y, inner - vec2(0.0, u_cell_uv.w));
    vec4 tex_lt = slot_texel(v_glyph_nb.z, inner + vec2(u_cell_uv.z, 0.0));
    vec4 tex_rt = slot_texel(v_glyph_nb.w, inner - vec2(u_cell_uv.z, 0.0));
    vec4 cov_nb = vec4(tex_up.a, tex_dn.a, tex_lt.a, tex_rt.a) * gate;

    // The owner: the neighbour laying down the most, the earlier in up-down-left-right order on a
    // tie.
    uint owner = v_glyph_nb.x;
    uint owner_fg = v_nb_fg.x;
    vec4 owner_tex = tex_up;
    float owner_gate = gate.x;
    float foreign = cov_nb.x;
    if (cov_nb.y > foreign) {
        owner = v_glyph_nb.y; owner_fg = v_nb_fg.y; owner_tex = tex_dn; owner_gate = gate.y;
        foreign = cov_nb.y;
    }
    if (cov_nb.z > foreign) {
        owner = v_glyph_nb.z; owner_fg = v_nb_fg.z; owner_tex = tex_lt; owner_gate = gate.z;
        foreign = cov_nb.z;
    }
    if (cov_nb.w > foreign) {
        owner = v_glyph_nb.w; owner_fg = v_nb_fg.w; owner_tex = tex_rt; owner_gate = gate.w;
        foreign = cov_nb.w;
    }

    // Foreign ink keeps its owner's ink, including the owner's emoji bit.
    vec3 foreign_ink = mix(unpack_rgb(owner_fg), owner_tex.rgb, float((owner >> 15u) & 1u));
    // ── end I_neighbour ───────────────────────────────────────────────────────────────────────

    // A block cursor recolours the cell before anything composites over it.
    float dx = cursor_dx();
    bool block = dx >= 0.0 && int(u_cursor.w) == 1;
    vec3 base_bg = block ? u_cursor_color : v_bg;
    vec3 base_fg = block ? u_cursor_text_color : v_fg;

    // A colour emoji (bit 15) samples the atlas RGB; a text glyph uses the packed foreground.
    float emoji = float((v_glyph >> 15u) & 1u);
    vec3 fg = mix(base_fg, texel.rgb, emoji);

    float underline = float((v_glyph >> 13u) & 1u);
    float strike = float((v_glyph >> 14u) & 1u);
    // Glyph-box y: decorations are glyph-local, the underline centred at 0.88 and the
    // strikethrough at 0.5 (#517, #338).
    float gy = (v_tex.y * u_cell_size.y - u_char_offset.y) / u_char_size.y;
    // The underline style (bits 17..19, #829) displaces or gates the one band; only double adds
    // a second.
    uint ustyle = (v_glyph >> 17u) & 7u;
    float ul_centre = 0.88;
    float ul_mask = 1.0;    // x-axis gate — dotted and dashed only
    float ul_second = -1.0; // a second band's centre, or < 0 for none — double only
    // Every value 0..7 is handled: 3 curly, 2 double, 4 dotted, 5 dashed, anything else a
    // straight band.
    if (ustyle == 3u) { // curly
        // Curly: one sine cycle per cell, oscillating upward from 0.88, with a device-px amplitude
        // floor.
        float amp = max(u_line_thickness, 1.0) / max(u_char_size.y, 1.0);
        float x = v_tex.x;
        ul_centre = 0.88 - amp - amp * sin(x * 6.2831853);
    } else if (ustyle == 2u) { // double
        // Double: a second band above the single, `max(2 * thickness, thickness + 2px)` centre to
        // centre.
        float th = max(u_line_thickness, 1.0);
        float sep_px = max(2.0 * th, th + 2.0);
        ul_second = ul_centre - sep_px / max(u_char_size.y, 1.0);
    } else if (ustyle == 4u) { // dotted
        // Dotted: `u_dots_per_cell` whole dots per cell — an antialiased centred dot at 2px and above,
        // a hard one below.
        float n = max(u_dots_per_cell, 1.0);
        float period_px = u_cell_size.x / n;
        float dot_px = 0.5 * period_px; // the lit half of one period, in device px
        if (dot_px >= 2.0) {
            ul_mask = xgate(fract(v_tex.x * n), 0.25, 0.75, 0.5 * fwidth(v_tex.x) * n);
        } else {
            // Below 2px: pixel-aligned and hard, half-open from the start of the period.
            ul_mask = 1.0 - step(0.5, fract(v_tex.x * n));
        }
    } else if (ustyle == 5u) { // dashed
        // Dashed: one period per cell, the dashes at the two outer quarters, joining across cells.
        ul_mask = 1.0 - xgate(v_tex.x, 0.25, 0.75, 0.5 * fwidth(v_tex.x));
    }
    float ul_band = hline(gy, ul_centre, u_line_thickness, u_char_size.y) * ul_mask;
    if (ul_second >= 0.0) {
        // The double's two bands are one ink source, so they merge as coverage.
        ul_band = max(ul_band, hline(gy, ul_second, u_line_thickness, u_char_size.y));
    }
    ul_band *= underline;
    float st_band = hline(gy, 0.5, u_line_thickness, u_char_size.y) * strike;
    // Each line draws in its own ink; a block cursor's text colour overrides both.
    vec3 base_ul = block ? u_cursor_text_color : v_underline_fg;
    vec3 base_st = block ? u_cursor_text_color : v_strike_fg;
    // Composite in steps (ADR-0019 rule 6): background-class ink, I_neighbour, the underline,
    // text-class ink, the strikethrough, the cursor's strokes — drawn last and opaque.
    float cur = stroke_coverage(dx);

    // #317 §2: the ink accumulates premultiplied from nothing. The background's opacity is
    // `u_bg_alpha` for the default backdrop only (#298, #455), else opaque.
    float bg_alpha = (!block && v_bg_default > 0.5) ? u_bg_alpha : 1.0;
    float bg_class = float((v_glyph >> 16u) & 1u);
    // Text-class coverage per channel over an opaque background (#961); the alpha coverage
    // otherwise.
    bool lcd = u_lcd_gamma > 0.0 && bg_alpha >= 1.0;
    vec3 text_cov = (lcd && emoji < 0.5) ? lcd_cov(texel.rgb, fg) : vec3(coverage);
    // The same for a neighbour's spilled ink, for a text-class, non-emoji owner.
    vec3 foreign_cov = vec3(foreign);
    if (lcd && ((owner >> 15u) & 1u) == 0u && ((owner >> 16u) & 1u) == 0u) {
        foreign_cov = lcd_cov(owner_tex.rgb, foreign_ink) * owner_gate;
    }
    vec3 ink = mix(vec3(0.0), fg, coverage * bg_class);        // background-class ink joins the bg
    ink = mix(ink, foreign_ink, foreign_cov);                  // I_neighbour, over this cell's tile
    ink = mix(ink, base_ul, ul_band);                          // the band, over that background
    ink = mix(ink, fg, text_cov * (1.0 - bg_class));           // text-class ink, over the band
    ink = mix(ink, base_st, st_band);
    ink = mix(ink, u_cursor_color, cur);

    // How much of the background survives every ink source above, per channel.
    vec3 w_bg = (1.0 - coverage * bg_class) * (1.0 - foreign_cov) * (1.0 - ul_band)
              * (1.0 - text_cov * (1.0 - bg_class)) * (1.0 - st_band) * (1.0 - cur);

    // Straight-alpha source-over onto the background at opacity `bg_alpha`; one channel of `w_bg`
    // serves `a` (#961).
    float a = 1.0 - w_bg.g * (1.0 - bg_alpha);
    FragColor = vec4((ink + base_bg * (bg_alpha * w_bg)) / max(a, 1e-4), a);
}
"#;
