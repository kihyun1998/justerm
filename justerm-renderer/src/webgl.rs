//! Thin `#[wasm_bindgen]` + WebGL2 glue — browser-only (wasm32), verified in the demo.
//!
//! The instanced pipeline — GL context → program → quad + per-instance buffers → ortho
//! projection uniform → one `drawArraysInstanced`. `apply_frame` resolves a frame's
//! background references with the injected palette and packs one instance per cell (#261,
//! pure logic in `frame`/`palette`); `render` clears then draws the whole grid in one
//! call. Glyphs land in #264.

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::color::gl_rgb;
use crate::frame::pack_bg_instances;
use crate::mat4::Mat4;
use crate::palette::Palette;

/// Unit-quad corners (triangle strip): the geometry each cell instance is scaled by.
const QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

const VERT_SRC: &str = r#"#version 300 es
layout(location = 0) in vec2 a_pos;   // unit-quad corner (0..1)
layout(location = 1) in vec2 a_cell;  // instance: (col, row)
layout(location = 2) in vec3 a_bg;    // instance: background rgb (0..1)
uniform mat4 u_projection;
uniform vec2 u_cell_size;             // cell size in device pixels
out vec3 v_bg;
void main() {
    vec2 origin = a_cell * u_cell_size;
    vec2 pos = floor(origin + a_pos * u_cell_size + 0.5); // pixel-snapped
    gl_Position = u_projection * vec4(pos, 0.0, 1.0);
    v_bg = a_bg;
}
"#;

const FRAG_SRC: &str = r#"#version 300 es
precision mediump float;
in vec3 v_bg;
out vec4 FragColor;
void main() { FragColor = vec4(v_bg, 1.0); }
"#;

/// The justerm-family WebGL2 terminal renderer.
///
/// Consumer-side (ADR-0018): consumes a decoded frame + an injected palette and paints
/// via WebGL2. `justerm-core` stays render-agnostic. `apply_frame` resolves each cell's
/// background reference with the injected palette and packs one instance per cell (#261);
/// glyphs land in #264.
#[wasm_bindgen]
pub struct JustermRenderer {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    /// The consumer's frozen colour scheme (ADR-0002: the consumer owns hex).
    palette: Palette,
    /// Cell size in device pixels (a fixed placeholder until the glyph atlas sets it, #264).
    cell_size: (f32, f32),
    /// Drawing-buffer size in device pixels.
    size: (i32, i32),
    /// Packed per-cell instance data `[col, row, r, g, b]` for the current frame.
    instances: Vec<f32>,
    /// Number of cells (instances) to draw.
    instance_count: i32,
}

/// Reinterpret an `f32` slice as bytes for `buffer_data` upload.
fn f32_bytes(v: &[f32]) -> &[u8] {
    // Safety: `f32` has no padding/invalid bytes; length is exact.
    unsafe { core::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

#[wasm_bindgen]
impl JustermRenderer {
    /// Bind a renderer to the canvas matched by `canvas_selector`. The consumer injects
    /// the colour scheme: `palette_colors` must be the **256** pre-built indexed colours
    /// (`0xRRGGBB`) — e.g. justerm-wasm-decode `buildPalette` (16 ANSI + 6×6×6 cube +
    /// grayscale) — plus the two defaults. A length other than 256 is rejected, not padded
    /// (padding with black would resolve `Indexed(16..=255)` to `0x000000`).
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

        let (program, vao, instance_vbo, u_projection, u_cell_size) = Self::build_pipeline(&gl)?;

        Ok(JustermRenderer {
            gl,
            program,
            vao,
            instance_vbo,
            u_projection,
            u_cell_size,
            palette,
            cell_size: (16.0, 32.0),
            size,
            instances: Vec::new(),
            instance_count: 0,
        })
    }

    /// Compile the program and wire the quad + per-instance vertex arrays. The instance
    /// buffer stays empty until `render` uploads the cell.
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

            // Per-instance [col, row, r, g, b] (stride 20) → locations 1 (cell) & 2 (bg).
            let instance_vbo = gl.create_buffer().map_err(js_err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 20, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_divisor(1, 1);
            gl.vertex_attrib_pointer_f32(2, 3, glow::FLOAT, false, 20, 8);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_divisor(2, 1);

            gl.bind_vertex_array(None);

            let u_projection = gl
                .get_uniform_location(program, "u_projection")
                .ok_or_else(|| JsValue::from_str("justerm-renderer: no u_projection"))?;
            let u_cell_size = gl
                .get_uniform_location(program, "u_cell_size")
                .ok_or_else(|| JsValue::from_str("justerm-renderer: no u_cell_size"))?;

            Ok((program, vao, instance_vbo, u_projection, u_cell_size))
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

    /// Resize the drawing buffer to `width`×`height` device pixels.
    pub fn resize(&mut self, width: i32, height: i32) {
        self.size = (width.max(1), height.max(1));
        unsafe {
            self.gl.viewport(0, 0, self.size.0, self.size.1);
        }
    }

    /// Apply a `cols`×`rows` frame's background: `bg` holds one tagged-u32 colour
    /// reference per cell in **dense row-major** order (length `cols*rows`), each in the
    /// `justerm_core::encode_color` encoding. A **Full** decoded frame is already dense
    /// row-major; a **Partial** frame is a span-ordered damaged *subset* and must be
    /// scattered into a full grid before it reaches here — that decoder→renderer frame
    /// adapter is tracked as its own slice. Each ref is resolved with the injected palette
    /// and packed into one instance per cell — the hot path stays in Rust (ADR-0018 "A-ii").
    /// #264 adds glyphs.
    pub fn apply_frame(&mut self, cols: u32, rows: u32, bg: &[u32]) {
        self.instances = pack_bg_instances(cols, rows, bg, &self.palette);
        self.instance_count = (cols * rows) as i32;
    }

    /// Clear to the palette's default background, then draw every cell of the current
    /// frame with one instanced draw call.
    pub fn render(&self) {
        let [dr, dg, db] = gl_rgb(self.palette.default_bg);
        unsafe {
            self.gl.clear_color(dr, dg, db, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

            if self.instance_count == 0 {
                return;
            }

            self.gl.use_program(Some(self.program));
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

/// Wrap a GL/string error as a `JsValue`.
fn js_err(msg: String) -> JsValue {
    JsValue::from_str(&format!("justerm-renderer: {msg}"))
}
