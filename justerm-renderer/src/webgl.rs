//! Thin `#[wasm_bindgen]` + WebGL2 glue — browser-only (wasm32), verified in the demo.
//!
//! The instanced pipeline draws the whole grid in one call: `apply_frame` resolves each
//! cell's bg/fg references (injected palette) and its glyph slot (glyph cache, rasterising
//! and uploading new glyphs on demand), packs one instance per cell, and `render`
//! composites each glyph's coverage from the atlas over its background, plus SGR attrs
//! (#267: bold/italic font variants, underline/strikethrough lines, inverse fg/bg swap) and
//! double-width glyphs (#268: a wide glyph splits across two atlas slots / two grid cells).
//! ASCII (`0x20..=0x7E`) is pre-rasterised. Colour emoji (#284) + clusters (#285) follow.

use std::cell::RefCell;
use std::rc::Rc;

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::bitmap::{PADDING, is_color_bitmap, split_wide_bitmap};
use crate::color::gl_rgb;
use crate::context_loss::{ContextState, FrameAction};
use crate::dpr::{device_px, dpr_changed};
use crate::emoji::is_emoji_text;
use crate::frame::{Frame, INSTANCE_FLOATS, pack_instances};
use crate::frame_grid::{DamageFrame, FrameGrid};
use crate::glyph_cache::{
    FontStyle, GLYPHS_PER_LAYER, GlyphCache, WIDE_BASE, WIDE_CAPACITY, slot_texcoord,
};
use crate::glyph_resolve::{Cells, ResolveError, resolve_frame};
use crate::mat4::Mat4;
use crate::palette::Palette;
use crate::rasterizer::Rasterizer;
use crate::upload::{UploadPlan, invalidate_baseline, plan_upload};

/// Texture-array layers covering the whole slot space (normal + wide = 6144 / 32 = 192),
/// so wide/emoji slots (layers 64..191) have storage.
const TOTAL_LAYERS: i32 = ((WIDE_BASE + WIDE_CAPACITY * 2) / GLYPHS_PER_LAYER) as i32;
/// Default font size (CSS px) for the atlas rasteriser.
const FONT_SIZE: f32 = 16.0;

/// Unit-quad corners (triangle strip): geometry + per-cell glyph texture coordinate.
const QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
/// Floats per instance: col, row, bg(3), fg(3), glyph_slot.
const INSTANCE_STRIDE: i32 = 9 * 4;

const VERT_SRC: &str = r#"#version 300 es
layout(location = 0) in vec2 a_pos;    // unit-quad corner (0..1) = local glyph texcoord
layout(location = 1) in vec2 a_cell;   // instance: (col, row)
layout(location = 2) in vec3 a_bg;     // instance: background rgb
layout(location = 3) in vec3 a_fg;     // instance: foreground rgb
layout(location = 4) in float a_glyph; // instance: atlas slot index
uniform mat4 u_projection;
uniform vec2 u_cell_size;
out vec3 v_bg;
out vec3 v_fg;
flat out uint v_glyph;
out vec2 v_tex;
void main() {
    vec2 origin = a_cell * u_cell_size;
    vec2 pos = floor(origin + a_pos * u_cell_size + 0.5); // pixel-snapped
    gl_Position = u_projection * vec4(pos, 0.0, 1.0);
    v_bg = a_bg;
    v_fg = a_fg;
    v_glyph = uint(a_glyph);
    v_tex = a_pos;
}
"#;

