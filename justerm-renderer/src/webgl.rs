//! Thin `#[wasm_bindgen]` + WebGL2 glue — browser-only (wasm32), verified in the demo.
//!
//! `JustermRenderer` and its three tiers (global, per-config, per-grid), the accessors every export
//! goes through, the constructor and the GL builders. Every other export except `cursorRects` lives
//! in the child modules below, one per axis. A frame's cells are resolved against the injected palette and the
//! glyph cache, packed one instance per cell — highlights folded into the packed background — and
//! drawn with one instanced call per grid; the cursor is a shader uniform composited last.

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::config_registry::{ConfigId, ConfigKey, ConfigRegistry};
use crate::css_font::FontWeight;
use crate::cursor::{
    Cursor, DEFAULT_CURSOR_CONTRAST, THICKNESS, cursor_cells_at, cursor_rects, shape_from_id,
};
use crate::decoration::{
    DecorationLayer, DecorationOverride, decoration_override_at, parse_decorations,
};
use crate::frame::{BACKDROP, BG_RGB, GLYPH_FIELD, INSTANCE_FLOATS, NEIGHBOUR_UP, NEIGHBOUR_UP_FG};
use crate::frame_grid::{DamageFrame, FrameGrid};
use crate::glyph_cache::{GLYPHS_PER_LAYER, GlyphCache, WIDE_BASE, WIDE_CAPACITY, slot_texcoord};
use crate::glyph_resolve::{Cells, FramePins};
use crate::overlay::{HighlightColors, Overlay};
use crate::palette::Palette;
use crate::preedit::{
    Codepoint as PreeditCodepoint, Patch as PreeditPatch, Span as PreeditSpan,
    caret_col as preedit_caret_col_of, is_wide as preedit_is_wide, patch as preedit_patch_of,
    writes as preedit_writes,
};
use crate::rasterizer::Rasterizer;
use crate::registry::{GridId, GridRegistry};
use crate::shader::{FRAG_SRC, VERT_SRC};
use crate::suggestion::patch as suggestion_patch_of;

/// The GL context lifecycle — the loss/restore listeners, the restore deadline, and the exports
/// and rebuild that act on a lost context.
mod context;

/// The grid registry — registering, removing and placing grids, and the configurations they hold.
mod grids;

/// Per-grid state — damage, the overlays, the palette, the caret, the preedit and suggestion runs, and the colour and cursor policies.
mod grid_state;

/// Font configurations — baking an atlas, the device pixel ratio, and the font and spacing selectors that choose a configuration.
mod font;

/// Cell and surface geometry — the cell a grid draws, resizing a grid, and the drawing buffer's size.
mod surface;

/// The frame path — a frame in, packed, uploaded and drawn.
mod draw;

use context::ContextLossHandler;

/// Texture-array layers covering the whole slot space (normal + wide = 6144 / 32 = 192),
/// so wide/emoji slots (layers 64..191) have storage.
const TOTAL_LAYERS: i32 = ((WIDE_BASE + WIDE_CAPACITY * 2) / GLYPHS_PER_LAYER) as i32;
/// Default font size (CSS px) for the atlas rasteriser.
const FONT_SIZE: f32 = 16.0;

/// The CSS `font-family` a grid is born with when `addGrid` names none (#773).
const DEFAULT_FONT_FAMILY: &str = "monospace";

/// A font weight a consumer named from JS: a number, or a weight keyword string (#928). `None` for
/// anything [`FontWeight`] refuses.
fn weight_from_js(v: &JsValue) -> Option<FontWeight> {
    match (v.as_f64(), v.as_string()) {
        (Some(n), _) => FontWeight::from_number(n),
        (None, Some(s)) => FontWeight::from_keyword(&s),
        (None, None) => None,
    }
}

/// Unit-quad corners (triangle strip): geometry + per-cell glyph texture coordinate.
const QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
/// Byte stride of one packed instance, **derived** from [`INSTANCE_FLOATS`] rather than written
/// out. Why: `docs/map/territory/gpu-upload.md` § The instance layout is stated once.
const INSTANCE_STRIDE: i32 = (INSTANCE_FLOATS * 4) as i32;

/// The GPU objects one grid owns (#771): its instance buffer and the VAO that points at it, built
/// together because the VAO's whole content is which buffer feeds the draw. Why:
/// `docs/map/territory/multi-viewport.md` § Registration.
struct GridBuffers {
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
}

