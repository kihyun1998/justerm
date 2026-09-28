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

use glow::HasContext;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::bitmap::{PADDING, is_color_bitmap, split_wide_bitmap};
use crate::color::gl_rgb;
use crate::config_registry::{ConfigId, ConfigKey, ConfigRegistry};
use crate::context_loss::{ContextLiveness, FrameAction};
use crate::css_font::FontWeight;
use crate::cursor::{
    Cursor, DEFAULT_CURSOR_CONTRAST, THICKNESS, cursor_cells_at, cursor_rects, cursor_thickness,
    guarded_cursor_colors, shape_from_id, shape_id,
};
use crate::decoration::{
    DecorationLayer, DecorationOverride, decoration_override_at, parse_decorations,
};
use crate::dpr::{css_px, dpr_changed};
use crate::emoji::is_emoji_text;
use crate::frame::{
    BACKDROP, BG_RGB, Frame, GLYPH_FIELD, INSTANCE_FLOATS, NEIGHBOUR_UP, NEIGHBOUR_UP_FG,
    pack_instances,
};
use crate::frame_grid::{DamageFrame, FrameGrid, cell_count};
use crate::glyph_cache::{
    FontStyle, GLYPHS_PER_LAYER, GlyphCache, GlyphSlot, WIDE_BASE, WIDE_CAPACITY, slot_texcoord,
};
use crate::glyph_resolve::{Cells, FramePins, ResolveError, resolve_frame};
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
use crate::registry::{GridId, GridRegistry, Viewport};
use crate::render_policy::ColorPolicy;
use crate::shader::{FRAG_SRC, VERT_SRC};
use crate::suggestion::patch as suggestion_patch_of;
use crate::upload::{UploadPlan, plan_upload};

/// The GL context lifecycle — the loss/restore listeners, the restore deadline, and the exports
/// and rebuild that act on a lost context.
mod context;

use context::ContextLossHandler;

/// Texture-array layers covering the whole slot space (normal + wide = 6144 / 32 = 192),
/// so wide/emoji slots (layers 64..191) have storage.
const TOTAL_LAYERS: i32 = ((WIDE_BASE + WIDE_CAPACITY * 2) / GLYPHS_PER_LAYER) as i32;
/// Default font size (CSS px) for the atlas rasteriser.
const FONT_SIZE: f32 = 16.0;

/// The CSS `font-family` a grid is born with (#773). A grid arrives on the configuration these
/// defaults key, and moves off it with `setFontFamily` / `setFontSize` / the spacing setters — so a
/// consumer whose terminals all share one non-default font pays exactly **one** bake for this
/// configuration, and it is released the moment the last grid leaves it.
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
/// Byte stride of one packed instance. **Derived** from [`INSTANCE_FLOATS`] rather than written out:
/// the two drifting apart silently mis-addresses every attribute, and nothing in the pipeline would
/// say so — the geometry would simply be wrong. It was a literal `9 * 4` until #513 widened the
/// record, then #455 (the `bg_default` provenance flag) and #525 (the second line ink) widened it
/// again — three times in one release cycle, which is the whole argument for deriving it.
/// The per-attribute **offsets** in `build_pipeline` are still literals and are not derived; a float
/// inserted anywhere but the end moves them, so they are edited with this constant, not after it.
const INSTANCE_STRIDE: i32 = (INSTANCE_FLOATS * 4) as i32;

/// The GL program + buffers + uniform locations built once at startup (`build_pipeline`).
/// The GPU objects one grid owns (#771). Built together because the VAO's whole content is where
/// the instance buffer is: `vertex_attrib_pointer_f32` captures the buffer bound at the time, so a
/// VAO is not a thing two grids can share — ADR-0021 D2 asks whether one instance can serve two
/// grids *byte-for-byte*, and this one cannot. #768 left that open as "either a per-grid VAO or a
/// re-pointer per grid per frame"; the re-pointer would keep the per-grid fact in the global tier
/// and rewrite it N times a frame, which is the arrangement D2 rejects rather than a cheaper form
/// of it.
struct GridBuffers {
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
}

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
    /// Every terminal grid this renderer holds, and which of them are drawn (#770, ADR-0021 D1/D2).
    /// This is the field multi-viewport (#287) multiplies; the other two tiers stay one each, which
    /// the types above say by being singular rather than by being asserted anywhere.
    ///
    /// **Empty until the consumer registers one** (#773). Every per-grid export names the grid it
    /// acts on, so there is nothing left for an implicit grid to serve — and one would be a third
    /// terminal painting under the two a surface actually holds.
    grids: GridRegistry<GridTier>,
    /// Count of `resolve_and_pack` runs — a diagnostic the proofs read to assert render packs once
    /// per frame, not once per setter (#421). Wraps harmlessly; only deltas are meaningful.
    pack_count: u32,
    /// The glyphs the current pack scope may not evict (#772).
    ///
    /// A field rather than a threaded parameter because the thing it models is a **scope**, and a
    /// scope with one owner is clearer as state than as an argument every layer forwards: `render`
    /// clears it once and every grid it packs shares it, so a grid cannot evict a slot a sibling
    /// committed to in the same frame; `apply_frame` clears it for its own immediate pack, which is
    /// the only scope that pack actually has.
    pins: FramePins,
    /// Count of atlas bakes — every construction of a `ConfigTier`, plus every in-place rebuild of
    /// one (a DPR change, a context restore). The diagnostic that makes *sharing* observable rather
    /// than inferred (#772): a grid joining an existing configuration must move this by zero.
    ///
    /// ADR-0021 D5 leaves where such counters live to whoever adds the second one; this is that
    /// one, and it lands beside `pack_count` because the question is the same shape — a proof
    /// reading a **delta** across an operation, not a rendering control. Wraps harmlessly.
    bake_count: u32,
}

/// The **global** tier — one per WebGL2 context (ADR-0021 D2: invalidated only by context loss).
///
/// Multi-viewport (#287) keeps exactly one of these however many grids the context draws, which is
/// the whole point of the design: the context, the compiled program and the vertex layout are what N
/// terminals stop paying for N times.
struct GlobalTier {
    gl: glow::Context,
    /// The bound canvas — kept so `resize_surface` can size its drawing buffer (device px).
    canvas: HtmlCanvasElement,
    /// devicePixelRatio the atlas + drawing buffer are currently sized for (#265). The atlas is
    /// rasterised at `font_size * dpr` (device px) so HiDPI stays sharp; a DPR change — or a
    /// `set_font_size` (#406) / `set_font_family` (#413) — re-bakes it.
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
    /// The guard band's fraction of a padded atlas cell. Held here — rather than being set once
    /// per program as it was until #772 — because the padded cell is a **per-config** dimension:
    /// two grids in different fonts sample atlases with different guard fractions, so this became
    /// a per-draw uniform the moment a second configuration was reachable.
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
    /// has to be rebuilt (a restore) and nobody is going to re-ask.
    ///
    /// **Stored verbatim, and that is the whole point.** Until S5 this role was played by the
    /// implicit default grid's `grid_size` — the buffer was `cols * cell`, a density-*independent*
    /// quantity — which stops being derivable the moment two grids in two cells share one canvas.
    /// The obvious replacement, a CSS box, is not one: converting device px to CSS px needs a
    /// density, the only density available here is *our copy* of the consumer's, and that copy lags
    /// by construction — `set_device_pixel_ratio` drops the notification outright while the context
    /// is lost. A box stored through a lagging basis comes back wrong by the ratio between the two
    /// densities, which is exactly what the old cell-count storage could not do. The fact belongs to
    /// the site it is first true at (ADR-0032, "a shared fact is owned by its producer"), and
    /// that site is the consumer's measurement — so it is kept as measured.
    requested: (i32, i32),
    /// Canvas context-loss listeners + the shared lost/pending-rebuild state (#269). `render`
    /// consults it every frame: skip while lost, rebuild once restored, otherwise draw.
    ctx_loss: ContextLossHandler,
}

/// The **per-config** tier — one per font configuration (family, size, spacing, DPR).
///
/// ADR-0021 D2: two grids with equal selectors are served by **one instance**, and rebuilding this
/// is expensive enough to repay keying it. #772 made that real: the facade holds a refcounted
/// [`ConfigRegistry`] of these and a grid holds a [`ConfigId`] into it, never its own copy.
///
/// D4: this tier names the **owner**, not the only place a value may sit. A reader may cache
/// `cell_size` — see `docs/map/invariant/cell-size-is-derived-state.md` for what such a copy owes.
struct ConfigTier {
    atlas: glow::Texture,
    /// The glyph box in device px — the face's floored advance wide and the ink box of its `█` tall
    /// (ADR-0022). Equal to `cell_size` only
    /// while both spacing options are at their defaults (#338).
    char_size: (u32, u32),
    /// Where the glyph box sits inside the cell, device px from its top-left (#338).
    char_offset: (u32, u32),
    rasterizer: Rasterizer,
    cache: GlyphCache,
    /// Physical (content) cell size in **device pixels** — the on-screen grid cell, and the exact
    /// `u_cell_size` the shader lays it out with. Integral by construction (a floored advance and an ink scan).
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
/// (`ConfigTier`) is not. `instance_vbo` is here rather than global because `uploaded` mirrors it —
/// one shared buffer with N per-grid baselines would let one grid's upload silently invalidate
/// another's — and `vao` followed it in #771, because a VAO records which buffer feeds the draw.
///
/// Multi-viewport (#287) multiplies **this struct and nothing else**, so a method that touches only
/// this tier is written as an inherent method here rather than on the facade. That is the split's
/// load-bearing half: moving a field out of this struct breaks those methods at compile time.
struct GridTier {
    /// The configuration this grid draws through — the atlas, rasteriser, glyph cache and cell it
    /// selects into (#772). A **handle, not a copy**: the seven selector fields below say what this
    /// grid asked for, and this says which shared entry serves it. The two are kept in step by
    /// `select_config`, the only writer of either.
    config: ConfigId,
    instance_vbo: glow::Buffer,
    /// The VAO that points at this grid's `instance_vbo`. Per-grid for the same reason the buffer
    /// is: an attribute pointer captures the buffer bound when it was set, so a VAO's content *is*
    /// which grid's cells feed the draw (#771 resolves the corollary #768 recorded).
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
    /// reports and what the frames fed to it are expected to carry.
    ///
    /// **Nothing derives the drawing buffer from it any more** (#773). While one grid owned the
    /// canvas the buffer was `cols * cell`, so a DPR change could re-derive it from here; a surface
    /// drawing N grids in M cell sizes has no such multiple to be, so the buffer is asked for
    /// directly and kept as asked (`GlobalTier::requested`).
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
    /// (Surfaced by the #263 adversarial 2-lens pass; implemented in #269.)
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
    /// composition. Unlike every other retained state here this describes something the **engine
    /// never sees** — the preedit reaches no frame and no wire — so it can only arrive from the
    /// consumer's browser events, and it is a *pass* over the composed cells rather than a layer in
    /// the stack (ADR-0019's amendment).
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
    /// The eviction count of this grid's configuration at its last successful pack (#772).
    ///
    /// Sharing a glyph cache means another grid's pack can **repoint** a slot this grid's instances
    /// still address, and the upload diff — the defence against a slot changing under an undamaged
    /// cell — cannot see it, because the instance floats did not change. `render`
    /// compares this against the configuration's live count and re-packs the difference away.
    ///
    /// **It converges wherever the drawn grids' *live* glyph sets fit a region together**, which is
    /// the regime that matters: an eviction only happens once a region has been filled, and the
    /// re-pack marks this grid's glyphs most-recently-used, so the next eviction takes one of the
    /// dead slots instead. Where the live sets do **not** fit together, nothing can be right —
    /// ADR-0021 leaves that open, and the single-grid form of the same impossibility is refused
    /// outright rather than drawn (`ResolveError::FrameExceedsCapacity`).
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
/// leaving the drawing configuration's `&mut cache` free for [`resolve_frame`].
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
    /// (#773).
    ///
    /// **Every per-grid export starts here**, which is the whole of what S5 changed at this layer:
    /// the state was already per-grid (#769) and the draw loop already walked all of it (#771);
    /// what was missing was the consumer being able to say *which*. An unknown or removed id is a
    /// caller error arriving from JS, so it throws rather than silently addressing something.
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

    /// The configuration a grid's seven selectors ask for (#772).
    fn key_of(&self, at: usize) -> ConfigKey {
        let grid = self.grid_at(at);
        ConfigKey::new(
            &grid.font_family,
            grid.font_size,
            grid.font_weight,
            grid.font_weight_bold,
            grid.letter_spacing,
            grid.line_height,
            grid.subpixel,
        )
    }

    /// The entry serving `key`, joining an existing one or building a new one — and **only**
    /// building when nothing already serves it (#772 AC 4). The returned id carries one reference,
    /// which the caller owes to a grid or to a `release`.
    fn acquire_config(&mut self, key: ConfigKey) -> Result<ConfigId, JsValue> {
        if let Some(id) = self.configs.find(&key) {
            self.configs.retain(id);
            return Ok(id);
        }
        let baked = Self::bake_config(
            &self.global.gl,
            self.global.max_texture_size,
            &key,
            None,
            self.global.dpr,
        )?;
        self.bake_count = self.bake_count.wrapping_add(1);
        Ok(self.configs.insert(key, ConfigTier::fresh(baked)))
    }

