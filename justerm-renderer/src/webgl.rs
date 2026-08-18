//! Thin `#[wasm_bindgen]` + WebGL2 glue — browser-only (wasm32), verified in the demo.
//!
//! The instanced pipeline draws the whole grid in one call: `apply_frame` resolves each
//! cell's bg/fg references (injected palette) and its glyph slot (glyph cache, rasterising
//! and uploading new glyphs on demand), packs one instance per cell, and `render`
//! composites each glyph's coverage from the atlas over its background, plus SGR attrs
//! (#267: bold/italic font variants, underline/strikethrough lines, inverse fg/bg swap; #272:
//! bold→bright + dim + minimum-contrast + selection fg + tile-glyph colours; #393: marker decoration
//! bg/fg overrides) and double-width glyphs (#268: a wide glyph splits across two atlas
//! slots / two grid cells).
//! ASCII (`0x20..=0x7E`) is pre-rasterised. Colour emoji (#284) + clusters (#285) follow.
//!
//! The selection / search overlay (#271, `setOverlay`) folds its highlight colour into each covered
//! cell's packed background at pack time (blend vs solid), so it rides the same instanced draw — no
//! overlay pass. The cursor (#270, `setCursor`) is a shader uniform composited last, over any highlight.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::bitmap::{PADDING, is_color_bitmap, split_wide_bitmap};
use crate::color::gl_rgb;
use crate::context_loss::{ContextLiveness, ContextState, DEFAULT_RESTORE_TIMEOUT_MS, FrameAction};
use crate::cursor::{
    Cursor, DEFAULT_CURSOR_CONTRAST, THICKNESS, cursor_cells_at, cursor_rects, cursor_thickness,
    guarded_cursor_colors, shape_from_id, shape_id,
};
use crate::decoration::parse_decorations;
use crate::dpr::{cells_that_fit, css_px, dpr_changed, grid_px};
use crate::emoji::is_emoji_text;
use crate::frame::{Frame, INSTANCE_FLOATS, pack_instances};
use crate::frame_grid::{DamageFrame, FrameGrid, cell_count};
use crate::glyph_cache::{
    FontStyle, GLYPHS_PER_LAYER, GlyphCache, WIDE_BASE, WIDE_CAPACITY, slot_texcoord,
};
use crate::glyph_resolve::{Cells, ResolveError, resolve_frame};
use crate::mat4::Mat4;
use crate::metrics::{device_cell, fit_cell_to_atlas, glyph_offset};
use crate::overlay::{HighlightColors, Overlay};
use crate::palette::Palette;
use crate::preedit::{
    Codepoint as PreeditCodepoint, Patch as PreeditPatch, Span as PreeditSpan,
    caret_col as preedit_caret_col_of, is_wide as preedit_is_wide, patch as preedit_patch_of,
    writes as preedit_writes,
};
use crate::rasterizer::Rasterizer;
use crate::render_policy::ColorPolicy;
use crate::upload::{UploadPlan, invalidate_baseline, plan_upload};

/// Texture-array layers covering the whole slot space (normal + wide = 6144 / 32 = 192),
/// so wide/emoji slots (layers 64..191) have storage.
const TOTAL_LAYERS: i32 = ((WIDE_BASE + WIDE_CAPACITY * 2) / GLYPHS_PER_LAYER) as i32;
/// Default font size (CSS px) for the atlas rasteriser.
const FONT_SIZE: f32 = 16.0;

/// Unit-quad corners (triangle strip): geometry + per-cell glyph texture coordinate.
const QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
/// Byte stride of one packed instance. **Derived** from [`INSTANCE_FLOATS`] rather than written out:
/// the two drifting apart silently mis-addresses every attribute, and nothing in the pipeline would
/// say so — the geometry would simply be wrong. It was a literal `9 * 4` until #513 widened the
/// record, then #455 (the `bg_default` provenance flag) and #525 (the second line ink) widened it
/// again — three times in one release cycle, which is the whole argument for deriving it.
/// The per-attribute **offsets** in `build_pipeline` are still literals and are not derived; a float
/// inserted anywhere but the end moves them, so they are edited with this constant, not after it.
const INSTANCE_STRIDE: i32 = (INSTANCE_FLOATS * 4) as i32;

const VERT_SRC: &str = r#"#version 300 es
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
}
"#;

const FRAG_SRC: &str = r#"#version 300 es
precision mediump float;
uniform mediump sampler2DArray u_atlas;
uniform vec2 u_padding_frac; // guard band as a fraction of the padded atlas cell (#288)
uniform float u_bg_alpha;    // background cell opacity: 0 = transparent, 1 = opaque (#298)
// The same uniforms the vertex stage declares — one per program, so the PRECISION must match too.
// This stage is `mediump float`, the vertex stage defaults to `highp`; an unqualified `vec2` here
// would fail to link ("Precisions of uniform 'u_cell_size' differ").
uniform highp vec2 u_cell_size;   // the grid cell in device px
uniform highp vec2 u_char_size;   // the glyph box inside it (#338) — decorations only
uniform highp vec2 u_char_offset; // where that box starts
uniform highp float u_line_thickness; // underline/strikethrough thickness in device px (#517)
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
float hline(float gy, float c, float thick_px, float char_h) {
    float th = max(thick_px, 1.0) / max(char_h, 1.0); // device-px thickness, in gy units
    float top = clamp(c - th * 0.5, 0.0, 1.0 - th);   // centre-clamp: stay in the cell
    float aa = 0.5 * fwidth(gy);
    return clamp((gy - (top - aa)) / max(aa, 1e-5), 0.0, 1.0)
         * clamp(((top + th + aa) - gy) / max(aa, 1e-5), 0.0, 1.0);
}
// Which cell of the cursor's `span`-wide box is this, or -1 for a fragment outside it? Mirrors
// `cursor::covers`.
float cursor_dx() {
    if (int(u_cursor.w) == 0) return -1.0;
    if (abs(v_cell.y - u_cursor.y) > 0.5) return -1.0;
    float dx = v_cell.x - u_cursor.x;
    return (dx < -0.5 || dx > u_cursor.z - 0.5) ? -1.0 : dx;
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
    // and the colour-emoji flag (bit 15, #284).
    uint slot = v_glyph & 0x1FFFu;
    uint layer = slot >> 5u;   // 32 glyphs stack vertically per layer
    uint band = slot & 31u;
    // Inset the cell-local texcoord into the padded atlas slot's content region, so the transparent
    // guard band is never sampled (beamterm cell.frag) — stops band bleed while the content maps
    // edge-to-edge of the CELL. Block elements are baked at cell size (#359), so they tile.
    vec2 inner = v_tex * (1.0 - 2.0 * u_padding_frac) + u_padding_frac;
    // Nudge off the exact texel edge so NEAREST can't round to a neighbour (beamterm cell.frag);
    // belt-and-suspenders for a fractional cell↔texel mapping (DPR != 1, #265).
    vec3 tc = vec3(inner.x + 0.001, (float(band) + inner.y + 0.001) / 32.0, float(layer));
    vec4 texel = texture(u_atlas, tc);
    float coverage = texel.a;

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
    float ul_band = hline(gy, 0.88, u_line_thickness, u_char_size.y) * underline;
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
    // Band-vs-band overlap is arithmetically out of reach: the centres are 0.38 of the glyph box
    // apart while `u_line_thickness / char_height` stays near 0.06 at every font size, so reaching it
    // needs a glyph box of about three device px. That is also why `cov` below may stay on `max`
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
    float bg_class = float((v_glyph >> 16u) & 1u);
    vec3 ink = mix(vec3(0.0), fg, coverage * bg_class);        // background-class ink joins the bg
    ink = mix(ink, base_ul, ul_band);                          // the band, over that background
    ink = mix(ink, fg, coverage * (1.0 - bg_class));           // text-class ink, over the band
    ink = mix(ink, base_st, st_band);
    ink = mix(ink, u_cursor_color, cur);

    // How much of the background survives every ink source above — the same coverages, as a product.
    // This REPLACES `max(coverage, max(ul_band, st_band))`, which was an approximation of the ink's
    // total weight and disagreed with the colour chain wherever two sources overlapped (a descender
    // crossing its underline is the reachable case, #712's own geometry). The two agreed only where
    // at most one source was partial, which is why it never showed while alpha was the only consumer.
    float w_bg = (1.0 - coverage * bg_class) * (1.0 - ul_band)
               * (1.0 - coverage * (1.0 - bg_class)) * (1.0 - st_band) * (1.0 - cur);

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
    float bg_alpha = (!block && v_bg_default > 0.5) ? u_bg_alpha : 1.0;
    float a = 1.0 - w_bg * (1.0 - bg_alpha);
    FragColor = vec4((ink + base_bg * (bg_alpha * w_bg)) / max(a, 1e-4), a);
}
"#;

/// Canvas `webglcontextlost` / `webglcontextrestored` listeners feeding a shared [`ContextState`]
/// (#269). The closures capture ONLY the `Rc`'d state — never the renderer — so they can fire while
/// a `&mut JustermRenderer` method is on the stack without a `RefCell` double-borrow.
struct ContextLossHandler {
    canvas: HtmlCanvasElement,
    state: Rc<RefCell<ContextState>>,
    /// Consumer callback for "the context did not come back within the deadline" (#327). `None`
    /// until injected, and cleared on `Drop` so a deadline that outlives the renderer finds nobody
    /// to call — the reason no `clearTimeout` is needed (see [`arm_restore_deadline`]).
    notify: Rc<RefCell<Option<js_sys::Function>>>,
    /// Consumer-injected grace period, in ms (ADR-0017: the renderer times, the consumer decides
    /// how long). Read when a loss arms its deadline.
    timeout_ms: Rc<Cell<i32>>,
    // Kept alive for as long as the listeners are attached; `Drop` detaches them.
    on_lost: Closure<dyn FnMut(web_sys::Event)>,
    on_restored: Closure<dyn FnMut(web_sys::Event)>,
}