/// The GL program, the shared quad buffer and the uniform locations (`build_pipeline`), built at
/// construction and again by each restore.
struct Pipeline {
    program: glow::Program,
    /// The one quad every cell instance is drawn from — static, identical for every grid, and
    /// referenced by each grid's VAO. Global by ADR-0021 D2: two grids share it byte-for-byte.
    quad_vbo: glow::Buffer,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    u_char_size: glow::UniformLocation,
    u_char_offset: glow::UniformLocation,
    u_line_thickness: glow::UniformLocation,
    u_dots_per_cell: glow::UniformLocation,
    u_cell_uv: glow::UniformLocation,
    u_bg_alpha: glow::UniformLocation,
    u_bleed_px: glow::UniformLocation,
    u_lcd_gamma: glow::UniformLocation,
    u_cursor: glow::UniformLocation,
    u_cursor_color: glow::UniformLocation,
    u_cursor_text_color: glow::UniformLocation,
    u_cursor_thickness: glow::UniformLocation,
}

/// The justerm-family WebGL2 terminal renderer.
#[wasm_bindgen]
pub struct JustermRenderer {
    /// Resources one per WebGL context, invalidated only by context loss (ADR-0021 D2).
    global: GlobalTier,
    /// Resources keyed **per font configuration** — expensive to rebuild, so shared rather than
    /// duplicated (ADR-0021 D2, #772). One grid selects into a configuration; it does not own one,
    /// and six terminals in one font hold one atlas between them.
    configs: ConfigRegistry<ConfigTier>,
    /// Every terminal grid this renderer holds, and which of them are drawn (#770, ADR-0021 D1/D2)
    /// — the one tier multi-viewport (#287) multiplies. **Empty until the consumer registers one**
    /// (#773). Why: `docs/map/territory/multi-viewport.md` § A renderer holds no terminal until the
    /// consumer registers one.
    grids: GridRegistry<GridTier>,
    /// Count of `resolve_and_pack` runs — a diagnostic the proofs read to assert render packs once
    /// per frame, not once per setter (#421). Wraps harmlessly; only deltas are meaningful.
    pack_count: u32,
    /// The glyphs the current pack scope may not evict (#772): `render` clears it once for every
    /// grid it packs, `apply_frame` for its own immediate pack. A field because it models a scope
    /// with one owner (`docs/map/territory/multi-viewport.md` § The draw loop).
    pins: FramePins,
    /// Count of atlas bakes — every construction of a `ConfigTier`, plus every in-place rebuild of
    /// one (a DPR change, a context restore). Read as a delta, like `pack_count`: a grid joining an
    /// existing configuration moves it by zero (#772). Wraps harmlessly. Why:
    /// `docs/map/territory/multi-viewport.md` § The counters.
    bake_count: u32,
}

/// The **global** tier — one per WebGL2 context (ADR-0021 D2: invalidated only by context loss),
/// however many grids the context draws.
struct GlobalTier {
    gl: glow::Context,
    /// The bound canvas — kept so `resize_surface` can size its drawing buffer (device px).
    canvas: HtmlCanvasElement,
    /// devicePixelRatio every configuration's atlas is currently baked for (#265): the atlas is
    /// rasterised at `font_size * dpr` (device px) so HiDPI stays sharp.
    dpr: f32,
    program: glow::Program,
    /// The shared per-vertex quad. Global, unlike the VAO that points at it: two grids share these
    /// four vertices byte-for-byte, and neither can share a VAO (ADR-0021 D2, #771).
    quad_vbo: glow::Buffer,
    u_projection: glow::UniformLocation,
    u_cell_size: glow::UniformLocation,
    u_char_size: glow::UniformLocation,
    u_char_offset: glow::UniformLocation,
    u_line_thickness: glow::UniformLocation,
    u_dots_per_cell: glow::UniformLocation,
    /// The guard band's fraction of a padded atlas cell — a per-draw uniform, since the padded cell
    /// is per configuration (`docs/map/territory/multi-viewport.md` § The draw loop).
    u_cell_uv: glow::UniformLocation,
    u_bg_alpha: glow::UniformLocation,
    u_bleed_px: glow::UniformLocation,
    u_lcd_gamma: glow::UniformLocation,
    u_cursor: glow::UniformLocation,
    u_cursor_color: glow::UniformLocation,
    u_cursor_text_color: glow::UniformLocation,
    u_cursor_thickness: glow::UniformLocation,
    /// `MAX_TEXTURE_SIZE`, read once. The atlas is `padded_w x padded_h * GLYPHS_PER_LAYER`, so a tall
    /// `lineHeight` can ask for a texture the implementation refuses to allocate — silently (#359).
    max_texture_size: u32,
    /// The same WebGL2 context `gl` wraps, kept for the handful of questions glow does not ask:
    /// `drawingBufferWidth`/`drawingBufferHeight`, which are the ONLY way to learn that the browser
    /// clamped the buffer we requested (#339). Context restore reuses the context object, so this
    /// handle survives a loss.
    raw_gl: WebGl2RenderingContext,
    /// Drawing-buffer size in device pixels — what the browser actually granted, which is not
    /// always what was asked for (#339).
    size: (i32, i32),
    /// The drawing buffer the consumer last **asked** for, in device px — as given, unconverted
    /// (#773). `size` above is what the browser granted; this is what to re-ask for when the buffer
    /// has to be rebuilt (a restore) and nobody is going to re-ask. Why verbatim:
    /// `docs/map/territory/cell-geometry.md` § The surface's size is the consumer's device-px
    /// request.
    requested: (i32, i32),
    /// Canvas context-loss listeners + the shared lost/pending-rebuild state (#269). `render`
    /// consults it every frame: skip while lost, rebuild once restored, otherwise draw.
    ctx_loss: ContextLossHandler,
}