    /// Drop one grid's reference to a configuration, deleting the atlas when the last grid leaves.
    fn release_config(&mut self, id: ConfigId) {
        if let Some(tier) = self.configs.release(id) {
            // Safety: live GL context. A texture that died with a lost context deletes as an error
            // flag with no state effect (measured, #770).
            unsafe { self.global.gl.delete_texture(tier.atlas) };
        }
    }

    /// Move the grid in slot `at` onto the configuration its selectors now ask for.
    ///
    /// **The shared entry is never edited to follow it.** Ghostty states the reason in one line —
    /// *"increasing the font size in one would increase it in all"* (`src/font/SharedGrid.zig:13-18`)
    /// — so a configuration change is a *move*: acquire the new entry, then release the old. Doing
    /// it in that order is what lets a grid re-select the same key without the entry being freed in
    /// between, and it is also the failure order: a build that fails leaves the grid exactly where
    /// it was, with nothing half-applied to roll back.
    ///
    /// The grid must then re-pack. Its packed instances address slots in the *old* entry's cache, and
    /// the new entry's are its own — ghostty ends `setFontGrid` with the same call for the same
    /// reason, *"cached rows may still reference an outdated atlas from the old grid and this can
    /// cause garbage to be rendered"* (`src/renderer/generic.zig:1112-1114`).
    ///
    /// **The instance count is dropped unconditionally, and the unconditional part is the point.**
    /// The retained-grid path (`apply_damage`, which is what `justerm-web` drives) re-packs inside
    /// the same `render`, so dropping it there is invisible — until the re-pack *fails*, which
    /// `render` deliberately survives rather than blanking the frame. Without this, that survival
    /// would draw the old entry's slot ids through the new entry's atlas: a wrong glyph rather than
    /// a stale one, which is the failure class this repo treats as sacred. The direct `apply_frame`
    /// path has no columns to re-pack from at all, so for it this is the whole repair. A grid that
    /// draws only its background until the consumer's next frame is honest; one that draws another
    /// configuration's glyphs is not.
    fn select_config(&mut self, at: usize, key: ConfigKey) -> Result<(), JsValue> {
        let old = self.grid_at(at).config;
        if *self.configs.key(old) == key {
            return Ok(());
        }
        let new = self.acquire_config(key)?;
        self.grids.grid_at_mut(at).config = new;
        self.release_config(old);
        let grid = self.grids.grid_at_mut(at);
        grid.needs_repack = true;
        grid.instance_count = 0;
        Ok(())
    }

    /// Register a terminal grid and return its id.
    ///
    /// The new grid is **registered but not drawn** — it holds its own per-grid state from this
    /// moment, and draws only once `setViewport` says where. That order is
    /// the consumer's, not a convenience: a widget's rect is a DOM measurement, and it has none
    /// until it is laid out.
    ///
    /// It costs **one** of the per-grid tier and nothing of the other two: one GPU instance buffer
    /// **and the VAO that points at it** ([ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md) D2 — no selector, not shareable, cheap to create;
    /// a VAO's whole content is *which* buffer feeds the draw, so it cannot be shared byte-for-byte
    /// and follows the buffer). No atlas, rasteriser, glyph cache, program or shared quad
    /// buffer — those stay one per context / per configuration.
    ///
    /// **The seven selectors are this grid's font, and they are optional and trailing**:
    /// `addGrid(palette, fg, bg)` takes the defaults (`"monospace"`, 16 CSS px, no letter spacing,
    /// line height 1, weights `"normal"` / `"bold"`, grayscale text), and any of the seven may be
    /// given instead. A weight takes what `setFontWeight` takes, and the last argument is what
    /// `setSubpixelAntialiasing` takes. They are what the
    /// grid's atlas is keyed by, so a grid whose selectors match a sibling's **joins that
    /// sibling's atlas and bakes nothing** — the whole economy of the middle tier, and the reason
    /// they belong here rather than in a setter called a line later: a grid born at the defaults and
    /// moved immediately would bake an atlas nobody asked for, once per registration.
    ///
    /// A non-finite selector, or a weight outside those values, is ignored in favour of the default,
    /// and size / line height are floored exactly as their setters floor them, so a grid cannot be
    /// born on a configuration `setFontSize` could not have produced.
    ///
    /// It is **not drawn** until `setViewport` places it, and until then it is
    /// not packed either: `render` skips a grid with no viewport before the pack, so feeding a hidden
    /// grid costs the scatter and nothing after it.
    // Three palette columns plus the seven font/metric selectors; the selectors are optional and
    // TRAILING, the `apply_frame` precedent, so `addGrid(palette, fg, bg)` still reads as a call.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = addGrid)]
    pub fn add_grid(
        &mut self,
        palette_colors: Vec<u32>,
        default_fg: u32,
        default_bg: u32,
        font_family: Option<String>,
        font_size: Option<f32>,
        letter_spacing: Option<f32>,
        line_height: Option<f32>,
        font_weight: Option<JsValue>,
        font_weight_bold: Option<JsValue>,
        subpixel: Option<bool>,
    ) -> Result<u32, JsValue> {
        let palette =
            Palette::from_colors(&palette_colors, default_fg, default_bg).map_err(|e| {
                JsValue::from_str(&format!(
                    "justerm-renderer: palette must be 256 colours, got {}",
                    e.got
                ))
            })?;
        // Safety: live GL context — and "live" is not checked here, deliberately.
        //
        // **Measured 2026-08-19, because the obvious assumption is false**: Chromium's
        // `createBuffer()` hands back a NON-null object on a lost context, both in the synchronous
        // window before `webglcontextlost` dispatches and after it, so `create_buffer` returns `Ok`
        // and a registration during a loss succeeds with a buffer that died with the context. (The
        // `Err` arm below is glow's `null` path, which this browser does not take.)
        //
        // **`createTexture` answers the same way — measured 2026-08-20 (#774)**, and that half was
        // load-bearing rather than symmetric. The `acquire_config` call below **bakes** on a cache
        // miss, with no liveness guard, so a grid asking mid-loss for a configuration nobody holds
        // creates a texture and rasterises the ASCII prebake into it. Had `createTexture` answered
        // `null`, `bake_config` would map it to `Err` and this function would **refuse** — exactly
        // the contract the paragraph below says would be wrong. It does not, so the contract holds.
        // What it costs instead is one thrown-away bake per such registration (measured: `bakes()`
        // +1 during the loss, and `restore` bakes that configuration again a moment later). The
        // grid recovers with correct pixels and nothing is left corrupt. Tracked rather than fixed
        // here, because the question it really raises is which liveness predicate a mid-life entry
        // point that performs GL work should ask, and ADR-0027's conformance map has no row for
        // this one.
        //
        // That is left alone rather than guarded, because refusing would be the wrong contract: a
        // consumer registering a terminal while the context happens to be dead wants the grid, and
        // `restore` gives **every** registered grid a fresh VAO and buffer and refills it — drawn
        // and not-drawn alike (#771 had to, since a stale per-grid VAO draws the *wrong* grid once
        // there is a draw loop). **Watched rather than reasoned since #774**:
        // `demo/context-loss-grids.html` registers *and feeds* a grid inside the loss window with
        // three siblings already on the registry, then places it after the restore and asserts it
        // draws its own ink rather than a neighbour's. See the map territory.
        // **A grid says which font it is born into, and that is what keeps the middle tier's
        // economy real** (#772 AC 4, #773). It joins rather than bakes whenever a sibling already
        // stands on the same configuration: six terminals in one font hold one atlas between them.
        //
        // Until S5 a new grid was born onto whichever configuration the implicit *default* grid
        // stood on, because it had no way to ask for its own. Taking the selectors here rather than
        // hardcoding the defaults is not a convenience on top of that: a grid born at the defaults
        // and moved a line later would **bake an atlas nobody asked for**, once per registration,
        // and free it again the moment the move released it — a bake per terminal, in the slice
        // whose whole point is one atlas per font.
        //
        // It also retires the mid-loss edge the inheritance carried: a grid registered while a
        // `setFontSize` was deferred used to be born at the configuration in force rather than the
        // one asked for, with no way back. It names its own font now, and `restore` reconciles it.
        let key = ConfigKey::new(
            font_family.as_deref().unwrap_or(DEFAULT_FONT_FAMILY),
            font_size
                .filter(|v| v.is_finite())
                .map_or(FONT_SIZE, |v| v.max(1.0)),
            font_weight
                .as_ref()
                .and_then(weight_from_js)
                .unwrap_or(FontWeight::NORMAL),
            font_weight_bold
                .as_ref()
                .and_then(weight_from_js)
                .unwrap_or(FontWeight::BOLD),
            letter_spacing.filter(|v| v.is_finite()).unwrap_or(0.0),
            line_height
                .filter(|v| v.is_finite())
                .map_or(1.0, |v| v.max(1.0)),
            subpixel.unwrap_or(false),
        );
        let config = self.acquire_config(key.clone())?;
        let buffers = match Self::build_grid_buffers(&self.global.gl, self.global.quad_vbo) {
            Ok(b) => b,
            // Hand back the reference just taken, or a failed registration would hold an atlas open
            // for the renderer's whole life.
            Err(e) => {
                self.release_config(config);
                return Err(e);
            }
        };
        let id = self.grids.register(GridTier::new(
            buffers,
            config,
            &key,
            palette,
            // No cells until this grid is sized. `cols`/`rows` answer 0 honestly rather than
            // inheriting a sibling's dimensions, which would be a size nobody asked for.
            (0, 0),
        ));
        Ok(id.raw())
    }

    /// Unregister a grid and release the GPU buffer it owned.
    ///
    /// This is the *session-close* operation, not the hide one: hiding is
    /// `clearViewport`, which keeps every byte resident so coming back is
    /// a placement rather than a rebuild. Removing and re-adding a grid to hide it would
    /// reintroduce exactly the re-attach cost the shared surface exists to remove.
    ///
    /// Errors on an unknown id. **Every** grid is removable, the first one included:
    /// there is no longer a grid whose lifetime someone other than the consumer owns.
    #[wasm_bindgen(js_name = removeGrid)]
    pub fn remove_grid(&mut self, grid: u32) -> Result<(), JsValue> {
        let removed = self
            .grids
            .remove(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))?;
        // Safety: live GL context. Deleting an object that died with a lost context raises
        // `INVALID_OPERATION` and changes nothing (measured, #770) — an error flag, not a no-op.
        unsafe {
            self.global.gl.delete_vertex_array(removed.vao);
            self.global.gl.delete_buffer(removed.instance_vbo);
        }
        // …and give up its share of the configuration. The atlas goes only if this was the last
        // grid standing on it (#772) — closing one of six terminals in one font frees a buffer and
        // a VAO, not the font machinery the other five are still drawing through.
        self.release_config(removed.config);
        Ok(())
    }

