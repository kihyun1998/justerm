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
// instance: the inks the underline and the strikethrough draw in, packed 0xRRGGBB one per float
// (#513, split by #525 — SGR 58 declares the UNDERLINE's colour and there is no SGR for a strike's,
// so a declared colour is authoritative over one band only). A colour is below 2^24 so an f32 carries
// it exactly; measured with a standalone WebGL2 probe.
layout(location = 5) in float a_underline_fg;
layout(location = 6) in float a_strike_fg;
// instance: 1.0 iff this cell's bg is the pristine DEFAULT backdrop — the only surface #298 makes
// translucent (#455). Provenance, packed by the Rust side, not re-inferred from the resolved colour.
layout(location = 7) in float a_bg_default;
// instance: `I_neighbour` (ADR-0019 R1.2) — the glyph field of the cell above, below (#791), left
// and right (#966), and the ink each of them draws in, in that order. BLANK_SLOT where that
// neighbour's ink is withdrawn, which the packer decides because only it holds both cells' resolved
// backgrounds.
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
    // Cell-local. The atlas slot IS the padded cell (#359), so the bitmap already carries the glyph
    // at its offset inside it — the shader neither places nor masks it. Widening the cell spaces the
    // text because the BITMAP has wider margins, and a wide glyph's halves touch because it was
    // baked centred over its two-cell advance.
    v_tex = a_pos;
    v_glyph_nb = uvec4(a_glyph_nb);
    v_nb_fg = uvec4(a_nb_fg);
}
"#;