/// The **per-config** tier — one per font configuration (the seven selectors a `ConfigKey` holds;
/// not the DPR, which every entry shares).
///
/// ADR-0021 D2: two grids with equal selectors are served by **one instance**, and rebuilding this
/// is expensive enough to repay keying it. #772 made that real: the facade holds a refcounted
/// [`ConfigRegistry`] of these and a grid holds a [`ConfigId`] into it, never its own copy.
///
/// D4: this tier names the **owner**, not the only place a value may sit. A reader may cache
/// `cell_size` — see `docs/map/invariant/cell-size-is-derived-state.md` for what such a copy owes.
struct ConfigTier {
    atlas: glow::Texture,
    /// The glyph box in device px — the face's floored advance wide and its line box tall
    /// (ADR-0022, #962, #986). Equal to `cell_size` only
    /// while both spacing options are at their defaults (#338).
    char_size: (u32, u32),
    /// Where the glyph box sits inside the cell, device px from its top-left (#338).
    char_offset: (u32, u32),
    rasterizer: Rasterizer,
    cache: GlyphCache,
    /// Physical (content) cell size in **device pixels** — the on-screen grid cell, and the exact
    /// `u_cell_size` the shader lays it out with. Integral by construction (a floored advance and a ceiled line box).
    cell_size: (u32, u32),
    /// Padded atlas cell size in device pixels (physical + `2*PADDING`) — glyph upload dims.
    atlas_cell: (u32, u32),
}

/// A configuration's resources, built and not yet committed (#772).
///
/// Every path that produces a `ConfigTier`'s GPU state goes through one function, because all three
/// need the same thing built the same way and only differ in what they do with it: a *new* entry
/// (`fresh`), and an in-place rebuild of an existing one at a new density (`adopt`) — a DPR change
/// or a context restore. Keeping the build separate from the commit is what makes those two atomic.
struct BakedConfig {
    atlas: glow::Texture,
    rasterizer: Rasterizer,
    cell_size: (u32, u32),
    char_size: (u32, u32),
    char_offset: (u32, u32),
    atlas_cell: (u32, u32),
}

impl ConfigTier {
    /// A brand-new configuration: the baked resources plus an empty glyph cache.
    fn fresh(baked: BakedConfig) -> Self {
        ConfigTier {
            atlas: baked.atlas,
            rasterizer: baked.rasterizer,
            cache: GlyphCache::new(),
            cell_size: baked.cell_size,
            char_size: baked.char_size,
            char_offset: baked.char_offset,
            atlas_cell: baked.atlas_cell,
        }
    }

    /// Swap in a rebuild of *this* configuration, **keeping the glyph cache**, and hand back the
    /// outgoing atlas for the caller to delete. The cache survives because the rebuild re-baked
    /// every resident glyph into the same slot, so the packed instances that address them stay
    /// valid — which is why a DPR change costs no re-pack.
    fn adopt(&mut self, baked: BakedConfig) -> glow::Texture {
        let old = self.atlas;
        self.atlas = baked.atlas;
        self.rasterizer = baked.rasterizer;
        self.cell_size = baked.cell_size;
        self.char_size = baked.char_size;
        self.char_offset = baked.char_offset;
        self.atlas_cell = baked.atlas_cell;
        old
    }
}