    /// Place a grid on the shared drawing buffer, in **device pixels**, top-left origin.
    ///
    /// A placed grid is a drawn grid — the state the draw loop reads. The GL flip to a
    /// bottom-origin y belongs to the site that issues `gl.viewport`, not here: this is the rect
    /// the consumer measured, stored as measured.
    ///
    /// Errors on an unknown id and on a rect with no area. It errors on **no grid in particular**:
    /// every rect has one producer — the consumer's measured box — so there is no grid
    /// whose placement someone else owns.
    #[wasm_bindgen(js_name = setViewport)]
    pub fn set_viewport(
        &mut self,
        grid: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), JsValue> {
        // A viewport with no area draws no pixels, so accepting one and then answering `true` to
        // `isGridDrawn` would be a lie about what this renderer does. The state for "this grid has
        // no rect yet" already exists and is called `clearViewport` — which is the honest answer
        // for the case that produces a zero rect in the first place, a consumer measuring a DOM box
        // that is still `display:none`.
        if width <= 0 || height <= 0 {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: a viewport must have area, got {width}x{height}"
            )));
        }
        self.grids
            .set_viewport(
                GridId::from_raw(grid),
                Viewport {
                    x,
                    y,
                    width,
                    height,
                },
            )
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// Stop drawing a grid **without unregistering it** — the hidden-workspace state.
    ///
    /// Every byte of the grid's state survives: its packed instances, its upload baseline, its
    /// palette, its cursor and overlays. Nothing is re-baked when it comes back, because nothing
    /// was released; the consumer re-supplies the rect, which it has to anyway — a hidden widget's
    /// DOM box reads back as zero, so a rect retained across the hide would be a copy that can be
    /// wrong on the way back.
    ///
    /// Every grid is hideable, the first one included — a rect has one producer now, the
    /// consumer's measured box, so there is no grid that would go on painting after being hidden.
    #[wasm_bindgen(js_name = clearViewport)]
    pub fn clear_viewport(&mut self, grid: u32) -> Result<(), JsValue> {
        self.grids
            .clear_viewport(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// How many grids are registered, drawn or not. Zero on a fresh renderer.
    ///
    /// Registry *state*, not a diagnostic counter: it answers what this renderer holds, which the
    /// consumer put there. ([ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md) D5 leaves where diagnostics like `packs` live to whoever adds
    /// the second one; this is not that question.)
    #[wasm_bindgen(js_name = gridCount)]
    pub fn grid_count(&self) -> usize {
        self.grids.len()
    }

    /// How many distinct font configurations this renderer holds resources for — i.e. how many
    /// glyph atlases exist.
    ///
    /// This is what makes sharing **observable** rather than asserted: six terminals in one font
    /// answer `1`, and a seventh that changes its font answers `2`. Ghostty exposes the same number
    /// for the same reason (`SharedGridSet.count`). Registry *state*, like
    /// `gridCount` — not a diagnostic counter.
    #[wasm_bindgen(js_name = atlasCount)]
    pub fn atlas_count(&self) -> usize {
        self.configs.len()
    }

    /// Number of atlas bakes run so far (a diagnostic) — every configuration built from nothing,
    /// plus every in-place rebuild of one (a DPR change, a context restore).
    ///
    /// The consumer/proofs read the **delta** across an operation, as they do with
    /// `packs`: a grid *joining* an existing configuration must move this by zero,
    /// which is the claim the middle tier exists to make and the one a memory figure cannot settle.
    ///
    /// It counts **committed** bakes. A rebuild that fails part-way discards every replacement it
    /// built and leaves this where it was, so the number tracks configurations this renderer is
    /// drawing through rather than rasterising work it performed — which is what a delta is read
    /// for, and what keeps the delta deterministic. Not a stable API surface; a counter for
    /// verification. Wraps harmlessly.
    #[wasm_bindgen(js_name = bakes)]
    pub fn bakes(&self) -> u32 {
        self.bake_count
    }

    /// Whether a grid currently has a viewport, i.e. whether it draws. Errors on an unknown
    /// id — the same answer `setViewport` gives, so a stale handle cannot read as "not drawn".
    #[wasm_bindgen(js_name = isGridDrawn)]
    pub fn is_grid_drawn(&self, grid: u32) -> Result<bool, JsValue> {
        self.grids
            .is_drawn(GridId::from_raw(grid))
            .map_err(|e| JsValue::from_str(&e.message()))
    }

    /// Consume a decoded **damage** frame directly (the damage adapter): scatter its span-ordered
    /// cells into the persistent grid, then resolve + pack the full viewport. A Full frame wipes
    /// the grid first, a scroll op shifts it before spans — so a Partial frame (the common case)
    /// no longer misaligns as dense row-major. Grapheme clusters ride the `extra` column
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
        grid: u32,
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
        let at = self.slot(grid)?;
        self.grid_at_mut(at).apply_damage(
            header,
            spans,
            codepoints,
            fg,
            bg,
            flags,
            extra,
            side_table,
            underline_colors,
        )
    }

    /// The in-progress IME composition to draw, anchored at `(col, row)` — the cursor cell the
    /// consumer's current frame reported. `codepoints` is the preedit as the OS reports it
    /// (`compositionupdate.data`); an empty array clears it.
    ///
    /// **This is the one piece of renderer state with no representation anywhere in the engine**
    /// ([ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md)): a composition is browser-owned, reaches no frame and no wire, so the consumer is
    /// the only possible source and must re-push on every `compositionupdate`. Skipping an update
    /// whose data is unchanged is worth doing — a real IME emits one settling update per syllable
    /// where nothing moved (measured).
    ///
    /// The run may extend past the anchor's row end: it shifts left to stay whole rather than
    /// clipping (`preedit::range` — crate-private, so no link from this page). Width is per codepoint, the same
    /// `unicode-width` answer the engine gives, so a VS16 emoji measures narrow here exactly as it
    /// does there.
    /// Returns the column the **caret and the IME anchor** belong at: one past the run's last cell,
    /// clamped to the grid. The consumer cannot compute this — it has no `wcwidth` — and it is not
    /// simply `col + len`, because the run shifts left at the right edge. Feeding it back to
    /// `setCursor` is [ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md) D5's position rule (the caret rides the composition's end, while
    /// DECTCEM still decides whether it is drawn at all), and feeding it to the hidden textarea is
    /// D4's voluntary writer.
    #[wasm_bindgen(js_name = setPreedit)]
    pub fn set_preedit(
        &mut self,
        grid: u32,
        col: u32,
        row: u32,
        codepoints: Vec<u32>,
    ) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.grid_at_mut(at).set_preedit(col, row, codepoints))
    }

    /// Text of the consumer's own to draw after the cursor — an autosuggestion at a shell prompt —
    /// anchored at `(col, row)`, a **viewport** cell. `codepoints` is the text; an empty array
    /// clears it. `fg` is a tagged colour reference in the frame's own encoding (high byte `0`
    /// Default, `1` Indexed, `2` Rgb; low 24 bits the payload), so a Default or Indexed colour
    /// follows a theme change; `dim` draws it with the SGR 2 treatment.
    ///
    /// This is renderer state only: no engine cell changes, so it never reaches copy or search.
    /// The run keeps its head and stops at the first cell that is not blank, at the right edge,
    /// and before a wide character that does not fit. A cell under the selection, a search match,
    /// a decoration or the hovered link keeps the engine's cell. Each written cell keeps its own
    /// background. Nothing is drawn while a preedit run is (see `setPreedit`), and the caret is not
    /// moved.
    ///
    /// Three things are the caller's, because nothing this renderer is given can see them: the
    /// anchor (re-send it when the cursor moves), the alternate screen (clear the suggestion on entering
    /// it; the frame this renderer receives carries no alt flag), and a composition that has started
    /// but has no preedit run drawn yet (clear it from `compositionstart`). Design and
    /// the reasons behind each rule: [justerm#972](https://github.com/kihyun1998/justerm/issues/972).
    #[wasm_bindgen(js_name = setSuggestion)]
    pub fn set_suggestion(
        &mut self,
        grid: u32,
        col: u32,
        row: u32,
        codepoints: Vec<u32>,
        fg: u32,
        dim: bool,
    ) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at)
            .set_suggestion(col, row, codepoints, fg, dim);
        Ok(())
    }

    /// Swap the palette + default fg/bg for a **live theme change** — the renderer-side of a
    /// theme picker or a runtime scheme swap, so a consumer need not tear down and rebuild the
    /// renderer to recolour. `palette_colors` is the 256 pre-built indexed colours (as the
    /// constructor takes); `default_fg`/`default_bg` the theme's defaults. Consumer policy
    /// ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)): the palette *values* are the consumer's (theme-agnostic core), the *mechanism*
    /// (re-resolve every retained cell against the new palette) is the renderer's.
    ///
    /// Marks the buffer dirty so the next `render` re-packs and the change
    /// shows with no new frame — like `setOverlay` (a no-op until the first `apply_damage`; the
    /// direct `apply_frame` path reflects the new palette on its next call). The re-pack is all that
    /// is needed: it re-resolves every cell's colour against the new palette, and the render's clear
    /// reads `default_bg` fresh. Translucency no longer needs a uniform re-push here:
    /// its trigger is the packer's per-cell `bg_default` provenance flag, which is palette-independent.
    ///
    /// `setOverlay`: Self::set_overlay
    #[wasm_bindgen(js_name = setPalette)]
    pub fn set_palette(
        &mut self,
        grid: u32,
        palette_colors: Vec<u32>,
        default_fg: u32,
        default_bg: u32,
    ) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at)
            .set_palette(palette_colors, default_fg, default_bg)
    }

    /// Set the selection / search highlight overlay: the two span directories (stride-3
    /// `(row, left, right)` viewport triples, exactly as `justerm-wasm-decode` `selectionSpans` /
    /// `matchSpans` ship them) plus their blend colours (packed `0xRRGGBB`, consumer policy —
    /// the renderer is theme-agnostic). A covered cell blends the colour over a non-default / inverse
    /// background so its own colour shows through, or paints it solid over the default background; a
    /// selection wins over a match on a cell both cover.
    ///
    /// Marks the buffer dirty so the next `render` re-packs — a selection
    /// dragged with no new frame shows because the consumer renders after. Possible only on the
    /// damage path, which retains the dense grid; the direct `apply_frame` path reflects the new
    /// overlay on its next call. Pass empty span lists to clear the highlight.
    ///
    /// **Contract — the spans are consumer-pushed, not frame-carried (same as `setCursor`).** They
    /// are viewport-relative and the decoder RE-PROJECTS them every frame, so a scroll or resize moves
    /// them: the consumer must re-issue `set_overlay` with the current frame's spans whenever the
    /// viewport changes *or* the selection changes — exactly as it re-issues `set_cursor`. Stale spans
    /// do not panic (an out-of-range span simply highlights nothing), but an in-range stale span
    /// highlights the wrong cells until the next call. Unlike beamterm, whose spans ride each decoded
    /// frame, this renderer cannot self-refresh — the split mirrors the cursor's, and the widget wires both.
    ///
    /// `setCursor`: Self::set_cursor
    #[wasm_bindgen(js_name = setOverlay)]
    pub fn set_overlay(
        &mut self,
        grid: u32,
        selection_spans: Vec<u32>,
        match_spans: Vec<u32>,
        selection_bg: u32,
        match_bg: u32,
    ) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at)
            .set_overlay(selection_spans, match_spans, selection_bg, match_bg)
    }

    /// Set the *active* (focused/current) search-match spans + their background — the xterm
    /// `activeMatchBackground` decoration, ranked **above the selection** (`highlight_at`). Additive
    /// beside `setOverlay`: the consumer pushes the current search result here
    /// as the search box navigates (`next`/`prev`), independent of the selection, so a user text
    /// selection and the current match coexist. The active match is *also* pushed in `set_overlay`'s
    /// `match_spans`; the ranking, not exclusion, makes the active colour win. Same viewport-relative,
    /// re-issue-every-frame contract as `setOverlay`. Empty spans clear the active match.
    #[wasm_bindgen(js_name = setActiveMatch)]
    pub fn set_active_match(
        &mut self,
        grid: u32,
        active_spans: Vec<u32>,
        active_match_bg: u32,
    ) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at)
            .set_active_match(active_spans, active_match_bg);
        Ok(())
    }

    /// Set the hovered link's spans: stride-3 `(row, left, right)` viewport triples, the
    /// cells drawn underlined in the cell's own line colour. A span covering either half of a wide
    /// pair covers both. Kept until the next call; empty spans clear it.
    #[wasm_bindgen(js_name = setLinkHover)]
    pub fn set_link_hover(&mut self, grid: u32, spans: Vec<u32>) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let g = self.grid_at_mut(at);
        g.link_hover_spans = spans;
        g.needs_repack = true;
        Ok(())
    }

    /// Set the marker-anchored decorations for this frame. `spans` is the flat
    /// `DECORATION_STRIDE` (`row, left, right, layer, bg, fg`) directory the consumer projects
    /// from its `DecorationRegistry` + core's markers — `layer` `0` = bottom (under the highlight) /
    /// `1` = top (over it), `bg`/`fg` **absolute** packed `0xRRGGBB` used **verbatim** (the consumer
    /// resolved its theme before pushing — unlike a *cell* colour, which arrives as a theme-agnostic
    /// ref for the renderer to resolve), or the wire's `NO_REF` sentinel for "no override". Pass an
    /// empty array to clear. Consumer-projected (the model is the consumer's; the renderer only
    /// composites, [ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)). Marks the buffer dirty; the next `render`
    /// re-packs.
    #[wasm_bindgen(js_name = setDecorations)]
    pub fn set_decorations(&mut self, grid: u32, spans: Vec<u32>) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_decorations(spans)
    }

    /// Place the cursor. `shape`: `0` block, `1` underline, `2` bar, `3` hollow block.
    /// `color` is the cursor's own `0xRRGGBB`; `text_color` the glyph colour a block paints under
    /// itself (xterm's `cursorAccent`, alacritty's `text_color`). Colours are resolved by the
    /// consumer — the renderer stays theme-agnostic.
    ///
    /// **Every shape, the block included, lives in the cursor uniform** (`u_cursor` + its colours,
    /// uniforms of the fragment shader) and is resolved per fragment. So any cursor change —
    /// move, blink, shape — takes effect on the next `render` alone: one uniform,
    /// no re-pack and no instance upload. Blink phase is the consumer's policy, exactly as
    /// `blink_on` is — call `clearCursor` for the off phase.
    ///
    /// A block *could* have been an instance: it is a colour override on the cell, not geometry,
    /// and both references draw it that way. It is not one because [ADR-0018](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0018-justerm-renderer.md)
    /// ([`docs/adr/0018-justerm-renderer.md`](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0018-justerm-renderer.md)) makes that **the contract, not an
    /// optimisation** — a blink tick produces no terminal output, so a block packed into the
    /// instances could not blink off without the consumer re-feeding the frame (an early draft
    /// did exactly that). Two consequences follow rather than cause it: un-painting would need a
    /// re-pack, and per-fragment resolution keeps the ordering free, since the instance colours
    /// arrive already inverse-swapped and the glyph already concealed.
    #[wasm_bindgen(js_name = setCursor)]
    pub fn set_cursor(
        &mut self,
        grid: u32,
        col: u32,
        row: u32,
        shape: u8,
        color: u32,
        text_color: u32,
    ) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at)
            .set_cursor(col, row, shape, color, text_color)
    }

    /// Remove the cursor — hidden (`DECTCEM`), or the blink's off phase.
    #[wasm_bindgen(js_name = clearCursor)]
    pub fn clear_cursor(&mut self, grid: u32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).clear_cursor();
        Ok(())
    }

    /// Re-resolve [`GridTier::cursor_cells`] against the last frame's flags. Called when a frame arrives
    /// (its flags may have changed under a still cursor) *and* when the cursor moves (onto either
    /// half of a wide char, with no new frame).
    fn resolve_cursor_cells(&mut self, at: usize) {
        self.grid_at_mut(at).resolve_cursor_cells()
    }

    /// The number of columns this grid was last sized to by `resizeGrid` —
    /// exactly that, and nothing else reads it.
    ///
    /// **It is an echo, and it has not always been one.** While the renderer sized the drawing buffer
    /// from the grid it could refuse one it could not draw, so this reported the grid actually
    /// adopted and a clamp was visible here. The buffer belongs to the *surface* now — N
    /// grids in M cell sizes share it — so `resizeSurface` adopts what the
    /// browser granted and a consumer that asked for more than fits learns it from
    /// `cssWidth`, never from this.
    ///
    /// A consumer that keeps sending frames of a grid larger than its rect does not corrupt
    /// anything — every per-cell read is bounds-checked and the surplus cells are clipped by the
    /// grid's own scissor — but its mouse mapping and reflow will be wrong. The grid is the
    /// consumer's to compute from its own box ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)), as xterm's `FitAddon` computes it.
    #[wasm_bindgen(js_name = cols)]
    pub fn cols(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.grid_at(at).cols())
    }

    /// The number of rows this grid was last sized to by `resizeGrid` — see
    /// `cols`.
    #[wasm_bindgen(js_name = rows)]
    pub fn rows(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.grid_at(at).rows())
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
    fn preedit_span(&self, at: usize, cols: u32, rows: u32) -> Option<PreeditSpan> {
        self.grid_at(at).preedit_span(cols, rows)
    }

    fn preedit_patch(
        &self,
        at: usize,
        cells: &Cells,
        bg: &[u32],
        fg: &[u32],
    ) -> Option<PreeditPatch<'static>> {
        self.grid_at(at).preedit_patch(cells, bg, fg)
    }

    /// Set the background cell opacity: `0` = fully transparent, `1` = opaque (default). The
    /// consumer injects this policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) to make the terminal background see-through to the
    /// page/desktop behind the canvas, while glyph pixels stay opaque. Clamped to `[0, 1]`; takes
    /// effect on the next `render`.
    ///
    /// **A translucent background contributes to a cell's colour in proportion to how translucent
    /// it is** (fixed 2026-08-18). It used to contribute in full: an antialiased glyph
    /// edge mixed toward the background colour with the coverage as its weight while the alpha
    /// said that background was only `alpha` present, so at `0` a half-covered pixel came out half
    /// background — a colour the caller had asked to be absent. At `1` nothing changed and nothing
    /// changes now; the two agree exactly there, which is why this shipped unnoticed.
    ///
    /// **A non-finite value falls back to `1.0` (opaque), like every other float setter here.**
    /// `f32::clamp` compares with `<` / `>`, both false for `NaN`, so a bare clamp *passes NaN
    /// through* — and this was the only float setter on this type without the guard. The
    /// consequence was not a wrong background: a `NaN` here reaches every fragment's alpha, so
    /// glyph pixels go transparent too and the whole terminal disappears with no error anywhere.
    /// Measured, not reasoned: booting the widget at `NaN` read `[30,30,46,0]` on a background
    /// cell **and `[205,214,244,0]` inside a glyph**, against `[…,128]` / `[…,255]` for a valid
    /// `0.5`. (The measurement stands; the expression it was taken against was
    /// `mix(u_bg_alpha, 1.0, cov)`, which the coverage-weighted form replaced — the `NaN` now reaches the colour
    /// through the same uniform as well, so the failure is if anything less subtle.)
    ///
    /// Reachable from type-correct consumer code, which is why the guard is here and not at the
    /// caller: TypeScript's `number` includes `NaN`, so an ordinary `Number(configValue)` arrives
    /// as one. Finite out-of-range values were never the problem — the clamp already handles them
    /// (`-3` and `9` measured as fully transparent and fully opaque respectively).
    #[wasm_bindgen(js_name = setBgAlpha)]
    pub fn set_bg_alpha(&mut self, grid: u32, alpha: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_bg_alpha(alpha);
        Ok(())
    }

    /// Draw bold text in the bright (8–15) ANSI colour — xterm's
    /// `drawBoldTextInBrightColors`. A bold `Indexed(0..=7)` foreground resolves to its `8..=15`
    /// bright variant; `Rgb`/`Indexed(8..=255)` foregrounds and non-bold cells are unaffected. On by
    /// default (xterm's default). Consumer policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)): the mechanism (index remap at resolve)
    /// is the renderer's, the on/off is the consumer's. Marks the buffer dirty; the next
    /// `render` re-packs, so a live toggle shows without a new frame.
    #[wasm_bindgen(js_name = setBoldToBright)]
    pub fn set_bold_to_bright(&mut self, grid: u32, enabled: bool) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_bold_to_bright(enabled)
    }

    /// Set (or clear) the selection foreground override — xterm's `selectionForeground`.
    /// A packed `0xRRGGBB` forces the fg of every **selected** cell (never a search match) to that
    /// colour; it still flows through the minimum-contrast pass. Pass `undefined` to clear it and keep
    /// each cell's own fg (the default). Consumer policy, focus-independent. Selection is a
    /// property of the cell, not of the bg winner: on a selected cell inside the ACTIVE search
    /// match this fg paints over the *active-match* background — pick the two colours to read on each
    /// other, or set `setMinimumContrastRatio` (it corrects
    /// against the final composited bg). Marks the buffer dirty; the next `render`
    /// re-packs.
    #[wasm_bindgen(js_name = setSelectionForeground)]
    pub fn set_selection_foreground(
        &mut self,
        grid: u32,
        color: Option<u32>,
    ) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_selection_foreground(color)
    }

    /// Set the minimum WCAG fg/bg contrast ratio — xterm's `minimumContrastRatio`. Below
    /// it, a cell's foreground is nudged lighter or darker (in 10% luminance steps, away from the bg)
    /// until it meets the ratio, against the colour it is actually drawn over (post-highlight). A DIM
    /// cell uses half the ratio, so it stays visibly dim rather than being corrected to full contrast.
    /// Consumer policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)): the mechanism (the WCAG adjustment on the resolved RGB) is the
    /// renderer's, the number is the consumer's. Default `1.0` = off (xterm's default). Clamped to
    /// `[1, 21]`; marks the buffer dirty so the next `render` re-packs and a
    /// live change shows.
    #[wasm_bindgen(js_name = setMinimumContrastRatio)]
    pub fn set_minimum_contrast_ratio(&mut self, grid: u32, ratio: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_minimum_contrast_ratio(ratio)
    }

    /// Set the minimum WCAG contrast a cursor must have with the cell it sits on. Below it,
    /// the cursor inverts to the terminal's default fg/bg so it never vanishes into a same-coloured
    /// cell. The mechanism is the renderer's — only it has the *resolved* per-cell RGB ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) —
    /// but the number is the consumer's policy. Default `1.5` (alacritty's `MIN_CURSOR_CONTRAST`);
    /// pass `1.0`, the floor of the contrast range, to disable the guard (xterm's behaviour). Clamped
    /// to `[1, 21]`; takes effect on the next `render`.
    #[wasm_bindgen(js_name = setCursorContrast)]
    pub fn set_cursor_contrast(&mut self, grid: u32, threshold: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_cursor_contrast(threshold);
        Ok(())
    }

    /// Set the cursor stroke thickness as a fraction of the cell width — the width of a
    /// bar, an underline, or a hollow block's outline. `cursor_thickness` turns it into device
    /// pixels as `(frac * cell_w).round().max(1)`, so it tracks both dpr and font size — alacritty's
    /// rule (`display/cursor.rs:25`), chosen over xterm's `dpr * cursorWidth` (that gives a
    /// 32px font the same hairline as a 12px one). This adds only the configurability the mechanism
    /// already had. A **block** ignores it — a block recolours its cell and draws no stroke.
    ///
    /// Default `0.15` (alacritty's `cursor.thickness`); clamped to `[0, 1]` (alacritty's
    /// `Percentage`). The clamp is load-bearing, not hygiene: `cursor_thickness` computes
    /// `(frac * cell_w).round() as u32`, and an unclamped `f32::INFINITY` saturates that cast to
    /// `u32::MAX` device pixels. `NaN` is caught a layer deeper — `frac.max(0.0)` returns `0.0` for
    /// it (`f32::max` yields the non-NaN operand) — so the floor below still gives it a 1px stroke.
    /// The mechanism's `.max(1)` floor also means even `0` leaves a one-pixel stroke rather than an
    /// invisible cursor. Takes effect on the next `render` — like a stroke's shape,
    /// it is a shader uniform, so changing it costs no upload.
    #[wasm_bindgen(js_name = setCursorThickness)]
    pub fn set_cursor_thickness(&mut self, grid: u32, frac: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_cursor_thickness(frac);
        Ok(())
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
        // at a task boundary and everything from here to the final `apply_surface_size` is
        // synchronous. The
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
            // Both registries start **empty** (#773): a renderer holds no terminal until the
            // consumer registers one, and therefore no font configuration either — there would be
            // nothing to key an atlas by, and baking one against the chance that a grid arrives is
            // a bake charged to nobody.
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

    /// One grid's instance buffer and the VAO that points at it (#771).
    ///
    /// The attribute *layout* is the same for every grid — there is one program, so the instance
    /// format is the union of what any terminal needs and a per-grid layout is not reachable
    /// (#287). What differs per grid is which buffer feeds it, and that is exactly what a VAO
    /// records.
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
            // Byte offsets are derived from `frame`'s named float offsets, not written out: this
            // table and the packer are two statements of one layout, and a literal here is how they
            // come apart when a field is appended (#791).
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
    /// Build one configuration's resources: rasteriser, cell geometry, atlas texture, and the
    /// glyphs baked into it (#772). Nothing here touches a live field, so every caller commits it
    /// atomically or throws it away.
    ///
    /// `resident` decides what gets baked, and that is the only difference between the two callers:
    /// `None` builds a **new** configuration and primes it with the 95 ASCII fast-path glyphs;
    /// `Some(cache)` rebuilds an **existing** one and re-bakes every resident glyph into the SAME
    /// slot it already occupies, so the instances that address them survive the rebuild.
    ///
    /// An associated function rather than a method because the constructor has no `self` yet, and
    /// because it must not be able to read a live tier by accident.
    fn bake_config(
        gl: &glow::Context,
        max_texture_size: u32,
        key: &ConfigKey,
        resident: Option<&GlyphCache>,
        dpr: f32,
    ) -> Result<BakedConfig, JsValue> {
        let mut rasterizer = Rasterizer::new(
            key.font_family(),
            key.font_size() * dpr,
            key.font_weight(),
            key.font_weight_bold(),
            key.subpixel(),
        )?;
        // The rasteriser measures the GLYPH box; the grid cell is that box plus the consumer's
        // spacing policy (#338). The atlas slot is the padded CELL (#359), so the rasteriser must
        // know the cell before anything is sized from `padded_size()`. And ask the implementation
        // rather than predicting it: a cell the atlas texture cannot hold leaves that texture
        // storage-less, and a storage-less sampler answers alpha 1 for every glyph (#339/#359).
        let char_size = rasterizer.glyph_box();
        // The bands this face needs, measured before the cell is sized because they are spent out of
        // the same texture budget (#791 the per-layer height, #966 the width).
        let bleed = (rasterizer.bleed_x(), rasterizer.bleed_y());
        let cell_size = fit_cell_to_atlas(
            device_cell(char_size, key.letter_spacing(), key.line_height(), dpr),
            PADDING,
            bleed,
            GLYPHS_PER_LAYER as u32,
            max_texture_size,
        );
        let char_offset = glyph_offset(cell_size, char_size);
        rasterizer.set_cell(cell_size, char_offset, bleed)?;
        let atlas_cell = rasterizer.padded_size();
        let atlas = Self::build_atlas(gl, atlas_cell.0, atlas_cell.1)?;
        let baked = match resident {
            Some(cache) => Self::bake_all_glyphs(gl, cache, &rasterizer, atlas, atlas_cell),
            None => Self::prebake_ascii_into(gl, &rasterizer, atlas, atlas_cell),
        };
        if let Err(e) = baked {
            unsafe { gl.delete_texture(atlas) }; // don't leak the half-built atlas
            return Err(e);
        }
        Ok(BakedConfig {
            atlas,
            rasterizer,
            cell_size,
            char_size,
            char_offset,
            atlas_cell,
        })
    }

    /// Bake the 95 normal ASCII glyphs into `atlas` using `rasterizer`.
    fn prebake_ascii_into(
        gl: &glow::Context,
        rasterizer: &Rasterizer,
        atlas: glow::Texture,
        atlas_cell: (u32, u32),
    ) -> Result<(), JsValue> {
        for cp in 0x20u32..=0x7E {
            let ch = char::from_u32(cp).unwrap();
            let text = ch.to_string();
            let rgba = rasterizer.rasterize(&text, FontStyle::Normal, false)?;
            let rgba = rasterizer.finish(rgba, &text, FontStyle::Normal, false, false)?;
            upload_glyph(gl, atlas, atlas_cell, (cp - 0x20) as u16, &rgba);
        }
        Ok(())
    }

    /// Bake one configuration's ENTIRE glyph set — the 95 prebaked ASCII plus every glyph resident
    /// in `cache` — into `atlas`, each into the SAME slot it already occupies. Preserving the slots
    /// is what lets the packed `instances` stay valid across the re-bake (no re-pack, no
    /// re-resolve). Shared by the DPR re-bake (#322) and the context-loss restore (#269), which
    /// both need "a fresh, correctly-sized atlas holding what the old one held".
    fn bake_all_glyphs(
        gl: &glow::Context,
        cache: &GlyphCache,
        rasterizer: &Rasterizer,
        atlas: glow::Texture,
        atlas_cell: (u32, u32),
    ) -> Result<(), JsValue> {
        let (pad_w, pad_h) = atlas_cell;
        Self::prebake_ascii_into(gl, rasterizer, atlas, atlas_cell)?;
        for (k, slot) in cache.entries() {
            // Two-cell iff the slot lives in the wide region — true for both `Wide` (CJK) and a
            // *wide* `Emoji` (2-cell colour emoji); a narrow `Emoji` (#297 EmojiNarrow) sits in
            // the normal region and is one cell. Keying off `slot_id() >= WIDE_BASE` (not
            // `matches!(Wide)`) is what catches the wide-emoji case.
            let wide = slot.slot_id() >= WIDE_BASE;
            let rgba = rasterizer.rasterize(&k.text, k.style, wide)?;
            let colour = matches!(slot, GlyphSlot::Emoji(_));
            let rgba = rasterizer.finish(rgba, &k.text, k.style, wide, colour)?;
            let base = slot.slot_id();
            if wide {
                let m = rasterizer.margin_x();
                let (left, right) = split_wide_bitmap(&rgba, 2 * pad_w - 2 * m, pad_w, pad_h, m);
                upload_glyph(gl, atlas, atlas_cell, base, &left);
                upload_glyph(gl, atlas, atlas_cell, base + 1, &right);
            } else {
                upload_glyph(gl, atlas, atlas_cell, base, &rgba);
            }
        }
        Ok(())
    }

    /// Notify the renderer that `window.devicePixelRatio` changed to `dpr`. The consumer
    /// drives this from a resolution `matchMedia` listener — a DPR change at the *same* CSS size
    /// (dragging to another-density monitor) does not fire a resize, so it must be signalled
    /// explicitly. **Every** configuration is re-baked at the new device size, each keeping its own
    /// glyph slots (so nothing has to re-pack). **The drawing buffer is deliberately left alone** —
    /// see the paragraph below the next one, which is where that decision is stated; this sentence
    /// said the opposite until 2026-08-19, describing the pre-0.15.0 behaviour
    /// fifteen lines above the paragraph that retired it. A no-op if the ratio is unchanged; on
    /// error every old atlas is left intact and `dpr` unadvanced, so the next notification retries
    /// (self-healing).
    ///
    /// **This is the "rebuild all of them" path, not the "re-key one" path** ([ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md)). One
    /// canvas means one drawing buffer and one DPR, so a density change is true of every entry at
    /// once — which is exactly the case where mutating a shared entry in place is right rather than
    /// wrong: nobody is being moved into a configuration they did not ask for.
    #[wasm_bindgen(js_name = setDevicePixelRatio)]
    pub fn set_device_pixel_ratio(&mut self, dpr: f32) -> Result<(), JsValue> {
        if !dpr_changed(self.global.dpr, dpr) {
            return Ok(());
        }
        // A lost context can only hand back invalidated atlas textures, so re-baking now would
        // burn the work and commit empty atlases. Drop the notification: `restore` re-reads the
        // *live* DPR and bakes at that density anyway (#269).
        if self.gpu_work_must_wait() {
            return Ok(());
        }
        self.rebuild_all_configs(dpr)?;
        self.global.dpr = dpr;
        // **The drawing buffer is deliberately left alone** (#773). Until S5 this re-derived it from
        // the implicit grid's `cols`/`rows` — a density-independent quantity that stayed meaningful
        // across the change. A surface has no such quantity: it is the consumer's device-px
        // measurement, and re-deriving it would mean converting through our copy of the density,
        // which is stale in exactly this window. So the buffer holds and the consumer re-issues it,
        // as it re-issues every viewport rect — which it must anyway, the cell having just moved.
        Ok(())
    }

    /// Re-bake **every** live configuration at `dpr`, each keeping its own glyph slots (#772).
    ///
    /// Atomic across the whole registry: every replacement is built before any is committed, so a
    /// failure part-way leaves every entry exactly as it was and the caller retries. That matters
    /// more here than it did with one entry — a half-applied density would leave two grids drawing
    /// through atlases baked at different densities with one shared `dpr` describing both.
    ///
    /// Does **not** advance `self.global.dpr`; the caller commits that once this returns.
    fn rebuild_all_configs(&mut self, dpr: f32) -> Result<(), JsValue> {
        let ids = self.configs.ids();
        let mut baked = Vec::with_capacity(ids.len());
        for &id in &ids {
            let key = self.configs.key(id).clone();
            let built = Self::bake_config(
                &self.global.gl,
                self.global.max_texture_size,
                &key,
                Some(&self.configs.get(id).cache),
                dpr,
            );
            match built {
                Ok(b) => baked.push(b),
                Err(e) => {
                    // Safety: live GL context; these textures are this function's own and unpublished.
                    unsafe {
                        for b in &baked {
                            self.global.gl.delete_texture(b.atlas);
                        }
                    }
                    return Err(e);
                }
            }
        }
        for (id, b) in ids.into_iter().zip(baked) {
            let old = self.configs.get_mut(id).adopt(b);
            // Safety: live GL context; the outgoing texture is no longer referenced.
            unsafe { self.global.gl.delete_texture(old) };
            self.bake_count = self.bake_count.wrapping_add(1);
        }
        Ok(())
    }

    /// Write one grid's font/metric selectors through `edit` and move that grid onto the
    /// configuration they now name (#772, per-grid since #773).
    ///
    /// It does **not** touch the drawing buffer, which it did until #773: the buffer is the
    /// surface's, and a surface drawing N grids belongs to none of them. The consumer re-fits after
    /// any selector that moves the cell — every one but the two weights and the subpixel setting,
    /// which do not.
    ///
    /// The single site every one of the seven setters goes through, so none of them can decide any of
    /// this differently. Three properties it owes, and each one is a bug that has been paid for
    /// here before:
    ///
    /// - **Atomic.** A rasterise can fail (it draws through the browser's 2D engine), and a
    ///   half-applied change is worse than a rejected one. On failure the selectors are put back, so
    ///   the grid and the entry it names still agree and the consumer can retry (#338/#359).
    /// - **Deferred while the context is dead.** An atlas built on a lost context comes back
    ///   invalidated. The selectors still advance — they are CPU state and outlive the loss — and
    ///   [`restore`](Self::restore) re-selects from them, which is where a mid-loss `setFontSize`
    ///   has always landed (#269/#406). What changed at #772 is that the *cell* no longer moves
    ///   ahead of the atlas either: the cell belongs to a configuration, and no configuration moved.
    /// - **It never edits the entry it is leaving.** That is the immutability rule the middle tier
    ///   is built on (ghostty `src/font/SharedGrid.zig:13-18`); `select_config` carries it out.
    fn adopt_selectors(
        &mut self,
        at: usize,
        edit: impl FnOnce(&mut GridTier),
    ) -> Result<(), JsValue> {
        let prev = {
            let g = self.grid_at(at);
            (
                g.font_size,
                g.font_family.clone(),
                g.font_weight,
                g.font_weight_bold,
                g.letter_spacing,
                g.line_height,
                g.subpixel,
            )
        };
        edit(self.grid_at_mut(at));
        if self.gpu_work_must_wait() {
            return Ok(());
        }
        let key = self.key_of(at);
        if let Err(e) = self.select_config(at, key) {
            let g = self.grid_at_mut(at);
            (
                g.font_size,
                g.font_family,
                g.font_weight,
                g.font_weight_bold,
                g.letter_spacing,
                g.line_height,
                g.subpixel,
            ) = prev;
            return Err(e);
        }
        Ok(())
    }

    /// Set **one grid's** font size in **CSS px** — it joins the configuration keyed by the
    /// new size, baking one only if no grid already stands on it. Consumer policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)):
    /// the size is the consumer's, the atlas mechanism the renderer's. A non-finite size is ignored;
    /// a smaller-than-`1.0` one is clamped (a zero/negative size would rasterise a degenerate
    /// atlas). A no-op if unchanged.
    ///
    /// **It moves that grid only**, which is what [ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md)'s D1 means by a selector being per-grid,
    /// and what makes two terminals in two fonts drawable side by side. A sibling on the
    /// configuration this grid is leaving keeps it, untouched — the entry is never edited to follow
    /// a grid out of it (ghostty `src/font/SharedGrid.zig:13-18`).
    ///
    /// The cell size changes, so `cssCellWidth`/`css_cell_height` move and
    /// **the consumer must re-fit**: re-divide its box, `resizeGrid`, and
    /// re-place the grid — nothing here resizes the surface for it, because a surface drawing N
    /// grids belongs to none of them. Takes effect on the next `render`.
    #[wasm_bindgen(js_name = setFontSize)]
    pub fn set_font_size(&mut self, grid: u32, css_px: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        if !css_px.is_finite() {
            return Ok(());
        }
        let css_px = css_px.max(1.0);
        if (css_px - self.grid_at(at).font_size).abs() < f32::EPSILON {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_size = css_px)
    }

    /// Set **one grid's** font family — a CSS `font-family` string (`"monospace"`,
    /// `"'Fira Code', monospace"`, …) the browser's text engine resolves, with its own fallback. It
    /// joins the configuration keyed by the new family, exactly as a size change does. Consumer policy
    /// ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) — the renderer stays font-agnostic; loading a webfont (`@font-face` / `FontFace`)
    /// before calling is the consumer's job (an unloaded family silently falls back). A no-op if
    /// unchanged, and like a size change it moves **this grid only**.
    ///
    /// The cell size may change, so `cssCellWidth`/`css_cell_height` can move
    /// and **the consumer must re-fit**, exactly as after a size change. Takes effect on the
    /// next `render`.
    #[wasm_bindgen(js_name = setFontFamily)]
    pub fn set_font_family(&mut self, grid: u32, family: String) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        if family == self.grid_at(at).font_family {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_family = family)
    }

    /// Set the weight **one grid's** regular text is drawn at: CSS `"normal"` / `"bold"`, a
    /// `"100"`..`"900"` keyword, or a number in `[1, 1000]` — the values xterm.js's `fontWeight`
    /// takes. Anything else is ignored. It joins the configuration keyed by the new weight, exactly
    /// as a family change does, and moves this grid only.
    ///
    /// The cell is measured at the `normal` weight whatever this is, so it does not move and no re-fit
    /// is needed. Takes effect on the next `render`.
    #[wasm_bindgen(js_name = setFontWeight)]
    pub fn set_font_weight(&mut self, grid: u32, weight: JsValue) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let Some(weight) = weight_from_js(&weight) else {
            return Ok(());
        };
        if weight == self.grid_at(at).font_weight {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_weight = weight)
    }

    /// Set the weight **one grid's** bold (SGR 1) text is drawn at. Takes the same values as
    /// `setFontWeight`, and ignores anything else. Like it, this re-bakes
    /// the atlas without moving the cell.
    #[wasm_bindgen(js_name = setFontWeightBold)]
    pub fn set_font_weight_bold(&mut self, grid: u32, weight: JsValue) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let Some(weight) = weight_from_js(&weight) else {
            return Ok(());
        };
        if weight == self.grid_at(at).font_weight_bold {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.font_weight_bold = weight)
    }

    /// Draw **one grid's** text with per-channel (LCD / subpixel) coverage, or back to grayscale.
    /// Off by default. It joins the configuration keyed by the new setting, exactly as a family
    /// change does, and moves this grid only; the cell does not move, so no re-fit is needed. Takes
    /// effect on the next `render`.
    ///
    /// Per-channel coverage applies only where the cell's background is opaque: a default-background
    /// cell under a translucent `setBgAlpha` keeps grayscale, since one alpha cannot carry three
    /// coverages. Colour emoji and builtin block glyphs are unchanged. Where the browser draws no LCD
    /// text the three channels come back equal and no colour fringe appears, though dark ink is still
    /// drawn at the lighter weight the browser gives it.
    #[wasm_bindgen(js_name = setSubpixelAntialiasing)]
    pub fn set_subpixel_antialiasing(&mut self, grid: u32, on: bool) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        if on == self.grid_at(at).subpixel {
            return Ok(());
        }
        self.adopt_selectors(at, |g| g.subpixel = on)
    }

    /// Adopt a spacing policy on one grid (#338/#359), or leave every field as it was.
    ///
    /// The atlas slot is the padded CELL, so a spacing change is a different *configuration* rather
    /// than an edit to the current one — which is why `setLetterSpacing`/`setLineHeight` cost an
    /// atlas bake rather than a uniform, and why two grids can now hold two spacings at once.
    /// [`adopt_selectors`](Self::adopt_selectors) owns the atomicity and the lost-context deferral;
    /// this only decides which fields move.
    ///
    /// The error is dropped rather than returned because both public setters return `()`; a failed
    /// bake leaves the policy unchanged, so the next call retries (self-healing).
    fn adopt_spacing(&mut self, at: usize, letter_spacing: f32, line_height: f32) {
        let _ = self.adopt_selectors(at, |g| {
            g.letter_spacing = letter_spacing;
            g.line_height = line_height;
        });
    }

    /// Extra space between columns, in **CSS pixels** — the consumer's policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)), applied
    /// as `round(letter_spacing * dpr)` device px on the cell. May be negative, which
    /// narrows the cell and crops the glyph rather than stretching it; the cell never reaches zero.
    ///
    /// Both references take this in device px (xterm `WebglRenderer.ts:671`, alacritty
    /// `config/font.rs:20`), so the same setting is a different gap on a Retina display. Ours is
    /// the unit `FONT_SIZE` already speaks.
    #[wasm_bindgen(js_name = setLetterSpacing)]
    pub fn set_letter_spacing(&mut self, grid: u32, css_px: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let ls = if css_px.is_finite() { css_px } else { 0.0 };
        self.adopt_spacing(at, ls, self.grid_at(at).line_height);
        Ok(())
    }

    /// A multiplier on the glyph height, `>= 1` — the consumer's policy. Clamped rather than
    /// rejected: xterm throws from its option setter (`OptionsService.ts:182`), and a renderer that
    /// panics across the wasm boundary is a worse contract than one that reports what it adopted.
    /// Read the result back with `cell_height` — it may be smaller than asked,
    /// because a cell the atlas texture cannot hold is shrunk to one it can.
    #[wasm_bindgen(js_name = setLineHeight)]
    pub fn set_line_height(&mut self, grid: u32, multiplier: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        let lh = if multiplier.is_finite() {
            multiplier.max(1.0)
        } else {
            1.0
        };
        self.adopt_spacing(at, self.grid_at(at).letter_spacing, lh);
        Ok(())
    }

    /// The cell width in **device pixels** — exactly the `u_cell_size.x` the shader lays the grid
    /// out with: the face's advance at `font_size * dpr`, floored as xterm.js floors it, **plus the
    /// consumer's `letterSpacing`**. It is the *grid* cell, as xterm's `device.cell.width` is; the glyph
    /// box inside it is smaller whenever the spacing policy is not the identity.
    ///
    /// This is *the* cell. The bare name carries it because it is the exact, measured
    /// one, as in xterm.js's `dimensions.device.cell` and beamterm's `cell_size()`. Anything that
    /// addresses the drawing buffer — `readPixels`, GL interop, a picking rect — belongs here;
    /// `cssCellWidth` is the derived view for CSS layout.
    pub fn cell_width(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.config_at(at).cell_size.0)
    }

    /// The cell height in **device pixels** (see `cell_width`).
    pub fn cell_height(&self, grid: u32) -> Result<u32, JsValue> {
        let at = self.slot(grid)?;
        Ok(self.config_at(at).cell_size.1)
    }

    /// The cell width in **CSS pixels**, unrounded. The consumer divides its available box by this
    /// to decide how many columns fit, exactly as xterm.js's `FitAddon` divides by
    /// `dimensions.css.cell.width`, and maps mouse coordinates through it (beamterm's
    /// `css_cell_size` doc says the same).
    ///
    /// It is a **float on purpose**. Rounding it to a whole CSS pixel loses the device cell for
    /// good — 33 device px at dpr 2 is 16.5, and 17 does not scale back to 33.
    #[wasm_bindgen(js_name = cssCellWidth)]
    pub fn css_cell_width(&self, grid: u32) -> Result<f32, JsValue> {
        let at = self.slot(grid)?;
        Ok(css_px(self.config_at(at).cell_size.0, self.global.dpr))
    }

    /// The cell height in **CSS pixels**, unrounded (see `cssCellWidth`).
    #[wasm_bindgen(js_name = cssCellHeight)]
    pub fn css_cell_height(&self, grid: u32) -> Result<f32, JsValue> {
        let at = self.slot(grid)?;
        Ok(css_px(self.config_at(at).cell_size.1, self.global.dpr))
    }

    /// Size **one grid** to `cols`×`rows` cells.
    ///
    /// Until S5 this was `resize(cols, rows)` and it wrote two tiers at once: the implicit grid's
    /// dimensions *and* the drawing buffer, which it snapped to `cols * cell` device px. [ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md)
    /// D3 is the rule that separates them — tier the **fields**, and describe a setter by which
    /// fields it writes — and multi-viewport is what makes the separation load-bearing: with two
    /// grids in two cells on one canvas there is no cell the *buffer* can be a multiple of. The
    /// buffer half became `resizeSurface`; this is the per-grid half.
    ///
    /// This is what `cols` / `rows` report back, and what the frames
    /// fed to this grid are expected to carry. At least one cell each way.
    ///
    /// **Nothing clamps it to the grid's rect, and that is a decision.** The cell-geometry guarantee — a
    /// column cannot fall outside the buffer holding it — was the renderer's to keep while the
    /// buffer was derived from the grid. The rect is now the consumer's own measured box, so
    /// keeping it is the consumer's: place a rect a whole number of cells wide, which it can,
    /// having divided that box by `cssCellWidth` to get `cols` in the
    /// first place. What this renderer still guarantees is that an overhang cannot reach a
    /// **neighbour** — every grid draws under its own `gl.scissor`, so a cell past the rect is
    /// clipped rather than painted over the terminal next door.
    #[wasm_bindgen(js_name = resizeGrid)]
    pub fn resize_grid(&mut self, grid: u32, cols: u32, rows: u32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        // A grid must have at least one cell: a zero would make `cols`/`rows` describe something
        // that cannot be fed a frame.
        self.grid_at_mut(at).grid_size = (cols.max(1), rows.max(1));
        Ok(())
    }

    /// Size the **surface** — the one canvas every grid draws into — to a drawing buffer of
    /// `width`×`height` **device pixels**.
    ///
    /// The consumer sets the canvas's CSS display box itself from `cssWidth` /
    /// `cssHeight`, exactly as it did when the buffer came from a grid (the measured cell geometry
    /// couples the two; beamterm's `auto_resize_canvas_css = false` is the same split). Forget it
    /// and the device-px buffer is displayed at device px — twice its intended size on a Retina
    /// display.
    ///
    /// **Device pixels, in the same space as `setViewport`.** The cell count
    /// this replaced is gone because the surface no longer belongs to a grid (see
    /// `resizeGrid`), and the obvious substitute — a CSS box, as three.js's
    /// `setSize` takes — would make this the one canvas-addressing export in CSS px while every
    /// rect placed on that canvas is in device px. One canvas, one space. It also keeps that guarantee
    /// *reachable*: a single-grid consumer wanting the exact guarantee that made the old
    /// `resize(cols, rows)` safe asks for `cols * cellWidth(grid)`, and both numbers are integers
    /// this crate handed it. A CSS box would put a rounding step between them.
    ///
    /// A non-finite or non-positive size is refused rather than clamped: it is a caller error, and
    /// the honest zero case — a container that is still `display:none` — has an answer already
    /// (leave the grid unplaced, or `clearViewport` it).
    ///
    /// **A clamp moves every placed rect**, because a rect's GL y is measured from the buffer's
    /// bottom edge (`Viewport::gl_rect`). A consumer that shrinks the surface must re-place the
    /// grids on it — which it is doing anyway, since its own layout is what shrank.
    ///
    /// **A density change does not move it either.** `set_device_pixel_ratio` re-bakes every atlas
    /// and leaves the buffer exactly as asked, so `css_width` reports a different CSS box for the
    /// same buffer and the canvas is displayed at a different size until the consumer re-issues
    /// this call. That is the same rule a viewport rect already follows, applied to the surface: a
    /// device-px quantity the consumer measured is re-issued by the consumer, never adjusted here.
    /// A consumer has to re-fit after a density change regardless — the cell moved, so its column
    /// count and every rect moved with it — so this costs it nothing and removes the one thing that
    /// could go wrong silently.
    ///
    /// **WebGL is not obliged to grant the buffer**, so this asks and then adopts what it
    /// got; `cssWidth` reports the *granted* box, which is what the consumer
    /// should size its display box to.
    ///
    /// **When the drawing buffer cannot be read, the box is adopted but not verified**. A
    /// resize can land at any moment in a context-loss window and a consumer has no obligation to
    /// notice, so this commits the box and defers only the read-back;
    /// the restore path re-derives the buffer from it on a live context, which is where
    /// the clamp settles instead. During that window `cssWidth` describes a buffer that does not
    /// exist yet, so a consumer sizing its canvas from it overshoots — and the overshoot outlives
    /// the restore, because the display box is the consumer's and nothing here can rewrite it
    /// (measured through `justerm-web`). Its remedy is to repeat its fit once the
    /// context is back.
    #[wasm_bindgen(js_name = resizeSurface)]
    pub fn resize_surface(&mut self, width: i32, height: i32) -> Result<(), JsValue> {
        if width <= 0 || height <= 0 {
            return Err(JsValue::from_str(&format!(
                "justerm-renderer: a surface needs a positive drawing buffer, got {width}x{height}"
            )));
        }
        self.global.requested = (width, height);
        self.apply_surface_size();
        Ok(())
    }

    /// Re-ask for the stored drawing buffer and adopt whatever the browser actually grants.
    ///
    /// Two callers, and the second is why the request is *stored* rather than passed: a consumer's
    /// `resizeSurface`, and a context restore (the loss reset the buffer, and nobody is going to
    /// re-ask). A density change does **not** call this — the buffer is the consumer's device-px
    /// measurement and only the consumer re-measures it.
    ///
    /// Two passes suffice, and the bound is a backstop against a browser that clamps
    /// non-monotonically: pass 2 asks for a buffer the browser has already granted, so it cannot be
    /// clamped again. `canvas.width` is re-set *down* to the grant rather than left oversized as
    /// xterm, beamterm and three.js all leave it, because #337 couples the CSS display box to it —
    /// a lying attribute would make `cssWidth()` describe a buffer that does not exist.
    fn apply_surface_size(&mut self) {
        let (mut dw, mut dh) = self.global.requested;
        for _ in 0..2 {
            self.global.canvas.set_width(dw as u32);
            self.global.canvas.set_height(dh as u32);

            let (bw, bh) = (
                self.global.raw_gl.drawing_buffer_width(),
                self.global.raw_gl.drawing_buffer_height(),
            );
            // **A buffer of no size is not a grant, it is the absence of an answer** (#639). A lost
            // context reports 0x0; adopting it would commit a 1x1 surface that `restore` then
            // rebuilds at, leaving the canvas one pixel wide permanently and silently. The
            // requested box stays committed and the verification is what defers.
            //
            // This guards on the READ-BACK rather than on the context's state, and that is the
            // load-bearing part: a browser kills a context synchronously and only queues
            // `webglcontextlost`, so in that window the state machine's flag is still clear while
            // `drawingBufferWidth` already reads 0 (measured in Chromium, same task as
            // `loseContext()`). The other entry points cannot phrase the question this way because
            // they read nothing back; this one has the answer in its hand.
            if bw <= 0 || bh <= 0 {
                break;
            }
            if bw >= dw && bh >= dh {
                break; // granted in full; a larger grant is ignored — the request leads
            }
            (dw, dh) = (dw.min(bw), dh.min(bh));
        }
        self.global.size = (dw, dh);
        // Safety: live GL context (or a dead one, where this is a no-op with an error flag).
        unsafe {
            self.global.gl.viewport(0, 0, dw, dh);
        }
    }

    /// The drawing buffer's width in **CSS pixels** — what the consumer should set the canvas's CSS
    /// display box to, so the device-px buffer is shown at as close to the right size as a CSS
    /// length can get. Unrounded, for the same reason as `cssCellWidth`,
    /// and for one more: a rounded box misses the buffer by up to `dpr/2` device px — an
    /// absolute error, so it is ruinous on a small canvas — where this one misses by at most the
    /// browser's layout grain (`dpr/128`; measured 0.0016..0.0156 at dpr 1.1). It can also round
    /// *up*, stretching the image over a box wider than the buffer feeding it.
    ///
    /// Round it yourself if your layout needs a whole CSS pixel; the reverse is not available.
    #[wasm_bindgen(js_name = cssWidth)]
    pub fn css_width(&self) -> f32 {
        css_px(self.global.size.0 as u32, self.global.dpr)
    }

    /// The drawing buffer's height in **CSS pixels** (see `cssWidth`).
    #[wasm_bindgen(js_name = cssHeight)]
    pub fn css_height(&self) -> f32 {
        css_px(self.global.size.1 as u32, self.global.dpr)
    }

    /// Apply a `cols`×`rows` frame (dense row-major, length `cols*rows` — see `applyDamage` for the
    /// Partial-frame adapter): `bg`/`fg` are tagged-u32 colour refs, `codepoints` the glyph
    /// per cell, `flags` the `CellFlags`. A `WIDE_CHAR` lead cell rasterises a double-width
    /// glyph and splits it into two atlas slots; its `WIDE_CHAR_SPACER` cell reuses the
    /// right-half slot. New glyphs are rasterised + uploaded on demand.
    ///
    /// Tracked limits (surfaced by adversarial passes, not silent): colour emoji and
    /// ZWJ/grapheme clusters are separate slices; a frame with more distinct glyphs
    /// than a region's capacity, or a rasterise failure, can strand a slot.
    // Seven typed-array / scalar columns at the wasm-bindgen boundary; each is a distinct JS view
    // that cannot be grouped without an AoS rewrite breaking the zero-copy SoA (as on `apply_damage`).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_frame(
        &mut self,
        grid: u32,
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
        let at = self.slot(grid)?;
        let underline_colors = underline_colors.unwrap_or_default();
        self.pins.clear(); // this pack's scope is itself — see the field's doc
        let result = self.resolve_and_pack(at, &cells, bg, fg, &underline_colors, blink_on);
        self.grid_at_mut(at).needs_repack = false;
        result
    }

    fn resolve_and_pack(
        &mut self,
        at: usize,
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
        // The suggestion (#972) is the second pass and yields to the first: while a composition is
        // open it draws nothing.
        let patch = self
            .preedit_patch(at, cells, bg, fg)
            .or_else(|| self.grid_at(at).suggestion_patch(cells, bg, fg));
        let patched = patch.as_ref().map(|p| Cells {
            cols: cells.cols,
            rows: cells.rows,
            codepoints: &p.codepoints,
            flags: &p.flags,
            clusters: &p.clusters[..],
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
        //
        // Which cache is the one this GRID selects into (#772) — not "the" cache, of which there is
        // no longer one. The field-level split is what keeps `&mut configs` (the cache) disjoint
        // from `&global` (the GL the upload closure needs); they are separate fields of the facade,
        // so this borrows neither through the other.
        let gl = &self.global.gl;
        let pins = &mut self.pins;
        let config = self.configs.get_mut(self.grids.grid_at(at).config);
        let ConfigTier {
            cache,
            rasterizer,
            atlas,
            atlas_cell,
            ..
        } = config;
        let (atlas, atlas_cell) = (*atlas, *atlas_cell);
        let rasterizer = &*rasterizer;
        let (pad_w, pad_h) = atlas_cell;
        let slots = resolve_frame(
            cells,
            cache,
            pins,
            |text, style, wide| {
                // Rasterise, then classify with the hybrid signal (#297): a colour emoji comes
                // back in its own palette (COLR/CBDT/SVG) → is_color_bitmap; an emoji the font
                // draws in pure grayscale (`⬛ ⬜ ⚫ ⚪`) has R=G=B so the bitmap misses it → the
                // unicode `is_emoji_text` (keyed off core's `wide`) recovers it. Either signal
                // routes the glyph to a colour-sampled slot; a text glyph satisfies neither.
                let rgba = rasterizer.rasterize(text, style, wide)?;
                let is_emoji = is_emoji_text(text, wide) || is_color_bitmap(&rgba);
                let rgba = rasterizer.finish(rgba, text, style, wide, is_emoji)?;
                Ok((rgba, is_emoji))
            },
            |base, wide, rgba: Vec<u8>| {
                if wide {
                    // The wide source is two content cells plus one outer margin (guard band and
                    // horizontal band, #966) each side; split into two padded cells.
                    let m = rasterizer.margin_x();
                    let (left, right) =
                        split_wide_bitmap(&rgba, 2 * pad_w - 2 * m, pad_w, pad_h, m);
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
                // Two causes since #772, and a consumer cannot tell them apart from the outside, so
                // the message names both: this frame alone, or this frame together with the other
                // grids drawn beside it through the same font configuration. Either way the pack is
                // refused rather than drawn wrong — the grid keeps its last frame and this reaches
                // the consumer as a thrown error.
                "justerm-renderer: more distinct glyphs than the atlas can hold — this frame, or \
                 this frame together with the other grids sharing its font configuration",
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
        self.grid_at_mut(at).last_flags.clear();
        self.grid_at_mut(at)
            .last_flags
            .extend_from_slice(cells.flags);
        self.grid_at_mut(at).last_cols = cells.cols;
        self.grid_at_mut(at).last_blink_on = blink_on;
        self.resolve_cursor_cells(at);
        let frame = Frame {
            cols: cells.cols,
            rows: cells.rows,
            // The same span the patch above took over — handed on so every stage after glyph
            // resolution stands down inside it (ADR-0028 D2). Derived here rather than carried out
            // of `preedit_patch` so that the two cannot disagree about which cells are composed.
            preedit: self.preedit_span(at, cells.cols, cells.rows),
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
            active: &self.grid_at(at).active_match_spans,
            selection: &self.grid_at(at).selection_spans,
            matches: &self.grid_at(at).match_spans,
            link_hover: &self.grid_at(at).link_hover_spans,
            colors: self.grid_at(at).highlight_colors,
        };
        // #272: the RGB-space colour policy (bold→bright, dim, minimum-contrast, …), assembled from
        // the renderer's fields.
        let policy = ColorPolicy {
            bold_to_bright: self.grid_at(at).bold_to_bright,
            min_contrast: self.grid_at(at).min_contrast,
            selection_fg: self.grid_at(at).selection_fg,
        };
        // #393: the consumer-projected marker decorations for this frame (parsed from the flat wire).
        let decorations = parse_decorations(&self.grid_at(at).decoration_spans);
        self.grid_at_mut(at).instances = pack_instances(
            &frame,
            &self.grid_at(at).palette,
            blink_on,
            &overlay,
            &policy,
            &decorations,
        );
        self.grid_at_mut(at).instance_count = count as i32;
        self.upload_instances(at);
        // Record the atlas state these instances were packed against, AFTER the resolve that may
        // itself have evicted (#772). Last, so a frame that failed above records nothing.
        let evictions = self.config_at(at).cache.evictions();
        self.grids.grid_at_mut(at).packed_at_evictions = evictions;
        Ok(())
    }

    /// Reconcile the GPU instance buffer with the freshly packed `self.grid().instances`, uploading
    /// only the cells that changed since the last upload (#263). A size change (first frame /
    /// resize) reallocates the whole buffer; otherwise each changed contiguous range goes up via
    /// `buffer_sub_data` and an unchanged frame does no GL work at all. `self.grid().uploaded` mirrors
    /// what the GPU holds so the next frame can diff against it.
    fn upload_instances(&mut self, at: usize) {
        // Bind the two tiers this touches once, as separate fields of `self`: the baseline lives
        // beside the buffer it mirrors (both per-grid), and the context that uploads it is global.
        // Going through `grid_mut()` at each site instead would re-borrow all of `self` per call —
        // and `uploaded.clone_from(&instances)` is two fields of ONE grid, which only splits when
        // the grid is a place expression.
        let gl = &self.global.gl;
        let grid = self.grids.grid_at_mut(at);
        match plan_upload(&grid.uploaded, &grid.instances, INSTANCE_FLOATS) {
            UploadPlan::Full => unsafe {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(grid.instance_vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    f32_bytes(&grid.instances),
                    glow::DYNAMIC_DRAW,
                );
                grid.uploaded.clone_from(&grid.instances);
            },
            UploadPlan::Ranges(ranges) => {
                if ranges.is_empty() {
                    return; // nothing changed — skip the bind + upload entirely
                }
                unsafe {
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(grid.instance_vbo));
                    for (start, end) in ranges {
                        let (lo, hi) = (start * INSTANCE_FLOATS, end * INSTANCE_FLOATS);
                        gl.buffer_sub_data_u8_slice(
                            glow::ARRAY_BUFFER,
                            (lo * std::mem::size_of::<f32>()) as i32,
                            f32_bytes(&grid.instances[lo..hi]),
                        );
                        grid.uploaded[lo..hi].copy_from_slice(&grid.instances[lo..hi]);
                    }
                }
            }
        }
    }

    /// Re-pack the instance buffer from the retained dense grid — the single pack [`render`] runs when
    /// a mutation dirtied the buffer (#421; #271 was the original overlay-only re-pack). A no-op until
    /// the first `apply_damage` (the direct `apply_frame` path keeps no columns to re-pack from). Takes
    /// the grid out so the `&mut self` pack does not borrow-conflict, then puts it back.
    ///
    /// [`render`]: Self::render
    fn repack_from_grid(&mut self, at: usize) -> Result<(), JsValue> {
        let Some(grid) = self.grid_at_mut(at).grid.take() else {
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
            at,
            &cells,
            grid.bg(),
            grid.fg(),
            grid.underline_colors(),
            self.grid_at(at).last_blink_on,
        );
        self.grid_at_mut(at).grid = Some(grid);
        result
    }

    /// Clear to the palette's default background, then draw every cell of the current frame
    /// (glyph composited over background) with one instanced draw call.
    ///
    /// Context loss is handled here, before any GL work: while the context is lost this is a
    /// silent no-op (a draw call on a dead context accomplishes nothing), and on the frame after
    /// `webglcontextrestored` it first rebuilds the destroyed resources. Recovery therefore needs
    /// no consumer cooperation beyond continuing to call `render`. A failed rebuild propagates and
    /// is retried on the next frame.
    ///
    /// **"While the context is lost" means either sense of lost, and it did not always**.
    /// The decision consults the context itself *and* the state machine's flag, because a browser
    /// destroys a context synchronously and only queues the event: asking the flag alone, this
    /// promise was false for that slice — a pending rebuild ran on a dead context and threw.
    pub fn render(&mut self) -> Result<(), JsValue> {
        // Ask the CONTEXT, then let the state machine compose that with the flag it owns
        // (#695, ADR-0027 D3). Bound to locals in this order deliberately: the liveness read
        // must not happen while the `Ref` below is alive, and the `Ref` must be released
        // before the `&mut self` calls in the match.
        let live = if self.global.raw_gl.is_context_lost() {
            ContextLiveness::Dead
        } else {
            ContextLiveness::Usable
        };
        let action = self.global.ctx_loss.state.borrow().action(live);
        match action {
            FrameAction::Skip => return Ok(()),
            FrameAction::Rebuild => {
                self.restore()?;
                // Only now that the rebuild is committed does the retry latch clear.
                self.global.ctx_loss.state.borrow_mut().rebuilt();
            }
            FrameAction::Draw => {}
        }
        // Pack once per drawn grid, here, if a mutation since the last render dirtied its buffer
        // (#421) — the context is live past the match above (Skip returned, Rebuild restored). A
        // frame that set overlay + decorations + `apply_damage` marked dirty three times but
        // re-packs once. On a pack error the flag stays set, so the next render retries
        // (self-healing).
        //
        // **A grid with no viewport is not packed either, and that is a decision** (#771). The
        // registry's `Option<Viewport>` says whether a grid *draws*; nothing said whether a hidden
        // grid still pays for the frames it is fed, and the consumer's adoption design keeps hidden
        // terminals mounted **and feeding**. Measured on a release build at 120x40, an ungated
        // hidden grid costs about 0.4 ms/frame — pack + upload about 0.33 of it — so ten of them
        // would spend a quarter of a 60 fps budget on pixels nobody sees. Gating it here is free
        // rather than clever: the dirty flag stays set while the grid is hidden, so the first
        // render after it is placed packs it, once. Ghostty gates the same two things on the same
        // state (`renderer/Thread.zig:526-531` the draw, `:644-650` the CPU rebuild); alacritty
        // gates only the paint.
        //
        // One grid's bad frame must not blank its neighbours, so a pack error is held and the
        // frame still draws. It surfaces after the draw, and the flag it left set means the next
        // render retries exactly as the single-grid path did.
        // One pin set for the whole loop, so a grid cannot evict a slot a sibling packed earlier in
        // the SAME frame (#772). Without it the second grid's pack repoints the first's committed
        // slots, the first is not re-diffed because its instance floats did not change, and it draws
        // **stably wrong** — measured, and invisible to a pixel check without a control: a grid drew
        // 911 lit subpixels beside a sibling and 891 alone, every frame, with no error anywhere.
        // With the pin the second pack is refused instead, which is exactly what an over-capacity
        // *single* frame has always got (`FrameExceedsCapacity`), extended to the union.
        self.pins.clear();
        let mut pack_error = None;
        for at in 0..self.grids.len() {
            if self.grids.viewport_at(at).is_none() {
                continue;
            }
            // …and a grid whose atlas moved under it re-packs too, even though nothing it owns
            // changed (#772). A sibling on the same configuration can evict a slot this grid's
            // instances still address; the upload diff cannot notice, because the floats are the
            // same and only the atlas behind them moved. Comparing counters costs a `u32` per grid
            // per frame and is the whole of the guarantee ADR-0021 asked this tier for.
            let stale =
                self.grid_at(at).packed_at_evictions != self.config_at(at).cache.evictions();
            if self.grid_at(at).needs_repack || stale {
                match self.repack_from_grid(at) {
                    Ok(()) => self.grid_at_mut(at).needs_repack = false,
                    Err(e) => {
                        // A refused pack has to leave a **fixed point**, or the frames alternate.
                        //
                        // The pin only covers grids that actually packed this frame, and a clean
                        // grid does not pack — so without this the cycle is: this grid is refused
                        // and its sibling stays correct; next frame the sibling is clean, packs
                        // nothing, leaves the pins empty, and *this* grid succeeds by repointing
                        // the sibling's slots. Measured: `a` alternating 891 (right) / 911 (wrong)
                        // with the error appearing only on alternate frames.
                        //
                        // Dirtying every grid that shares this configuration makes them all pack,
                        // every frame, for as long as the overflow lasts — so the earlier ones pin
                        // their glyphs first and stay correct, and the same grid is refused each
                        // time. Registration order decides who wins, which is the order everything
                        // else in this loop already uses (#771): the grid registered first wins. The extra packing costs what a re-pack costs, in a state that is
                        // already reporting an error every frame; it is bounded by the overflow.
                        let config = self.grid_at(at).config;
                        for other in 0..self.grids.len() {
                            if self.grids.viewport_at(other).is_some()
                                && self.grid_at(other).config == config
                            {
                                self.grid_at_mut(other).needs_repack = true;
                            }
                        }
                        pack_error = pack_error.or(Some(e));
                    }
                }
            }
        }
        self.draw();
        match pack_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Number of instance-buffer packs run so far (a diagnostic). The consumer/proofs read the
    /// **delta** across an operation to assert `render` packs once per *dirty drawn grid* per frame
    /// — not once per setter, and not at all for a grid with no viewport.
    /// Not a stable API surface — a counter for verification, not a rendering control.
    #[wasm_bindgen(js_name = packs)]
    pub fn packs(&self) -> u32 {
        self.pack_count
    }

    /// Issue the frame's GL commands: one clear of the whole drawing buffer, then one pass per
    /// **drawn** grid (#771). The caller has established that the context is live and its resources
    /// are intact.
    ///
    /// The full-buffer clear happens once and before the loop, and it clears to **transparent**
    /// rather than to any grid's background. The buffer is one shared plane that the grids do not
    /// have to tile (ADR-0021's z-order constraint: every terminal is an overlay on it), so the
    /// area between two rects belongs to the page behind the canvas — painting a terminal's colour
    /// there would be this renderer deciding a background it was never given. A single-grid
    /// consumer sees no difference *if it places its grid over the whole buffer*, which is what the
    /// single-grid arrangement is since #773 — its own clear then covers every pixel this one
    /// touched. It is the consumer's arrangement now rather than something `resize` guaranteed, so
    /// a consumer that places a smaller rect gets a transparent margin, honestly.
    ///
    /// three.js's multiple-views example does no full clear at all
    /// (`examples/webgl_multiple_views.html:252-278`) — its views tile the canvas, so it has no
    /// uncovered area to answer for. That is a silence rather than a divergence.
    ///
    /// **Grids are painted in registration order and are not composited with each other.** A later
    /// grid's rect *replaces* what is under it rather than blending with it, because each grid opens
    /// with a `clear` and a clear writes. So a translucent grid (`setBgAlpha`) shows the page behind
    /// the canvas, never the grid it overlaps. Overlapping rects are the consumer's business —
    /// tiling panes do not produce one — and the same is true of the reference's per-view clear.
    fn draw(&self) {
        unsafe {
            self.global.gl.disable(glow::SCISSOR_TEST);
            self.global
                .gl
                .viewport(0, 0, self.global.size.0, self.global.size.1);
            self.global.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            self.global.gl.clear(glow::COLOR_BUFFER_BIT);

            // Every grid's clear and every grid's cells are confined to its own rect from here.
            // The viewport alone would already clip the *cells* (they are drawn in clip space and
            // the transform maps NDC onto the rect), but `clear` ignores the viewport entirely —
            // so without the scissor each grid's background clear would wipe the whole buffer,
            // leaving only the last grid visible.
            self.global.gl.enable(glow::SCISSOR_TEST);
            for at in 0..self.grids.len() {
                if let Some(viewport) = self.grids.viewport_at(at) {
                    self.draw_grid(at, viewport);
                }
            }
            self.global.gl.disable(glow::SCISSOR_TEST);
        }
    }

    /// Draw one grid into its rect. Assumes `SCISSOR_TEST` is enabled by the caller.
    ///
    /// # Safety
    ///
    /// Live context with intact resources, as [`draw`](Self::draw) establishes.
    unsafe fn draw_grid(&self, at: usize, viewport: Viewport) {
        let grid = self.grid_at(at);
        // The configuration THIS grid selects into (#772). Two grids in two fonts draw through two
        // atlases with two cell geometries in the same frame, which is why every one of these is a
        // per-draw uniform rather than a per-program one — `u_cell_uv` included, since the
        // guard band is a fixed pixel count and its *fraction* differs with the padded cell.
        let config = self.config_at(at);
        let (vx, vy, vw, vh) = viewport.gl_rect(self.global.size.1);
        let [dr, dg, db] = gl_rgb(grid.palette.default_bg);
        unsafe {
            self.global.gl.viewport(vx, vy, vw, vh);
            self.global.gl.scissor(vx, vy, vw, vh);
            // Clear with the injected background opacity so any area of this grid's rect not
            // covered by a cell is see-through too; cells then write their own per-pixel alpha
            // (#298). A rect is the consumer's box and may be any size, so the uncovered area is
            // whatever its cells do not reach — asking for `cols * cell_width(grid)` device px is
            // what makes it none, and since #773 that is the consumer's arithmetic (#331).
            self.global.gl.clear_color(dr, dg, db, grid.bg_alpha);
            self.global.gl.clear(glow::COLOR_BUFFER_BIT);

            if grid.instance_count == 0 {
                return;
            }

            self.global.gl.use_program(Some(self.global.program));
            self.global.gl.active_texture(glow::TEXTURE0);
            self.global
                .gl
                .bind_texture(glow::TEXTURE_2D_ARRAY, Some(config.atlas));
            // The instance buffer already holds the current frame — `upload_instances` (in the
            // pack path) uploaded only the changed cells (#263), so render just binds + draws.
            // Binding THIS grid's VAO is what points the attributes at THIS grid's buffer: the
            // pointer is VAO state, which is why the VAO is per-grid (#771 resolving #768).
            self.global.gl.bind_vertex_array(Some(grid.vao));

            // The projection is sized to the RECT, not to the buffer: `gl.viewport` above already
            // maps clip space onto the rect, so a buffer-sized projection would scale every grid
            // by `buffer / rect`. Identical for a grid placed over the whole buffer, which is
            // what the single-grid arrangement is.
            let proj = Mat4::orthographic_from_size(vw as f32, vh as f32);
            self.global.gl.uniform_matrix_4_f32_slice(
                Some(&self.global.u_projection),
                false,
                &proj.data,
            );
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_cell_size),
                config.cell_size.0 as f32,
                config.cell_size.1 as f32,
            );
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_char_size),
                config.char_size.0 as f32,
                config.char_size.1 as f32,
            );
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_char_offset),
                config.char_offset.0 as f32,
                config.char_offset.1 as f32,
            );
            // How much of each padded atlas cell is guard band, so the shader insets the texcoord
            // to the content region (see FRAG_SRC). Set once per program until #772; per draw now,
            // because the padded cell belongs to the configuration rather than to the context.
            let ((ox, oy), (sx, sy)) = crate::metrics::cell_uv(
                crate::metrics::SlotGeometry {
                    padded: config.atlas_cell,
                    draw_origin: (0, 0), // unused by `cell_uv`
                },
                config.cell_size,
            );
            self.global
                .gl
                .uniform_4_f32(Some(&self.global.u_cell_uv), ox, oy, sx, sy);
            let line_thickness = crate::metrics::line_thickness(grid.font_size * self.global.dpr);
            self.global
                .gl
                .uniform_1_f32(Some(&self.global.u_line_thickness), line_thickness as f32);
            // Per grid, like the thickness it is derived from — both depend on the font size, and
            // the dot count also on the cell width, so neither is a per-config constant (#830).
            self.global.gl.uniform_1_f32(
                Some(&self.global.u_dots_per_cell),
                crate::metrics::dots_per_cell(config.cell_size.0, line_thickness) as f32,
            );
            self.global
                .gl
                .uniform_1_f32(Some(&self.global.u_bg_alpha), grid.bg_alpha);
            // Per configuration, not per grid: the bands are a property of the face (#791, #966).
            let (bleed_x, bleed_y) = config.rasterizer.bleed();
            self.global.gl.uniform_2_f32(
                Some(&self.global.u_bleed_px),
                bleed_x as f32,
                bleed_y as f32,
            );
            // Per configuration too: whether its atlas carries per-channel coverage, and the
            // dark-ink curve it was measured with (#961).
            self.global.gl.uniform_1_f32(
                Some(&self.global.u_lcd_gamma),
                config.rasterizer.lcd_gamma(),
            );
            // `u_cursor.w == 0` means NO cursor; a shape is `shape_id + 1`. Every shape — block
            // included — reaches the shader this way, so a move or a blink is a uniform, not an
            // upload (#270).
            let (cx, cy, span, shape) = match grid.cursor {
                Some(c) => (
                    grid.cursor_cells.0 as f32,
                    c.row as f32,
                    grid.cursor_cells.1 as f32,
                    shape_id(c.shape) as f32 + 1.0,
                ),
                None => (0.0, 0.0, 1.0, 0.0),
            };
            self.global
                .gl
                .uniform_4_f32(Some(&self.global.u_cursor), cx, cy, span, shape);
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
            let (color, text_color) = match grid.cursor {
                Some(c) => {
                    let cell_bg = (c.row as usize)
                        .checked_mul(grid.last_cols as usize)
                        .and_then(|i| i.checked_add(grid.cursor_cells.0 as usize))
                        .and_then(|i| i.checked_mul(INSTANCE_FLOATS))
                        .and_then(|base| grid.instances.get(base + 2..base + 5));
                    match cell_bg {
                        Some(bg) => guarded_cursor_colors(
                            c.color,
                            c.text_color,
                            [bg[0], bg[1], bg[2]],
                            grid.palette.default_fg,
                            grid.palette.default_bg,
                            grid.cursor_contrast,
                        ),
                        None => (c.color, c.text_color),
                    }
                }
                None => (0, 0),
            };
            let [cr, cg, cb] = gl_rgb(color);
            self.global
                .gl
                .uniform_3_f32(Some(&self.global.u_cursor_color), cr, cg, cb);
            let [tr, tg, tb] = gl_rgb(text_color);
            self.global
                .gl
                .uniform_3_f32(Some(&self.global.u_cursor_text_color), tr, tg, tb);
            self.global.gl.uniform_1_f32(
                Some(&self.global.u_cursor_thickness),
                cursor_thickness(grid.cursor_thickness_frac, config.cell_size.0) as f32,
            );

            self.global
                .gl
                .draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, grid.instance_count);
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
    /// grid's fields and the entry its handle names cannot be born disagreeing. `select_config` is
    /// the only thing that moves either afterwards, and it moves both.
    ///
    /// **One constructor, and since #773 exactly one caller** (`add_grid`): the second path, which
    /// built the implicit default grid inside `new`, is gone with the grid it built. What that
    /// sentence used to buy — a field added to this tier cannot be initialised in one path and
    /// forgotten in the other — is now true by there being nowhere else to forget it.
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

/// Per-grid operations (ADR-0021 D1/D2). These live here rather than on the facade because
/// multi-viewport (#287) multiplies this struct and nothing else: a method written against
/// `GridTier` is already the per-grid form #770 needs, and one that reached into another tier
/// would not compile here.
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

/// Per-grid operations (ADR-0021 D1/D2). These live here rather than on the facade because
/// multi-viewport (#287) multiplies this struct and nothing else: a method written against
/// `GridTier` is already the per-grid form #770 needs, and one that reached into another tier
/// would not compile here.
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
        // A re-pack is all a live theme swap needs: it re-resolves every cell's colour against the new
        // palette (the render's clear reads `self.palette.default_bg` fresh). #298 translucency used to
        // also re-push a `u_default_bg` uniform here; since #455 its trigger is the packer's per-cell
        // `bg_default` provenance flag — palette-independent — so the re-pack alone carries it.
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

/// Per-grid operations (ADR-0021 D1/D2). These live here rather than on the facade because
/// multi-viewport (#287) multiplies this struct and nothing else: a method written against
/// `GridTier` is already the per-grid form #770 needs, and one that reached into another tier
/// would not compile here.
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