pub(crate) const FRAG_SRC: &str = r#"#version 300 es
precision mediump float;
uniform mediump sampler2DArray u_atlas;
// Where a CELL sits inside its padded atlas slot: (origin.xy, span.xy), all 0..1 (#288, #791).
// Not a symmetric inset any more — with a bleed band the slot's two vertical edges differ, and a
// cell that insets symmetrically stretches itself over the band instead of leaving it alone.
uniform vec4 u_cell_uv;
uniform float u_bg_alpha;    // background cell opacity: 0 = transparent, 1 = opaque (#298)
// The same uniforms the vertex stage declares — one per program, so the PRECISION must match too.
// This stage is `mediump float`, the vertex stage defaults to `highp`; an unqualified `vec2` here
// would fail to link ("Precisions of uniform 'u_cell_size' differ").
uniform highp vec2 u_cell_size;   // the grid cell in device px
uniform highp vec2 u_char_size;   // the glyph box inside it (#338) — decorations only
uniform highp vec2 u_char_offset; // where that box starts
uniform highp float u_line_thickness; // underline/strikethrough thickness in device px (#517)
// Dots per cell for a DOTTED underline (#830) — a whole number, so the pattern is cell-periodic and
// the cell-local x below needs no cross-cell phase. Computed host-side by `metrics::dots_per_cell`
// rather than here, and not only to keep the arithmetic testable: GLSL ES 3.00 leaves `round()`
// implementation-dependent at exactly 0.5 (`roundEven` is the defined one), so an odd cell width
// could yield a different dot count on a different GPU. Rust's `round` is one answer everywhere.
uniform highp float u_dots_per_cell;
// The cursor (#270): (col, row, span, shape). Shape 0 = NO cursor; otherwise `shape_id + 1`, so
// 1 = block, 2 = underline, 3 = bar, 4 = hollow block. Every shape lives here rather than in the
// instance buffer, so moving or blinking the cursor costs one uniform and no upload — a block
// that lived in the instances could not be un-painted without re-packing the frame.
//
// A BLOCK is still a colour override on the cell, not geometry: both references draw it that way
// (xterm `RectangleRenderer.ts:251` emits no vertices, alacritty `display/cursor.rs:33` no rects;
// each recolours the cell). Doing it per-fragment rather than per-instance keeps the order — the
// instance colours arrive already inverse-swapped, the glyph already concealed.
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
// How deep a band of THIS cell's left/right (x) and top/bottom (y) edges may receive a neighbour's
// ink, device px (#966, #791). Derived per font configuration by `metrics::horizontal_bleed` and
// `metrics::vertical_bleed`, which both floor at their headroom — so 0 does not reach here in
// practice, and the `armed` guards below defend a value the pipeline does not currently produce
// rather than a mode anything selects.
uniform vec2 u_bleed_px;
// Per-channel (LCD) text coverage (#961): 0 for a grayscale configuration, else the exponent dark ink
// raises the slot's light mask to (`lcd::fit_dark_gamma`).
uniform float u_lcd_gamma;
out vec4 FragColor;
// A horizontal line centred at `c` (cell-local y, 0..1) with soft edges (beamterm cell.frag).
// A horizontal line at glyph-box-normalised centre `c`, half-thickness `half` (also normalised),
// resolved to FULL coverage on the device-pixel rows it covers — not the `1 - smoothstep` tent it was
// (a beamterm port, #267). The tent peaks at 1 only at the exact centre and has no plateau, so a
// sub-pixel band integrates below 1 and the line reads grey at small cells (measured 118/255 at
// dpr 1). Every GPU terminal (kitty/ghostty/wezterm) draws a straight line as a solid, pixel-snapped
// fill instead; this does the same in the fragment shader (#515).
//
// `char_h` is `u_char_size.y`, the glyph box in device px (a fragment uniform since #338), and
// `thick_px` is the line thickness in device px — `u_line_thickness`, computed host-side as
// `max(1, round(font_size * dpr / 15))`. That is xterm.js's rule (`TextureAtlas.ts`,
// `max(1, floor(fontSize*dpr/15))`), the right reference because it is a Canvas renderer under our
// constraint (no font file, so no `underline_thickness` metric — #517). The old `0.05 * box`
// half-thickness was a beamterm inheritance (#267), ~2x too heavy (measured 11.3% of the cell vs
// xterm's 6.2%). Working in device px and dividing by `char_h` only to reach `gy` space keeps the
// thickness tied to the font size, not the box, so `lineHeight` and font family cannot distort it.
//
// The band is snapped to the pixel grid and its centre pulled inside `[0,1]` so it never spills into
// the next row (the invariant alacritty holds with `max_y` and we did not). `fwidth` is one device
// pixel in normalised units, for a single-pixel antialiased edge — crisp, not stair-stepped at
// fractional DPR.
// Per-channel coverage from a subpixel slot's light mask for ink of colour `ink` (#961), chosen per
// channel: the mask as-is where that channel of the ink is light (>= 0.75), raised to the
// configuration's measured exponent where it is darker.
vec3 lcd_cov(vec3 mask, vec3 ink) {
    return mix(pow(mask, vec3(u_lcd_gamma)), mask, step(vec3(0.75), ink));
}
float hline(float gy, float c, float thick_px, float char_h) {
    float th = max(thick_px, 1.0) / max(char_h, 1.0); // device-px thickness, in gy units
    float top = clamp(c - th * 0.5, 0.0, 1.0 - th);   // centre-clamp: stay in the cell
    float aa = 0.5 * fwidth(gy);
    return clamp((gy - (top - aa)) / max(aa, 1e-5), 0.0, 1.0)
         * clamp(((top + th + aa) - gy) / max(aa, 1e-5), 0.0, 1.0);
}
// Coverage of an x-axis gate: 1 inside `[lo, hi]`, antialiased at both edges with the same
// half-pixel ramp `hline` uses on y (#830). The mirror of `hline` on the other axis, and it is what
// turns one band into a dotted or dashed one — the band itself is unchanged, so `I_line`'s ink and
// class are untouched and everything #513/#525/#712 settled about the channel keeps holding.
//
// `aa` is an argument rather than `fwidth(t)` because a caller gates a WRAPPED coordinate: the
// derivative of `fract(x)` spikes at the seam, and deriving the ramp from it draws a visible line
// there. The caller passes the ramp of the *unwrapped* coordinate instead.
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
// Is the cell at grid `cell` (col, row) painted by a block cursor? (#791; any column since #966)
//
// A block replaces the cell's background at fragment time (`base_bg`, below), so it never reaches
// the packed instance — and the packer is where ADR-0019 rule 5 withdraws `I_neighbour` at a
// background edge. That leaves the block cursor as a background edge rule 5 structurally cannot
// see, so the withdrawal is completed here, on the one stage that knows. Both directions were wrong
// without it: a neighbour's descender drew *over* the block, and a cursor cell's own glyph — which
// paints in `u_cursor_text_color` — spilled into the next row in the pre-cursor `fg`, one glyph in
// two colours split at the cell boundary.
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
// Does this fragment fall on a cursor STROKE? Mirrors `cursor::cursor_rects` in device pixels; a
// hard edge, like the rects it mirrors — the strokes are pixel-aligned, so antialiasing them would
// only blur a rectangle onto its own boundary. A block draws no stroke.
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
    // The glyph field packs slot (bits 0..12), underline (bit 13), strikethrough (bit 14),
    // the colour-emoji flag (bit 15, #284), the glyph's ink class (bit 16, #712) and the
    // underline STYLE (bits 17..19, #829). The last two sit above the `u16` the rest fits in
    // because this is read as `uint(a_glyph)` and an `f32` carries every integer below 2²⁴
    // exactly — the field is full, the transport is not.
    uint slot = v_glyph & 0x1FFFu;
    uint layer = slot >> 5u;   // 32 glyphs stack vertically per layer
    uint band = slot & 31u;
    // Inset the cell-local texcoord into the padded atlas slot's content region, so the transparent
    // guard band is never sampled (beamterm cell.frag) — stops band bleed while the content maps
    // edge-to-edge of the CELL. Block elements are baked at cell size (#359), so they tile.
    vec2 inner = u_cell_uv.xy + v_tex * u_cell_uv.zw;
    // Nudge off the exact texel edge so NEAREST can't round to a neighbour (beamterm cell.frag);
    // belt-and-suspenders for a fractional cell↔texel mapping (DPR != 1, #265).
    vec3 tc = vec3(inner.x + 0.001, (float(band) + inner.y + 0.001) / 32.0, float(layer));
    vec4 texel = texture(u_atlas, tc);
    float coverage = texel.a;

    // ── I_neighbour (ADR-0019 R1.2 / rule 6, #791) ────────────────────────────────────────────
    // Reader-side: this fragment reads the ADJACENT cells' slots and folds their ink into its own
    // chain. The quad is still exactly this cell, so nothing overlaps, the composite stays one
    // evaluation per pixel, and no GL blending is involved.
    //
    // A slot holds `bleed | cell | bleed` on each axis, so the band that spilled toward me sits
    // exactly one CELL away from where my own texel reads — `± u_cell_uv.w` down the slot for the
    // rows above and below (#791), `± u_cell_uv.z` across it for the cells beside (#966) — and no
    // arithmetic about padding or band depth is needed here. `metrics::ink_rows` states the same
    // mapping in device px and is where it is tested, on both axes; this is its texcoord form.
    //
    // Sampled unconditionally and masked rather than branched: an implicit-LOD fetch under
    // non-uniform control flow is undefined in GLSL ES 3.00.
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

    // One neighbour supplies this fragment's foreign ink: the one laying down the most, the earlier
    // in up-down-left-right order on a tie (so a fragment only rows reach behaves as before #966).
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

    // It carries its OWNER's ink — including the emoji rule, which is the *owner's* bit 15 and not
    // this cell's. Reading only coverage here drew a colour emoji's overflow as a monochrome
    // silhouette in the receiving cell's SGR foreground: measured, a brown pile spilling into the row
    // below arrived as 11 device px of pure red. R1.2 says foreign ink keeps its owner's ink, and the
    // owner's ink for an emoji lives in the atlas rather than in the instance.
    //
    // ONE position in rule 6 serves every direction, and that is a property of the rule rather than
    // of what can spill: the clause is "above the receiver's own tile, below everything else the
    // receiver owns", which never asks the neighbour's class. (An earlier comment here justified it by
    // claiming a background-class glyph cannot overflow. That is false — Powerline `U+E0A4..=U+E0D6`
    // is background-class by `glyph_class`, is drawn by the FONT rather than by `builtin`, and is free
    // to spill like any other font glyph.)
    vec3 foreign_ink = mix(unpack_rgb(owner_fg), owner_tex.rgb, float((owner >> 15u) & 1u));
    // ── end I_neighbour ───────────────────────────────────────────────────────────────────────

    // A BLOCK cursor recolours the cell before anything composites over it. The instance colours
    // arrive already inverse-swapped and the glyph already concealed, so the cursor lands last —
    // the order alacritty gets by overwriting `cell.fg`/`cell.bg` in `display/content.rs:167`.
    float dx = cursor_dx();
    bool block = dx >= 0.0 && int(u_cursor.w) == 1;
    vec3 base_bg = block ? u_cursor_color : v_bg;
    vec3 base_fg = block ? u_cursor_text_color : v_fg;

    // A colour emoji (bit 15) samples the atlas RGB (the font's own colours); a text glyph uses
    // the packed foreground (beamterm cell.frag `mix(base_fg, glyph.rgb, emoji_factor)`).
    float emoji = float((v_glyph >> 15u) & 1u);
    vec3 fg = mix(base_fg, texel.rgb, emoji);

    float underline = float((v_glyph >> 13u) & 1u);
    float strike = float((v_glyph >> 14u) & 1u);
    // Fixed glyph-box positions (underline below baseline, strikethrough mid-cell); the THICKNESS is
    // no longer a box fraction — it is `u_line_thickness`, xterm.js's `max(1, round(font_size*dpr/15))`
    // in device px (#517), so it tracks the font size, not the cell. The *rendering* of the band is
    // also no longer beamterm's tent — `hline` snaps it to whole device pixels and fills solid (#515),
    // which is why it stays crisp at small cells. The positions (0.88 / 0.5) are still fixed fractions;
    // deriving them from font metrics is not available here (Canvas 2D exposes no `underline_position`
    // — #517) and a better fraction is a later refinement.
    // Decorations are GLYPH-local, not cell-local: with `lineHeight = 1.5` a cell-local 0.88 would
    // drop the underline far below the text it underlines. That glyph-box space is also what keeps
    // the band inside the cell under a tall lineHeight — `gy` is bounded to the box, so `hline`'s
    // centre-clamp holds without any cell-relative `max_y` (the invariant alacritty needs a clamp
    // for). The glyph's own coverage no longer needs these uniforms (#359 bakes the offset into the
    // bitmap), but its decorations still do. Identical at the default, where the two spaces coincide
    // (#338).
    float gy = (v_tex.y * u_cell_size.y - u_char_offset.y) / u_char_size.y;
    // #525: the two bands carry SEPARATE coverages now, because they carry separate inks. Folding
    // them with `max()` first was free while one colour served both; it is lossy the moment SGR 58
    // makes them differ, and the loss is total (the underline's colour would paint the strike).
    // #829: the underline STYLE is a 3-bit field at bits 17..19, and it displaces the band's
    // centre rather than adding a second draw — so a curl is the same band, the same ink and the
    // same thickness, and everything #513/#525 settled about the channel keeps holding.
    //
    // The period is exactly ONE cycle per cell, which makes the phase cell-local and continuous at
    // the same time: `sin` is 2*pi-periodic, so a row-continuous x and a per-cell x agree at every
    // boundary and everywhere else. A first draft added the cell's column to the phase to "join the
    // curls across boundaries" and a mutation proved the term dead — removing it changed no pixel,
    // because there is no window in which the two models differ. Per-cell is also what the two
    // references that draw a curl at all do, each baking one per cell (ghostty a sprite codepoint,
    // xterm.js a stroke into the glyph atlas). This comment used to send the next reader to
    // xterm.js's `variantOffset` as *the* answer for DOTTED, whose period is not a whole cell.
    // #830 landed that mark and took the other one: `metrics::dots_per_cell` quantises the count so
    // the period is cell-periodic by construction, which is ghostty's route and keeps the term
    // deleted above deleted.
    uint ustyle = (v_glyph >> 17u) & 7u;
    float ul_centre = 0.88;
    // The two axes the remaining marks move, and the reason they are variables rather than three
    // more `hline` calls (#830). Curly displaces the centre; DOTTED and DASHED gate the same band
    // along x; DOUBLE is the only one that adds a band, which is what the maintainer settled when
    // this ticket's "make no new structural decision" rule met it. The #829 comment below still
    // describes a curl correctly and no longer describes the feature.
    float ul_mask = 1.0;    // x-axis gate — dotted and dashed only
    float ul_second = -1.0; // a second band's centre, or < 0 for none — double only
    //
    // **This chain must stay total over all EIGHT representable values, not six.** `attrs.rs`
    // forwards the raw 3-bit field and names none of them on purpose, and that crate is published
    // separately — `apply_frame` takes any `u16` from any caller. So 0, 1, 6 and 7 fall through to a
    // straight band here. For 1, 6 and 7 that is exactly what `UnderlineStyle::from_bits`
    // normalises them to one crate away; **0 is reconciled by a different mechanism and the two are
    // worth not conflating** — `from_bits(0)` is `None`, not a single, and what makes the two agree
    // is the `underline` bit, which core's one writer (`set_underline_style`) arms with the style.
    // An earlier version of this comment said `from_bits` normalises all four to a band, which a
    // refuting pass measured false. An `else if` ending at 5 with no fall-through would break the
    // agreement in silence: no error, no failing test, a mark that vanishes on a malformed input.
    if (ustyle == 3u) { // curly
        // The amplitude carries a device-px FLOOR for the reason #515 gave the band a minimum
        // thickness: a curl a fraction of a pixel tall IS a straight line, so without it the
        // feature would be a visual no-op at the smallest supported cell while every pixel
        // assertion still passed.
        // A note on the antialiasing, measured rather than assumed. `hline` derives its ramp from
        // `fwidth(gy)`, and `gy` is a function of the vertical coordinate alone, so the ramp is
        // vertical. That is exact for a straight band, whose edge is horizontal; a displaced
        // centre makes the edge DIAGONAL, and a vertical ramp across a diagonal edge is narrower
        // than the geometry warrants by `cos(theta)`. Max slope is `2*pi*max(line_thickness,1) /
        // cell_width` — `char_h` cancels, so this is fixed by the cell's ASPECT, not by font size:
        // ~0.79 px/px at an 8x16 cell, i.e. theta ~38 deg and cos ~0.79.
        //
        // Measured consequence: coverage is a function of `gy` only, so the VERTICAL extent stays
        // constant across the curl (browser proof, `steepOverFlatExtent` at font 48: 1.00 / 0.90 /
        // 1.00 / 0.93 over dpr 1 / 1.1 / 1.5 / 2, against a straight-band control of exactly 1 at
        // every one). A perpendicular-correct shader would instead read ~1.27. So the band really
        // is ~21% thinner PERPENDICULARLY where the sine is steepest — which at the default font,
        // where the band is 1-2 device px, is 0.2-0.4 px and therefore sub-pixel. It reaches ~1 px
        // only at very large fonts.
        //
        // Deliberately not asserted as a check: the ratio is the *current* answer, and correcting
        // the ramp for slope would move it to ~1.27, so a test pinning 1.00 would redden on the
        // fix. The measurement is published in the proof's `measured` block instead.
        float amp = max(u_line_thickness, 1.0) / max(u_char_size.y, 1.0);
        float x = v_tex.x;
        // Oscillate UPWARD from 0.88 so the curl's lowest point sits where the straight band does
        // and `hline`'s centre-clamp still holds at the bottom of the glyph box.
        ul_centre = 0.88 - amp - amp * sin(x * 6.2831853);
    } else if (ustyle == 2u) { // double
        // Two bands `2 * thickness` apart centre to centre, with the LOWER one left where a single
        // underline sits and the second placed above it.
        //
        // The separation is 2 of 3: xterm.js `yBotDefault = yTopDefault + lineWidth * 2`
        // (`TextureAtlas.ts:590-591`) and ghostty "one above ... and one below by one thickness"
        // (`special.zig:57-70`) agree; alacritty instead straddles the descent at 0.25 / 0.75 of it
        // (`rects.rs:82-83`), which is a metric this renderer does not have — there is no font file,
        // which is the same reason #517 took the thickness from the font SIZE.
        //
        // The DIRECTION is not a preference, it is `hline`'s clamp. `top` is clamped into the glyph
        // box, so a pair placed downward has both bands pulled to the same `top` and collapses into
        // ONE line — with no error, and a pixel assertion that still happily sees an underline.
        // Going up is free, which is why the curl above oscillates upward for the same reason.
        // ghostty centres the pair instead and can, because it bakes into a canvas with padding
        // below the cell; xterm.js, which DOES restrict to the cell height, shifts the pair up so
        // the bottom band lands where the single would — the same answer under the same constraint.
        // justerm is permanently in that regime.
        // **`2 * thickness` is the references' rule and it does not survive OUR rasteriser.**
        // Measured: at the default 16px font and dpr 1 the thickness is one device pixel, so the
        // bands sit 2px apart with a 1px gap — and `hline` carries a half-pixel `fwidth` ramp on
        // each edge, which closes it. The browser proof read `doubleBandHistogram: {"1": 96}`: one
        // band, at every column, with the merged run's centre exactly 1px above the single's. The
        // references do not meet this because their marks are not ramped — xterm.js strokes onto a
        // canvas that snaps a horizontal 1px line, ghostty fills whole rects into a sprite, and
        // alacritty emits rects. Following their number here would ship a double underline that is
        // a slightly thicker single one at the size almost every user runs.
        //
        // So the separation is `max(2 * thickness, thickness + 2px)`, which is the reference rule
        // everywhere it has room and a floor of **one** device pixel of genuinely clear air where it
        // does not — the nominal gap is two, and the two `hline` ramps spend half a pixel each. A
        // refuting pass caught an earlier version of this sentence claiming two, which is the same
        // error (a gap stated before the ramp is subtracted) that the paragraph above it corrects.
        // The floor binds at one-pixel thickness and nowhere else: `max(2,3) = 3`, `max(4,4) = 4`,
        // `max(6,5) = 6`.
        // A deliberate divergence with a measurement behind it, on the tie-breaker's "renderer cell
        // composition is justerm's own model" row.
        float th = max(u_line_thickness, 1.0);
        float sep_px = max(2.0 * th, th + 2.0);
        ul_second = ul_centre - sep_px / max(u_char_size.y, 1.0);
    } else if (ustyle == 4u) { // dotted
        // `u_dots_per_cell` is a whole number (`metrics::dots_per_cell`), so this period is
        // cell-periodic BY CONSTRUCTION and the cell-local `v_tex.x` needs no cross-cell phase —
        // the term #829 proved inert stays deleted. The two references whose period is *not* a whole
        // cell both pay for it with cross-cell state (xterm.js's `variantOffset`, alacritty's
        // every-two-cells inversion); ghostty quantises as this does.
        //
        // The dot is CENTRED in its period, so `fract`'s seam falls inside the GAP, where coverage
        // is zero on both sides. Anchoring the dot at [0, 0.5) instead would put a hard edge exactly
        // on the discontinuity — visible as a seam at every dot.
        // **A one-pixel dot cannot be antialiased, and trying erases the mark.** Measured before
        // this branch existed: at the default 16px font the cell is 8 device px, `dots_per_cell`
        // gives 4, so a dot is ONE device pixel — and a half-pixel ramp on each side is 0.25 + 0.25
        // in a gate 0.5 wide, i.e. the whole period. The browser proof read `dottedDuty: 1.0` and
        // `dottedRuns: 1`: a solid line, every "is drawn" check green.
        //
        // alacritty splits on exactly this and it is where the threshold comes from — `draw_dotted`
        // is a hard per-pixel on/off used below a 2px thickness, and `draw_dotted_aliased` (which
        // rounds the dots) is used only at or above it (`rect.f.glsl`, dispatched at its `main`).
        // `aa = 0` makes `xgate` a hard step, which is the same split one function down.
        float n = max(u_dots_per_cell, 1.0);
        float period_px = u_cell_size.x / n;
        float dot_px = 0.5 * period_px; // the lit half of one period, in device px
        if (dot_px >= 2.0) {
            ul_mask = xgate(fract(v_tex.x * n), 0.25, 0.75, 0.5 * fwidth(v_tex.x) * n);
        } else {
            // Below two device pixels the dot is **pixel-aligned and hard**, tested on the whole
            // pixel index rather than on a continuous coordinate. Both halves of that are load
            // bearing and each was measured wrong first:
            //
            // Antialiasing a 1px dot ERASES the mark. At the default 16px font the cell is 8 device
            // px and `dots_per_cell` gives 4, so the dot is one pixel and a half-pixel ramp on each
            // side fills the whole 0.5-wide gate: the proof read `dottedDuty: 1.0`, `dottedRuns: 1`
            // — a solid line with every "is drawn" check green.
            //
            // The gate is **half-open from the start of the period**, not centred on it. Centred
            // was the first attempt and it erases the mark the other way: with a 2px period the
            // fragment centres land on `fract == 0.25` and `0.75`, which are exactly a centred
            // gate's two edges and are symmetric about the dot, so any symmetric test admits both
            // or neither. Measured `dottedRuns: 0`. Half-open breaks that symmetry — `0.25` is in,
            // `0.75` is out — which is the same asymmetry alacritty gets from a parity test on a
            // pixel INDEX (`rect.f.glsl`, `draw_dotted`).
            //
            // **What it must NOT be is that pixel index, and a refuting pass measured why.**
            // alacritty's parity test is exact because its period is the integer 2; ours is
            // `cell_w / n`, which is a whole number of pixels only when the cell width is even. On
            // an odd cell `mod(pixel, period)` drifts across the cell and the residue walks out of
            // the gate for good, so the dots crowd the left and the rest of every cell goes blank —
            // computed over the shipped arithmetic, an 11px cell lit columns 0, 2, 4 and nothing
            // from 5 to 10, and a 17px cell lit 0, 2, 4, 6 and nothing from 7 to 16. And it is
            // invisible to the cross-cell check by construction, because the drift is *identical*
            // in every cell. The normalised coordinate tiles the cell exactly whatever `n` is, which
            // is the property `dots_per_cell` was quantised for in the first place; the pixel index
            // threw it away one line after buying it.
            //
            // The two branches differ in phase by a quarter period. That is safe because the branch
            // is chosen from `u_cell_size.x` and `n`, both uniform over the grid — no frame mixes
            // them.
            ul_mask = 1.0 - step(0.5, fract(v_tex.x * n));
        }
    } else if (ustyle == 5u) { // dashed
        // One period per CELL, with the dash at the two outer quarters so adjacent cells' dashes
        // JOIN across the boundary into one half-cell dash separated by a half-cell gap. That is
        // alacritty's construction and its comment states the reason — "since dashes of adjacent
        // cells connect with each other our dash length is half of the desired total length"
        // (`rect.f.glsl`, `draw_dashed`). All three references are cell-periodic for dashed, so
        // unlike dotted this one needed no decision.
        ul_mask = 1.0 - xgate(v_tex.x, 0.25, 0.75, 0.5 * fwidth(v_tex.x));
    }
    float ul_band = hline(gy, ul_centre, u_line_thickness, u_char_size.y) * ul_mask;
    if (ul_second >= 0.0) {
        // `max`, not a second composite: the two bands are ONE ink source (ADR-0019 rule 4 splits
        // the marks by *authorship of the colour*, and both halves of a double underline are the
        // same authorship), so they must merge as coverage before the ink is applied. Compositing
        // them separately would apply `v_underline_fg` twice where they overlap.
        ul_band = max(ul_band, hline(gy, ul_second, u_line_thickness, u_char_size.y));
    }
    ul_band *= underline;
    float st_band = hline(gy, 0.5, u_line_thickness, u_char_size.y) * strike;
    // #513: the line draws in its OWN ink, which the packer resolved without the glyph-only rules
    // (ADR-0019 rule 4 — `I_line` is TEXT class). Still overridden by a block cursor, because the
    // cursor recolours the whole cell rather than the glyph: the line bases follow `base_fg` there.
    // Emoji is unchanged in spirit — the line was never the texture's colour, only now it is not
    // the glyph's either.
    vec3 base_ul = block ? u_cursor_text_color : v_underline_fg;
    vec3 base_st = block ? u_cursor_text_color : v_strike_fg;
    // Composite in steps — glyph over background, THEN each line over that. Folding a line into
    // `fg` first and compositing once applies the band's coverage twice (`mix(bg, mix(fg, line, L), L)`),
    // which leaves `L(1-L)` of the GLYPH's ink in the line — up to 25 % at half coverage. That was
    // invisible while the two inks were equal and became an error the moment #513 made them differ,
    // proportional to exactly the divergence the channel exists to create: at the default font size
    // an underline on a selected tile was never the cell's ink, only mostly it.
    //
    // The strike goes LAST, so where a thick band makes the two overlap the strikethrough wins. That
    // is xterm's band order (`TextureAtlas.ts` strokes the underline at :565-688 and the strikethrough
    // at :762) rather than a coin toss taken here — and all three references agree the strike goes
    // over the glyph, so it composites after everything below.
    //
    // #712 — the UNDERLINE's place is not a band-order question but an ink-CLASS one (ADR-0019 rule
    // 6). R1 puts a BACKGROUND-class glyph's ink on the background channel while `I_underline` is
    // `TEXT` class always, so background-class ink cannot occlude it: the band draws OVER a tile and
    // UNDER a letter, whose descender therefore survives. `bg_class` splits the glyph's coverage
    // between the two sides of the band — exactly one side is ever non-zero, so this is one `mix`
    // more than the old chain, not a branch. Blanket "underline first", which both reordering
    // references take (ghostty `generic.zig:2932` states the descender reason; xterm's `fillText` at
    // :735 sits between the two bands), would trade this defect for its mirror: measured on our own
    // renderer, a red underline over `█▄▓░` goes from 66 red px per cell to 0. Neither reference has
    // a background ink class driving occlusion, so neither faced the choice.
    //
    // Band-vs-band overlap is arithmetically out of reach *for the single underline*: its centre
    // is 0.38 of the glyph box from the strikethrough's while `u_line_thickness / char_height` stays
    // near 0.06 at every font size, so reaching it needs a glyph box of about three device px.
    // **#830's second band is a different quantity and moves that threshold**: it sits a fixed
    // number of device PIXELS above 0.88 rather than a fixed fraction, so its distance to the
    // strikethrough shrinks with the box — the two approach at roughly `0.38 * H < sep_px + T`,
    // about ten device px rather than three. A completeness pass raised it; the proof now mounts a
    // struck double (`demo/underline-marks.html`, `aStruckDoubleKeepsThreeSeparateBands`) and reads
    // three separate bands at every dpr it sweeps, and a mutation that walks the strikethrough
    // toward the underline reddens that check and only it. That is also why `cov` below may stay on `max`
    // while the colour path composites in sequence — the two agree everywhere the bands do not meet,
    // and reordering the underline does not change *whether* anything is drawn at a pixel.
    // The cursor's strokes draw last and opaque, over the glyph — both references append the
    // cursor rects after the text pass.
    float cur = stroke_coverage(dx);

    // #317 §2 — the ink accumulates PREMULTIPLIED, starting from nothing rather than from the
    // background. Same chain, same rule-6 order, same `mix` per source; only the seed changed.
    //
    // The old chain seeded with `base_bg` and computed alpha separately, which made the two channels
    // describe different cells whenever the background was translucent: the colour had already mixed
    // toward `base_bg` while the alpha said that background was barely there. At `u_bg_alpha = 0`,
    // `cov = 0.5` a pixel came out `0.5*bg + 0.5*fg` at alpha `0.5` where a fully transparent
    // background can contribute nothing at all and the answer is `fg`. Measured on this renderer
    // before the fix (white 'A' on a Default blue, dpr 2, `bg_alpha = 0`): of 174 pixels with any
    // alpha, **35** were the foreground and the rest carried background blue that was not there —
    // `a = 126` read `rgb(150,175,207)`, which is `mix(blue, white, 0.494)` to the byte.
    // ADR-0019's Coherence clause is what this violated: a channel resolution and the surface it
    // describes must agree, and these two described the same pixel differently.
    //
    // It is NOT inherent to compositing in one pass — the premise #317 recorded from beamterm and
    // that nobody had re-derived for this shader. Straight-alpha source-over of opaque ink onto a
    // background of opacity `A` is `a = 1 - w_bg*(1-A)` and `rgb = (ink + base_bg*A*w_bg) / a`,
    // both available here. No second pass, no premultiplied context, no GL blending (this renderer
    // enables none — the references reach the same result with separate passes and hardware blend:
    // alacritty `BlendFuncSeparate` at `renderer/mod.rs:252`, ghostty a whole `AlphaBlending` mode).
    //
    // Accumulating rather than subtracting `base_bg * w_bg` back out is a precision choice under
    // `mediump`: every term here stays positive and numerator and denominator shrink together, so a
    // small `a` does not amplify anything. The subtraction form cancels two near-equal quantities
    // exactly where `a` is smallest.
    // The background's opacity here; the rule is stated where `a` is computed, below.
    float bg_alpha = (!block && v_bg_default > 0.5) ? u_bg_alpha : 1.0;
    float bg_class = float((v_glyph >> 16u) & 1u);
    // #961: text-class coverage per channel. A subpixel configuration's slot carries the light mask
    // in RGB (`lcd.rs`), read through `lcd_cov`. Only over an opaque background — one alpha cannot
    // carry three coverages — and never for a colour emoji, whose RGB is its own colour. A grayscale
    // configuration and every excluded fragment read the alpha coverage on all three channels.
    bool lcd = u_lcd_gamma > 0.0 && bg_alpha >= 1.0;
    vec3 text_cov = (lcd && emoji < 0.5) ? lcd_cov(texel.rgb, fg) : vec3(coverage);
    // The same for ink a neighbour spilled into this cell (I_neighbour): its owner's slot, its
    // owner's ink colour, and only for a text-class owner that is not an emoji — a background-class
    // slot's RGB is not a mask (a builtin glyph keeps white there).
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

    // How much of the background survives every ink source above — the same coverages, as a product.
    // This REPLACES `max(coverage, max(ul_band, st_band))`, which was an approximation of the ink's
    // total weight and disagreed with the colour chain wherever two sources overlapped (a descender
    // crossing its underline is the reachable case, #712's own geometry). The two agreed only where
    // at most one source was partial, which is why it never showed while alpha was the only consumer.
    vec3 w_bg = (1.0 - coverage * bg_class) * (1.0 - foreign_cov) * (1.0 - ul_band)
              * (1.0 - text_cov * (1.0 - bg_class)) * (1.0 - st_band) * (1.0 - cur);

    // Only the DEFAULT terminal background is translucent (the see-through backdrop). An explicit
    // SGR background or an inverse/selection/cursor background is *content* and stays opaque — else
    // a highlight would vanish on a translucent terminal (#298). Ink is always opaque, including a
    // BACKGROUND-class glyph's — ADR-0019 **R1.1** carries why, and it is not "a `█` is obviously
    // ink": translucency is gated on `v_bg_default`, i.e. on no layer having touched the bg, so R1
    // has no treatment to transfer at the moment the question arises.
    //
    // #455: translucency keys on PROVENANCE (`v_bg_default`, packed by the Rust side that knows which
    // layers touched the bg), not on `base_bg == u_default_bg`. The colour test went translucent on any
    // content cell whose composite coincidentally landed on the default RGB (an SGR 48 set to the theme
    // bg, an Indexed slot resolving to it, a decoration painting it) — a pinhole in opaque content.
    // A block cursor is still forced opaque here, even where its colour happens to equal the default
    // background — alacritty forces `bg_alpha = 1.` for the cursor cell unconditionally
    // (`display/content.rs:175`, "we must adjust alpha to make it visible"). The cursor's STROKES no
    // longer need `max(bg_a, cur)` to stay opaque: `cur` is in `w_bg` above, so a stroked pixel has
    // no background left to be translucent and `a` reaches 1 by construction.
    //
    // `w_bg` is per channel since #961, and one channel serves `a`: the three are equal wherever
    // `bg_alpha < 1` (`text_cov` is scalar there), and `bg_alpha == 1` makes `a` 1 whatever they are.
    float a = 1.0 - w_bg.g * (1.0 - bg_alpha);
    FragColor = vec4((ink + base_bg * (bg_alpha * w_bg)) / max(a, 1e-4), a);
}
"#;
