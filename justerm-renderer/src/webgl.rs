//! Thin `#[wasm_bindgen]` + WebGL2 glue — browser-only (wasm32), verified in the demo.
//!
//! The instanced pipeline draws the whole grid in one call: `apply_frame` resolves each
//! cell's bg/fg references (injected palette) and its glyph slot (glyph cache, rasterising
//! and uploading new glyphs on demand), packs one instance per cell, and `render`
//! composites each glyph's coverage from the atlas over its background, plus SGR attrs
//! (#267: bold/italic font variants, underline/strikethrough lines, inverse fg/bg swap) and
//! double-width glyphs (#268: a wide glyph splits across two atlas slots / two grid cells).
//! ASCII (`0x20..=0x7E`) is pre-rasterised. Colour emoji (#284) + clusters (#285) follow.

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::attrs::{font_style, is_wide_lead, is_wide_spacer};
use crate::bitmap::split_wide_bitmap;
use crate::color::gl_rgb;
use crate::frame::pack_instances;
use crate::glyph_cache::{
    FontStyle, GLYPHS_PER_LAYER, GlyphCache, GlyphKey, GlyphKind, WIDE_BASE, WIDE_CAPACITY,
    slot_texcoord,
};
use crate::mat4::Mat4;
use crate::palette::Palette;
use crate::rasterizer::Rasterizer;

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
    // The glyph field packs slot (bits 0..12), underline (bit 13), strikethrough (bit 14).
    uint slot = v_glyph & 0x1FFFu;
    uint layer = slot >> 5u;   // 32 glyphs stack vertically per layer
    uint band = slot & 31u;
    vec3 tc = vec3(v_tex.x, (float(band) + v_tex.y) / 32.0, float(layer));
    float coverage = texture(u_atlas, tc).a;

    float underline = float((v_glyph >> 13u) & 1u);
    float strike = float((v_glyph >> 14u) & 1u);
    // Fixed cell-local positions (underline below baseline, strikethrough mid-cell). beamterm
    // derives these per-font from metrics; a font-metric-driven position is a later refinement.
    float line = max(hline(v_tex.y, 0.88, 0.05) * underline, hline(v_tex.y, 0.5, 0.05) * strike);

    FragColor = vec4(mix(v_bg, v_fg, max(coverage, line)), 1.0);
}
"#;

/// The justerm-family WebGL2 terminal renderer.
#[wasm_bindgen]
pub struct JustermRenderer {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    atlas: glow::Texture,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    palette: Palette,
    rasterizer: Rasterizer,
    cache: GlyphCache,
    /// Measured cell size in device pixels.
    cell_size: (f32, f32),
    /// Drawing-buffer size in device pixels.
    size: (i32, i32),
    instances: Vec<f32>,
    instance_count: i32,
}