const FRAG_SRC: &str = r#"#version 300 es
precision mediump float;
uniform mediump sampler2DArray u_atlas;
uniform vec2 u_padding_frac; // guard band as a fraction of the padded atlas cell (#288)
uniform float u_bg_alpha;    // background cell opacity: 0 = transparent, 1 = opaque (#298)
uniform vec3 u_default_bg;    // the default terminal background — only IT is made translucent (#298)
in vec3 v_bg;
in vec3 v_fg;
flat in uint v_glyph;
in vec2 v_tex;
out vec4 FragColor;
// A horizontal line centred at `c` (cell-local y, 0..1) with soft edges (beamterm cell.frag).
float hline(float y, float c, float thick) {
    return 1.0 - smoothstep(0.0, thick, abs(y - c));
}
void main() {
    // The glyph field packs slot (bits 0..12), underline (bit 13), strikethrough (bit 14),
    // and the colour-emoji flag (bit 15, #284).
    uint slot = v_glyph & 0x1FFFu;
    uint layer = slot >> 5u;   // 32 glyphs stack vertically per layer
    uint band = slot & 31u;
    // Inset the physical-cell texcoord into the padded atlas cell's content region, so the
    // transparent guard band is never sampled (beamterm cell.frag) — stops band bleed while
    // the content still maps edge-to-edge (box-drawing connects across cells).
    vec2 inner = v_tex * (1.0 - 2.0 * u_padding_frac) + u_padding_frac;
    // Nudge off the exact texel edge so NEAREST can't round to a neighbour (beamterm cell.frag);
    // belt-and-suspenders for a fractional cell↔texel mapping (DPR != 1, #265).
    vec3 tc = vec3(inner.x + 0.001, (float(band) + inner.y + 0.001) / 32.0, float(layer));
    vec4 texel = texture(u_atlas, tc);
    float coverage = texel.a;

    // A colour emoji (bit 15) samples the atlas RGB (the font's own colours); a text glyph uses
    // the packed foreground (beamterm cell.frag `mix(base_fg, glyph.rgb, emoji_factor)`).
    float emoji = float((v_glyph >> 15u) & 1u);
    vec3 fg = mix(v_fg, texel.rgb, emoji);

    float underline = float((v_glyph >> 13u) & 1u);
    float strike = float((v_glyph >> 14u) & 1u);
    // Fixed cell-local positions (underline below baseline, strikethrough mid-cell). beamterm
    // derives these per-font from metrics; a font-metric-driven position is a later refinement.
    float line = max(hline(v_tex.y, 0.88, 0.05) * underline, hline(v_tex.y, 0.5, 0.05) * strike);
    // Underline/strikethrough always draw in the base foreground, even over an emoji.
    fg = mix(fg, v_fg, line);

    // Only the DEFAULT terminal background is translucent (the see-through backdrop). An explicit
    // SGR background or an inverse/selection/cursor background is *content* and stays opaque — else
    // a highlight would vanish on a translucent terminal (#298). A glyph/line pixel is always opaque.
    float cov = max(coverage, line);
    float bg_a = (v_bg == u_default_bg) ? mix(u_bg_alpha, 1.0, cov) : 1.0;
    FragColor = vec4(mix(v_bg, fg, cov), bg_a);
}
"#;

/// Canvas `webglcontextlost` / `webglcontextrestored` listeners feeding a shared [`ContextState`]
/// (#269). The closures capture ONLY the `Rc`'d state — never the renderer — so they can fire while
/// a `&mut JustermRenderer` method is on the stack without a `RefCell` double-borrow.
struct ContextLossHandler {
    canvas: HtmlCanvasElement,
    state: Rc<RefCell<ContextState>>,
    // Kept alive for as long as the listeners are attached; `Drop` detaches them.
    on_lost: Closure<dyn FnMut(web_sys::Event)>,
    on_restored: Closure<dyn FnMut(web_sys::Event)>,
}