/// Schedule the restore deadline for the loss episode `epoch` (#327).
///
/// The timer is **never cancelled**. `clearTimeout` would work — a merely-queued timer task aborts
/// when it finds its id gone from the map (HTML spec, timer initialization steps), which is how
/// xterm.js does it — but cancelling means *owning* the `Closure`, and the consumer's notification
/// handler is exactly the place that destroys the renderer (VSCode's `onContextLoss` calls
/// `_disposeOfWebglRenderer()`). Dropping the handler would free the very closure whose body is
/// running. JS gets away with this because its closures are garbage-collected; we cannot.
///
/// So the closure is handed to JS instead (`Closure::once_into_js` keeps it alive through an
/// internal `Rc` cycle that the single invocation breaks, freeing it *after* the body returns), and
/// every deadline that has nothing to say identifies itself: `on_restore_deadline` rejects it if the
/// context came back, if we already notified, or if it belongs to an earlier loss. A stale deadline
/// costs one no-op task.
///
/// The `epoch` is what makes this safe, and what it is safe *against* is the
/// **lost → restored → lost** order: the first loss's timer is still pending when the second loss
/// arms its own, and without the stamp it would land inside the second loss's grace period and cut
/// it short. That sequence is reachable — every transition into "lost" dispatches — and
/// `context_loss.rs`'s `a_deadline_left_over_from_a_previous_loss_never_notifies` is written on it.
///
/// **This used to claim more, and the extra claim was wrong** (measured 2026-08-04, #579). It said
/// the epoch made us stricter than xterm.js, whose single `_contextRestorationTimeout` is
/// overwritten without being cleared when a second `webglcontextlost` arrives *with no restore
/// between* (`WebglRenderer.ts:131`) — "both timers then fire and its `onContextLoss` is delivered
/// twice". The overwrite is real in its source; the antecedent is not reachable. A second
/// `WEBGL_lose_context.loseContext()` on an already-lost context delivers **no** second event
/// (headless Chromium: two `loseContext()` calls with no restore between produce exactly **1**
/// `webglcontextlost`), because the event fires on the transition into lost and an already-lost
/// context has none to make. So that comparison described a state neither implementation can be
/// put into. The epoch still earns its place on the order above — a favourable comparison is just
/// the kind nobody re-checks.
fn arm_restore_deadline(
    state: &Rc<RefCell<ContextState>>,
    notify: &Rc<RefCell<Option<js_sys::Function>>>,
    epoch: u32,
    timeout_ms: i32,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let (state, notify) = (Rc::clone(state), Rc::clone(notify));
    let deadline = Closure::once_into_js(move || {
        // Release the borrow before calling out to JS: the consumer's handler runs re-entrantly and
        // may touch the renderer (dispose it, poll `isRestoreOverdue`).
        let should_notify = state.borrow_mut().on_restore_deadline(epoch);
        if !should_notify {
            return;
        }
        // Clone the callback out for the same reason — the handler is free to replace it.
        let callback = notify.borrow().clone();
        if let Some(callback) = callback {
            let _ = callback.call0(&JsValue::NULL);
        }
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        deadline.unchecked_ref(),
        timeout_ms,
    );
}

impl ContextLossHandler {
    fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsValue> {
        let state = Rc::new(RefCell::new(ContextState::default()));
        let notify: Rc<RefCell<Option<js_sys::Function>>> = Rc::new(RefCell::new(None));
        let timeout_ms = Rc::new(Cell::new(DEFAULT_RESTORE_TIMEOUT_MS));

        let lost_state = Rc::clone(&state);
        let lost_notify = Rc::clone(&notify);
        let lost_timeout = Rc::clone(&timeout_ms);
        let on_lost = Self::listen(canvas, "webglcontextlost", move |event: web_sys::Event| {
            // Without `preventDefault()` the browser never fires `webglcontextrestored` — the
            // context stays dead forever. Every reference implementation does this first
            // (beamterm context_loss.rs, xterm.js WebglRenderer.ts).
            event.prevent_default();
            let epoch = {
                let mut state = lost_state.borrow_mut();
                state.on_lost();
                state.loss_epoch()
            };
            arm_restore_deadline(&lost_state, &lost_notify, epoch, lost_timeout.get());
        })?;

        let restored_state = Rc::clone(&state);
        let on_restored = Self::listen(canvas, "webglcontextrestored", move |_event| {
            restored_state.borrow_mut().on_restored();
        })?;

        Ok(Self {
            canvas: canvas.clone(),
            state,
            notify,
            timeout_ms,
            on_lost,
            on_restored,
        })
    }

    fn listen(
        canvas: &HtmlCanvasElement,
        event: &str,
        f: impl 'static + FnMut(web_sys::Event),
    ) -> Result<Closure<dyn FnMut(web_sys::Event)>, JsValue> {
        let closure = Closure::wrap(Box::new(f) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
        Ok(closure)
    }
}

impl Drop for ContextLossHandler {
    fn drop(&mut self) {
        // A restore deadline may still be pending in the browser and we do not cancel it (see
        // `arm_restore_deadline`), so disarm it at the other end: with no callback there is nobody
        // to notify, and the `Rc`s it captured keep its state alive until it runs once and frees
        // itself. Same observable contract as xterm.js's `clearTimeout` on dispose
        // (WebglRenderer.ts:161-163).
        *self.notify.borrow_mut() = None;
        for (event, closure) in [
            ("webglcontextlost", self.on_lost.as_ref()),
            ("webglcontextrestored", self.on_restored.as_ref()),
        ] {
            let _ = self
                .canvas
                .remove_event_listener_with_callback(event, closure.unchecked_ref());
        }
    }
}

/// The GL program + buffers + uniform locations built once at startup (`build_pipeline`).
struct Pipeline {
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    u_char_size: glow::UniformLocation,
    u_char_offset: glow::UniformLocation,
    u_line_thickness: glow::UniformLocation,
    u_padding_frac: glow::UniformLocation,
    u_bg_alpha: glow::UniformLocation,
    u_cursor: glow::UniformLocation,
    u_cursor_color: glow::UniformLocation,
    u_cursor_text_color: glow::UniformLocation,
    u_cursor_thickness: glow::UniformLocation,
}

/// The justerm-family WebGL2 terminal renderer.
#[wasm_bindgen]
pub struct JustermRenderer {
    gl: glow::Context,
    /// The bound canvas — kept so `resize` can size its drawing buffer (device px) and CSS box.
    canvas: HtmlCanvasElement,
    /// devicePixelRatio the atlas + drawing buffer are currently sized for (#265). The atlas is
    /// rasterised at `font_size * dpr` (device px) so HiDPI stays sharp; a DPR change — or a
    /// `set_font_size` (#406) / `set_font_family` (#413) — re-bakes it.
    dpr: f32,
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    atlas: glow::Texture,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    u_char_size: glow::UniformLocation,
    u_char_offset: glow::UniformLocation,
    u_line_thickness: glow::UniformLocation,
    u_bg_alpha: glow::UniformLocation,
    u_cursor: glow::UniformLocation,
    u_cursor_color: glow::UniformLocation,
    u_cursor_text_color: glow::UniformLocation,
    u_cursor_thickness: glow::UniformLocation,
    /// The cursor this frame, or `None` for hidden / blinked off (#270). Blink timing is the
    /// consumer's policy, as `blink_on` is (#282) — the renderer only draws what it is handed.
    cursor: Option<Cursor>,
    /// The cells the cursor covers — `(start column, span)`. The start is not always the cursor's
    /// own column: a caret resting on a wide glyph's trailing spacer moves back onto the lead, so
    /// the pair is lit as one thing (#454, `cursor::cursor_cells_at`).
    cursor_cells: (u32, u32),
    /// The last frame's cell flags + width, kept so `setCursor` can resolve the span of a cursor
    /// that moves onto a wide char with no new frame. Without it a caret moved onto a CJK glyph
    /// would half-cover it until the next `applyFrame`.
    last_flags: Vec<u16>,
    last_cols: u32,
    /// Background cell opacity (0 = transparent, 1 = opaque), consumer-injected policy (#298).
    bg_alpha: f32,
    /// The minimum WCAG contrast a cursor must have with the cell it sits on, or it inverts to the
    /// default fg/bg to stay visible (#368). Consumer-injected policy (the mechanism is the
    /// renderer's — only it has the resolved cell RGB); `1.0` disables the guard.
    cursor_contrast: f32,
    /// The stroke thickness as a fraction of the cell width (#369), turned into device pixels by
    /// `cursor_thickness`. Consumer-injected policy (ADR-0017) — the pixel mechanism is the
    /// renderer's, the fraction is the consumer's. Default `0.15` (alacritty's `cursor.thickness`),
    /// clamped to `[0, 1]`; a **block** ignores it (it recolours its cell, drawing no stroke).
    cursor_thickness_frac: f32,
    /// The glyph box in device px — the rasteriser's ink-scan of `█`. Equal to `cell_size` only
    /// while both spacing options are at their defaults (#338).
    char_size: (u32, u32),
    /// Where the glyph box sits inside the cell, device px from its top-left (#338).
    char_offset: (u32, u32),
    /// `MAX_TEXTURE_SIZE`, read once. The atlas is `padded_w x padded_h * GLYPHS_PER_LAYER`, so a tall
    /// `lineHeight` can ask for a texture the implementation refuses to allocate — silently (#359).
    max_texture_size: u32,
    /// Consumer-injected policy (ADR-0017), in **CSS px** — see `metrics::device_cell` for why the
    /// references' device-px choice is not ours (#338).
    letter_spacing: f32,
    /// Consumer-injected policy: a multiplier on the glyph height. Clamped to `>= 1` (#338).
    line_height: f32,
    /// Consumer-injected font size in **CSS px** (#406); the atlas rasterises at `font_size * dpr`.
    /// Default [`FONT_SIZE`]. Changed by `set_font_size`, which re-bakes the atlas (same seam as a
    /// DPR change), so a restored context bakes at the consumer's size, not the hardcoded default.
    font_size: f32,
    /// Consumer-injected CSS `font-family` (#413); default `"monospace"`. Changed by `set_font_family`,
    /// which re-bakes the atlas — same seam as a size change — so a restored context bakes the
    /// consumer's family. The browser's text engine resolves it (with fallback); the renderer stays
    /// font-agnostic.
    font_family: String,
    palette: Palette,
    rasterizer: Rasterizer,
    cache: GlyphCache,
    /// Physical (content) cell size in **device pixels** — the on-screen grid cell, and the exact
    /// `u_cell_size` the shader lays it out with. Integral by construction (an ink-scan).
    cell_size: (u32, u32),
    /// Padded atlas cell size in device pixels (physical + `2*PADDING`) — glyph upload dims.
    atlas_cell: (u32, u32),
    /// The same WebGL2 context `gl` wraps, kept for the handful of questions glow does not ask:
    /// `drawingBufferWidth`/`drawingBufferHeight`, which are the ONLY way to learn that the browser
    /// clamped the buffer we requested (#339). Context restore reuses the context object, so this
    /// handle survives a loss.
    raw_gl: WebGl2RenderingContext,
    /// Drawing-buffer size in device pixels.
    size: (i32, i32),
    /// The `cols`×`rows` grid last passed to [`resize`](Self::resize). A DPR change re-measures the
    /// cell and re-derives the buffer from this, so nothing has to be re-passed and no CSS length is
    /// rounded twice (#322/#331).
    grid_size: (u32, u32),
    instances: Vec<f32>,
    instance_count: i32,
    /// The instance floats currently in the GPU buffer — the baseline the next pack diffs against
    /// so only changed cells re-upload (#263). Empty until the first upload (forces a `Full`).
    ///
    /// INVARIANT: this mirrors what the live `instance_vbo` holds, so it is valid ONLY while that
    /// buffer persists. WebGL **context loss** destroys the buffer, so [`restore`](Self::restore)
    /// calls [`invalidate_baseline`] on it — otherwise the next identical frame diffs to zero
    /// ranges and never refills the fresh (empty) buffer → a blank render that won't self-heal.
    /// (Surfaced by the #263 adversarial 2-lens pass; implemented in #269.)
    uploaded: Vec<f32>,
    /// Persistent dense grid for the decoder→renderer frame adapter (#277): a Partial frame's
    /// span-ordered damage scatters into this before packing. `None` until the first
    /// `apply_damage`; re-created when the grid dimensions change.
    grid: Option<FrameGrid>,
    /// The selection / search overlay spans this frame (#271), owned so a re-pack can borrow them.
    /// Stride-3 `(row, left, right)` viewport triples, as the decoder ships them. Empty = no
    /// highlight. Updated by [`set_overlay`](Self::set_overlay); composited into each cell's packed
    /// background at pack time.
    selection_spans: Vec<u32>,
    match_spans: Vec<u32>,
    /// The *active* (focused/current) search-match spans (#427), same stride. Set via
    /// [`set_active_match`](Self::set_active_match); the active match is also present in
    /// `match_spans`, and the `highlight_at` ranking (ActiveMatch > Selection > Match) is what makes
    /// its colour win where they overlap. Empty = no active match.
    active_match_spans: Vec<u32>,
    /// The in-progress IME composition and the cell it is anchored to (#249, ADR-0028). Empty = no
    /// composition. Unlike every other retained state here this describes something the **engine
    /// never sees** — the preedit reaches no frame and no wire — so it can only arrive from the
    /// consumer's browser events, and it is a *pass* over the composed cells rather than a layer in
    /// the stack (ADR-0019's amendment).
    preedit_run: Vec<PreeditCodepoint>,
    preedit_col: u32,
    preedit_row: u32,
    /// The consumer-injected blend colours for the overlay kinds (policy #115).
    highlight_colors: HighlightColors,
    /// Draw bold text in the bright (8–15) ANSI colour (#223/#272), consumer policy (xterm's
    /// `drawBoldTextInBrightColors`). Default on, as xterm; toggled via `set_bold_to_bright`.
    bold_to_bright: bool,
    /// Minimum WCAG fg/bg contrast ratio (#225/#272), consumer policy (xterm's `minimumContrastRatio`).
    /// `1.0` = off (default). Set via `set_minimum_contrast_ratio`; clamped to `[1, 21]`.
    min_contrast: f32,
    /// Force a SELECTED cell's fg to this packed `0xRRGGBB` (#227/#272, xterm's `selectionForeground`).
    /// `None` = keep each cell's own fg (default). Selection only, never a search match.
    selection_fg: Option<u32>,
    /// Marker-anchored decoration rects this frame (#393), the flat `DECORATION_STRIDE` wire the
    /// consumer projects each frame. Parsed at pack time; empty = no decorations. Owned so a re-pack
    /// can borrow it. Updated by [`set_decorations`](Self::set_decorations).
    decoration_spans: Vec<u32>,
    /// The last blink phase packed, so a [`set_overlay`](Self::set_overlay) re-pack (no new frame)
    /// keeps the cursor/blink cells in the phase the render loop last drove.
    last_blink_on: bool,
    /// Set by every state mutation that changes the packed instance buffer (overlay, decorations,
    /// colour policy, palette, `apply_damage`); cleared by the re-pack in [`render`](Self::render).
    /// Lets a frame that sets overlay + decorations + damage re-pack **once** at render instead of
    /// three times, one per setter (#421). The direct `apply_frame` path packs immediately (no grid
    /// to defer to) and clears it.
    needs_repack: bool,
    /// Count of `resolve_and_pack` runs — a diagnostic the proofs read to assert render packs once
    /// per frame, not once per setter (#421). Wraps harmlessly; only deltas are meaningful.
    pack_count: u32,
    /// Canvas context-loss listeners + the shared lost/pending-rebuild state (#269). `render`
    /// consults it every frame: skip while lost, rebuild once restored, otherwise draw.
    ctx_loss: ContextLossHandler,
}

/// Reinterpret an `f32` slice as bytes for `buffer_data` upload.
fn f32_bytes(v: &[f32]) -> &[u8] {
    // Safety: `f32` has no padding/invalid bytes; length is exact.
    unsafe { core::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// Upload one glyph's RGBA bitmap to its `(layer, band)` in the atlas. A free function (not
/// a `&self` method) so the frame resolver's upload closure can borrow only the GL fields,
/// leaving `&mut self.cache` free for [`glyph_resolve::resolve_frame`].
fn upload_glyph(
    gl: &glow::Context,
    atlas: glow::Texture,
    cell_size: (u32, u32),
    slot: u16,
    rgba: &[u8],
) {
    let (cell_w, cell_h) = (cell_size.0 as i32, cell_size.1 as i32);
    let (layer, band) = slot_texcoord(slot);
    // Safety: live GL context; the sub-image fits the allocated storage.
    unsafe {
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(atlas));
        gl.tex_sub_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            0,
            band as i32 * cell_h,
            layer as i32,
            cell_w,
            cell_h,
            1,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(rgba)),
        );
    }
}

#[wasm_bindgen]
impl JustermRenderer {
    /// Bind a renderer to the canvas matched by `canvas_selector`. `palette_colors` must be
    /// the 256 pre-built indexed colours (see `Palette::from_colors`).
    #[wasm_bindgen(constructor)]
    pub fn new(
        canvas_selector: &str,
        palette_colors: Vec<u32>,
        default_fg: u32,
        default_bg: u32,
    ) -> Result<JustermRenderer, JsValue> {
        console_error_panic_hook::set_once();

        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no document"))?;
        let canvas: HtmlCanvasElement = document
            .query_selector(canvas_selector)?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: canvas not found"))?
            .dyn_into()?;
        // Request a non-premultiplied alpha context so the shader's straight-colour output
        // composites correctly over the page when the background is translucent (#298). `alpha`
        // is already the WebGL default; setting it explicitly documents the intent.
        let ctx_opts = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&ctx_opts, &"alpha".into(), &JsValue::TRUE);
        let _ = js_sys::Reflect::set(&ctx_opts, &"premultipliedAlpha".into(), &JsValue::FALSE);
        let webgl2: WebGl2RenderingContext = canvas
            .get_context_with_context_options("webgl2", &ctx_opts)?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no webgl2 context"))?
            .dyn_into()?;

        // `getContext` succeeding is NOT a liveness property (#688): on a canvas whose context is
        // already lost it hands back the SAME lost object. Everything below then runs on a dead
        // context, and the very first thing that does is glow's own constructor — which enumerates
        // the extensions (`get_supported_extensions().unwrap()`, glow `web_sys.rs:237-239`) and
        // PANICS on the `null` a lost context answers with, before any code here is reached. A
        // panic crosses into JS as a `RuntimeError`, not as this crate's `Err`, so a consumer's
        // `catch` gets something no other failure here produces.
        //
        // One check covers the whole constructor, and NOT because a context cannot die inside it —
        // it can; what cannot arrive inside it is the *report*, since `webglcontextlost` dispatches
        // at a task boundary and everything from here to the final `resize` is synchronous. The
        // guard is sufficient for a different reason, and this is the load-bearing half: every
        // remaining glow call on this path fails **cleanly**. `create_*` return `Result`,
        // `get_uniform_location` an `Option`, the status getters `.as_bool().unwrap_or(false)` —
        // so a context dying mid-construction yields this crate's bare-string `Err`, never the
        // `RuntimeError` a panic produces. The check above exists to cover the one call that does
        // NOT have that shape.
        //
        // It asks the CONTEXT, not this crate's own state machine. The machine's flag is fresh
        // here and therefore always `false` — spine #689's proxy ①, and not a weaker predicate but
        // a constant. Measured as a mutation on `demo/context-loss-construct.html`, whose
        // pre-dispatch section is exactly where the two answers differ.
        if webgl2.is_context_lost() {
            return Err(JsValue::from_str(
                "justerm-renderer: webgl2 context is lost",
            ));
        }

        // Attached before any GL work, including glow's constructor below — but read what that
        // does and does not buy (#688). It cannot *catch* a loss during construction: listeners
        // fire at a task boundary and there is none between here and the end of this function, so
        // this site and the old one (seven lines below, under the first GL call) observe exactly
        // the same set of events — none. What it buys is that the promise this comment makes is
        // now true, and that the handler is in place before the first thing that could need it.
        // The original wording claimed the stronger property; a reader deriving from it would
        // conclude a mid-construction loss is reported, and it is not (#269).
        let ctx_loss = ContextLossHandler::new(&canvas)?;

        let raw_gl = webgl2.clone();
        let gl = glow::Context::from_webgl2_context(webgl2);
        // Read once: the atlas is sized from the cell (#359), and the cell is the consumer's to grow.
        // `.max(1)` defends glow's fallback, not the driver: `get_parameter_i32` answers `0` for a
        // `null` rather than failing (glow `web_sys.rs:3590`), so the clamp is about the binding's
        // shape, not about a limit any live implementation would report (#688 — and the guard above
        // is what keeps a `null` from reaching here in the first place).
        let max_texture_size =
            unsafe { gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE).max(1) as u32 };
        let size = (canvas.width() as i32, canvas.height() as i32);

        // devicePixelRatio: rasterise the atlas at device px (FONT_SIZE * dpr) so HiDPI is sharp,
        // and size the drawing buffer in device px. The consumer speaks CSS px (#252); the renderer
        // owns the DPR (beamterm `device_pixel_ratio`). Fallback 1.0 off the main thread / in tests.
        let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio() as f32);

        let palette =
            Palette::from_colors(&palette_colors, default_fg, default_bg).map_err(|e| {
                JsValue::from_str(&format!(
                    "justerm-renderer: palette must be 256 colours, got {}",
                    e.got
                ))
            })?;

        let rasterizer = Rasterizer::new("monospace", FONT_SIZE * dpr)?;
        // The rasteriser measures the GLYPH box; the grid cell is that box plus the consumer's
        // spacing policy, which starts at its identity (#338). The atlas slot is the padded CELL
        // (#359), so the rasteriser must know it before anything is sized from `padded_size()`.
        let char_size = rasterizer.glyph_box();
        let (letter_spacing, line_height) = (0.0f32, 1.0f32);
        let (cell_w, cell_h) = fit_cell_to_atlas(
            device_cell(char_size, letter_spacing, line_height, dpr),
            PADDING,
            GLYPHS_PER_LAYER as u32,
            max_texture_size,
        );
        let char_offset = glyph_offset((cell_w, cell_h), char_size);
        let mut rasterizer = rasterizer;
        rasterizer.set_cell((cell_w, cell_h), char_offset)?;
        let (pad_w, pad_h) = rasterizer.padded_size(); // padded atlas cell

        let Pipeline {
            program,
            vao,
            instance_vbo,
            u_projection,
            u_cell_size,
            u_char_size,
            u_char_offset,
            u_line_thickness,
            u_padding_frac,
            u_bg_alpha,
            u_cursor,
            u_cursor_color,
            u_cursor_text_color,
            u_cursor_thickness,
        } = Self::build_pipeline(&gl)?;
        // The atlas stores padded cells; the glyph is drawn inset by PADDING.
        let atlas = Self::build_atlas(&gl, pad_w, pad_h)?;
        // Tell the shader how much of each padded atlas cell is guard band, so it insets the
        // texcoord to the content region (see FRAG_SRC).
        unsafe {
            gl.use_program(Some(program));
            gl.uniform_2_f32(
                Some(&u_padding_frac),
                PADDING as f32 / pad_w as f32,
                PADDING as f32 / pad_h as f32,
            );
        }

        let mut renderer = JustermRenderer {
            gl,
            canvas,
            dpr,
            program,
            vao,
            instance_vbo,
            atlas,
            u_projection,
            u_cell_size,
            u_char_size,
            u_char_offset,
            u_line_thickness,
            u_bg_alpha,
            u_cursor,
            u_cursor_color,
            u_cursor_text_color,
            u_cursor_thickness,
            cursor: None,
            cursor_cells: (0, 1),
            last_flags: Vec::new(),
            last_cols: 0,
            bg_alpha: 1.0,                            // opaque by default (#298)
            cursor_contrast: DEFAULT_CURSOR_CONTRAST, // guard on by default (#368)
            cursor_thickness_frac: THICKNESS,         // alacritty's 0.15 by default (#369)
            palette,
            rasterizer,
            cache: GlyphCache::new(),
            cell_size: (cell_w, cell_h),
            char_size,
            char_offset,
            letter_spacing,
            line_height,
            font_size: FONT_SIZE,
            font_family: "monospace".to_string(),
            atlas_cell: (pad_w, pad_h),
            max_texture_size,
            raw_gl,
            size,
            // Whole cells that fit the canvas as authored. `resize` below snaps the buffer to
            // exactly that grid, so `size == grid_px(grid_size, cell_size)` holds from the start.
            grid_size: (
                (size.0 as u32 / cell_w).max(1),
                (size.1 as u32 / cell_h).max(1),
            ),
            instances: Vec::new(),
            instance_count: 0,
            uploaded: Vec::new(),
            grid: None,
            selection_spans: Vec::new(),
            match_spans: Vec::new(),
            active_match_spans: Vec::new(), // no active/focused match by default (#427)
            preedit_run: Vec::new(),        // no composition open (#249)
            preedit_col: 0,
            preedit_row: 0,
            highlight_colors: HighlightColors::default(),
            bold_to_bright: true, // xterm's drawBoldTextInBrightColors default (#223)
            min_contrast: 1.0,    // xterm's minimumContrastRatio default: off (#225)
            selection_fg: None,   // no selectionForeground override by default (#227)
            decoration_spans: Vec::new(), // no marker decorations by default (#393)
            last_blink_on: true,
            needs_repack: false,
            pack_count: 0,
            ctx_loss,
        };
        renderer.prebake_ascii()?;
        // Snap the drawing buffer to a whole number of cells straight away, so the invariant
        // `size == grid_px(grid_size, cell_size)` holds for the renderer's whole life and never has
        // to be re-established (beamterm's `create_with_canvas` likewise ends in a `resize`).
        let (cols, rows) = renderer.grid_size;
        renderer.resize(cols, rows);
        Ok(renderer)
    }

    fn build_pipeline(gl: &glow::Context) -> Result<Pipeline, JsValue> {
        let program = Self::link_program(gl, VERT_SRC, FRAG_SRC)?;

        // Safety: all calls are on a live GL context; buffers/attribs are set up once.
        unsafe {
            let vao = gl.create_vertex_array().map_err(js_err)?;
            gl.bind_vertex_array(Some(vao));

            // Per-vertex quad geometry → location 0.
            let quad_vbo = gl.create_buffer().map_err(js_err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, f32_bytes(&QUAD), glow::STATIC_DRAW);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.enable_vertex_attrib_array(0);

            // Per-instance [col, row, bg(3), fg(3), glyph, underline_fg, strike_fg, bg_default]
            // → locations 1..7.
            let instance_vbo = gl.create_buffer().map_err(js_err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
            for (loc, size, offset) in [
                (1u32, 2i32, 0i32),
                (2, 3, 8),
                (3, 3, 20),
                (4, 1, 32),
                (5, 1, 36),
                (6, 1, 40),
                (7, 1, 44),
            ] {
                gl.vertex_attrib_pointer_f32(
                    loc,
                    size,
                    glow::FLOAT,
                    false,
                    INSTANCE_STRIDE,
                    offset,
                );
                gl.enable_vertex_attrib_array(loc);
                gl.vertex_attrib_divisor(loc, 1);
            }

            gl.bind_vertex_array(None);

            let u_projection = uniform(gl, program, "u_projection")?;
            let u_cell_size = uniform(gl, program, "u_cell_size")?;
            let u_char_size = uniform(gl, program, "u_char_size")?;
            let u_char_offset = uniform(gl, program, "u_char_offset")?;
            let u_line_thickness = uniform(gl, program, "u_line_thickness")?;
            let u_padding_frac = uniform(gl, program, "u_padding_frac")?;
            let u_bg_alpha = uniform(gl, program, "u_bg_alpha")?;
            let u_cursor = uniform(gl, program, "u_cursor")?;
            let u_cursor_color = uniform(gl, program, "u_cursor_color")?;
            let u_cursor_text_color = uniform(gl, program, "u_cursor_text_color")?;
            let u_cursor_thickness = uniform(gl, program, "u_cursor_thickness")?;
            // The atlas sampler stays on texture unit 0.
            gl.use_program(Some(program));
            let u_atlas = uniform(gl, program, "u_atlas")?;
            gl.uniform_1_i32(Some(&u_atlas), 0);

            Ok(Pipeline {
                program,
                vao,
                instance_vbo,
                u_projection,
                u_cell_size,
                u_char_size,
                u_char_offset,
                u_line_thickness,
                u_padding_frac,
                u_bg_alpha,
                u_cursor,
                u_cursor_color,
                u_cursor_text_color,
                u_cursor_thickness,
            })
        }
    }

    /// Allocate the glyph atlas texture array: `cell_w` × (`32*cell_h`) × `TOTAL_LAYERS`,
    /// RGBA8 (glyph coverage in the alpha channel).
    fn build_atlas(gl: &glow::Context, cell_w: u32, cell_h: u32) -> Result<glow::Texture, JsValue> {
        // Safety: live GL context.
        unsafe {
            let tex = gl.create_texture().map_err(js_err)?;
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
            gl.tex_storage_3d(
                glow::TEXTURE_2D_ARRAY,
                1,
                glow::RGBA8,
                cell_w as i32,
                (cell_h * GLYPHS_PER_LAYER as u32) as i32,
                TOTAL_LAYERS,
            );
            // NEAREST (matching beamterm): 32 glyphs pack vertically per layer with no
            // guard band, so LINEAR would interpolate across a band seam (adjacent glyph
            // bleed) under mediump precision or a non-1:1 cell↔texel mapping (#265 DPR).
            gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            Ok(tex)
        }
    }

    fn link_program(gl: &glow::Context, vert: &str, frag: &str) -> Result<glow::Program, JsValue> {
        // Safety: all calls are on a live GL context.
        unsafe {
            let program = gl.create_program().map_err(js_err)?;
            let mut shaders = Vec::with_capacity(2);
            for (kind, src) in [(glow::VERTEX_SHADER, vert), (glow::FRAGMENT_SHADER, frag)] {
                let shader = gl.create_shader(kind).map_err(js_err)?;
                gl.shader_source(shader, src);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    return Err(js_err(gl.get_shader_info_log(shader)));
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(js_err(gl.get_program_info_log(program)));
            }
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            Ok(program)
        }
    }

    /// Rasterise + upload the 95 normal-styled ASCII glyphs into their fixed fast-path
    /// slots (`0..=94`), so a cell using the ASCII fast path samples a real bitmap.
    fn prebake_ascii(&self) -> Result<(), JsValue> {
        self.prebake_ascii_into(&self.rasterizer, self.atlas, self.atlas_cell)
    }

    /// Bake the 95 normal ASCII glyphs into `atlas` using `rasterizer` — parameterised so a DPR
    /// re-bake (#322) can prime a *new* atlas before committing it (see [`set_device_pixel_ratio`]).
    ///
    /// [`set_device_pixel_ratio`]: Self::set_device_pixel_ratio
    fn prebake_ascii_into(
        &self,
        rasterizer: &Rasterizer,
        atlas: glow::Texture,
        atlas_cell: (u32, u32),
    ) -> Result<(), JsValue> {
        for cp in 0x20u32..=0x7E {
            let ch = char::from_u32(cp).unwrap();
            let rgba = rasterizer.rasterize(&ch.to_string(), FontStyle::Normal, false)?;
            upload_glyph(&self.gl, atlas, atlas_cell, (cp - 0x20) as u16, &rgba);
        }
        Ok(())
    }

    /// Bake the renderer's ENTIRE current glyph set — the 95 prebaked ASCII plus every resident
    /// dynamic glyph — into `atlas`, each into the SAME slot it already occupies. Preserving the
    /// slots is what lets the packed `instances` stay valid across the re-bake (no re-pack, no
    /// re-resolve). Shared by the DPR re-bake (#322) and the context-loss restore (#269), which
    /// both need "a fresh, correctly-sized atlas holding what the old one held".
    fn bake_all_glyphs(
        &self,
        rasterizer: &Rasterizer,
        atlas: glow::Texture,
        atlas_cell: (u32, u32),
    ) -> Result<(), JsValue> {
        let (pad_w, pad_h) = atlas_cell;
        self.prebake_ascii_into(rasterizer, atlas, atlas_cell)?;
        for (k, slot) in self.cache.entries() {
            // Two-cell iff the slot lives in the wide region — true for both `Wide` (CJK) and a
            // *wide* `Emoji` (2-cell colour emoji); a narrow `Emoji` (#297 EmojiNarrow) sits in
            // the normal region and is one cell. Keying off `slot_id() >= WIDE_BASE` (not
            // `matches!(Wide)`) is what catches the wide-emoji case.
            let wide = slot.slot_id() >= WIDE_BASE;
            let rgba = rasterizer.rasterize(&k.text, k.style, wide)?;
            let base = slot.slot_id();
            if wide {
                let (left, right) = split_wide_bitmap(&rgba, 2 * pad_w - 2 * PADDING, pad_w, pad_h);
                upload_glyph(&self.gl, atlas, atlas_cell, base, &left);
                upload_glyph(&self.gl, atlas, atlas_cell, base + 1, &right);
            } else {
                upload_glyph(&self.gl, atlas, atlas_cell, base, &rgba);
            }
        }
        Ok(())
    }

    /// Point the shader at the guard-band fraction of a `pad_w`×`pad_h` atlas cell. The band is a
    /// fixed pixel count, so its *fraction* shifts whenever the padded cell is resized (#288/#322)
    /// — and the location belongs to `program`, so a relinked program (#269 restore) must re-set it.
    fn set_padding_frac(
        &self,
        program: glow::Program,
        u_padding_frac: &glow::UniformLocation,
        pad_w: u32,
        pad_h: u32,
    ) {
        unsafe {
            self.gl.use_program(Some(program));
            self.gl.uniform_2_f32(
                Some(u_padding_frac),
                PADDING as f32 / pad_w as f32,
                PADDING as f32 / pad_h as f32,
            );
        }
    }

    /// Notify the renderer that `window.devicePixelRatio` changed to `dpr` (#322). The consumer
    /// drives this from a resolution `matchMedia` listener — a DPR change at the *same* CSS size
    /// (dragging to another-density monitor) does not fire a resize, so it must be signalled
    /// explicitly. The atlas is re-baked at the new device size, the current grid re-resolved into
    /// it (so glyphs stay present and sharpen), and the drawing buffer re-derived from the stored
    /// grid at the new cell size. A no-op if the ratio is unchanged; on error the old atlas is left intact
    /// and `dpr` unadvanced, so the next notification retries (self-healing).
    #[wasm_bindgen(js_name = setDevicePixelRatio)]
    pub fn set_device_pixel_ratio(&mut self, dpr: f32) -> Result<(), JsValue> {
        if !dpr_changed(self.dpr, dpr) {
            return Ok(());
        }
        // A lost context can only hand back an invalidated atlas texture, so re-baking now would
        // burn the work and commit an empty atlas. Drop the notification: `restore` re-reads the
        // *live* DPR and bakes at that density anyway (#269).
        if self.gpu_work_must_wait() {
            return Ok(());
        }
        self.rebake_atlas(self.font_family.clone(), self.font_size, dpr)
    }

    /// Re-bake the atlas for `font_family` at `font_size` (CSS px) × `dpr` and re-derive the buffer
    /// from the stored grid. The shared body of a DPR change (#322), a font-size change (#406), and a
    /// font-family change (#413) — they differ only in which of the three inputs moved, so all three
    /// must re-rasterise the glyphs into a fresh atlas, re-derive the cell, and re-fit the buffer.
    /// Atomic: builds the replacement atlas + rasteriser first and commits (swapping them in +
    /// advancing `self.font_family`/`self.font_size`/`self.dpr`) only on success, so a failure leaves
    /// the old atlas / metrics untouched and the caller retries (self-healing). Assumes a **live**
    /// context — the callers skip when it is lost.
    fn rebake_atlas(
        &mut self,
        font_family: String,
        font_size: f32,
        dpr: f32,
    ) -> Result<(), JsValue> {
        // 1. Build a new atlas at the new device size and re-rasterise the CURRENT glyphs into it —
        //    ASCII fast path + every resident dynamic glyph, each into its SAME slot — all before
        //    committing. A failure leaves the old atlas / rasteriser / size / dpr untouched, and the
        //    glyph *slots* are preserved, so the existing instances stay valid (no re-pack / re-upload).
        //    The ~tens-of-µs cost (#321) is fine for a rare DPR / font-size / font-family change.
        let mut rasterizer = Rasterizer::new(&font_family, font_size * dpr)?;
        // The glyph box is re-measured at the new device size; the spacing policy survives, so the
        // cell must be re-derived from both BEFORE the atlas is sized (#322 + #338 + #359).
        let char_size = rasterizer.glyph_box();
        let cell = fit_cell_to_atlas(
            device_cell(char_size, self.letter_spacing, self.line_height, dpr),
            PADDING,
            GLYPHS_PER_LAYER as u32,
            self.max_texture_size,
        );
        rasterizer.set_cell(cell, glyph_offset(cell, char_size))?;
        let (pad_w, pad_h) = rasterizer.padded_size();
        let atlas = Self::build_atlas(&self.gl, pad_w, pad_h)?;
        let rebake = (|| -> Result<(), JsValue> {
            self.bake_all_glyphs(&rasterizer, atlas, (pad_w, pad_h))?;
            // The guard band's fraction of the (now device-sized) padded cell changed. Inside the
            // closure so a failure here is caught by the delete-on-error guard below (no leaked atlas).
            let u_padding_frac = uniform(&self.gl, self.program, "u_padding_frac")?;
            self.set_padding_frac(self.program, &u_padding_frac, pad_w, pad_h);
            Ok(())
        })();
        if let Err(e) = rebake {
            unsafe { self.gl.delete_texture(atlas) }; // don't leak the half-built atlas
            return Err(e);
        }
        // 2. Commit atomically: swap in the new atlas + rasteriser + metrics (KEEP the cache — its
        //    slots are now valid in the new atlas — and the instances, whose slots are unchanged),
        //    drop the old atlas, and advance `font_size`/`dpr` only now that the re-bake succeeded.
        let old_atlas = self.atlas;
        self.rasterizer = rasterizer;
        self.atlas = atlas;
        self.char_size = char_size;
        self.atlas_cell = (pad_w, pad_h);
        self.font_family = font_family;
        self.font_size = font_size;
        self.dpr = dpr;
        // The spacing policy survives; the cell it produces does not (#322 + #338 + #406).
        self.recompute_cell();
        unsafe { self.gl.delete_texture(old_atlas) };
        // 3. Re-derive the buffer from the stored grid at the NEW cell size (the cells sharpen via the
        //    new atlas + the new device `cell_size` uniform on the next render).
        let (cols, rows) = self.grid_size;
        self.resize(cols, rows);
        Ok(())
    }

    /// Set the font size in **CSS px** (#406) — re-bakes the atlas at `css_px * dpr`, exactly as a DPR
    /// change does (shared `rebake_atlas`), so glyphs stay sharp and the grid
    /// cell re-derives from the new size. Consumer policy (ADR-0017): the size is the consumer's, the
    /// atlas mechanism the renderer's. A non-finite size is ignored; a smaller-than-`1.0` one is
    /// clamped (a zero/negative size would rasterise a degenerate atlas). A no-op if unchanged.
    ///
    /// The cell size changes, so [`css_cell_width`](Self::css_cell_width)/`css_cell_height` move and
    /// **the consumer must re-fit** its column/row count and re-`resize`. Takes effect on the next
    /// [`render`](Self::render).
    #[wasm_bindgen(js_name = setFontSize)]
    pub fn set_font_size(&mut self, css_px: f32) -> Result<(), JsValue> {
        if !css_px.is_finite() {
            return Ok(());
        }
        let css_px = css_px.max(1.0);
        if (css_px - self.font_size).abs() < f32::EPSILON {
            return Ok(());
        }
        // Unlike the DPR, the font size is not re-read from anywhere external on a context restore —
        // it lives only here. So on a lost context, still advance the field (so `restore` bakes at the
        // new size) but skip the immediate re-bake (a dead atlas would burn the work, #269).
        if self.gpu_work_must_wait() {
            self.font_size = css_px;
            return Ok(());
        }
        self.rebake_atlas(self.font_family.clone(), css_px, self.dpr)
    }

    /// Set the font family (#413) — a CSS `font-family` string (`"monospace"`, `"'Fira Code', monospace"`,
    /// …) the browser's text engine resolves, with its own fallback. Re-bakes the atlas for the new
    /// family, exactly as a size change does (shared `rebake_atlas`): the glyph
    /// box is re-measured, so the grid cell re-derives. Consumer policy (ADR-0017) — the renderer stays
    /// font-agnostic; loading a webfont (`@font-face` / `FontFace`) before calling is the consumer's
    /// job (an unloaded family silently falls back). A no-op if unchanged.
    ///
    /// The cell size may change, so [`css_cell_width`](Self::css_cell_width)/`css_cell_height` can move
    /// and **the consumer must re-fit** its column/row count and re-`resize`. Takes effect on the next
    /// [`render`](Self::render).
    #[wasm_bindgen(js_name = setFontFamily)]
    pub fn set_font_family(&mut self, family: String) -> Result<(), JsValue> {
        if family == self.font_family {
            return Ok(());
        }
        // Like the font size (#406) and unlike the DPR, the family lives only in this field — it is not
        // re-read on a context restore. So on a lost context, advance the field (so `restore` bakes the
        // new family) but skip the immediate re-bake (a dead atlas would burn the work, #269).
        if self.gpu_work_must_wait() {
            self.font_family = family;
            return Ok(());
        }
        self.rebake_atlas(family, self.font_size, self.dpr)
    }

    /// Re-derive the grid cell and the glyph's place inside it from the current glyph box, DPR and
    /// spacing policy (#338). Every path that can change any of those four — construction, a DPR
    /// change (#322), a context restore (#269), and the font-size/family (#406/#413) + letter-spacing/
    /// line-height setters — goes through here, so they cannot drift apart. It does NOT resize the buffer; the caller does, because `resize` also
    /// re-reads what WebGL granted (#339).
    fn recompute_cell(&mut self) {
        let asked = device_cell(
            self.char_size,
            self.letter_spacing,
            self.line_height,
            self.dpr,
        );
        // Ask the implementation, do not predict it (#339's lesson, #359's bug): a cell the atlas
        // texture cannot hold leaves that texture storage-less, and a storage-less sampler answers
        // alpha 1 for every glyph. The terminal fills solid instead of failing.
        self.cell_size = fit_cell_to_atlas(
            asked,
            PADDING,
            GLYPHS_PER_LAYER as u32,
            self.max_texture_size,
        );
        self.char_offset = glyph_offset(self.cell_size, self.char_size);
    }

    /// Adopt a new cell in the rasteriser and rebuild the atlas around it (#359).
    ///
    /// The atlas slot is the padded CELL, so a spacing change resizes every slot and every baked
    /// bitmap: block elements must be redrawn at the new cell, and every other glyph re-inset. This
    /// is the same shape as a DPR change (#322) — build the replacement, commit only on success —
    /// and it is why `setLetterSpacing`/`setLineHeight` cost an atlas re-bake rather than a uniform.
    fn rebake_for_cell(&mut self) -> Result<(), JsValue> {
        self.rasterizer.set_cell(self.cell_size, self.char_offset)?;
        let (pad_w, pad_h) = self.rasterizer.padded_size();
        if (pad_w, pad_h) == self.atlas_cell {
            return Ok(()); // same slot: the resident bitmaps are still right
        }
        let atlas = Self::build_atlas(&self.gl, pad_w, pad_h)?;
        let rebake = (|| -> Result<(), JsValue> {
            self.bake_all_glyphs(&self.rasterizer, atlas, (pad_w, pad_h))?;
            let u_padding_frac = uniform(&self.gl, self.program, "u_padding_frac")?;
            self.set_padding_frac(self.program, &u_padding_frac, pad_w, pad_h);
            Ok(())
        })();
        if let Err(e) = rebake {
            unsafe { self.gl.delete_texture(atlas) };
            return Err(e);
        }
        let old = self.atlas;
        self.atlas = atlas;
        self.atlas_cell = (pad_w, pad_h);
        unsafe { self.gl.delete_texture(old) };
        Ok(())
    }

    /// Adopt a spacing policy, or leave every field exactly as it was (#338/#359).
    ///
    /// The atlas slot is the padded CELL, so a policy change re-bakes it. That can fail — the
    /// rasteriser draws through the browser's 2D engine — and a half-applied change is worse than a
    /// rejected one: `cell_size` would describe a cell the atlas does not hold, `draw` would send a
    /// `u_cell_size` the viewport was not sized for, and the next `apply_frame` would upload bitmaps
    /// of the wrong size into slots of the old one. `set_device_pixel_ratio` has always built its
    /// replacement in a local and committed only on success; these setters did not, and their comment
    /// claimed they did. Roll back instead.
    fn adopt_spacing(&mut self, letter_spacing: f32, line_height: f32) {
        let prev = (
            self.letter_spacing,
            self.line_height,
            self.cell_size,
            self.char_offset,
        );
        self.letter_spacing = letter_spacing;
        self.line_height = line_height;
        self.recompute_cell();

        // Keep the policy, defer the re-bake: an atlas built on a dead context comes back
        // invalidated, so the work would be burned and then committed empty. `restore` re-derives
        // everything — cell, atlas and buffer — from the surviving policy and the stored grid (#269).
        // Note that `recompute_cell` above has already run, so the *cell* moves immediately while
        // the atlas and the buffer wait; that ordering is a contract the consumer's docs describe.
        //
        // This used to be the resize's guard too, and said so: `resize` reads `drawingBufferWidth`
        // back (#339), which is 0 while lost, so the adopt-what-fits loop shrank the grid to 1x1.
        // `resize` guards itself as of #639 — on the read-back rather than on this predicate, since
        // it holds the answer — so returning here is now about the re-bake alone.
        if self.gpu_work_must_wait() {
            return;
        }
        if self.rebake_for_cell().is_err() {
            (
                self.letter_spacing,
                self.line_height,
                self.cell_size,
                self.char_offset,
            ) = prev;
            // The rasteriser was moved to the new cell before the bake; move it back, or it keeps
            // drawing bitmaps sized for a cell nothing else believes in.
            let _ = self.rasterizer.set_cell(prev.2, prev.3);
            return;
        }
        debug_assert_eq!(self.atlas_cell, self.rasterizer.padded_size());
        let (cols, rows) = self.grid_size;
        self.resize(cols, rows);
    }

    /// Extra space between columns, in **CSS pixels** — the consumer's policy (ADR-0017), applied
    /// as `round(letter_spacing * dpr)` device px on the cell (#338). May be negative, which
    /// narrows the cell and crops the glyph rather than stretching it; the cell never reaches zero.
    ///
    /// Both references take this in device px (xterm `WebglRenderer.ts:671`, alacritty
    /// `config/font.rs:20`), so the same setting is a different gap on a Retina display. Ours is
    /// the unit `FONT_SIZE` already speaks.
    #[wasm_bindgen(js_name = setLetterSpacing)]
    pub fn set_letter_spacing(&mut self, css_px: f32) {
        let ls = if css_px.is_finite() { css_px } else { 0.0 };
        self.adopt_spacing(ls, self.line_height);
    }

    /// A multiplier on the glyph height, `>= 1` — the consumer's policy (#338). Clamped rather than
    /// rejected: xterm throws from its option setter (`OptionsService.ts:182`), and a renderer that
    /// panics across the wasm boundary is a worse contract than one that reports what it adopted.
    /// Read the result back with [`cell_height`](Self::cell_height) — it may be smaller than asked,
    /// because a cell the atlas texture cannot hold is shrunk to one it can (#359).
    #[wasm_bindgen(js_name = setLineHeight)]
    pub fn set_line_height(&mut self, multiplier: f32) {
        let lh = if multiplier.is_finite() {
            multiplier.max(1.0)
        } else {
            1.0
        };
        self.adopt_spacing(self.letter_spacing, lh);
    }

    /// Whether the WebGL context is currently lost (#269). While lost the renderer draws nothing;
    /// it recovers by itself when the browser fires `webglcontextrestored`. Exposed so the consumer
    /// can surface the state (e.g. dim the terminal); no consumer action is required.
    ///
    /// This is the **event-driven** view — what the browser has told us — which is the honest thing
    /// to report to a consumer, and deliberately *not* what the crate's own internals guard on
    /// (`gpu_work_must_wait`, private): a context dies synchronously while its event is merely
    /// queued, so this answers `false` for a window in which every GL call is already dead. Read it
    /// as *"has a loss been reported"*, not *"is the GPU usable right now"* (#639).
    #[wasm_bindgen(js_name = isContextLost)]
    pub fn is_context_lost(&self) -> bool {
        self.ctx_loss.state.borrow().is_lost()
    }

    /// Whether GPU work must be deferred *right now* — the internal counterpart of
    /// [`is_context_lost`](Self::is_context_lost), and deliberately a different question (#639).
    ///
    /// It asks **two** sources because each covers a window the other misses, and both are
    /// event-vs-state races around the same pair of DOM events:
    ///
    /// - `raw_gl.is_context_lost()` — the context itself. The browser kills a context
    ///   **synchronously** and only *queues* `webglcontextlost`, so between those two moments the
    ///   state machine still says "live" while every GL call is already dead and
    ///   `drawingBufferWidth` already reads 0. Measured in Chromium: immediately after
    ///   `WEBGL_lose_context.loseContext()`, `gl.isContextLost()` is `true` and the flag below is
    ///   still `false`. Guarding on the flag alone is what let #639 survive its own first fix.
    /// - the state machine's flag — our own bookkeeping. The mirror window: a context can come back
    ///   before we have processed `webglcontextrestored`, so the GL answers "live" while the
    ///   program, VAO and atlas it owned are still the destroyed ones and `restore` has not run.
    ///   Baking into those would be as wrong as baking into a dead context.
    ///
    /// So: defer if **either** says so. Note that `resize` does not use this — it holds the actual
    /// answer, having just read the drawing buffer back, and guards on that instead.
    fn gpu_work_must_wait(&self) -> bool {
        self.raw_gl.is_context_lost() || self.ctx_loss.state.borrow().is_lost()
    }

    /// Register a callback invoked when a lost context has not been restored within the deadline
    /// (#327) — xterm.js's `onContextLoss`. It fires **at most once per loss**, and only if the
    /// context is still lost when the deadline lands.
    ///
    /// This is a *warning*, not a verdict: Chromium keeps re-attempting a real context restore once
    /// a second indefinitely, so a `webglcontextrestored` may still arrive afterwards, and the
    /// renderer will rebuild and repaint as usual. What to do in the meantime is consumer policy
    /// (ADR-0017) — VSCode tears its WebGL renderer down and falls back to a DOM one. The callback
    /// may safely destroy this renderer.
    #[wasm_bindgen(js_name = setOnContextLoss)]
    pub fn set_on_context_loss(&mut self, callback: js_sys::Function) {
        *self.ctx_loss.notify.borrow_mut() = Some(callback);
    }

    /// Override how long a lost context is given to come back before
    /// [`setOnContextLoss`](Self::set_on_context_loss) fires. Defaults to
    /// `DEFAULT_RESTORE_TIMEOUT_MS` (3000 ms, xterm.js parity). Applies to the *next* loss; a
    /// deadline already armed keeps the duration it was armed with. Negative values clamp to 0.
    #[wasm_bindgen(js_name = setContextRestoreTimeoutMs)]
    pub fn set_context_restore_timeout_ms(&mut self, ms: i32) {
        self.ctx_loss.timeout_ms.set(ms.max(0));
    }

    /// Whether a lost context has missed its restore deadline (#327). The poll counterpart of
    /// [`setOnContextLoss`](Self::set_on_context_loss), for a consumer that attaches late. Cleared
    /// by a late `webglcontextrestored`, which also heals the renderer.
    #[wasm_bindgen(js_name = isRestoreOverdue)]
    pub fn is_restore_overdue(&self) -> bool {
        self.ctx_loss.state.borrow().restore_overdue()
    }

    /// Recreate every GPU resource the lost context destroyed (#269), then refill the instance
    /// buffer so the very next `render` paints the pre-loss frame. Called by [`render`](Self::render)
    /// when the state machine reports [`FrameAction::Rebuild`] — never on a lost context, which is a
    /// property of the predicate that reports it rather than of this function: `Rebuild` requires
    /// the *context's own* answer as well as the flag, because the flag alone said "live" for the
    /// slice before a re-loss was dispatched and this ran there anyway (#695, ADR-0027 D3).
    ///
    /// The context *object* survives a loss (the browser reuses it; xterm.js keeps its `_gl` and
    /// beamterm's re-`getContext` hands back the same object), so only the objects it owned —
    /// program, VAO, buffers, atlas texture, and the uniform locations bound to that program — are
    /// rebuilt. CPU state (glyph cache, `instances`, `grid`, palette) survives untouched, which is
    /// what preserves the terminal's content across the loss.
    ///
    /// The DPR is re-read *first*, because the display may have changed density while the context
    /// was dead (#322 is the same re-bake driven by a `matchMedia` notification) — so the fresh
    /// atlas is baked once at the live density instead of baked at the stale one and immediately
    /// re-baked, as beamterm's `restore_context` → `handle_pixel_ratio_change` does.
    ///
    /// On any failure the old resources are left in place and `pending_rebuild` stays set, so the
    /// next frame retries (self-healing, mirroring [`set_device_pixel_ratio`](Self::set_device_pixel_ratio)).
    fn restore(&mut self) -> Result<(), JsValue> {
        let dpr = web_sys::window().map_or(self.dpr, |w| w.device_pixel_ratio() as f32);
        // Bake at the consumer's font family + size (#406/#413), not the hardcoded defaults — a
        // set_font_size/set_font_family that arrived while lost advanced the field but skipped the bake.
        let mut rasterizer = Rasterizer::new(&self.font_family, self.font_size * dpr)?;
        let (cell_w, cell_h) = rasterizer.glyph_box();
        // Same as the DPR path: the policy outlives the lost context, and the atlas slot is the cell.
        let cell = fit_cell_to_atlas(
            device_cell((cell_w, cell_h), self.letter_spacing, self.line_height, dpr),
            PADDING,
            GLYPHS_PER_LAYER as u32,
            self.max_texture_size,
        );
        rasterizer.set_cell(cell, glyph_offset(cell, (cell_w, cell_h)))?;
        let (pad_w, pad_h) = rasterizer.padded_size();

        // 1. Build the replacements without touching any live field.
        let pipeline = Self::build_pipeline(&self.gl)?;
        let atlas = Self::build_atlas(&self.gl, pad_w, pad_h)?;
        let rebake = (|| -> Result<(), JsValue> {
            self.bake_all_glyphs(&rasterizer, atlas, (pad_w, pad_h))?;
            // The guard-band fraction is set once per program at construction, so the relinked program
            // needs it again. (#298 translucency no longer needs a default-bg uniform — since #455 the
            // packer emits a per-cell `bg_default` provenance flag instead of the shader inferring it.)
            self.set_padding_frac(pipeline.program, &pipeline.u_padding_frac, pad_w, pad_h);
            Ok(())
        })();
        if let Err(e) = rebake {
            // Don't leak the half-built replacements; the live ones stay in place.
            unsafe {
                self.gl.delete_texture(atlas);
                self.gl.delete_program(pipeline.program);
                self.gl.delete_vertex_array(pipeline.vao);
                self.gl.delete_buffer(pipeline.instance_vbo);
            }
            return Err(e);
        }

        // 2. Commit. The outgoing GL objects died with the context, so deleting them is a no-op on
        //    the GL side — it only frees glow's handle slots.
        let (old_program, old_vao, old_vbo, old_atlas) =
            (self.program, self.vao, self.instance_vbo, self.atlas);
        self.program = pipeline.program;
        self.vao = pipeline.vao;
        self.instance_vbo = pipeline.instance_vbo;
        self.atlas = atlas;
        self.u_projection = pipeline.u_projection;
        self.u_cell_size = pipeline.u_cell_size;
        self.u_char_size = pipeline.u_char_size;
        self.u_line_thickness = pipeline.u_line_thickness;
        self.u_char_offset = pipeline.u_char_offset;
        self.u_bg_alpha = pipeline.u_bg_alpha;
        self.u_cursor = pipeline.u_cursor;
        self.u_cursor_color = pipeline.u_cursor_color;
        self.u_cursor_text_color = pipeline.u_cursor_text_color;
        self.u_cursor_thickness = pipeline.u_cursor_thickness;
        self.rasterizer = rasterizer;
        self.char_size = (cell_w, cell_h);
        // The spacing policy outlives the lost context (#269 + #338).
        self.recompute_cell();
        self.atlas_cell = (pad_w, pad_h);
        self.dpr = dpr;
        unsafe {
            self.gl.delete_texture(old_atlas);
            self.gl.delete_program(old_program);
            self.gl.delete_vertex_array(old_vao);
            self.gl.delete_buffer(old_vbo);
        }

        // 3. The new `instance_vbo` is empty and the baseline still describes the dead one — drop it
        //    so the refill below plans a `Full` upload even when the frame is byte-identical (#263).
        invalidate_baseline(&mut self.uploaded);

        // 4. The loss reset the drawing-buffer size and the viewport; re-derive them from the grid at
        //    the (possibly new) DPR, then refill the buffer so `render` draws the pre-loss frame.
        //    This is also where a `resize` that arrived *during* the loss gets its adopt-what-fits
        //    pass: it committed the grid and skipped the read-back, leaving that to this call (#639).
        //    Nothing extra is stored for it — the grid it committed IS `grid_size`.
        let (cols, rows) = self.grid_size;
        self.resize(cols, rows);
        self.upload_instances();
        Ok(())
    }

    /// The cell width in **device pixels** — exactly the `u_cell_size.x` the shader lays the grid
    /// out with: the rasteriser's ink-scan of `█` at `font_size * dpr`, **plus the consumer's
    /// `letterSpacing`** (#338). It is the *grid* cell, as xterm's `device.cell.width` is; the glyph
    /// box inside it is smaller whenever the spacing policy is not the identity.
    ///
    /// This is *the* cell (#331/#335). The bare name carries it because it is the exact, measured
    /// one, as in xterm.js's `dimensions.device.cell` and beamterm's `cell_size()`. Anything that
    /// addresses the drawing buffer — `readPixels`, GL interop, a picking rect — belongs here;
    /// [`css_cell_width`](Self::css_cell_width) is the derived view for CSS layout.
    pub fn cell_width(&self) -> u32 {
        self.cell_size.0
    }

    /// The cell height in **device pixels** (see [`cell_width`](Self::cell_width)).
    pub fn cell_height(&self) -> u32 {
        self.cell_size.1
    }

    /// The cell width in **CSS pixels**, unrounded. The consumer divides its available box by this
    /// to decide how many columns fit, exactly as xterm.js's `FitAddon` divides by
    /// `dimensions.css.cell.width`, and maps mouse coordinates through it (beamterm's
    /// `css_cell_size` doc says the same).
    ///
    /// It is a **float on purpose**. Rounding it to a whole CSS pixel loses the device cell for
    /// good — 33 device px at dpr 2 is 16.5, and 17 does not scale back to 33 (#331).
    #[wasm_bindgen(js_name = cssCellWidth)]
    pub fn css_cell_width(&self) -> f32 {
        css_px(self.cell_size.0, self.dpr)
    }

    /// The cell height in **CSS pixels**, unrounded (see [`css_cell_width`](Self::css_cell_width)).
    #[wasm_bindgen(js_name = cssCellHeight)]
    pub fn css_cell_height(&self) -> f32 {
        css_px(self.cell_size.1, self.dpr)
    }

    /// Size the renderer to a `cols`×`rows` **grid**. The drawing buffer becomes
    /// `cols * cell_width()` × `rows * cell_height()` device pixels — an exact multiple of the cell,
    /// so no column or row can be clipped by the buffer that holds it.
    ///
    /// This takes a grid, not a pixel box, deliberately (#331). Sizing the buffer from a pixel box
    /// while laying cells out as `cols * cell` computes the two from different quantities, and they
    /// stopped agreeing at fractional device pixel ratios — the last column fell outside its own
    /// buffer. xterm.js sizes its canvas from the grid the same way
    /// (`device.canvas.width = cols * device.cell.width`); beamterm instead derives the grid from the
    /// buffer and letterboxes the remainder with a padding colour. Both are sound; this one makes the
    /// overhang unrepresentable. The consumer already knows `cols`/`rows` — it computed them by
    /// dividing its box by [`css_cell_width`](Self::css_cell_width).
    ///
    /// **The consumer must size the canvas's CSS display box itself** (as with beamterm's
    /// `auto_resize_canvas_css = false`): [`css_width`](Self::css_width) and
    /// [`css_height`](Self::css_height) report what to set it to. Forget that and the device-px buffer
    /// is displayed at device px — twice its intended size on a Retina display.
    ///
    /// The atlas survives; a DPR change re-derives the buffer from this grid.
    ///
    /// **When the drawing buffer cannot be read, the grid is adopted but not verified** (#639). A
    /// resize can land at any moment in a context-loss window, and a consumer has no obligation to
    /// notice — so this takes the grid, sizes the canvas, and reports it back through
    /// [`cols`](Self::cols) / [`cssWidth`](Self::css_width) as usual, deferring only the one step
    /// that needs a live context: reading the buffer back to adopt what actually fits. `restore`
    /// re-derives the buffer from the stored grid, which is where that check happens instead.
    ///
    /// Note *"cannot be read"*, not *"the context is lost"*: the test is a non-positive read-back,
    /// because a browser kills a context synchronously and only queues `webglcontextlost`, so
    /// [`isContextLost`](Self::is_context_lost) still answers `false` for a window in which every
    /// GL call is already dead. Two consequences worth knowing: a clamp is normally visible in
    /// `cols`/`rows` the instant this returns, but one deferred this way settles at restore time
    /// with no signal; and during the window `cssWidth` describes a buffer that does not exist yet,
    /// so a consumer sizing its canvas from it overshoots.
    ///
    /// **That overshoot does not end at the restore, and this said it did** (corrected #717, after
    /// a consumer existed to measure it — #579). The restore fixes what *this crate* owns: `cols`,
    /// `size` and `cssWidth` are all correct on the far side of it. The display box is not ours —
    /// the consumer wrote it, from the provisional `cssWidth`, and nothing here can rewrite it. So
    /// it stays overshot until that consumer fits again. Measured through `justerm-web`: 4000
    /// columns asked for mid-loss leaves a `36000px` canvas box over the `8190px` buffer the
    /// browser granted, still there after the restoring `render`. The consumer's remedy is to
    /// repeat its fit once the context is back — **not** to re-read `cols`/`rows`, which by then
    /// are already right.
    pub fn resize(&mut self, cols: u32, rows: u32) {
        // A grid must have at least one cell: `grid_px` floors the *buffer* to 1, and letting
        // `grid_size` keep a 0 would break `size == grid_px(grid_size, cell_size)`.
        let (mut cols, mut rows) = (cols.max(1), rows.max(1));

        // WebGL is not obliged to give us the buffer we ask for (#339). The spec: "If the requested
        // width or height cannot be satisfied … a drawing buffer with smaller dimensions shall be
        // created. The dimensions actually used are implementation dependent and there is no
        // guarantee that a buffer with the same aspect ratio will be created." No exception, no lost
        // context (`webglcontextcreationerror` fires only at `getContext`), and `canvas.width` keeps
        // the value we asked for while `drawingBufferWidth` reports what we got.
        //
        // **Do not try to predict the limit.** Chromium's `WebGLRenderingContextBase::Reshape` first
        // clamps each axis to `min(max_texture_size, max_renderbuffer_size, max_viewport_dims[axis])`
        // and *then* applies a hard-coded `5760 * 5760` area budget, scaling both axes by
        // `sqrt(kMaxArea / area)`. Neither stage is derivable from a single `getParameter`, the area
        // constant is derivable from none of them, and the spec promises no rule at all. Measured:
        // 16385 wide comes back 16384 on a GPU / 8192 headless (texture size wins the `min` there),
        // and a square 8192x8192 comes back 5760x5760 on both. So: ask, then adopt what fits.
        //
        // No reference does this. xterm, beamterm and three.js all set `canvas.width` to the request
        // and draw into the granted buffer with `viewport(0, 0, drawingBufferWidth, …)`, leaving the
        // attribute oversized — their grids then overhang a clamped buffer, silently. We re-set the
        // canvas down instead, because #337 couples the CSS display box to `canvas.width`: a lying
        // attribute would make `cssWidth()` describe a buffer that does not exist. The extra
        // allocation is the price of that coupling; do not "simplify" it away.
        //
        // Two passes suffice, and the loop bound is only a backstop against a browser that clamps
        // non-monotonically. Pass 2 asks for a buffer the browser already granted (per-axis and by
        // area), so it cannot be clamped again; each pass shrinks at least one axis, so it ends.
        //
        // A pass that exhausts the bound without adopting anything would leave `canvas.width` at the
        // last request while `size`/`grid_size` still held the *previous* grid — three values
        // disagreeing. So the adoption is unconditional: the loop only refines what to adopt.
        let (mut dw, mut dh) = (1, 1);
        for _ in 0..4 {
            (dw, dh) = (
                grid_px(cols, self.cell_size.0),
                grid_px(rows, self.cell_size.1),
            );
            self.canvas.set_width(dw as u32);
            self.canvas.set_height(dh as u32);

            let (bw, bh) = (
                self.raw_gl.drawing_buffer_width(),
                self.raw_gl.drawing_buffer_height(),
            );

            // **A buffer of no size is not a grant, it is the absence of an answer** (#639). A lost
            // context reports 0x0, and `cells_that_fit(0, cell)` floors to 1 — so without this the
            // loop "adopts" a 1x1 grid, and `restore`, which re-derives the buffer from `grid_size`,
            // then rebuilds at it: the terminal comes back one cell wide, permanently and silently.
            // Measured, dpr 2: `resize(10, 3)` mid-loss gave `[1, 1]`, still `[1, 1]` after restore,
            // CSS box one cell. This used to add that a web consumer could not even see the loss to
            // hold its re-fit back "until #579"; that landed on 2026-08-04 and changes nothing here,
            // which is the point worth keeping. Seeing the loss is not being obliged to check for
            // it: a consumer re-fitting on a container resize has no reason to ask first, so the
            // guard cannot be delegated outward and this remains the only thing standing between a
            // mid-loss re-fit and a one-cell terminal.
            //
            // **This guards on the read-back rather than on the context's state, and that is the
            // load-bearing part.** The first fix for #639 asked `is_context_lost()` — the state
            // machine's flag — and the defect survived it verbatim, because the browser kills a
            // context *synchronously* and only queues `webglcontextlost`: in that window the flag
            // is still clear while `drawingBufferWidth` already reads 0. Measured in Chromium,
            // same task as `loseContext()`: `gl.isContextLost()` true, our flag false, `resize`
            // still adopting `[1, 1]`. The other four entry points cannot phrase the question this
            // way because they are not reading anything back, so they ask `gpu_work_must_wait()`
            // instead; this one has the answer in its hand and must not ask anyone else for it.
            //
            // Zero is never a legitimate clamp, so this costs nothing on a live context. What it
            // defers is the verification, not the grid: the caller's `cols`/`rows` are committed
            // below either way, and `restore` re-runs this loop on a live context and clamps then
            // (measured: `resize(4000, 8)` at `MAX_TEXTURE_SIZE` 8192 reports `[4000, 8]` during
            // the loss and `[1024, 8]` after the restore). The price is that a clamp settled at
            // restore time moves `grid_size`/`size` with **no signal**, where a clamp on a live
            // context is visible in `cols`/`rows` the moment this returns.
            if bw <= 0 || bh <= 0 {
                break;
            }

            if bw >= dw && bh >= dh {
                break; // granted in full; a larger grant is ignored, the grid still leads (#331)
            }
            (cols, rows) = (
                cells_that_fit(bw, self.cell_size.0),
                cells_that_fit(bh, self.cell_size.1),
            );
        }
        self.grid_size = (cols, rows);
        self.size = (dw, dh);

        // `size` is a whole number of cells, always: every caller of `orthographic_from_size` and
        // `gl.viewport` below assumes it, and #331 is what happens when it stops being true.
        debug_assert_eq!(
            self.size,
            (
                grid_px(self.grid_size.0, self.cell_size.0),
                grid_px(self.grid_size.1, self.cell_size.1)
            ),
        );
        unsafe {
            self.gl.viewport(0, 0, self.size.0, self.size.1);
        }
    }

    /// The number of columns actually adopted by the last [`resize`](Self::resize). Usually the
    /// `cols` that was asked for; smaller when the browser clamped the drawing buffer (#339).
    ///
    /// A consumer that keeps sending frames of the grid it *asked* for does not corrupt anything —
    /// every per-cell read is bounds-checked and the surplus cells are clipped by the viewport — but
    /// its mouse mapping and reflow will be wrong, so read this back rather than assuming.
    ///
    /// The requested grid is **not remembered**. `set_device_pixel_ratio` and the context-restore
    /// path both re-derive the buffer from *this* value, so a clamped grid stays clamped even if a
    /// later DPR drop would shrink the cell enough for the original to fit. That is deliberate — the
    /// consumer owns the grid (ADR-0017) and recomputes it from its own box, as xterm's `FitAddon`
    /// does — but it is not obvious from the field alone.
    ///
    /// One case reports a grid that has **not** been checked against the buffer yet: a
    /// [`resize`](Self::resize) that arrived while the context was lost (#639). It is the grid that
    /// was asked for, and the restore may still clamp it.
    #[wasm_bindgen(js_name = cols)]
    pub fn cols(&self) -> u32 {
        self.grid_size.0
    }

    /// The number of rows actually adopted by the last [`resize`](Self::resize) — see
    /// [`cols`](Self::cols).
    #[wasm_bindgen(js_name = rows)]
    pub fn rows(&self) -> u32 {
        self.grid_size.1
    }

    /// The drawing buffer's width in **CSS pixels** — what the consumer should set the canvas's CSS
    /// display box to, so the device-px buffer is shown at as close to the right size as a CSS
    /// length can get. Unrounded, for the same reason as [`css_cell_width`](Self::css_cell_width),
    /// and for one more (#337): a rounded box misses the buffer by up to `dpr/2` device px — an
    /// absolute error, so it is ruinous on a small canvas — where this one misses by at most the
    /// browser's layout grain (`dpr/128`; measured 0.0016..0.0156 at dpr 1.1). It can also round
    /// *up*, stretching the image over a box wider than the buffer feeding it.
    ///
    /// Round it yourself if your layout needs a whole CSS pixel; the reverse is not available.
    #[wasm_bindgen(js_name = cssWidth)]
    pub fn css_width(&self) -> f32 {
        css_px(self.size.0 as u32, self.dpr)
    }

    /// The drawing buffer's height in **CSS pixels** (see [`css_width`](Self::css_width)).
    #[wasm_bindgen(js_name = cssHeight)]
    pub fn css_height(&self) -> f32 {
        css_px(self.size.1 as u32, self.dpr)
    }

    /// Apply a `cols`×`rows` frame (dense row-major, length `cols*rows` — see #277 for the
    /// Partial-frame adapter): `bg`/`fg` are tagged-u32 colour refs, `codepoints` the glyph
    /// per cell, `flags` the `CellFlags`. A `WIDE_CHAR` lead cell rasterises a double-width
    /// glyph and splits it into two atlas slots; its `WIDE_CHAR_SPACER` cell reuses the
    /// right-half slot. New glyphs are rasterised + uploaded on demand.
    ///
    /// Tracked limits (surfaced by adversarial passes, not silent): colour emoji (#284) and
    /// ZWJ/grapheme clusters (#285) are separate slices; a frame with more distinct glyphs
    /// than a region's capacity, or a rasterise failure, can strand a slot (#280).
    // Seven typed-array / scalar columns at the wasm-bindgen boundary; each is a distinct JS view
    // that cannot be grouped without an AoS rewrite breaking the zero-copy SoA (as on `apply_damage`).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_frame(
        &mut self,
        cols: u32,
        rows: u32,
        bg: &[u32],
        fg: &[u32],
        codepoints: &[u32],
        flags: &[u16],
        blink_on: bool,
        // #520: the underline colour (SGR 58) column, tagged-u32 like `fg`/`bg`. Optional and
        // TRAILING so a caller (or demo) that predates it keeps working — omitted / `undefined`
        // ⇒ every underline follows the fg (Default). Not grouped with the colour columns because
        // that would shift every existing call; the packer reads it tolerantly regardless.
        underline_colors: Option<Vec<u32>>,
    ) -> Result<(), JsValue> {
        // The direct (dense, cluster-free) path: one base codepoint per cell.
        let cells = Cells {
            cols,
            rows,
            codepoints,
            flags,
            clusters: &[],
        };
        // The direct path packs immediately — it retains no grid for `render` to re-pack from, so
        // it cannot defer (#421). Clear the dirty flag: this pack IS the current state.
        let underline_colors = underline_colors.unwrap_or_default();
        let result = self.resolve_and_pack(&cells, bg, fg, &underline_colors, blink_on);
        self.needs_repack = false;
        result
    }

    /// Resolve each cell's glyph slot then pack the instance buffer. Shared by [`apply_frame`]
    /// (no clusters) and [`apply_damage`] (grapheme clusters from the persistent grid, #285).
    ///
    /// [`apply_frame`]: Self::apply_frame
    /// [`apply_damage`]: Self::apply_damage
    /// The composed cells, re-supplied (#249, ADR-0028 D2).
    ///
    /// A preedit is a **pass**, not a layer: ADR-0019's stack can recolour a channel or blank a
    /// slot but nothing in it can *supply* a glyph, and its rule 5 authorship axis has no value for
    /// content the browser owns and the application never declared. So the covered cells leave the
    /// stack entirely and come back with background, foreground and glyph together — which is also
    /// the only way a selection tint under a composition stops reading as *selected text*.
    ///
    /// Returns owned columns, and only while a composition is open: a page that never composes
    /// allocates nothing here. `0` is the `Default` colour tag (see [`palette`](crate::palette)),
    /// so the run draws in the terminal's own default fg over its default bg — ghostty's choice
    /// (`state.colors.foreground`, no background cell at all).
    /// The inclusive span the open composition covers, or `None` when nothing is composing or the
    /// anchor is off the grid. The packer takes it so the layers below glyph resolution can stand
    /// down over those cells; `preedit_patch` writes the same cells, and both derive from
    /// [`preedit::writes`](crate::preedit::writes) so they cannot disagree.
    fn preedit_span(&self, cols: u32, rows: u32) -> Option<PreeditSpan> {
        let w = preedit_writes(
            &self.preedit_run,
            self.preedit_col,
            self.preedit_row,
            cols,
            rows,
            &[], // no grid flags: the span is the RUN, never the repair cells beside it
        );
        let cols_usize = cols as usize;
        if w.is_empty() || cols_usize == 0 {
            return None;
        }
        let first = w.first()?.idx;
        let last = w.last()?.idx;
        Some(PreeditSpan {
            row: (first / cols_usize) as u32,
            start: (first % cols_usize) as u32,
            end: (last % cols_usize) as u32,
        })
    }

    fn preedit_patch(&self, cells: &Cells, bg: &[u32], fg: &[u32]) -> Option<PreeditPatch> {
        preedit_patch_of(
            &self.preedit_run,
            self.preedit_col,
            self.preedit_row,
            cells,
            bg,
            fg,
        )
    }

    fn resolve_and_pack(
        &mut self,
        cells: &Cells,
        bg: &[u32],
        fg: &[u32],
        underline_colors: &[u32],
        blink_on: bool,
    ) -> Result<(), JsValue> {
        self.pack_count = self.pack_count.wrapping_add(1); // #421 diagnostic — see `packs()`
        // The same multiply `resolve_frame` guards, evaluated one frame earlier — so guarding only
        // the pure layer left the panic exactly where it was (#355). This is the first arithmetic a
        // JS-supplied `cols`/`rows` touches; `resolve_frame` re-checks it because it is a public,
        // separately-tested surface, not because this line can be trusted to have run.
        let count = cell_count(cells.cols, cells.rows).ok_or_else(|| {
            JsValue::from_str(&format!(
                "justerm-renderer: grid {}x{} has more cells than a u32 can count",
                cells.cols, cells.rows
            ))
        })?;
        // ADR-0028 D2: the preedit takes its cells out of the stack before anything resolves them,
        // so the glyph it supplies is rasterised like any other and every later stage — contrast,
        // overlay compositing, the cursor span — sees the composed cell rather than the one the
        // application last wrote there.
        let patch = self.preedit_patch(cells, bg, fg);
        let patched = patch.as_ref().map(|p| Cells {
            cols: cells.cols,
            rows: cells.rows,
            codepoints: &p.codepoints,
            flags: &p.flags,
            clusters: &p.clusters,
        });
        let (cells, bg, fg) = match (patched.as_ref(), patch.as_ref()) {
            (Some(c), Some(p)) => (c, &p.bg[..], &p.fg[..]),
            _ => (cells, bg, fg),
        };

        // Resolve the per-cell glyph slots via the pure host-tested resolver (#280): it
        // rasterises before committing (a failure strands nothing), pins this frame's
        // working set (an over-capacity frame is surfaced, not silently corrupted), and
        // sanitises control codepoints to space. Field-level borrows keep `&mut cache`
        // disjoint from the GL fields the upload closure needs.
        let cache = &mut self.cache;
        let rasterizer = &self.rasterizer;
        let gl = &self.gl;
        let atlas = self.atlas;
        let atlas_cell = self.atlas_cell; // padded (upload) dims
        let (pad_w, pad_h) = atlas_cell;
        let slots = resolve_frame(
            cells,
            cache,
            |text, style, wide| {
                // Rasterise, then classify with the hybrid signal (#297): a colour emoji comes
                // back in its own palette (COLR/CBDT/SVG) → is_color_bitmap; an emoji the font
                // draws in pure grayscale (`⬛ ⬜ ⚫ ⚪`) has R=G=B so the bitmap misses it → the
                // unicode `is_emoji_text` (keyed off core's `wide`) recovers it. Either signal
                // routes the glyph to a colour-sampled slot; a text glyph satisfies neither.
                let rgba = rasterizer.rasterize(text, style, wide)?;
                let is_emoji = is_emoji_text(text, wide) || is_color_bitmap(&rgba);
                Ok((rgba, is_emoji))
            },
            |base, wide, rgba: Vec<u8>| {
                if wide {
                    // The wide source is 2*padded_w - 2*PADDING wide (two content halves plus
                    // one outer guard band each side); split into two padded cells.
                    let (left, right) =
                        split_wide_bitmap(&rgba, 2 * pad_w - 2 * PADDING, pad_w, pad_h);
                    upload_glyph(gl, atlas, atlas_cell, base, &left);
                    upload_glyph(gl, atlas, atlas_cell, base + 1, &right);
                } else {
                    upload_glyph(gl, atlas, atlas_cell, base, &rgba);
                }
            },
        )
        .map_err(|e| match e {
            ResolveError::Rasterize(js) => js,
            ResolveError::FrameExceedsCapacity => JsValue::from_str(
                "justerm-renderer: frame references more distinct glyphs than the atlas can hold",
            ),
            ResolveError::GridOverflows { cols, rows } => JsValue::from_str(&format!(
                "justerm-renderer: grid {cols}x{rows} has more cells than a u32 can count"
            )),
            ResolveError::FrameShorterThanGrid { cells, got } => JsValue::from_str(&format!(
                "justerm-renderer: grid claims {cells} cells but the frame carries {got}"
            )),
        })?;

        // `resolve_frame` bounds `codepoints`/`flags`, the two columns it reads, and allocates only
        // `count <= codepoints.len()` — so this can wait until after it. `bg`/`fg` are read by
        // `pack_instances`, which `.get(idx).unwrap_or(0)`s them: no panic, but a short colour column
        // renders silently in Default rather than being refused. Same rule for every column — a frame
        // that does not carry its cells is not a frame (#355).
        //
        // It runs *after* so that a frame short in every column reports the cells it is missing, not
        // just its colours; `FrameShorterThanGrid` is the more useful diagnosis.
        if bg.len() < count || fg.len() < count {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: grid claims {count} cells but bg/fg carry {}/{}",
                bg.len(),
                fg.len()
            )));
        }

        // Keep the flags: a cursor may move onto a wide char before the next frame arrives.
        self.last_flags.clear();
        self.last_flags.extend_from_slice(cells.flags);
        self.last_cols = cells.cols;
        self.last_blink_on = blink_on;
        self.resolve_cursor_cells();
        let frame = Frame {
            cols: cells.cols,
            rows: cells.rows,
            // The same span the patch above took over — handed on so every stage after glyph
            // resolution stands down inside it (ADR-0028 D2). Derived here rather than carried out
            // of `preedit_patch` so that the two cannot disagree about which cells are composed.
            preedit: self.preedit_span(cells.cols, cells.rows),
            bg,
            fg,
            slots: &slots,
            flags: cells.flags,
            codepoints: cells.codepoints,
            underline_colors,
        };
        // #271: composite the current selection / search overlay into each cell's packed bg. The
        // spans are owned by the renderer so they outlive the borrow; empty ⇒ no highlight.
        let overlay = Overlay {
            active: &self.active_match_spans,
            selection: &self.selection_spans,
            matches: &self.match_spans,
            colors: self.highlight_colors,
        };
        // #272: the RGB-space colour policy (bold→bright, dim, minimum-contrast, …), assembled from
        // the renderer's fields.
        let policy = ColorPolicy {
            bold_to_bright: self.bold_to_bright,
            min_contrast: self.min_contrast,
            selection_fg: self.selection_fg,
        };
        // #393: the consumer-projected marker decorations for this frame (parsed from the flat wire).
        let decorations = parse_decorations(&self.decoration_spans);
        self.instances = pack_instances(
            &frame,
            &self.palette,
            blink_on,
            &overlay,
            &policy,
            &decorations,
        );
        self.instance_count = count as i32;
        self.upload_instances();
        Ok(())
    }

    /// Reconcile the GPU instance buffer with the freshly packed `self.instances`, uploading
    /// only the cells that changed since the last upload (#263). A size change (first frame /
    /// resize) reallocates the whole buffer; otherwise each changed contiguous range goes up via
    /// `buffer_sub_data` and an unchanged frame does no GL work at all. `self.uploaded` mirrors
    /// what the GPU holds so the next frame can diff against it.
    fn upload_instances(&mut self) {
        match plan_upload(&self.uploaded, &self.instances, INSTANCE_FLOATS) {
            UploadPlan::Full => unsafe {
                self.gl
                    .bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
                self.gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    f32_bytes(&self.instances),
                    glow::DYNAMIC_DRAW,
                );
                self.uploaded.clone_from(&self.instances);
            },
            UploadPlan::Ranges(ranges) => {
                if ranges.is_empty() {
                    return; // nothing changed — skip the bind + upload entirely
                }
                unsafe {
                    self.gl
                        .bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
                    for (start, end) in ranges {
                        let (lo, hi) = (start * INSTANCE_FLOATS, end * INSTANCE_FLOATS);
                        self.gl.buffer_sub_data_u8_slice(
                            glow::ARRAY_BUFFER,
                            (lo * std::mem::size_of::<f32>()) as i32,
                            f32_bytes(&self.instances[lo..hi]),
                        );
                        self.uploaded[lo..hi].copy_from_slice(&self.instances[lo..hi]);
                    }
                }
            }
        }
    }

    /// Consume a decoded **damage** frame directly (#277 adapter): scatter its span-ordered
    /// cells into the persistent grid, then resolve + pack the full viewport. A Full frame wipes
    /// the grid first, a scroll op shifts it before spans — so a Partial frame (the common case)
    /// no longer misaligns as dense row-major. Grapheme clusters (#285) ride the `extra` column
    /// + `side_table` and are resolved to text at scatter (the index is frame-local).
    ///
    /// `header` carries the frame's scalars, `[cols, rows, kind, has_scroll, scroll_top,
    /// scroll_bottom, scroll_count, blink_on]` (kind `0` = Full / `1` = Partial; `scroll_count`
    /// reinterpreted as `i16`; `blink_on` `0`/`1`). `spans` is the span directory
    /// (`SPAN_STRIDE` `u32`s each);
    /// `codepoints`/`fg`/`bg`/`flags`/`extra` are the span-ordered cell columns.
    // 8 typed-array / vec columns at the wasm-bindgen boundary; each is a distinct JS view that
    // can't be structurally grouped without an AoS rewrite that would break the zero-copy SoA.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_damage(
        &mut self,
        header: &[u32],
        spans: &[u32],
        codepoints: &[u32],
        fg: &[u32],
        bg: &[u32],
        flags: &[u16],
        extra: &[u32],
        side_table: Vec<String>,
        // #520: the span-ordered underline colour column (SGR 58), tagged-u32 like `fg`/`bg`.
        // Optional + TRAILING for the same reason as `apply_frame` — a caller that predates it
        // keeps working, and the scatter reads it tolerantly (omitted ⇒ all Default).
        underline_colors: Option<Vec<u32>>,
    ) -> Result<(), JsValue> {
        if header.len() < 8 {
            return Err(JsValue::from_str(
                "justerm-renderer: apply_damage header needs 8 u32s [cols, rows, kind, has_scroll, scroll_top, scroll_bottom, scroll_count, blink_on]",
            ));
        }
        let cols = header[0];
        let rows = header[1];
        let kind = header[2] as u8;
        let scroll = if header[3] != 0 {
            Some((header[4] as u16, header[5] as u16, header[6] as i32 as i16))
        } else {
            None
        };
        let blink_on = header[7] != 0;

        // Take the grid out so scattering (`&mut grid`) and the `&mut self` resolve/pack don't
        // borrow-conflict; the grid is a local during the call and moves back after. Re-create
        // it when the dimensions change (a resize is followed by a Full frame).
        let mut grid = match self.grid.take() {
            Some(g) if g.cols() == cols && g.rows() == rows => g,
            _ => FrameGrid::try_new(cols, rows).ok_or_else(|| {
                JsValue::from_str(&format!(
                    "justerm-renderer: grid {cols}x{rows} has more cells than a u32 can count"
                ))
            })?,
        };
        // A malformed span directory refuses the whole frame; the grid is untouched and the
        // renderer stays usable. Before #355 it trapped the module and poisoned every later call.
        let underline_colors = underline_colors.unwrap_or_default();
        let scattered = grid.apply(&DamageFrame {
            kind,
            scroll,
            spans,
            codepoints,
            fg,
            bg,
            underline_colors: &underline_colors,
            flags,
            extra,
            side_table: &side_table,
        });
        if let Err(e) = scattered {
            // Put the grid back before returning: a refused frame must not also lose the renderer's
            // persistent viewport (`self.grid` is `take`n above).
            self.grid = Some(grid);
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: apply_damage refused a malformed frame: {e:?}"
            )));
        }
        // Defer the pack to `render` (#421): the frame's overlay/decoration setters, which the
        // consumer calls around this, would otherwise each re-pack the same grid. The scatter above
        // is done, so store the blink phase the deferred `repack_from_grid` reads (`last_blink_on`),
        // put the grid back, and mark dirty. A pack error now surfaces at `render`, not here.
        self.last_blink_on = blink_on;
        self.grid = Some(grid);
        self.needs_repack = true;
        Ok(())
    }

    /// Set the background cell opacity: `0` = fully transparent, `1` = opaque (default). The
    /// consumer injects this policy (ADR-0017) to make the terminal background see-through to the
    /// page/desktop behind the canvas, while glyph pixels stay opaque. Clamped to `[0, 1]`; takes
    /// effect on the next [`render`](Self::render) (#298).
    ///
    /// **A translucent background contributes to a cell's colour in proportion to how translucent
    /// it is** (#317 §2, fixed 2026-08-18). It used to contribute in full: an antialiased glyph
    /// edge mixed toward the background colour with the coverage as its weight while the alpha
    /// said that background was only `alpha` present, so at `0` a half-covered pixel came out half
    /// background — a colour the caller had asked to be absent. At `1` nothing changed and nothing
    /// changes now; the two agree exactly there, which is why this shipped unnoticed.
    ///
    /// **A non-finite value falls back to `1.0` (opaque), like every other float setter here.**
    /// `f32::clamp` compares with `<` / `>`, both false for `NaN`, so a bare clamp *passes NaN
    /// through* — and this was the only float setter on this type without the guard (#577). The
    /// consequence was not a wrong background: a `NaN` here reaches every fragment's alpha, so
    /// glyph pixels go transparent too and the whole terminal disappears with no error anywhere.
    /// Measured, not reasoned: booting the widget at `NaN` read `[30,30,46,0]` on a background
    /// cell **and `[205,214,244,0]` inside a glyph**, against `[…,128]` / `[…,255]` for a valid
    /// `0.5`. (The measurement stands; the expression it was taken against was
    /// `mix(u_bg_alpha, 1.0, cov)`, which #317 §2 replaced — the `NaN` now reaches the colour
    /// through the same uniform as well, so the failure is if anything less subtle.)
    ///
    /// Reachable from type-correct consumer code, which is why the guard is here and not at the
    /// caller: TypeScript's `number` includes `NaN`, so an ordinary `Number(configValue)` arrives
    /// as one. Finite out-of-range values were never the problem — the clamp already handles them
    /// (`-3` and `9` measured as fully transparent and fully opaque respectively).
    #[wasm_bindgen(js_name = setBgAlpha)]
    pub fn set_bg_alpha(&mut self, alpha: f32) {
        self.bg_alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    /// Draw bold text in the bright (8–15) ANSI colour (#223/#272) — xterm's
    /// `drawBoldTextInBrightColors`. A bold `Indexed(0..=7)` foreground resolves to its `8..=15`
    /// bright variant; `Rgb`/`Indexed(8..=255)` foregrounds and non-bold cells are unaffected. On by
    /// default (xterm's default). Consumer policy (ADR-0017): the mechanism (index remap at resolve)
    /// is the renderer's, the on/off is the consumer's. Marks the buffer dirty; the next
    /// [`render`](Self::render) re-packs (#421), so a live toggle shows without a new frame.
    #[wasm_bindgen(js_name = setBoldToBright)]
    pub fn set_bold_to_bright(&mut self, enabled: bool) -> Result<(), JsValue> {
        self.bold_to_bright = enabled;
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    /// Set the marker-anchored decorations for this frame (#393/#120). `spans` is the flat
    /// `DECORATION_STRIDE` (`row, left, right, layer, bg, fg`) directory the consumer projects
    /// from its `DecorationRegistry` + core's markers — `layer` `0` = bottom (under the highlight) /
    /// `1` = top (over it), `bg`/`fg` **absolute** packed `0xRRGGBB` used **verbatim** (the consumer
    /// resolved its theme before pushing — unlike a *cell* colour, which arrives as a theme-agnostic
    /// ref for the renderer to resolve), or the wire's `NO_REF` sentinel for "no override". Pass an
    /// empty array to clear. Consumer-projected (the model is the consumer's; the renderer only
    /// composites, ADR-0017). Marks the buffer dirty; the next [`render`](Self::render)
    /// re-packs (#421).
    #[wasm_bindgen(js_name = setDecorations)]
    pub fn set_decorations(&mut self, spans: Vec<u32>) -> Result<(), JsValue> {
        self.decoration_spans = spans;
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    /// Set (or clear) the selection foreground override (#227/#272) — xterm's `selectionForeground`.
    /// A packed `0xRRGGBB` forces the fg of every **selected** cell (never a search match) to that
    /// colour; it still flows through the minimum-contrast pass. Pass `undefined` to clear it and keep
    /// each cell's own fg (the default). Consumer policy (#115), focus-independent. Selection is a
    /// property of the cell, not of the bg winner (#430): on a selected cell inside the ACTIVE search
    /// match this fg paints over the *active-match* background — pick the two colours to read on each
    /// other, or set [`set_minimum_contrast_ratio`](Self::set_minimum_contrast_ratio) (it corrects
    /// against the final composited bg). Marks the buffer dirty; the next [`render`](Self::render)
    /// re-packs (#421).
    #[wasm_bindgen(js_name = setSelectionForeground)]
    pub fn set_selection_foreground(&mut self, color: Option<u32>) -> Result<(), JsValue> {
        self.selection_fg = color.map(|c| c & 0xFF_FFFF);
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    /// Set the minimum WCAG fg/bg contrast ratio (#225/#272) — xterm's `minimumContrastRatio`. Below
    /// it, a cell's foreground is nudged lighter or darker (in 10% luminance steps, away from the bg)
    /// until it meets the ratio, against the colour it is actually drawn over (post-highlight). A DIM
    /// cell uses half the ratio, so it stays visibly dim rather than being corrected to full contrast.
    /// Consumer policy (ADR-0017): the mechanism (the WCAG adjustment on the resolved RGB) is the
    /// renderer's, the number is the consumer's. Default `1.0` = off (xterm's default). Clamped to
    /// `[1, 21]`; marks the buffer dirty so the next [`render`](Self::render) re-packs (#421) and a
    /// live change shows.
    #[wasm_bindgen(js_name = setMinimumContrastRatio)]
    pub fn set_minimum_contrast_ratio(&mut self, ratio: f32) -> Result<(), JsValue> {
        self.min_contrast = if ratio.is_finite() {
            ratio.clamp(1.0, 21.0)
        } else {
            1.0
        };
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    /// Swap the palette + default fg/bg for a **live theme change** (#405) — the renderer-side of a
    /// theme picker or a runtime scheme swap, so a consumer need not tear down and rebuild the
    /// renderer to recolour. `palette_colors` is the 256 pre-built indexed colours (as the
    /// constructor takes); `default_fg`/`default_bg` the theme's defaults. Consumer policy
    /// (ADR-0017): the palette *values* are the consumer's (theme-agnostic core), the *mechanism*
    /// (re-resolve every retained cell against the new palette) is the renderer's.
    ///
    /// Marks the buffer dirty so the next [`render`](Self::render) re-packs (#421) and the change
    /// shows with no new frame — like [`set_overlay`] (a no-op until the first `apply_damage`; the
    /// direct `apply_frame` path reflects the new palette on its next call). The re-pack is all that
    /// is needed: it re-resolves every cell's colour against the new palette, and the render's clear
    /// reads `default_bg` fresh. #298 translucency no longer needs a uniform re-push here — since #455
    /// its trigger is the packer's per-cell `bg_default` provenance flag, which is palette-independent.
    ///
    /// [`set_overlay`]: Self::set_overlay
    #[wasm_bindgen(js_name = setPalette)]
    pub fn set_palette(
        &mut self,
        palette_colors: Vec<u32>,
        default_fg: u32,
        default_bg: u32,
    ) -> Result<(), JsValue> {
        self.palette =
            Palette::from_colors(&palette_colors, default_fg, default_bg).map_err(|e| {
                JsValue::from_str(&format!(
                    "justerm-renderer: palette must be 256 colours, got {}",
                    e.got
                ))
            })?;
        // A re-pack is all a live theme swap needs: it re-resolves every cell's colour against the new
        // palette (the render's clear reads `self.palette.default_bg` fresh). #298 translucency used to
        // also re-push a `u_default_bg` uniform here; since #455 its trigger is the packer's per-cell
        // `bg_default` provenance flag — palette-independent — so the re-pack alone carries it.
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    /// Set the selection / search highlight overlay (#271): the two span directories (stride-3
    /// `(row, left, right)` viewport triples, exactly as `justerm-wasm-decode` `selectionSpans` /
    /// `matchSpans` ship them) plus their blend colours (packed `0xRRGGBB`, consumer policy #115 —
    /// the renderer is theme-agnostic). A covered cell blends the colour over a non-default / inverse
    /// background so its own colour shows through, or paints it solid over the default background; a
    /// selection wins over a match on a cell both cover.
    ///
    /// Marks the buffer dirty so the next [`render`](Self::render) re-packs (#421) — a selection
    /// dragged with no new frame shows because the consumer renders after. Possible only on the
    /// damage path, which retains the dense grid; the direct `apply_frame` path reflects the new
    /// overlay on its next call. Pass empty span lists to clear the highlight.
    ///
    /// **Contract — the spans are consumer-pushed, not frame-carried (same as [`set_cursor`]).** They
    /// are viewport-relative and the decoder RE-PROJECTS them every frame, so a scroll or resize moves
    /// them: the consumer must re-issue `set_overlay` with the current frame's spans whenever the
    /// viewport changes *or* the selection changes — exactly as it re-issues `set_cursor`. Stale spans
    /// do not panic (an out-of-range span simply highlights nothing), but an in-range stale span
    /// highlights the wrong cells until the next call. Unlike beamterm, whose spans ride each decoded
    /// frame, this renderer cannot self-refresh — the split mirrors the cursor's, and #273 wires both.
    ///
    /// [`set_cursor`]: Self::set_cursor
    #[wasm_bindgen(js_name = setOverlay)]
    pub fn set_overlay(
        &mut self,
        selection_spans: Vec<u32>,
        match_spans: Vec<u32>,
        selection_bg: u32,
        match_bg: u32,
    ) -> Result<(), JsValue> {
        self.selection_spans = selection_spans;
        self.match_spans = match_spans;
        // Update the two colours this setter owns WITHOUT clobbering `active_match_bg` (#427), which
        // `set_active_match` owns — the active channel is set independently.
        self.highlight_colors.selection_bg = selection_bg;
        self.highlight_colors.match_bg = match_bg;
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    /// Set the *active* (focused/current) search-match spans + their background (#427) — the xterm
    /// `activeMatchBackground` decoration, ranked **above the selection** (`highlight_at`). Additive
    /// beside [`set_overlay`](Self::set_overlay): the consumer pushes the current search result here
    /// as the search box navigates (`next`/`prev`), independent of the selection, so a user text
    /// selection and the current match coexist. The active match is *also* pushed in `set_overlay`'s
    /// `match_spans`; the ranking, not exclusion, makes the active colour win. Same viewport-relative,
    /// re-issue-every-frame contract as [`set_overlay`](Self::set_overlay). Empty spans clear the active match.
    #[wasm_bindgen(js_name = setActiveMatch)]
    pub fn set_active_match(&mut self, active_spans: Vec<u32>, active_match_bg: u32) {
        self.active_match_spans = active_spans;
        self.highlight_colors.active_match_bg = active_match_bg;
        self.needs_repack = true; // defer the pack to render (#421), same as set_overlay
    }

    /// The in-progress IME composition to draw, anchored at `(col, row)` — the cursor cell the
    /// consumer's current frame reported. `codepoints` is the preedit as the OS reports it
    /// (`compositionupdate.data`); an empty array clears it.
    ///
    /// **This is the one piece of renderer state with no representation anywhere in the engine**
    /// (ADR-0028): a composition is browser-owned, reaches no frame and no wire, so the consumer is
    /// the only possible source and must re-push on every `compositionupdate`. Skipping an update
    /// whose data is unchanged is worth doing — a real IME emits one settling update per syllable
    /// where nothing moved (measured, #249).
    ///
    /// The run may extend past the anchor's row end: it shifts left to stay whole rather than
    /// clipping (`preedit::range` — crate-private, so no link from this page). Width is per codepoint, the same
    /// `unicode-width` answer the engine gives, so a VS16 emoji measures narrow here exactly as it
    /// does there.
    /// Returns the column the **caret and the IME anchor** belong at: one past the run's last cell,
    /// clamped to the grid. The consumer cannot compute this — it has no `wcwidth` — and it is not
    /// simply `col + len`, because the run shifts left at the right edge. Feeding it back to
    /// `setCursor` is ADR-0028 D5's position rule (the caret rides the composition's end, while
    /// DECTCEM still decides whether it is drawn at all), and feeding it to the hidden textarea is
    /// D4's voluntary writer.
    #[wasm_bindgen(js_name = setPreedit)]
    pub fn set_preedit(&mut self, col: u32, row: u32, codepoints: Vec<u32>) -> u32 {
        self.preedit_run = codepoints
            .into_iter()
            .map(|cp| PreeditCodepoint {
                cp,
                wide: preedit_is_wide(cp),
            })
            .collect();
        self.preedit_col = col;
        self.preedit_row = row;
        self.needs_repack = true; // defer the pack to render (#421), same as set_overlay
        self.preedit_caret_col()
    }

    /// One past the composition's last cell, clamped — see [`set_preedit`](Self::set_preedit).
    /// With no composition open this is the anchor cell itself, so a consumer that always reads it
    /// gets the cursor cell back and needs no special case for "not composing".
    fn preedit_caret_col(&self) -> u32 {
        let cols = self.cols();
        let last = cols.saturating_sub(1);
        if self.preedit_run.is_empty() || cols == 0 {
            return self.preedit_col.min(last);
        }
        preedit_caret_col_of(&self.preedit_run, self.preedit_col, last)
    }

    /// Re-pack the instance buffer from the retained dense grid — the single pack [`render`] runs when
    /// a mutation dirtied the buffer (#421; #271 was the original overlay-only re-pack). A no-op until
    /// the first `apply_damage` (the direct `apply_frame` path keeps no columns to re-pack from). Takes
    /// the grid out so the `&mut self` pack does not borrow-conflict, then puts it back.
    ///
    /// [`render`]: Self::render
    fn repack_from_grid(&mut self) -> Result<(), JsValue> {
        let Some(grid) = self.grid.take() else {
            return Ok(());
        };
        let cells = Cells {
            cols: grid.cols(),
            rows: grid.rows(),
            codepoints: grid.codepoints(),
            flags: grid.flags(),
            clusters: grid.clusters(),
        };
        let result = self.resolve_and_pack(
            &cells,
            grid.bg(),
            grid.fg(),
            grid.underline_colors(),
            self.last_blink_on,
        );
        self.grid = Some(grid);
        result
    }

    /// Set the minimum WCAG contrast a cursor must have with the cell it sits on (#368). Below it,
    /// the cursor inverts to the terminal's default fg/bg so it never vanishes into a same-coloured
    /// cell. The mechanism is the renderer's — only it has the *resolved* per-cell RGB (ADR-0017) —
    /// but the number is the consumer's policy. Default `1.5` (alacritty's `MIN_CURSOR_CONTRAST`);
    /// pass `1.0`, the floor of the contrast range, to disable the guard (xterm's behaviour). Clamped
    /// to `[1, 21]`; takes effect on the next [`render`](Self::render).
    #[wasm_bindgen(js_name = setCursorContrast)]
    pub fn set_cursor_contrast(&mut self, threshold: f32) {
        self.cursor_contrast = threshold.clamp(1.0, 21.0);
    }

    /// Set the cursor stroke thickness as a fraction of the cell width (#369) — the width of a
    /// bar, an underline, or a hollow block's outline. `cursor_thickness` turns it into device
    /// pixels as `(frac * cell_w).round().max(1)`, so it tracks both dpr and font size — alacritty's
    /// rule (`display/cursor.rs:25`), which #270 chose over xterm's `dpr * cursorWidth` (that gives a
    /// 32px font the same hairline as a 12px one). This adds only the configurability the mechanism
    /// already had. A **block** ignores it — a block recolours its cell and draws no stroke.
    ///
    /// Default `0.15` (alacritty's `cursor.thickness`); clamped to `[0, 1]` (alacritty's
    /// `Percentage`). The clamp is load-bearing, not hygiene: `cursor_thickness` computes
    /// `(frac * cell_w).round() as u32`, and an unclamped `f32::INFINITY` saturates that cast to
    /// `u32::MAX` device pixels. `NaN` is caught a layer deeper — `frac.max(0.0)` returns `0.0` for
    /// it (`f32::max` yields the non-NaN operand) — so the floor below still gives it a 1px stroke.
    /// The mechanism's `.max(1)` floor also means even `0` leaves a one-pixel stroke rather than an
    /// invisible cursor. Takes effect on the next [`render`](Self::render) — like a stroke's shape,
    /// it is a shader uniform, so changing it costs no upload.
    #[wasm_bindgen(js_name = setCursorThickness)]
    pub fn set_cursor_thickness(&mut self, frac: f32) {
        self.cursor_thickness_frac = frac.clamp(0.0, 1.0);
    }

    /// Place the cursor (#270). `shape`: `0` block, `1` underline, `2` bar, `3` hollow block.
    /// `color` is the cursor's own `0xRRGGBB`; `text_color` the glyph colour a block paints under
    /// itself (xterm's `cursorAccent`, alacritty's `text_color`). Colours are resolved by the
    /// consumer — the renderer stays theme-agnostic.
    ///
    /// **Every shape, the block included, lives in the cursor uniform** (`u_cursor` + its colours,
    /// declared with `FRAG_SRC` in this file) and is resolved per fragment. So any cursor change —
    /// move, blink, shape — takes effect on the next [`render`](Self::render) alone: one uniform,
    /// no re-pack and no instance upload. Blink phase is the consumer's policy, exactly as
    /// `blink_on` is (#282) — call `clearCursor` for the off phase.
    ///
    /// A block *could* have been an instance: it is a colour override on the cell, not geometry,
    /// and both references draw it that way. It is not one because ADR-0018
    /// (`docs/adr/0018-justerm-renderer.md`) makes that **the contract, not an
    /// optimisation** — a blink tick produces no terminal output, so a block packed into the
    /// instances could not blink off without the consumer re-feeding the frame (an early #270 draft
    /// did exactly that). Two consequences follow rather than cause it: un-painting would need a
    /// re-pack, and per-fragment resolution keeps the ordering free, since the instance colours
    /// arrive already inverse-swapped and the glyph already concealed.
    #[wasm_bindgen(js_name = setCursor)]
    pub fn set_cursor(
        &mut self,
        col: u32,
        row: u32,
        shape: u8,
        color: u32,
        text_color: u32,
    ) -> Result<(), JsValue> {
        let Some(shape) = shape_from_id(shape) else {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: cursor shape {shape} is not one of 0..=3"
            )));
        };
        self.cursor = Some(Cursor {
            col,
            row,
            shape,
            color,
            text_color,
        });
        self.resolve_cursor_cells();
        Ok(())
    }

    /// Re-resolve [`Self::cursor_cells`] against the last frame's flags. Called when a frame arrives
    /// (its flags may have changed under a still cursor) *and* when the cursor moves (onto either
    /// half of a wide char, with no new frame).
    fn resolve_cursor_cells(&mut self) {
        self.cursor_cells = self.cursor.map_or((0, 1), |c| {
            cursor_cells_at(&self.last_flags, self.last_cols, c.col, c.row)
        });
    }

    /// Remove the cursor — hidden (`DECTCEM`), or the blink's off phase.
    #[wasm_bindgen(js_name = clearCursor)]
    pub fn clear_cursor(&mut self) {
        self.cursor = None;
    }

    /// Clear to the palette's default background, then draw every cell of the current frame
    /// (glyph composited over background) with one instanced draw call.
    ///
    /// Context loss (#269) is handled here, before any GL work: while the context is lost this is a
    /// silent no-op (a draw call on a dead context accomplishes nothing), and on the frame after
    /// `webglcontextrestored` it first rebuilds the destroyed resources. Recovery therefore needs
    /// no consumer cooperation beyond continuing to call `render`. A failed rebuild propagates and
    /// is retried on the next frame.
    ///
    /// **"While the context is lost" means either sense of lost, and it did not always** (#695).
    /// The decision consults the context itself *and* the state machine's flag, because a browser
    /// destroys a context synchronously and only queues the event: asking the flag alone, this
    /// promise was false for that slice — a pending rebuild ran on a dead context and threw.
    pub fn render(&mut self) -> Result<(), JsValue> {
        // Ask the CONTEXT, then let the state machine compose that with the flag it owns
        // (#695, ADR-0027 D3). Bound to locals in this order deliberately: the liveness read
        // must not happen while the `Ref` below is alive, and the `Ref` must be released
        // before the `&mut self` calls in the match.
        let live = if self.raw_gl.is_context_lost() {
            ContextLiveness::Dead
        } else {
            ContextLiveness::Usable
        };
        let action = self.ctx_loss.state.borrow().action(live);
        match action {
            FrameAction::Skip => return Ok(()),
            FrameAction::Rebuild => {
                self.restore()?;
                // Only now that the rebuild is committed does the retry latch clear.
                self.ctx_loss.state.borrow_mut().rebuilt();
            }
            FrameAction::Draw => {}
        }
        // Pack once, here, if a mutation since the last render dirtied the buffer (#421) — the
        // context is live past the match above (Skip returned, Rebuild restored). A frame that set
        // overlay + decorations + `apply_damage` marked dirty three times but re-packs once. On a
        // pack error the flag stays set, so the next render retries (self-healing).
        if self.needs_repack {
            self.repack_from_grid()?;
            self.needs_repack = false;
        }
        self.draw();
        Ok(())
    }

    /// Number of instance-buffer packs run so far (#421 diagnostic). The consumer/proofs read the
    /// **delta** across an operation to assert `render` packs once per frame, not once per setter.
    /// Not a stable API surface — a counter for verification, not a rendering control.
    #[wasm_bindgen(js_name = packs)]
    pub fn packs(&self) -> u32 {
        self.pack_count
    }

    /// Issue the frame's GL commands. The caller has established that the context is live and its
    /// resources are intact.
    fn draw(&self) {
        let [dr, dg, db] = gl_rgb(self.palette.default_bg);
        unsafe {
            // Clear with the injected background opacity so any area not covered by a cell is
            // see-through too; cells then write their own per-pixel alpha (#298). The buffer is now
            // an exact multiple of the cell (#331), so the only uncovered area is a frame whose grid
            // is smaller than the one `resize` was given.
            self.gl.clear_color(dr, dg, db, self.bg_alpha);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

            if self.instance_count == 0 {
                return;
            }

            self.gl.use_program(Some(self.program));
            self.gl.active_texture(glow::TEXTURE0);
            self.gl
                .bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas));
            // The instance buffer already holds the current frame — `upload_instances` (in the
            // pack path) uploaded only the changed cells (#263), so render just binds + draws.
            self.gl.bind_vertex_array(Some(self.vao));

            let proj = Mat4::orthographic_from_size(self.size.0 as f32, self.size.1 as f32);
            self.gl
                .uniform_matrix_4_f32_slice(Some(&self.u_projection), false, &proj.data);
            self.gl.uniform_2_f32(
                Some(&self.u_cell_size),
                self.cell_size.0 as f32,
                self.cell_size.1 as f32,
            );
            self.gl.uniform_2_f32(
                Some(&self.u_char_size),
                self.char_size.0 as f32,
                self.char_size.1 as f32,
            );
            self.gl.uniform_2_f32(
                Some(&self.u_char_offset),
                self.char_offset.0 as f32,
                self.char_offset.1 as f32,
            );
            self.gl.uniform_1_f32(
                Some(&self.u_line_thickness),
                crate::metrics::line_thickness(self.font_size * self.dpr) as f32,
            );
            self.gl.uniform_1_f32(Some(&self.u_bg_alpha), self.bg_alpha);
            // `u_cursor.w == 0` means NO cursor; a shape is `shape_id + 1`. Every shape — block
            // included — reaches the shader this way, so a move or a blink is a uniform, not an
            // upload (#270).
            let (cx, cy, span, shape) = match self.cursor {
                Some(c) => (
                    self.cursor_cells.0 as f32,
                    c.row as f32,
                    self.cursor_cells.1 as f32,
                    shape_id(c.shape) as f32 + 1.0,
                ),
                None => (0.0, 0.0, 1.0, 0.0),
            };
            self.gl
                .uniform_4_f32(Some(&self.u_cursor), cx, cy, span, shape);
            // The visibility guard (#368): look up the cursor cell's RESOLVED bg in the packed
            // instances (row-major, `bg` at float offset 2 of each `INSTANCE_FLOATS` cell) and invert
            // the cursor to the default fg/bg if its contrast is below the injected threshold. Only the
            // renderer has this resolved RGB, which is why the mechanism lives here (ADR-0017). If the
            // cursor sits off the current grid (no packed cell), honour the consumer's colours as-is.
            //
            // The index is bounded only by `get()`, not by `col < last_cols`: a cursor with
            // `col >= last_cols` but a small row would read a DIFFERENT row's cell here. That is
            // harmless because the shader's `covers()` paints the cursor only where a real cell has
            // `col ∈ [cursor.col, cursor.col + span)`, i.e. only when `col < cols` — so a mis-read
            // guarded colour is never sampled by any fragment. Valid as long as `covers()` keeps that
            // gate.
            let (color, text_color) = match self.cursor {
                Some(c) => {
                    let cell_bg = (c.row as usize)
                        .checked_mul(self.last_cols as usize)
                        .and_then(|i| i.checked_add(self.cursor_cells.0 as usize))
                        .and_then(|i| i.checked_mul(INSTANCE_FLOATS))
                        .and_then(|base| self.instances.get(base + 2..base + 5));
                    match cell_bg {
                        Some(bg) => guarded_cursor_colors(
                            c.color,
                            c.text_color,
                            [bg[0], bg[1], bg[2]],
                            self.palette.default_fg,
                            self.palette.default_bg,
                            self.cursor_contrast,
                        ),
                        None => (c.color, c.text_color),
                    }
                }
                None => (0, 0),
            };
            let [cr, cg, cb] = gl_rgb(color);
            self.gl
                .uniform_3_f32(Some(&self.u_cursor_color), cr, cg, cb);
            let [tr, tg, tb] = gl_rgb(text_color);
            self.gl
                .uniform_3_f32(Some(&self.u_cursor_text_color), tr, tg, tb);
            self.gl.uniform_1_f32(
                Some(&self.u_cursor_thickness),
                cursor_thickness(self.cursor_thickness_frac, self.cell_size.0) as f32,
            );

            self.gl
                .draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.instance_count);
        }
    }
}