/// Reinterpret an `f32` slice as bytes for `buffer_data` upload.
fn f32_bytes(v: &[f32]) -> &[u8] {
    // Safety: `f32` has no padding/invalid bytes; length is exact.
    unsafe { core::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
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
        let webgl2: WebGl2RenderingContext = canvas
            .get_context("webgl2")?
            .ok_or_else(|| JsValue::from_str("justerm-renderer: no webgl2 context"))?
            .dyn_into()?;

        let gl = glow::Context::from_webgl2_context(webgl2);
        let size = (canvas.width() as i32, canvas.height() as i32);

        let palette =
            Palette::from_colors(&palette_colors, default_fg, default_bg).map_err(|e| {
                JsValue::from_str(&format!(
                    "justerm-renderer: palette must be 256 colours, got {}",
                    e.got
                ))
            })?;

        let rasterizer = Rasterizer::new("monospace", FONT_SIZE)?;
        let (cell_w, cell_h) = rasterizer.cell_size();

        let (program, vao, instance_vbo, u_projection, u_cell_size) = Self::build_pipeline(&gl)?;
        let atlas = Self::build_atlas(&gl, cell_w, cell_h)?;

        let mut renderer = JustermRenderer {
            gl,
            program,
            vao,
            instance_vbo,
            atlas,
            u_projection,
            u_cell_size,
            palette,
            rasterizer,
            cache: GlyphCache::new(),
            cell_size: (cell_w as f32, cell_h as f32),
            size,
            instances: Vec::new(),
            instance_count: 0,
        };
        renderer.prebake_ascii()?;
        Ok(renderer)
    }

    fn build_pipeline(
        gl: &glow::Context,
    ) -> Result<
        (
            glow::Program,
            glow::VertexArray,
            glow::Buffer,
            glow::UniformLocation,
            glow::UniformLocation,
        ),
        JsValue,
    > {
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
            // The atlas sampler stays on texture unit 0.
            gl.use_program(Some(program));
            let u_atlas = uniform(gl, program, "u_atlas")?;
            gl.uniform_1_i32(Some(&u_atlas), 0);

            Ok((program, vao, instance_vbo, u_projection, u_cell_size))
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
    fn prebake_ascii(&mut self) -> Result<(), JsValue> {
        for cp in 0x20u32..=0x7E {
            let ch = char::from_u32(cp).unwrap();
            let rgba = self
                .rasterizer
                .rasterize(&ch.to_string(), FontStyle::Normal, false)?;
            self.upload_glyph((cp - 0x20) as u16, &rgba);
        }
        Ok(())
    }

    /// Upload one glyph's RGBA bitmap to its `(layer, band)` in the atlas.
    fn upload_glyph(&self, slot: u16, rgba: &[u8]) {
        let (cell_w, cell_h) = (self.cell_size.0 as i32, self.cell_size.1 as i32);
        let (layer, band) = slot_texcoord(slot);
        // Safety: live GL context; the sub-image fits the allocated storage.
        unsafe {
            self.gl
                .bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas));
            self.gl.tex_sub_image_3d(
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

    /// Measured cell width in device pixels (from the atlas rasteriser).
    pub fn cell_width(&self) -> u32 {
        self.cell_size.0 as u32
    }

    /// Measured cell height in device pixels.
    pub fn cell_height(&self) -> u32 {
        self.cell_size.1 as u32
    }

    /// Resize the drawing buffer to `width`×`height` device pixels.
    pub fn resize(&mut self, width: i32, height: i32) {
        self.size = (width.max(1), height.max(1));
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
    ) -> Result<(), JsValue> {
        let count = (cols * rows) as usize;
        let mut slots = Vec::with_capacity(count);
        // A wide lead assigns its right half to the next (spacer) cell.
        let mut pending_right: Option<u16> = None;
        for idx in 0..count {
            // A wide glyph never spans rows (justerm-core wraps a lead off the last column),
            // so a pending right-half must not leak across a row boundary — reset at col 0
            // to stay correct even on a malformed (direct-caller) frame.
            if (idx as u32).is_multiple_of(cols) {
                pending_right = None;
            }
            let cell_flags = flags.get(idx).copied().unwrap_or(0);

            // A spacer draws the lead glyph's right-half slot (no rasterise of its own).
            if is_wide_spacer(cell_flags) {
                slots.push(pending_right.take().unwrap_or(0));
                continue;
            }
            pending_right = None;

            let cp = codepoints.get(idx).copied().unwrap_or(0x20);
            let ch = char::from_u32(cp).filter(|c| *c != '\0').unwrap_or(' ');
            let text = ch.to_string();
            let style = font_style(cell_flags);
            let wide = is_wide_lead(cell_flags);
            let kind = if wide {
                GlyphKind::Wide
            } else {
                GlyphKind::Normal
            };

            let alloc = self.cache.get_or_insert(
                GlyphKey {
                    text: text.clone(),
                    style,
                },
                kind,
            );
            let base = alloc.slot.slot_id();
            if alloc.is_new {
                let rgba = self.rasterizer.rasterize(&text, style, wide)?;
                if wide {
                    let (left, right) =
                        split_wide_bitmap(&rgba, self.cell_size.0 as u32, self.cell_size.1 as u32);
                    self.upload_glyph(base, &left);
                    self.upload_glyph(base + 1, &right);
                } else {
                    self.upload_glyph(base, &rgba);
                }
            }
            slots.push(base);
            if wide {
                pending_right = Some(base + 1);
            }
        }
        self.instances = pack_instances(cols, rows, bg, fg, &slots, flags, &self.palette);
        self.instance_count = count as i32;
        Ok(())
    }

    /// Clear to the palette's default background, then draw every cell of the current frame
    /// (glyph composited over background) with one instanced draw call.
    pub fn render(&self) {
        let [dr, dg, db] = gl_rgb(self.palette.default_bg);
        unsafe {
            self.gl.clear_color(dr, dg, db, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

            if self.instance_count == 0 {
                return;
            }

            self.gl.use_program(Some(self.program));
            self.gl.active_texture(glow::TEXTURE0);
            self.gl
                .bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas));
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl
                .bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
            self.gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                f32_bytes(&self.instances),
                glow::DYNAMIC_DRAW,
            );

            let proj = Mat4::orthographic_from_size(self.size.0 as f32, self.size.1 as f32);
            self.gl
                .uniform_matrix_4_f32_slice(Some(&self.u_projection), false, &proj.data);
            self.gl
                .uniform_2_f32(Some(&self.u_cell_size), self.cell_size.0, self.cell_size.1);

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