/// The **per-grid** tier — one terminal's own state (ADR-0021 D1/D2).
///
/// Everything a consumer can set differently per terminal is a **selector** and lands here, including
/// the seven font/metric fields: they are per-grid *as settings*, while the machinery they key
/// (`ConfigTier`) is not. `instance_vbo` and `vao` are per-grid too (ADR-0021 D2). A method that
/// touches only this tier is an inherent method here rather than on the facade. Why:
/// `docs/map/territory/multi-viewport.md` § Resources sort into three tiers, and its `## Blast
/// radius` entry for GPU upload.
struct GridTier {
    /// The configuration this grid draws through — the atlas, rasteriser, glyph cache and cell it
    /// selects into (#772). A **handle, not a copy**: the seven selector fields below say what this
    /// grid asked for, and this says which shared entry serves it. `adopt_selectors` writes the
    /// selectors and `select_config` moves this handle to match — on a lost context the handle
    /// follows at the restore.
    config: ConfigId,
    instance_vbo: glow::Buffer,
    /// The VAO that points at this grid's `instance_vbo` — per-grid for the same reason the buffer
    /// is (#771).
    vao: glow::VertexArray,
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
    /// Consumer-injected policy (ADR-0017), in **CSS px** — ADR-0023 for why not device px (#338).
    letter_spacing: f32,
    /// Consumer-injected policy: a multiplier on the glyph height. Clamped to `>= 1` (#338).
    line_height: f32,
    /// Consumer-injected font size in **CSS px** (#406); the atlas rasterises at `font_size * dpr`.
    /// Default [`FONT_SIZE`]. Changed by `set_font_size`, which joins the configuration it names, so
    /// a restored context bakes at the consumer's size, not the hardcoded default.
    font_size: f32,
    /// Consumer-injected CSS `font-family` (#413); default `"monospace"`. Changed by
    /// `set_font_family`, which joins a configuration as a size change does. The browser's text
    /// engine resolves it (with fallback); the renderer stays font-agnostic.
    font_family: String,
    /// Consumer-injected weights for regular and bold text (#928); default CSS `normal` / `bold`.
    /// Changed by `set_font_weight` / `set_font_weight_bold`, which join a configuration as a family
    /// change does.
    font_weight: FontWeight,
    font_weight_bold: FontWeight,
    /// Whether this grid's text carries per-channel (LCD) coverage (#961); default off. Changed by
    /// `set_subpixel_antialiasing`, which joins a configuration as a family change does.
    subpixel: bool,
    palette: Palette,
    /// The `cols`×`rows` grid last passed to `resizeGrid` — what this grid
    /// reports and what the frames fed to it are expected to carry. Nothing derives the drawing
    /// buffer from it (#773; that is `GlobalTier::requested`).
    grid_size: (u32, u32),
    instances: Vec<f32>,
    instance_count: i32,
    /// The instance floats currently in the GPU buffer — the baseline the next pack diffs against
    /// so only changed cells re-upload (#263). Empty until the first upload (forces a `Full`).
    ///
    /// INVARIANT: this mirrors what the live `instance_vbo` holds, so it is valid ONLY while that
    /// buffer persists. WebGL **context loss** destroys the buffer, so [`restore`](JustermRenderer::restore)
    /// calls [`invalidate_baseline`](crate::upload::invalidate_baseline) on it — otherwise the next identical frame diffs to zero
    /// ranges and never refills the fresh (empty) buffer → a blank render that won't self-heal.
    uploaded: Vec<f32>,
    /// Persistent dense grid for the decoder→renderer frame adapter (#277): a Partial frame's
    /// span-ordered damage scatters into this before packing. `None` until the first
    /// `apply_damage`; re-created when the grid dimensions change.
    grid: Option<FrameGrid>,
    /// The selection / search overlay spans this frame (#271), owned so a re-pack can borrow them.
    /// Stride-3 `(row, left, right)` viewport triples, as the decoder ships them. Empty = no
    /// highlight. Updated by `setOverlay`; composited into each cell's packed
    /// background at pack time.
    selection_spans: Vec<u32>,
    match_spans: Vec<u32>,
    /// The *active* (focused/current) search-match spans (#427), same stride. Set via
    /// [`set_active_match`](Self::set_active_match); the active match is also present in
    /// `match_spans`, and the `highlight_at` ranking (ActiveMatch > Selection > Match) is what makes
    /// its colour win where they overlap. Empty = no active match.
    active_match_spans: Vec<u32>,
    /// The hovered link's spans (#934), same stride — drawn underlined. Set via
    /// [`set_link_hover`](JustermRenderer::set_link_hover). Empty = no link hovered.
    link_hover_spans: Vec<u32>,
    /// The in-progress IME composition and the cell it is anchored to (#249, ADR-0028). Empty = no
    /// composition. The engine never sees it — it reaches no frame and no wire — so it arrives only
    /// from the consumer's browser events, and it is a *pass* over the composed cells rather than a
    /// layer in the stack (ADR-0019's amendment).
    preedit_run: Vec<PreeditCodepoint>,
    preedit_col: u32,
    preedit_row: u32,
    /// The consumer's suggestion run, the cell it starts at, its tagged fg reference and whether it
    /// draws dim (#972). Empty = no suggestion. A separate writer from the preedit (ADR-0028 D4),
    /// drawn only while no composition is open.
    suggestion_run: Vec<PreeditCodepoint>,
    suggestion_col: u32,
    suggestion_row: u32,
    suggestion_fg: u32,
    suggestion_dim: bool,
    /// The consumer-injected blend colours for the overlay kinds (policy #115).
    highlight_colors: HighlightColors,
    /// Draw bold text in the bright (8–15) ANSI colour, consumer policy (xterm's
    /// `drawBoldTextInBrightColors`). Default on, as xterm; toggled via `set_bold_to_bright`.
    bold_to_bright: bool,
    /// Minimum WCAG fg/bg contrast ratio, consumer policy (xterm's `minimumContrastRatio`).
    /// `1.0` = off (default). Set via `set_minimum_contrast_ratio`; clamped to `[1, 21]`.
    min_contrast: f32,
    /// Force a SELECTED cell's fg to this packed `0xRRGGBB` (#227/#272, xterm's `selectionForeground`).
    /// `None` = keep each cell's own fg (default). Selection only, never a search match.
    selection_fg: Option<u32>,
    /// Marker-anchored decoration rects this frame (#393), the flat `DECORATION_STRIDE` wire the
    /// consumer projects each frame. Parsed at pack time; empty = no decorations. Owned so a re-pack
    /// can borrow it. Updated by [`set_decorations`](Self::set_decorations).
    decoration_spans: Vec<u32>,
    /// The last blink phase packed, so a `setOverlay` re-pack (no new frame)
    /// keeps the cursor/blink cells in the phase the render loop last drove.
    last_blink_on: bool,
    /// The eviction count of this grid's configuration at its last successful pack (#772). `render`
    /// compares it against the configuration's live count and re-packs the difference away — a
    /// sibling's pack can repoint a slot this grid's instances still address, where the upload diff
    /// cannot see it. Why it converges, and where it cannot: `docs/map/territory/multi-viewport.md`
    /// § The middle tier's hazard went LIVE.
    packed_at_evictions: u32,
    /// Set by every state mutation that changes the packed instance buffer (overlay, decorations,
    /// colour policy, palette, `apply_damage`); cleared by the re-pack in `render`.
    /// Lets a frame that sets overlay + decorations + damage re-pack **once** at render instead of
    /// three times, one per setter (#421). The direct `apply_frame` path packs immediately (no grid
    /// to defer to) and clears it.
    needs_repack: bool,
}