impl ContextLossHandler {
    fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsValue> {
        let state = Rc::new(RefCell::new(ContextState::default()));

        let lost_state = Rc::clone(&state);
        let on_lost = Self::listen(canvas, "webglcontextlost", move |event: web_sys::Event| {
            // Without `preventDefault()` the browser never fires `webglcontextrestored` — the
            // context stays dead forever. Every reference implementation does this first
            // (beamterm context_loss.rs, xterm.js WebglRenderer.ts).
            event.prevent_default();
            lost_state.borrow_mut().on_lost();
        })?;

        let restored_state = Rc::clone(&state);
        let on_restored = Self::listen(canvas, "webglcontextrestored", move |_event| {
            restored_state.borrow_mut().on_restored();
        })?;

        Ok(Self {
            canvas: canvas.clone(),
            state,
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
    u_padding_frac: glow::UniformLocation,
    u_bg_alpha: glow::UniformLocation,
    u_default_bg: glow::UniformLocation,
}

/// The justerm-family WebGL2 terminal renderer.
#[wasm_bindgen]
pub struct JustermRenderer {
    gl: glow::Context,
    /// The bound canvas — kept so `resize` can size its drawing buffer (device px) and CSS box.
    canvas: HtmlCanvasElement,
    /// devicePixelRatio the atlas + drawing buffer are currently sized for (#265). The atlas is
    /// rasterised at `FONT_SIZE * dpr` (device px) so HiDPI stays sharp; a DPR change re-bakes it.
    dpr: f32,
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    atlas: glow::Texture,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    u_bg_alpha: glow::UniformLocation,
    /// Background cell opacity (0 = transparent, 1 = opaque), consumer-injected policy (#298).
    bg_alpha: f32,
    palette: Palette,
    rasterizer: Rasterizer,
    cache: GlyphCache,
    /// Physical (content) cell size in device pixels — the on-screen grid cell.
    cell_size: (f32, f32),
    /// Padded atlas cell size in device pixels (physical + `2*PADDING`) — glyph upload dims.
    atlas_cell: (u32, u32),
    /// Drawing-buffer size in device pixels.
    size: (i32, i32),
    /// The last CSS-px size passed to [`resize`](Self::resize) (#322): a DPR change re-sizes the
    /// buffer to `css × new_dpr` without the consumer re-passing it (mirrors beamterm's
    /// `logical_size`). Defaults to the initial canvas size until the first resize.
    css_size: (i32, i32),
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
    /// the 256 pre-built indexed colours (see [`Palette::from_colors`]).
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

        let gl = glow::Context::from_webgl2_context(webgl2);
        let size = (canvas.width() as i32, canvas.height() as i32);

        // Attach before ANY GL work, so a loss during construction is observed rather than missed
        // (the pipeline/atlas built below would then be silently invalidated) (#269).
        let ctx_loss = ContextLossHandler::new(&canvas)?;

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
        let (cell_w, cell_h) = rasterizer.cell_size(); // physical (on-screen grid) cell
        let (pad_w, pad_h) = rasterizer.padded_size(); // padded atlas cell

        let Pipeline {
            program,
            vao,
            instance_vbo,
            u_projection,
            u_cell_size,
            u_padding_frac,
            u_bg_alpha,
            u_default_bg,
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
            // The default background is fixed for the life of the renderer (the palette is set at
            // construction), so the shader can compare each cell's bg against it once (#298).
            let [dbr, dbg, dbb] = gl_rgb(palette.default_bg);
            gl.uniform_3_f32(Some(&u_default_bg), dbr, dbg, dbb);
        }

        let renderer = JustermRenderer {
            gl,
            canvas,
            dpr,
            program,
            vao,
            instance_vbo,
            atlas,
            u_projection,
            u_cell_size,
            u_bg_alpha,
            bg_alpha: 1.0, // opaque by default (#298)
            palette,
            rasterizer,
            cache: GlyphCache::new(),
            cell_size: (cell_w as f32, cell_h as f32),
            atlas_cell: (pad_w, pad_h),
            size,
            // The initial canvas dims are device px; store their CSS equivalent so a `resize`-less
            // `set_device_pixel_ratio` fallback stays dimensionally consistent (resize multiplies
            // css_size back by the DPR). A real consumer calls `resize(css)` before rendering.
            css_size: (
                (size.0 as f32 / dpr).round().max(1.0) as i32,
                (size.1 as f32 / dpr).round().max(1.0) as i32,
            ),
            instances: Vec::new(),
            instance_count: 0,
            uploaded: Vec::new(),
            grid: None,
            ctx_loss,
        };
        renderer.prebake_ascii()?;
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

            // Per-instance [col, row, bg(3), fg(3), glyph] → locations 1..4.
            let instance_vbo = gl.create_buffer().map_err(js_err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
            for (loc, size, offset) in [(1u32, 2i32, 0i32), (2, 3, 8), (3, 3, 20), (4, 1, 32)] {
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
            let u_padding_frac = uniform(gl, program, "u_padding_frac")?;
            let u_bg_alpha = uniform(gl, program, "u_bg_alpha")?;
            let u_default_bg = uniform(gl, program, "u_default_bg")?;
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
                u_padding_frac,
                u_bg_alpha,
                u_default_bg,
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
    /// it (so glyphs stay present and sharpen), and the drawing buffer re-sized to the stored CSS
    /// size × the new DPR. A no-op if the ratio is unchanged; on error the old atlas is left intact
    /// and `dpr` unadvanced, so the next notification retries (self-healing).
    #[wasm_bindgen(js_name = setDevicePixelRatio)]
    pub fn set_device_pixel_ratio(&mut self, dpr: f32) -> Result<(), JsValue> {
        if !dpr_changed(self.dpr, dpr) {
            return Ok(());
        }
        // A lost context can only hand back an invalidated atlas texture, so re-baking now would
        // burn the work and commit an empty atlas. Drop the notification: `restore` re-reads the
        // *live* DPR and bakes at that density anyway (#269).
        if self.ctx_loss.state.borrow().is_lost() {
            return Ok(());
        }
        // 1. Build a new atlas at the new device size and re-rasterise the CURRENT glyphs into it —
        //    ASCII fast path + every resident dynamic glyph, each into its SAME slot — all before
        //    committing. A failure leaves the old atlas / rasteriser / dpr untouched (the next
        //    notify retries), and the glyph *slots* are preserved, so the existing instances stay
        //    valid (no re-pack / re-upload). Independent of apply_frame vs apply_damage — both
        //    populate the glyph cache. The ~tens-of-µs cost (#321) is fine for a rare DPR change.
        let rasterizer = Rasterizer::new("monospace", FONT_SIZE * dpr)?;
        let (cell_w, cell_h) = rasterizer.cell_size();
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
        //    drop the old atlas, and advance `dpr` only now that the re-bake succeeded.
        let old_atlas = self.atlas;
        self.rasterizer = rasterizer;
        self.atlas = atlas;
        self.cell_size = (cell_w as f32, cell_h as f32);
        self.atlas_cell = (pad_w, pad_h);
        self.dpr = dpr;
        unsafe { self.gl.delete_texture(old_atlas) };
        // 3. Re-size the buffer to the stored CSS size at the new DPR (the cells sharpen via the
        //    new atlas + the new device `cell_size` uniform on the next render).
        let (cw, ch) = self.css_size;
        self.resize(cw, ch);
        Ok(())
    }

    /// Whether the WebGL context is currently lost (#269). While lost the renderer draws nothing;
    /// it recovers by itself when the browser fires `webglcontextrestored`. Exposed so the consumer
    /// can surface the state (e.g. dim the terminal); no consumer action is required.
    #[wasm_bindgen(js_name = isContextLost)]
    pub fn is_context_lost(&self) -> bool {
        self.ctx_loss.state.borrow().is_lost()
    }

    /// Recreate every GPU resource the lost context destroyed (#269), then refill the instance
    /// buffer so the very next `render` paints the pre-loss frame. Called by [`render`](Self::render)
    /// when the state machine reports [`FrameAction::Rebuild`] — never on a lost context.
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
        let rasterizer = Rasterizer::new("monospace", FONT_SIZE * dpr)?;
        let (cell_w, cell_h) = rasterizer.cell_size();
        let (pad_w, pad_h) = rasterizer.padded_size();

        // 1. Build the replacements without touching any live field.
        let pipeline = Self::build_pipeline(&self.gl)?;
        let atlas = Self::build_atlas(&self.gl, pad_w, pad_h)?;
        let rebake = (|| -> Result<(), JsValue> {
            self.bake_all_glyphs(&rasterizer, atlas, (pad_w, pad_h))?;
            // Both uniforms are set once per program at construction, so the relinked program needs
            // them again: the guard-band fraction and the default background the shader compares
            // each cell's bg against to decide translucency (#298).
            self.set_padding_frac(pipeline.program, &pipeline.u_padding_frac, pad_w, pad_h);
            let [dbr, dbg, dbb] = gl_rgb(self.palette.default_bg);
            unsafe {
                self.gl
                    .uniform_3_f32(Some(&pipeline.u_default_bg), dbr, dbg, dbb);
            }
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
        self.u_bg_alpha = pipeline.u_bg_alpha;
        self.rasterizer = rasterizer;
        self.cell_size = (cell_w as f32, cell_h as f32);
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

        // 4. The loss reset the drawing-buffer size and the viewport; re-apply the stored CSS box at
        //    the (possibly new) DPR, then refill the buffer so `render` draws the pre-loss frame.
        let (cw, ch) = self.css_size;
        self.resize(cw, ch);
        self.upload_instances();
        Ok(())
    }

    /// Measured cell width in **CSS pixels** — the consumer lays out in CSS and the renderer owns
    /// the DPR (#252/#265). Internally the cell is device px (`cell_size`); this divides it back.
    pub fn cell_width(&self) -> u32 {
        (self.cell_size.0 / self.dpr).round().max(1.0) as u32
    }

    /// Measured cell height in **CSS pixels** (see [`cell_width`](Self::cell_width)).
    pub fn cell_height(&self) -> u32 {
        (self.cell_size.1 / self.dpr).round().max(1.0) as u32
    }

    /// Resize to a `width`×`height` **CSS-pixel** box (#252/#265): the renderer sizes the GL
    /// drawing buffer to `CSS × devicePixelRatio` (device px, so HiDPI is sharp) — the caller must
    /// NOT pre-multiply by DPR. The atlas is kept (AC "아틀라스 유지"); the DPR is fixed at
    /// construction, so a mid-session DPR change (dragging to another-density monitor) is not yet
    /// re-baked (tracked follow-up) and stays at the construction density until recreation.
    pub fn resize(&mut self, width: i32, height: i32) {
        // Size the GL drawing buffer to device px; the canvas's CSS display box is owned by the
        // consumer / external CSS (as with beamterm's `auto_resize_canvas_css = false`), so the
        // device-px buffer shown in a CSS-px box gives the HiDPI density.
        self.css_size = (width, height);
        let (dw, dh) = (device_px(width, self.dpr), device_px(height, self.dpr));
        self.canvas.set_width(dw as u32);
        self.canvas.set_height(dh as u32);
        self.size = (dw, dh);
        unsafe {
            self.gl.viewport(0, 0, self.size.0, self.size.1);
        }
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
    pub fn apply_frame(
        &mut self,
        cols: u32,
        rows: u32,
        bg: &[u32],
        fg: &[u32],
        codepoints: &[u32],
        flags: &[u16],
        blink_on: bool,
    ) -> Result<(), JsValue> {
        // The direct (dense, cluster-free) path: one base codepoint per cell.
        let cells = Cells {
            cols,
            rows,
            codepoints,
            flags,
            clusters: &[],
        };
        self.resolve_and_pack(&cells, bg, fg, blink_on)
    }

    /// Resolve each cell's glyph slot then pack the instance buffer. Shared by [`apply_frame`]
    /// (no clusters) and [`apply_damage`] (grapheme clusters from the persistent grid, #285).
    ///
    /// [`apply_frame`]: Self::apply_frame
    /// [`apply_damage`]: Self::apply_damage
    fn resolve_and_pack(
        &mut self,
        cells: &Cells,
        bg: &[u32],
        fg: &[u32],
        blink_on: bool,
    ) -> Result<(), JsValue> {
        let count = (cells.cols * cells.rows) as usize;
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
        })?;

        let frame = Frame {
            cols: cells.cols,
            rows: cells.rows,
            bg,
            fg,
            slots: &slots,
            flags: cells.flags,
        };
        self.instances = pack_instances(&frame, &self.palette, blink_on);
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
    /// ([`SPAN_STRIDE`](crate::frame_grid::SPAN_STRIDE) `u32`s each);
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
        extra: &[u16],
        side_table: Vec<String>,
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
            _ => FrameGrid::new(cols, rows),
        };
        grid.apply(&DamageFrame {
            kind,
            scroll,
            spans,
            codepoints,
            fg,
            bg,
            flags,
            extra,
            side_table: &side_table,
        });
        let cells = Cells {
            cols,
            rows,
            codepoints: grid.codepoints(),
            flags: grid.flags(),
            clusters: grid.clusters(),
        };
        let result = self.resolve_and_pack(&cells, grid.bg(), grid.fg(), blink_on);
        self.grid = Some(grid);
        result
    }

    /// Set the background cell opacity: `0` = fully transparent, `1` = opaque (default). The
    /// consumer injects this policy (ADR-0017) to make the terminal background see-through to the
    /// page/desktop behind the canvas, while glyph pixels stay opaque. Clamped to `[0, 1]`; takes
    /// effect on the next [`render`](Self::render) (#298).
    #[wasm_bindgen(js_name = setBgAlpha)]
    pub fn set_bg_alpha(&mut self, alpha: f32) {
        self.bg_alpha = alpha.clamp(0.0, 1.0);
    }

    /// Clear to the palette's default background, then draw every cell of the current frame
    /// (glyph composited over background) with one instanced draw call.
    ///
    /// Context loss (#269) is handled here, before any GL work: while the context is lost this is a
    /// silent no-op (a draw call on a dead context accomplishes nothing), and on the frame after
    /// `webglcontextrestored` it first rebuilds the destroyed resources. Recovery therefore needs
    /// no consumer cooperation beyond continuing to call `render`. A failed rebuild propagates and
    /// is retried on the next frame.
    pub fn render(&mut self) -> Result<(), JsValue> {
        // Bind the decision to a local: the `Ref` must be released before the `&mut self` calls.
        let action = self.ctx_loss.state.borrow().action();
        match action {
            FrameAction::Skip => return Ok(()),
            FrameAction::Rebuild => {
                self.restore()?;
                // Only now that the rebuild is committed does the retry latch clear.
                self.ctx_loss.state.borrow_mut().rebuilt();
            }
            FrameAction::Draw => {}
        }
        self.draw();
        Ok(())
    }

    /// Issue the frame's GL commands. The caller has established that the context is live and its
    /// resources are intact.
    fn draw(&self) {
        let [dr, dg, db] = gl_rgb(self.palette.default_bg);
        unsafe {
            // Clear with the injected background opacity so any area not covered by a cell (canvas
            // margins) is see-through too; cells then write their own per-pixel alpha (#298).
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
            self.gl
                .uniform_2_f32(Some(&self.u_cell_size), self.cell_size.0, self.cell_size.1);
            self.gl.uniform_1_f32(Some(&self.u_bg_alpha), self.bg_alpha);

            self.gl
                .draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, self.instance_count);
        }
    }
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