/// The pure cursor geometry (`cursor::cursor_rects`) as a flat `[x, y, w, h, ...]`, exposed so a
/// proof page can hold the fragment shader's per-pixel test to the same rectangles. Two
/// independent formulations of one spec: a drift between them is the bug this exists to catch.
#[wasm_bindgen(js_name = cursorRects)]
pub fn cursor_rects_js(shape: u8, cell_w: u32, cell_h: u32, span: u32, thickness: u32) -> Vec<u32> {
    let Some(shape) = shape_from_id(shape) else {
        return Vec::new();
    };
    cursor_rects(shape, (cell_w, cell_h), span, thickness)
        .into_iter()
        .flat_map(|r| [r.x, r.y, r.w, r.h])
        .collect()
}

/// Fetch a required uniform location or error.
fn uniform(
    gl: &glow::Context,
    program: glow::Program,
    name: &str,
) -> Result<glow::UniformLocation, JsValue> {
    // Safety: live GL context.
    unsafe {
        gl.get_uniform_location(program, name)
            .ok_or_else(|| JsValue::from_str(&format!("justerm-renderer: no uniform {name}")))
    }
}

/// Wrap a GL/string error as a `JsValue`.
fn js_err(msg: String) -> JsValue {
    JsValue::from_str(&format!("justerm-renderer: {msg}"))
}