/// Reinterpret an `f32` slice as bytes for `buffer_data` upload.
fn f32_bytes(v: &[f32]) -> &[u8] {
    // Safety: `f32` has no padding/invalid bytes; length is exact.
    unsafe { core::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// Upload one glyph's RGBA bitmap to its `(layer, band)` in the atlas. A free function (not
/// a `&self` method) so the frame resolver's upload closure can borrow only the GL fields,
/// leaving the drawing configuration's `&mut cache` free for [`resolve_frame`](crate::glyph_resolve::resolve_frame).
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
    /// The registry slot a consumer's grid handle addresses, or the error the wasm boundary throws
    /// (#773). Every export that addresses a grid's state starts here; the registry exports ask
    /// `GridRegistry` by id. An unknown or removed id is a caller error arriving from JS, so it throws
    /// rather than silently addressing something.
    fn slot(&self, grid: u32) -> Result<usize, JsValue> {
        self.grids
            .index_of(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// The configuration a registry slot's grid draws through — the pack path's and the draw loop's
    /// form, addressed by slot for the same reason `grid_at` is.
    fn config_at(&self, at: usize) -> &ConfigTier {
        self.configs.get(self.grid_at(at).config)
    }

    /// The grid in a registry slot — how the draw loop and the pack path address a grid (#771).
    /// See `GridRegistry::viewport_at` for why those two walk slots while every consumer-facing
    /// export takes an id.
    fn grid_at(&self, at: usize) -> &GridTier {
        self.grids.grid_at(at)
    }

    /// Mutable form of [`grid_at`](Self::grid_at).
    fn grid_at_mut(&mut self, at: usize) -> &mut GridTier {
        self.grids.grid_at_mut(at)
    }

    /// Bind a renderer to the canvas matched by `canvas_selector`.
    ///
    /// It arrives holding **no terminal and no font configuration**: the palette and the seven
    /// font selectors belong to a grid, and `addGrid` is what creates one. So the
    /// first two calls a consumer makes are `new` then `addGrid`, and nothing is baked in between —
    /// there is nothing yet to key an atlas by.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_selector: &str) -> Result<JustermRenderer, JsValue> {
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

        // `getContext` hands back an already-lost context unchanged, and glow's constructor below
        // panics on it (#688). One check, asking the context itself, covers the whole constructor.
        // Why: `docs/map/territory/gl-context-lifecycle.md` § Construction is the one entry point
        // that refuses.
        if webgl2.is_context_lost() {
            return Err(JsValue::from_str(
                "justerm-renderer: webgl2 context is lost",
            ));
        }

        // Attached before any GL work; it cannot report a loss during construction (#688).
        let ctx_loss = ContextLossHandler::new(&canvas)?;

        let raw_gl = webgl2.clone();
        let gl = glow::Context::from_webgl2_context(webgl2);
        // Read once: the atlas is sized from the cell (#359), and the cell is the consumer's to grow.
        // `.max(1)` defends glow's `0` for a `null` answer, not the driver (#688).
        let max_texture_size =
            unsafe { gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE).max(1) as u32 };
        let size = (canvas.width() as i32, canvas.height() as i32);

        // devicePixelRatio: the atlas rasterises at device px (FONT_SIZE * dpr) so HiDPI is sharp;
        // the consumer speaks CSS px (#252). Fallback 1.0 off the main thread / in tests.
        let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio() as f32);

        let Pipeline {
            program,
            quad_vbo,
            u_projection,
            u_cell_size,
            u_char_size,
            u_char_offset,
            u_line_thickness,
            u_dots_per_cell,
            u_cell_uv,
            u_bg_alpha,
            u_bleed_px,
            u_lcd_gamma,
            u_cursor,
            u_cursor_color,
            u_cursor_text_color,
            u_cursor_thickness,
        } = Self::build_pipeline(&gl)?;

        let renderer = JustermRenderer {
            global: GlobalTier {
                gl,
                canvas,
                dpr,
                program,
                quad_vbo,
                u_projection,
                u_cell_size,
                u_char_size,
                u_char_offset,
                u_line_thickness,
                u_dots_per_cell,
                u_cell_uv,
                u_bg_alpha,
                u_bleed_px,
                u_lcd_gamma,
                u_cursor,
                u_cursor_color,
                u_cursor_text_color,
                u_cursor_thickness,
                max_texture_size,
                raw_gl,
                size,
                // The canvas as authored. The consumer normally overwrites this immediately with
                // `resizeSurface`; what it is for is that a context restore — which reaches
                // `apply_surface_size` with no consumer in the loop — always has a request to
                // re-ask for.
                requested: size,
                ctx_loss,
            },
            // Both registries start **empty** (#773): no terminal until the consumer registers one,
            // and so no font configuration to key an atlas by.
            configs: ConfigRegistry::new(),
            grids: GridRegistry::new(),
            pack_count: 0,
            pins: FramePins::new(),
            bake_count: 0,
        };
        let mut renderer = renderer;
        // End in a resize, as beamterm's `create_with_canvas` does: it sets the GL viewport and
        // adopts whatever buffer the browser actually granted for the canvas as authored.
        renderer.apply_surface_size();
        Ok(renderer)
    }

    fn build_pipeline(gl: &glow::Context) -> Result<Pipeline, JsValue> {
        let program = Self::link_program(gl, VERT_SRC, FRAG_SRC)?;

        // Safety: all calls are on a live GL context; buffers/attribs are set up once.
        unsafe {
            // Per-vertex quad geometry. Filled here and pointed at location 0 by every grid's VAO
            // (`build_grid_buffers`) — an attribute pointer is VAO state, so it cannot be set here.
            let quad_vbo = gl.create_buffer().map_err(js_err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, f32_bytes(&QUAD), glow::STATIC_DRAW);

            let u_projection = uniform(gl, program, "u_projection")?;
            let u_cell_size = uniform(gl, program, "u_cell_size")?;
            let u_char_size = uniform(gl, program, "u_char_size")?;
            let u_char_offset = uniform(gl, program, "u_char_offset")?;
            let u_line_thickness = uniform(gl, program, "u_line_thickness")?;
            let u_dots_per_cell = uniform(gl, program, "u_dots_per_cell")?;
            let u_cell_uv = uniform(gl, program, "u_cell_uv")?;
            let u_bg_alpha = uniform(gl, program, "u_bg_alpha")?;
            let u_bleed_px = uniform(gl, program, "u_bleed_px")?;
            let u_lcd_gamma = uniform(gl, program, "u_lcd_gamma")?;
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
                quad_vbo,
                u_projection,
                u_cell_size,
                u_char_size,
                u_char_offset,
                u_line_thickness,
                u_dots_per_cell,
                u_cell_uv,
                u_bg_alpha,
                u_bleed_px,
                u_lcd_gamma,
                u_cursor,
                u_cursor_color,
                u_cursor_text_color,
                u_cursor_thickness,
            })
        }
    }

    /// One grid's instance buffer and the VAO that points at it (#771). The attribute *layout* is
    /// the same for every grid — there is one program; what differs is which buffer feeds it.
    fn build_grid_buffers(
        gl: &glow::Context,
        quad_vbo: glow::Buffer,
    ) -> Result<GridBuffers, JsValue> {
        // Safety: live GL context. Every call below is buffer/attribute setup on a fresh VAO.
        unsafe {
            let vao = gl.create_vertex_array().map_err(js_err)?;
            gl.bind_vertex_array(Some(vao));

            // Per-vertex quad geometry → location 0. The buffer is the shared one; only the
            // pointer into it is per-VAO.
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.enable_vertex_attrib_array(0);

            // Per-instance [col, row, bg(3), fg(3), glyph, underline_fg, strike_fg, bg_default,
            // neighbour slots(4), neighbour inks(4)] → locations 1..9.
            let instance_vbo = gl.create_buffer().map_err(js_err)?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
            // Byte offsets come from `frame`'s named float offsets (#791), except three literals
            // (`docs/map/territory/gpu-upload.md` § The instance layout is stated once).
            const F: i32 = 4; // bytes per float
            for (loc, size, offset) in [
                (1u32, 2i32, 0i32),
                (2, 3, BG_RGB as i32 * F),
                (3, 3, 5 * F),
                (4, 1, GLYPH_FIELD as i32 * F),
                (5, 1, 9 * F),
                (6, 1, 10 * F),
                (7, 1, BACKDROP as i32 * F),
                // The four neighbours' slots and the four inks, one `vec4` each (#966).
                (8, 4, NEIGHBOUR_UP as i32 * F),
                (9, 4, NEIGHBOUR_UP_FG as i32 * F),
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
            Ok(GridBuffers { vao, instance_vbo })
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
            // NEAREST, matching beamterm: a cell samples texel-exact. The band seam is guarded by
            // `bitmap::PADDING`, not by the filter (`docs/map/territory/glyph-atlas.md` § How the
            // fragment stage reads a slot).
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

impl GridTier {
    /// One terminal's state at rest: no cells, no cursor, no overlays, every consumer policy at
    /// its default. The caller supplies what a grid cannot default — its own GPU buffers
    /// (ADR-0021 D2), the configuration it selects into and the key that names it — which carries
    /// the seven font/metric **selectors** (D1: per-grid settings, even though the machinery they
    /// key is per-config) — its palette, and the grid it is sized to.
    ///
    /// The seven selectors are **unpacked from the key itself** rather than passed beside it, so the
    /// grid's fields and the entry its handle names cannot be born disagreeing. One caller,
    /// `add_grid` (#773).
    fn new(
        buffers: GridBuffers,
        config: ConfigId,
        key: &ConfigKey,
        palette: Palette,
        grid_size: (u32, u32),
    ) -> Self {
        GridTier {
            config,
            instance_vbo: buffers.instance_vbo,
            vao: buffers.vao,
            cursor: None,
            cursor_cells: (0, 1),
            last_flags: Vec::new(),
            last_cols: 0,
            bg_alpha: 1.0,                            // opaque by default (#298)
            cursor_contrast: DEFAULT_CURSOR_CONTRAST, // guard on by default (#368)
            cursor_thickness_frac: THICKNESS,         // alacritty's 0.15 by default (#369)
            palette,
            letter_spacing: key.letter_spacing(),
            line_height: key.line_height(),
            font_size: key.font_size(),
            font_family: key.font_family().to_string(),
            font_weight: key.font_weight(),
            font_weight_bold: key.font_weight_bold(),
            subpixel: key.subpixel(),
            grid_size,
            instances: Vec::new(),
            instance_count: 0,
            uploaded: Vec::new(),
            grid: None,
            selection_spans: Vec::new(),
            match_spans: Vec::new(),
            active_match_spans: Vec::new(), // no active/focused match by default (#427)
            link_hover_spans: Vec::new(),   // no link hovered (#934)
            preedit_run: Vec::new(),        // no composition open (#249)
            preedit_col: 0,
            preedit_row: 0,
            suggestion_run: Vec::new(), // no suggestion (#972)
            suggestion_col: 0,
            suggestion_row: 0,
            suggestion_fg: 0,
            suggestion_dim: false,
            highlight_colors: HighlightColors::default(),
            bold_to_bright: true, // xterm's drawBoldTextInBrightColors default (#223)
            min_contrast: 1.0,    // xterm's minimumContrastRatio default: off (#225)
            selection_fg: None,   // no selectionForeground override by default (#227)
            decoration_spans: Vec::new(), // no marker decorations by default (#393)
            last_blink_on: true,
            // Nothing packed yet, and the fresh configuration has evicted nothing — so a grid born
            // into an OLD configuration whose cache has already evicted reads as stale on its first
            // render and packs, which is the answer that costs nothing and cannot be wrong.
            packed_at_evictions: 0,
            needs_repack: false,
        }
    }
}

/// Per-grid operations (ADR-0021 D1/D2): inherent methods on the one tier multi-viewport
/// (#287) multiplies, so a method that reached into another tier would not compile here.
impl GridTier {
    fn set_bg_alpha(&mut self, alpha: f32) {
        self.bg_alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    fn set_bold_to_bright(&mut self, enabled: bool) -> Result<(), JsValue> {
        self.bold_to_bright = enabled;
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    fn set_selection_foreground(&mut self, color: Option<u32>) -> Result<(), JsValue> {
        self.selection_fg = color.map(|c| c & 0xFF_FFFF);
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    fn set_minimum_contrast_ratio(&mut self, ratio: f32) -> Result<(), JsValue> {
        self.min_contrast = if ratio.is_finite() {
            ratio.clamp(1.0, 21.0)
        } else {
            1.0
        };
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    fn set_cursor_contrast(&mut self, threshold: f32) {
        self.cursor_contrast = threshold.clamp(1.0, 21.0);
    }

    fn set_cursor_thickness(&mut self, frac: f32) {
        self.cursor_thickness_frac = frac.clamp(0.0, 1.0);
    }
}

/// Per-grid operations, continued.
impl GridTier {
    fn set_palette(
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
        // A re-pack is all a live theme swap needs (`docs/map/territory/colour-policy.md` § A live
        // palette swap is a re-pack).
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    fn set_overlay(
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

    fn set_active_match(&mut self, active_spans: Vec<u32>, active_match_bg: u32) {
        self.active_match_spans = active_spans;
        self.highlight_colors.active_match_bg = active_match_bg;
        self.needs_repack = true; // defer the pack to render (#421), same as set_overlay
    }

    fn set_decorations(&mut self, spans: Vec<u32>) -> Result<(), JsValue> {
        self.decoration_spans = spans;
        self.needs_repack = true; // defer the pack to render (#421)
        Ok(())
    }

    fn set_cursor(
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

    fn clear_cursor(&mut self) {
        self.cursor = None;
    }

    fn resolve_cursor_cells(&mut self) {
        self.cursor_cells = self.cursor.map_or((0, 1), |c| {
            cursor_cells_at(&self.last_flags, self.last_cols, c.col, c.row)
        });
    }

    fn cols(&self) -> u32 {
        self.grid_size.0
    }

    fn rows(&self) -> u32 {
        self.grid_size.1
    }

    fn preedit_caret_col(&self) -> u32 {
        let cols = self.cols();
        let last = cols.saturating_sub(1);
        if self.preedit_run.is_empty() || cols == 0 {
            return self.preedit_col.min(last);
        }
        preedit_caret_col_of(&self.preedit_run, self.preedit_col, last)
    }

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

    fn preedit_patch(
        &self,
        cells: &Cells,
        bg: &[u32],
        fg: &[u32],
    ) -> Option<PreeditPatch<'static>> {
        preedit_patch_of(
            &self.preedit_run,
            self.preedit_col,
            self.preedit_row,
            cells,
            bg,
            fg,
        )
    }
}

/// Per-grid operations, continued.
impl GridTier {
    #[allow(clippy::too_many_arguments)]
    fn apply_damage(
        &mut self,
        header: &[u32],
        spans: &[u32],
        codepoints: &[u32],
        fg: &[u32],
        bg: &[u32],
        flags: &[u16],
        extra: &[u32],
        side_table: Vec<String>,
        // #520: the span-ordered underline colour column (SGR 58), tagged-u32 like `fg`/`bg`;
        // omitted ⇒ all Default.
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
        // renderer stays usable (#355).
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
        // Defer the pack to `render` (#421), which re-packs once however many setters the consumer
        // calls around this: store the blink phase the deferred `repack_from_grid` reads, put the
        // grid back, and mark dirty. A pack error surfaces at `render`, not here.
        self.last_blink_on = blink_on;
        self.grid = Some(grid);
        self.needs_repack = true;
        Ok(())
    }

    fn set_preedit(&mut self, col: u32, row: u32, codepoints: Vec<u32>) -> u32 {
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

    fn set_suggestion(&mut self, col: u32, row: u32, codepoints: Vec<u32>, fg: u32, dim: bool) {
        self.suggestion_run = codepoints
            .into_iter()
            .map(|cp| PreeditCodepoint {
                cp,
                wide: preedit_is_wide(cp),
            })
            .collect();
        self.suggestion_col = col;
        self.suggestion_row = row;
        self.suggestion_fg = fg;
        self.suggestion_dim = dim;
        self.needs_repack = true; // defer the pack to render (#421), same as set_overlay
    }

    /// The frame's columns with the suggestion written in (#972), or `None` when there is none, when
    /// a composition is open, or when it writes no cell. A cell covered by a highlight, either
    /// decoration layer or the hovered link is withheld.
    fn suggestion_patch<'c>(
        &self,
        cells: &Cells<'c>,
        bg: &'c [u32],
        fg: &[u32],
    ) -> Option<PreeditPatch<'c>> {
        if self.suggestion_run.is_empty() || !self.preedit_run.is_empty() {
            return None;
        }
        let overlay = Overlay {
            active: &self.active_match_spans,
            selection: &self.selection_spans,
            matches: &self.match_spans,
            link_hover: &self.link_hover_spans,
            colors: self.highlight_colors,
        };
        let decorations = parse_decorations(&self.decoration_spans);
        let row = self.suggestion_row;
        let withheld = |col: u32| {
            overlay.highlight_at(row, col, None).is_some()
                || overlay.is_link_hovered(row, col, None)
                || [DecorationLayer::Bottom, DecorationLayer::Top]
                    .into_iter()
                    .any(|layer| {
                        decoration_override_at(&decorations, row, col, None, layer)
                            != DecorationOverride::default()
                    })
        };
        suggestion_patch_of(
            &self.suggestion_run,
            self.suggestion_col,
            row,
            self.suggestion_fg,
            self.suggestion_dim,
            cells,
            bg,
            fg,
            withheld,
        )
    }
}
