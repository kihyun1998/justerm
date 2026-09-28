//! Pure frame → GPU instance packing (host-testable; the GL upload is browser-only).
//!
//! The renderer's hot path (ADR-0018 "A-ii"): resolve each cell's colour references with
//! the injected palette + colour policy (inverse, bold→bright, dim — [`render_policy`]), composite
//! the marker [`decoration`] overrides and the selection/search highlight ([`overlay`]) back-to-front
//! (base < bottom-decoration < highlight < top-decoration), fold the underline/strikethrough into the
//! glyph field, and pack per-cell instance data in Rust — one flat buffer for a single instanced draw
//! call. Glyph-slot resolution (the stateful atlas cache) and rasterisation happen in the browser
//! layer; this packer takes the already-resolved slots + cell flags.
//!
//! [`render_policy`]: crate::render_policy
//! [`overlay`]: crate::overlay
//! [`decoration`]: crate::decoration

use crate::attrs::{
    BLANK_SLOT, STRIKETHROUGH, UNDERLINE, USTYLE_SHIFT, glyph_field, is_concealed, is_dim,
    is_inverse,
};

/// The underline style field's value for a single straight line — what core writes for `SGR 4`.
const SINGLE_UNDERLINE: u16 = 1 << USTYLE_SHIFT;
use crate::color::gl_rgb;
use crate::contrast::ensure_contrast_ratio;
use crate::decoration::{
    DecorationLayer, DecorationOverride, DecorationRect, decoration_override_at,
};
use crate::glyph_class::treat_glyph_as_background_color;
use crate::overlay::{
    HIGHLIGHT_BLEND_ALPHA, HighlightKind, Overlay, blend_over, composite_bg, should_blend_kind,
};
use crate::palette::{Palette, resolve_indexed_or_rgb};
use crate::render_policy::{ColorPolicy, dim_foreground, resolve_cell};

/// Floats per cell instance: `col, row, bg(3), fg(3), glyph_field, underline_fg, strike_fg,
/// bg_default`.
///
/// `underline_fg` / `strike_fg` are the inks the two marks draw in (#513, split by #525), carried
/// **packed** as a single `0xRRGGBB` each rather than unpacked into three floats like `bg` and `fg`.
/// A colour maxes at `2²⁴ − 1` and an `f32` represents every integer below `2²⁴` exactly, so the value
/// survives the attribute and `uint()` in the shader — measured, not assumed: a standalone WebGL2
/// probe round-tripped ten values including `0x000000`, `0xFFFFFF`, `0x010101` and both sides of the
/// sign-bit boundary, all exact. Packed because a line is *rare* — every cell would otherwise pay
/// three floats per band for a channel almost none of them use, and ADR-0021 keeps one instance
/// buffer resident per grid.
///
/// **Why two, when #513 shipped one.** They are one ink source (ADR-0019 rule 4's `I_line`) split by
/// **authorship of the colour**, the same axis rules 5 and #520 already turn on: `SGR 58` declares the
/// *underline's* colour and there is no SGR for a strikethrough's, so a declared colour is
/// authoritative over one band and has nothing to say about the other. With no `SGR 58` the two are
/// equal and the second float is redundant — that is the common case, and it is what
/// `sgr58_colours_the_underline_only_and_the_strike_keeps_the_follow_fg_ink` pins from the other side.
/// The alternative that costs nothing — draw the strike from `v_fg` — is wrong on exactly the cells
/// #513 exists for: a glyph-only rule (the #239 re-tint) moves `fg` and must not reach `I_line`.
///
/// `bg_default` is the #455 translucency-provenance flag (`1.0` / `0.0`): whether this cell's
/// background is the pristine **default backdrop** — the only surface #298 makes translucent. It is
/// *provenance*, not colour: the shader used to infer it by comparing the resolved bg to
/// `u_default_bg`, so any cell whose composite happened to land on the default RGB (an `SGR 48` set to
/// the theme bg, an `Indexed` slot resolving to it, a decoration painting it) went translucent inside
/// otherwise-opaque content. ADR-0019's totality clause forbids that — resolution follows the cell's
/// state, not an accident of the fold — so the packer, which knows every layer that touched the bg,
/// emits the answer and the shader stops guessing. It costs its own float because #513 already spent
/// the line inks' exact-integer budget: a colour fills all 24 f32-safe bits, leaving no spare bit to ride.
pub const INSTANCE_FLOATS: usize = 20;

/// Named float offsets inside one instance. They exist because the layout used to be addressed by
/// arithmetic — `INSTANCE_FLOATS - 1` for the last field, a literal `12` for the stride — and both
/// forms silently read the **wrong** float the moment a field is appended (#791). A name cannot.
pub const BG_RGB: usize = 2;
/// The cell's own resolved foreground, 3 floats.
pub const FG_RGB: usize = 5;
/// The glyph field: slot in bits 0..12, underline/strike/emoji/ink-class in the high bits.
pub const GLYPH_FIELD: usize = 8;
/// 1.0 iff this cell's bg is the pristine default backdrop (#455).
pub const BACKDROP: usize = 11;
/// `I_neighbour` handles (ADR-0019 R1.2): the glyph field of the cell above, below (#791), to the
/// left and to the right (#966), or [`BLANK_SLOT`] where that neighbour's ink is withdrawn. Four
/// consecutive floats, read by the shader as one `vec4` attribute.
pub const NEIGHBOUR_UP: usize = 12;
pub const NEIGHBOUR_DN: usize = 13;
pub const NEIGHBOUR_LT: usize = 14;
pub const NEIGHBOUR_RT: usize = 15;
/// The ink each of those neighbours draws in, packed `0xRRGGBB` one per float — the same
/// idiom the two line inks use (#513). ADR-0019 R1.2: foreign ink keeps its OWNER's colour,
/// so the receiver's own `fg` is the wrong answer even though it is already to hand. Four
/// consecutive floats in the same order, one `vec4` attribute.
pub const NEIGHBOUR_UP_FG: usize = 16;
pub const NEIGHBOUR_DN_FG: usize = 17;
pub const NEIGHBOUR_LT_FG: usize = 18;
pub const NEIGHBOUR_RT_FG: usize = 19;

/// A decoded frame's per-cell grid: dimensions + the four parallel column arrays the packer
/// reads (all row-major, ideally length `cols*rows`). `bg`/`fg` are tagged-u32 colour refs,
/// `slots` the resolved atlas slot per cell, `flags` the `CellFlags` bits. A short/missing
/// entry resolves as `Default` / slot `0` / no flags — every cell is still emitted (#255).
pub struct Frame<'a> {
    pub cols: u32,
    pub rows: u32,
    /// The cells an open IME composition covers (#249, ADR-0028 D2), or `None`. Every stage below
    /// glyph resolution stands down inside it: the pass **replaces** those cells rather than
    /// layering over them, so a selection, a search match or a decoration covering the run must not
    /// tint the text the user is composing — and neither may the covered cell's `SGR 58` colour the
    /// underline the pass itself drew (#711). The stand-downs are the packer's half of D2; the other
    /// half re-supplies the columns upstream, and a column answered by *neither* describes the cell
    /// the composition erased.
    pub preedit: Option<crate::preedit::Span>,
    pub bg: &'a [u32],
    pub fg: &'a [u32],
    pub slots: &'a [u16],
    pub flags: &'a [u16],
    /// Per-cell **base** codepoint — the first scalar of the resolved glyph. Only the tile-glyph
    /// classifier ([`treat_glyph_as_background_color`], #226/#239) reads it; a short/missing entry
    /// resolves as `0` (not a tile). The atlas *slot* is already resolved in `slots`; this is kept
    /// solely to classify the glyph — which decides its contrast exclusion (#226), its selection
    /// re-tint (#239/#241), and whether a bg-only TOP decoration paints over it (#494).
    ///
    /// The packer sees **only** the base — a cell's grapheme cluster (`extra` + `side_table`) stops
    /// at [`glyph_resolve`](crate::glyph_resolve), which rasterises it. That is the declared rule,
    /// not an omission: coverage is set by the base and a combining mark can only add ink to it
    /// (#495, reasoned in [`glyph_class`](crate::glyph_class)).
    pub codepoints: &'a [u32],
    /// Per-cell **underline colour** reference (SGR 58, #520), tagged-u32 like `fg`/`bg`
    /// (`0` = `Default`). A cell that draws a coloured underline resolves this — via the
    /// palette, exactly as `fg`/`bg` — as the base ink for `I_line` (the underline /
    /// strikethrough), replacing the glyph's ink that #513 forked the line from. A short
    /// or missing entry resolves as `Default`, i.e. the line follows the glyph ink (the
    /// #513 behaviour). It never touches the glyph — only the line's colour channel.
    ///
    /// It reaches the **underline band only** (#525). `I_line` was one ink for both marks while
    /// #513's single channel was all there was, so this colour painted the strikethrough too — and
    /// ADR-0019 rule 4, which named `I_line` as one source, said it should. The record now carries
    /// `underline_fg` and `strike_fg` separately and rule 4 splits them by **authorship of the
    /// colour**: SGR 58 declares the underline's, nothing declares a strikethrough's, so the strike
    /// stays on the follow-fg pipeline whatever the underline does. `justerm-core` arms this column
    /// only when `UNDERLINE` is set (`term.rs::pen_ext_attrs`), so a strike-only cell never carried
    /// a colour to begin with; the both-marks cell is what changed.
    ///
    /// **Not read at all inside an open composition** (#711, ADR-0028 D2): the preedit pass writes
    /// its own `UNDERLINE`, so that mark's ink is the fg the pass supplied and this column still
    /// describes the cell the composition replaced. [`preedit`](Self::preedit) is what withholds it.
    pub underline_colors: &'a [u32],
}

/// Pack a [`Frame`] (row-major) into per-cell instance data
/// `[col, row, bg_r, bg_g, bg_b, fg_r, fg_g, fg_b, glyph_field, underline_fg, strike_fg, bg_default]`. Colours resolve through the injected
/// `policy`: inverse swaps the fg/bg and a bold ANSI 0–7 fg brightens to 8–15 ([`resolve_cell`], #223);
/// a DIM cell's fg fades toward its bg (#232, [`dim_foreground`]); `minimumContrastRatio` nudges an
/// illegible fg (#225); and a *selected* cell takes `selectionForeground` (#227) and has its DIM
/// cleared (#224) — the selection-side rules key off the [`overlay`](crate::overlay).
/// Underline/strikethrough fold into the glyph field's high bits. A *concealed* cell —
/// hidden (`ESC[8m`), or blink with `blink_on == false` — collapses to the blank slot
/// ([`BLANK_SLOT`]) so only its (inverse-aware) background shows; `blink_on` is the render
/// loop's phase, driven by the consumer (timing is policy, #282). **Every cell is emitted** —
/// no cell is left un-drawn (#255).
///
/// A cell covered by the selection / search `overlay` (#271/#400) has its resolved background
/// composited with the injected highlight colour: a *selection* blends over a non-default / inverse
/// cell (painting solid over the default background), while a search *match* always paints solid
/// ([`should_blend_kind`], [`composite_bg`]). Compositing happens in packed colour space, before the
/// `gl_rgb` unpack, so the blend matches the web reference to the byte; and because the whole viewport
/// re-packs each frame and the #263 upload diff re-sends only changed cells, a cell that gains or loses
/// a highlight re-uploads with no extra bookkeeping (unlike beamterm's overlay delta).
pub fn pack_instances(
    frame: &Frame,
    palette: &Palette,
    blink_on: bool,
    overlay: &Overlay,
    policy: &ColorPolicy,
    decorations: &[DecorationRect],
) -> Vec<f32> {
    let Frame {
        cols,
        rows,
        preedit,
        bg,
        fg,
        slots,
        flags,
        codepoints,
        underline_colors,
    } = *frame;
    // `usize` is 32 bits on wasm32, so `cells * INSTANCE_FLOATS` overflows it for a grid `resolve_frame` would
    // still accept (it only bounds `cells` by the slice length). A failed reservation is not worth
    // an abort: fall back to growing on demand — the loop below is bounded by `rows * cols` either
    // way, and `resolve_frame` has already refused a grid its slices cannot back (#355).
    let cells = (cols as usize).saturating_mul(rows as usize);
    let mut out = Vec::with_capacity(cells.checked_mul(INSTANCE_FLOATS).unwrap_or(0));
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
            let cell_flags = flags.get(idx).copied().unwrap_or(0);

            // Resolve refs → packed 0xRRGGBB, applying inverse + bold→bright (#223); keep the values
            // packed through the highlight composite + the fg policy, unpacking to gl floats only at the
            // end — the blends are integer math (match the reference to the byte).
            let bg_ref = bg.get(idx).copied().unwrap_or(0);
            let fg_ref = fg.get(idx).copied().unwrap_or(0);
            // The cell's OWN resolved fg (inverse + bold→bright) — kept undimmed for the tile-glyph
            // re-tint below, which starts from it (discarding any selectionForeground).
            let (cell_fg, cell_bg) =
                resolve_cell(fg_ref, bg_ref, cell_flags, palette, policy.bold_to_bright);
            // The highlight covering this cell (if any) — the *bg* channel's winner. The fg channel
            // is INDEPENDENT (#430, xterm's model): the selection-only fg rules (#224/#227/#239) key
            // on selection *coverage*, not on the winning kind, so they survive on a cell whose bg
            // the ACTIVE match outranks (CellColorResolver keys its selection stage on `$isSelected`
            // while the active match is a bg-only top decoration).
            // ADR-0028 D2 — inside an open composition every stage from here down stands down.
            // The pass took these cells out of the stack at resolve time; leaving the overlay and
            // decoration lookups live would put them straight back in, one channel at a time.
            let composed = preedit.is_some_and(|p| p.covers(row, col));
            // #454: every span lookup below asks about this cell AND the other half of the wide
            // pair it belongs to, so a range whose edge falls between the halves cannot paint one
            // of them. Read from the frame's flags, which are the PATCHED ones inside a preedit —
            // the pass writes its own pairs, and they exist nowhere else.
            //
            // The partner's OWN `composed` state is deliberately not re-checked, and the condition
            // that makes that safe is `preedit::range`: it counts a wide codepoint as **two** cells
            // (`end = start + (w - 1)`), so a pair the run writes lies wholly inside the span and
            // both halves stand down together — a non-composed cell can never have a composed
            // partner. If that counting ever changes, this lookup gains a `!partner_composed` term.
            let partner = crate::pair::partner_at(flags, cols, row, col);
            let kind = if composed {
                None
            } else {
                overlay.highlight_at(row, col, partner)
            };
            let is_selection = !composed && overlay.is_selected(row, col, partner);
            // #934: a hovered link draws a single underline where the cell has none of its own. The
            // underline and the glyph field read these flags; the colour stages read `cell_flags`.
            let line_flags =
                if overlay.is_link_hovered(row, col, partner) && cell_flags & UNDERLINE == 0 {
                    cell_flags | UNDERLINE | SINGLE_UNDERLINE
                } else {
                    cell_flags
                };
            // #226: a Powerline / box-drawing / block glyph tiles with the bg — excluded from the
            // contrast demand and re-tinted under selection (classify the base codepoint).
            let exclude =
                treat_glyph_as_background_color(codepoints.get(idx).copied().unwrap_or(0));
            // #120/#393 decorations compose back-to-front around the highlight (justerm-web
            // `composeCellColors`): base < BOTTOM decoration < highlight < TOP decoration. A decoration
            // overrides the bg and/or fg with an **absolute** `0xRRGGBB` (the consumer owns its theme
            // and resolves it before pushing — a decoration is NOT a core colour ref, so it is used
            // verbatim, no palette/inverse/bold→bright). A fg override sets `fg_overridden`, which the
            // #230 re-dim below keys off. `fg` starts from the cell's own fg (undimmed).
            let mut fg = cell_fg;
            let mut fg_overridden = false;
            let mut bg_running = cell_bg;
            // #444: whether a real colour from a bottom decoration now sits beneath the highlight —
            // the selection blend decision reads this too, so the decoration is not erased.
            let mut deco_bg = false;
            // #452: bg and fg merge INDEPENDENTLY across every decoration covering the cell, so a
            // bg-only and an fg-only decoration both apply (xterm's per-property last-wins).
            let bottom = if composed {
                DecorationOverride::default()
            } else {
                decoration_override_at(decorations, row, col, partner, DecorationLayer::Bottom)
            };
            if let Some(c) = bottom.bg {
                bg_running = c;
                deco_bg = true;
            }
            if let Some(c) = bottom.fg {
                fg = c;
                fg_overridden = true;
            }
            // The TOP layer is read here rather than at its composite site below, because whether it
            // takes the glyph decides what the *ink* rules downstream are even for. It is a pure
            // lookup over the rect slice, so reading it early costs nothing and moves nothing.
            let top = if composed {
                DecorationOverride::default()
            } else {
                decoration_override_at(decorations, row, col, partner, DecorationLayer::Top)
            };
            // #508: a bg-only TOP decoration over a background-class glyph drops the glyph (see the
            // composite site). Once it has, the cell's ink channel carries `I_line` and nothing else,
            // and ADR-0019 rule 4 puts that on the TEXT side unconditionally — so every R1 rule below
            // must stand down, and the TEXT-class ones must not.
            let glyph_taken_by_decoration = exclude && top.bg.is_some() && top.fg.is_none();
            // #227 selectionForeground: force a selected cell's fg to the injected colour (never a
            // match), overriding the cell's own / bottom-decoration fg. A tile glyph discards it (#239
            // below). It is selection-only, so it never triggers the #230 re-dim (`!is_selection`).
            if is_selection && let Some(sfg) = policy.selection_fg {
                fg = sfg;
            }
            // #271/#400/#444: composite the selection / search highlight onto the (bottom-decorated)
            // background — the fg policy then sees the EFFECTIVE bg the glyph is drawn over.
            // `should_blend_kind` reads the highlight kind + everything with a real colour beneath it:
            // the PRE-inverse *cell* ref/flags and (#444) whether a bottom decoration painted a bg. A
            // SELECTION blends so what is underneath shows through, and paints solid only over a bare
            // default bg; a search MATCH always paints solid, whatever is beneath (xterm/alacritty
            // parity — a match must read crisp, not a tint).
            let mut eff_bg = composite_bg(
                bg_running,
                kind.is_some_and(|k| should_blend_kind(k, bg_ref, cell_flags, deco_bg)),
                kind.map(|k| overlay.colors.of(k)),
            );
            // #239/#241: a tile glyph under a SELECTION fuses into the band — xterm re-tints it toward
            // the RAW selection colour (not the effective post-blend bg), starting from the cell's own
            // undimmed fg and discarding selectionForeground. #241: an inverse cell with a DEFAULT bg
            // is "treated as transparent" — it contributes no colour of its own, so its fg becomes the
            // band over whatever IS beneath: the raw selection colour with no blend, or (#453) the
            // selection over a bottom decoration's bg when one painted there.
            //
            // The re-tint starts from `cell_fg`, which has bold→bright (#223) applied. That is the
            // model's answer, not an accident: ADR-0019 rule 1 puts `L0` at "the cell after inverse
            // and bold→bright", and rule 4 sends a background-class glyph's ink through the bg fold
            // from there — so `cell_fg` IS the ink source for this cell class.
            //
            // xterm differs for a BOLD + ANSI-0..7 + tile + selection cell, re-tinting from the *base*
            // ANSI colour (its `CellColorResolver` bypasses the `+8`): a corner-of-corner, and
            // sub-perceptible under the 0x80 blend. That difference is **documentation for a consumer
            // porting from xterm, not a defect** — ADR-0019 makes xterm a design input for cell
            // composition rather than a validator. #398 asked for the xterm value and was closed
            // won't-fix on exactly this rule; do not "restore parity" here without amending the ADR.
            // (Its older framing — a *family* change to keep justerm-web byte-neutral — is doubly
            // dead: the widget'"'"'s compositing half went with #504.)
            // #513: the line's ink forks from the glyph's HERE, and only here. Rules 1-3 (the cell's
            // own ink, a bottom decoration's fg, `selectionForeground`) are "what colour is this
            // cell's ink" and reach both; the re-tint below is the one rule about *the glyph* —
            // ADR-0019 R1 — so the line keeps the value as it stands at this point. Rules 5-7 (top
            // decoration, dim, contrast) then run over both, which is why this is a fork rather than
            // a second pipeline.
            //
            // #520: SGR 58 declares the underline's OWN colour, resolved through the palette exactly
            // like fg/bg (a Default/Indexed/Rgb *reference*, not an absolute decoration colour). An
            // explicit colour is **authoritative**: it is drawn RAW and immune to the glyph's ink
            // treatments — top/bottom decoration fg, DIM, and minimum-contrast all leave it alone
            // (the `underline_packed` fork below). This is xterm's rule (`TextureAtlas` sets the
            // underline `strokeStyle` from the raw `getUnderlineColor()` and disables its threshold
            // clear) and, more importantly, the only *coherent* one: the two-lens found that adjusting
            // an "explicit" colour by some rules but not others is an invented asymmetry with no basis
            // in the layer model. `Default` (0) keeps #513's behaviour — the line follows the cell ink
            // (rules 1-3) and rides rules 5-7 with the glyph.
            //
            // #525: that regime is the UNDERLINE's alone, because SGR 58 is the underline's colour and
            // there is no SGR for a strikethrough's. `line_fg` below is the ink BOTH marks start from
            // — the follow-fg base — and the fork past rules 5-7 is where the declared colour reaches
            // one band and not the other.
            //
            // #711: inside a composition the declared colour is not this run's to obey. The pass
            // writes the `UNDERLINE` flag itself (`preedit::writes`), so the mark belongs to the
            // composition — and both grid-drawing references give that mark the run's OWN fg:
            // alacritty literally, as a field beside the glyph's (`renderer/mod.rs:225`,
            // `underline: fg`), ghostty by passing one `screen_fg` into the glyph and into both
            // `addUnderline` calls (`generic.zig:3299`, `:3335`). `SGR 58` spoke for the *covered*
            // cell's underline, which is no longer on screen, so reading it drew the composition in
            // the colour of the text it erased — and in a different colour per column, since the run
            // moves on every keystroke. This is the fifth of D2's stand-downs, joining the four above.
            //
            // Zeroing the ref here **is** that declaration rather than a suppression of one: `0` is
            // `Default`, which means *follow the fg*, and the fg is the one the pass supplied. So the
            // alternative #711 left open — mirroring the column into `preedit_patch` beside bg/fg —
            // writes the same `0` and packs byte-identically; the two shapes are one declaration in
            // two places, not two models. The choice was therefore made on the one axis where they
            // are not equivalent: `webgl.rs` is `#[cfg(target_arch = "wasm32")]` and 0-compiles on
            // host, so a patch-side fix is unreachable by `cargo test --manifest-path
            // justerm-renderer/Cargo.toml`, which is where every test of this behaviour lives. Here
            // it also sits with the other four, so what a composition stands down reads in one place.
            let ucolor_ref = if composed {
                0
            } else {
                underline_colors.get(idx).copied().unwrap_or(0)
            };
            let explicit_line =
                (ucolor_ref != 0).then(|| resolve_indexed_or_rgb(ucolor_ref, palette));
            let mut line_fg = fg;
            if is_selection && exclude && !glyph_taken_by_decoration {
                let raw_sel = overlay.colors.of(HighlightKind::Selection);
                let inverse_default_bg = is_inverse(cell_flags) && (bg_ref >> 24) == 0;
                fg = if inverse_default_bg {
                    // #453: the cell contributes nothing, so the tile shows the band as it falls on
                    // whatever IS beneath — since #444 that includes a bottom decoration's bg. Recompute
                    // the band with the cell taken out of the stack: `bg_running` here is the
                    // decoration's colour when one painted (`deco_bg`), and `composite_bg` then blends;
                    // with no decoration it returns the RAW selection colour, byte-identical to before
                    // and to xterm (`CellColorResolver.ts:139` sets the selection colour flat). NOT
                    // `eff_bg` — that is the band over THIS cell, and for an inverse cell it carries the
                    // cell's own colour (probe: 0x97AFDF vs raw 0x3060C0), which is exactly the colour
                    // "transparent" says to drop.
                    //
                    // The decoration folds in only when the SELECTION is the layer painted over it: a
                    // match paints solid (#400) and erases the decoration from the bg channel, so
                    // blending over it would compose a stack no pixel shows. The fg is selection-keyed
                    // either way (#430) — under an active match it stays the raw selection colour, as
                    // the undecorated sibling already pinned.
                    // The kind literal is the collapsed form of "did the bg channel blend?"
                    // (`deco_bg && kind.is_some_and(|k| should_blend_kind(k, bg_ref, cell_flags,
                    // deco_bg))`): this arm requires `is_inverse`, which makes `should_blend`
                    // unconditionally true, so the two are provably equal here. Valid as long as no
                    // future `HighlightKind` both outranks `Selection` AND blends — such a kind would
                    // blend on the bg channel while this literal kept the fg flat.
                    let beneath_the_band =
                        deco_bg && matches!(kind, Some(HighlightKind::Selection));
                    composite_bg(bg_running, beneath_the_band, Some(raw_sel))
                } else {
                    blend_over(cell_fg, raw_sel, HIGHLIGHT_BLEND_ALPHA)
                };
            }
            // #120/#393 TOP decoration: paints OVER the highlight (foreground-most), overriding the
            // effective bg and/or the fg (`composeCellColors` applies `top` last, after selection).
            // Its bg/fg also merge per-property across the top layer (#452), independently of the
            // bottom layer's merge — xterm runs one accumulating pass per layer. (`top` itself was
            // read above, before the ink rules that need to know whether it takes the glyph.)
            if let Some(c) = top.bg {
                eff_bg = c;
                // #494, a DELIBERATE divergence (the #444 family): a tile glyph FOLLOWS a bg-only top
                // decoration, so the cell paints solid in the decoration's colour instead of the glyph
                // occluding the layer that sits above it.
                //
                // It follows from the rule #495 states for this very classifier: a tile glyph is
                // background-shaped ink, not text. Without it the measured cell is self-contradictory:
                // `bg` is the decoration's while `fg` stays the selection's, i.e. the layer ABOVE the
                // selection loses the glyph area to it.
                //
                // **This diverges from xterm, which is unanimous the other way** (the #494 two-lens
                // corrected an earlier, wrong reading of this — do not restore it). Both of xterm's
                // cell renderers let the glyph paint over a bg-only decoration: the webgl addon
                // (`CellColorResolver.ts:178-187`, applied after the selection stage, `$hasBg` and
                // `$hasFg` independent) and the DOM renderer (`DomRendererRowFactory.ts:357-408`). Its
                // decoration *elements* (`css/xterm.css:194-201`, z-index 6 / 7 over the screen) can
                // cover a glyph, but xterm never styles them — a consumer does, in `onRender`, and they
                // are registered for every renderer (`CoreBrowserTerminal.ts:617`), so they are a
                // separate feature justerm has no equivalent of, not a second xterm answer to
                // this question. That is settled rather than incidental: ADR-0024 R1 states "colours
                // + a mark, not an object" without a condition (#502, 2026-08-18), so no future
                // slice can turn xterm's element path into an answer here. What xterm genuinely leaves undefined is only the *interaction*:
                // `layer` is documented purely against the selection (`typings/xterm.d.ts:688-692`,
                // whose `*` footnote has no text), never against glyphs.
                //
                // The precedent this rule generalises is **weaker than it looks**, and that is stated
                // rather than glossed: xterm's tile re-tint under selection BLENDS 50 % from the cell's
                // own fg (`CellColorResolver.ts:168-171`), leaving the tile distinguishable; it flattens
                // to the band only for an inverse + Default-bg cell it calls *transparent* (`:139`).
                // So "the tile participates in what is painted over it" is xterm's; "the tile is
                // replaced by it" is justerm's own step — taken because a decoration bg REPLACES the
                // background (`eff_bg = c` above) where a selection washes over it, so the tile, being
                // background, gets replaced too.
                //
                // xterm's DOM renderer resolves the same contradiction a THIRD way, and it is rejected
                // knowingly: `DomRendererRowFactory.ts:399-408` applies decorations before selection and
                // then paints the selection only `if (!isTop && isInSelection)`, so a top decoration
                // suppresses the selection colouring outright and the contradictory state never arises.
                // That drops a highlight the user explicitly made, and justerm's fg channel already
                // follows the webgl model deliberately (#430) — mixing in the DOM renderer's ordering
                // here would split that model across two features.
                //
                // #508, FIXED: the decoration used to take the glyph by setting `fg = bg`, which
                // erased everything else the shader draws in the foreground — the UNDERLINE and
                // STRIKETHROUGH lines (`webgl.rs`) and the visible half of a BLINK phase — none of
                // which is the glyph. Rule 4 puts those on the TEXT side unconditionally, so the
                // glyph is now dropped by SLOT instead and the ink channel is left for them. The
                // BLINK half is deliberately *not* restored: a dropped glyph has nothing to blink,
                // which is rule 5 working rather than a residue of this one.
                //
                // Only a **bg-only** decoration means "this whole cell is background now": one that
                // sets `fg` too keeps the art in the consumer's colour (the escape hatch), and BOTTOM
                // is untouched — "bottom" means *under* the glyph, so an opaque tile occluding it is
                // correct (a transparent one lets it through: #453).
                //
                // Two consequences a consumer mirroring xterm's search addon should know (surfaced by
                // the two-lens, tracked on #508): that addon marks the ACTIVE match `layer: 'top'` with
                // a background and every other match `'bottom'` (`addon-search/DecorationManager.ts:
                // 134-144`), so under this rule a box-drawing / Powerline cell loses its glyph on the
                // active hit while keeping it on the others — the glyph blinks as the user cycles. And
                // the escape hatch above is out of reach there: `ISearchDecorationOptions`
                // (`addon-search.d.ts:46-76`) has no foreground field at all. justerm's own active
                // match is an overlay *kind* (#427/#430), not a decoration, so this bites only a
                // consumer that ports xterm's decoration-based model verbatim — which is why this is
                // a note here rather than an issue of its own.
                //
                // Flat, not blended: the selection blends (#239) because it is a translucent wash, but
                // a decoration bg REPLACES (`eff_bg = c` above). The tile takes whatever treatment the
                // bg channel took, so it is replaced too.
                //
                // A DIM cell survives this either way, and the reason is arithmetic, not a flag: both
                // dim paths resolve `dim_foreground(c, c)` — `blend_over(c, c, DIM_BLEND_ALPHA)` adds
                // `round(0)` per channel, so it is exactly identity. (`fg_overridden` is deliberately
                // left alone here, but it is NOT what protects this: a bottom fg-only decoration on the
                // same cell already set it, so the `dim && fg_overridden` arm can run regardless. The
                // two-lens caught that mis-stated reason.) Pinned by
                // `a_dim_tile_following_a_top_decoration_is_not_dimmed_away_from_the_bg`.
                //
                // Route difference to state (#494's AC): justerm's own ACTIVE search match is an
                // overlay kind, and a tile under it keeps the raw selection colour (pinned by
                // `an_inverse_default_bg_tile_on_an_active_matched_selected_cell_uses_the_raw_selection
                // _colour`). The same visual concept expressed as a consumer-pushed bg-only top
                // decoration goes solid instead.
                //
                // Both are intended, and the reason is **ADR-0019 rule 5**: an interaction highlight
                // does not remove content; a declared decoration may. The two layers are the same
                // shape here — above the selection, declaring a bg and no fg — so paint mode cannot
                // tell them apart. **Authorship** can: a decoration is the application saying "this
                // cell is now this colour", knowing what it covered; the active match is the *user*
                // stepping through results. Erasing box-drawing and shading as someone cycles matches
                // is content loss, and the tile is often the only thing drawing a table border or a
                // progress bar. So the asymmetry is the rule, not drift — do not "unify" the routes
                // (that was #511, closed won't-do; the seam it named is real and accepted).
                //
                // The cost lands on a consumer that ports xterm's decoration-based search model, which
                // marks the active match `layer: 'top'` with a background and no foreground: there the
                // tile does go solid, so a box-drawing cell loses its glyph on the active hit while
                // keeping it on the others (#506, closed as not currently real for justerm itself).
            }
            if let Some(c) = top.fg {
                fg = c;
                // #513 rule 5: a decoration declares the cell's INK, not the glyph's specifically, so
                // it reaches the line too — but only a FOLLOW-FG line. #520 makes an explicit SGR 58
                // colour authoritative, so when `explicit_line` is set the `line_packed` short-circuit
                // discards this assignment (the underline keeps its own colour). This write is the
                // follow-fg case's rule 5; it is dead for an explicit line, which is why the fork left
                // it unguarded.
                line_fg = c;
                fg_overridden = true;
            } else if exclude && top.bg.is_some() {
                // #494's assignment lives in the `else` on purpose. Written as its own guarded `if`
                // (`exclude && top.fg.is_some()`) it was **behaviourally dead** — the branch above
                // re-applies `top.fg` immediately after, so the escape hatch held either way and no
                // mutation of the guard could turn a test red (the two-lens caught this: dropping the
                // guard left the whole suite green). As an `else` it is the only thing that keeps a
                // both-channel decoration's art, so `a_top_decoration_setting_both_channels_keeps_the
                // _tile_glyph` now discriminates it.
                //
                // #508: the decoration takes the GLYPH, and it takes it by DROPPING it — not by
                // recolouring the ink to match the background. The visible cell is identical either
                // way, but the ink channel is not: ADR-0019 rule 4 puts `I_line` (underline,
                // strikethrough) and `I_cursor` on the TEXT side unconditionally, and the shader
                // drew the line in `base_fg` until #513 gave it a channel of its own, so
                // an `fg` made equal to `bg` erases the line along with the glyph it was never about.
                // Blanking the slot instead leaves `fg` holding the cell's own ink for the line to
                // use, and states the rule structurally rather than achieving it by a colour
                // coincidence. `glyph_taken_by_decoration` (computed with `top`, above) is what
                // carries it to the glyph field and stands the R1 ink rules down; this arm is now
                // only the statement that a bg-only top decoration does NOT write the ink channel.
            }
            // The fg colour policy, applied ONCE against the effective bg on the UNDIMMED fg — xterm's
            // model (`TextureAtlas._getMinimumContrastColor`), which the renderer can follow because the
            // highlight is already folded into `eff_bg` (beamterm couldn't, so justerm-web double-passes
            // — a compromise the renderer sheds; the #272 2-lens pinned this). minimumContrastRatio
            // (#225) is checked FIRST: if it fires, the corrected fg wins and DIM is skipped (mutually
            // exclusive, xterm `TextureAtlas.ts:329`); a dim cell that already clears the halved ratio is
            // dimmed instead (#232). #226: a tile glyph is EXCLUDED from the contrast demand.
            // #224 selection un-dim: a *selected* cell's DIM is cleared (xterm `& ~BgFlags.DIM`), so its
            // text stays legible over the highlight and the contrast ratio is NOT halved. `dim` folding
            // in `!is_selection` handles both.
            let dim = is_dim(cell_flags) && !is_selection;
            let mcr = policy.min_contrast as f64;
            // #230: a decoration fg override on a dim non-selected cell KEEPS the cell's DIM — xterm
            // leaves `BgFlags.DIM` set, so the resolved override is dimmed too. It is re-dimmed here
            // (before contrast); the base fg's own dim is the `!fg_overridden` arm of the policy below,
            // so exactly one path dims the fg. (composeCellColors #230 → then the contrast pass.)
            if dim && fg_overridden {
                fg = dim_foreground(fg, eff_bg);
                // #513 rule 6: DIM is a property of the CELL, so a dim cell's underline is dim.
                // `fg_overridden` is shared here because a decoration that set the fg set both.
                line_fg = dim_foreground(line_fg, eff_bg);
            }
            // #226's exclusion is about a tiling glyph SEAMING against its neighbour if its colour is
            // nudged. Once a decoration has taken the glyph there is no such glyph, and the ink left
            // in this channel is `I_line` — TEXT class by rule 4 — so the exclusion has no referent
            // and must not reach it. The exclusion is scoped here, not repealed: an undecorated tile
            // still keeps it, which is what the control in
            // `minimum_contrast_reaches_the_line_on_a_taken_tile` pins.
            let fg_packed = if mcr > 1.0 && (!exclude || glyph_taken_by_decoration) {
                let ratio = if dim { mcr / 2.0 } else { mcr };
                match ensure_contrast_ratio(eff_bg, fg, ratio) {
                    Some(adjusted) => adjusted,
                    None if dim && !fg_overridden => dim_foreground(fg, eff_bg),
                    None => fg,
                }
            } else if dim && !fg_overridden {
                dim_foreground(fg, eff_bg)
            } else {
                fg
            };
            // #513 rules 6 and 7 over the line's ink. Same two policies, same `eff_bg`, run again
            // rather than shared: the two inks can start from different colours (that is the point of
            // the channel), so they can need different corrections.
            //
            // The contrast gate is the glyph's, verbatim, and that is a correction of this change's
            // first shape. #226 excludes a tiling glyph because `ensure_contrast_ratio` is a function
            // of `eff_bg`, so two cells of one run over different backgrounds get nudged differently
            // and the run SEAMS. An underline is exactly as continuous across cells as a tile is —
            // dropping the term let a `────` run with `minimumContrastRatio` on change colour at a
            // background boundary, which is the symptom #513 exists to remove, re-entered through
            // contrast. `glyph_taken_by_decoration` keeps #508: there the glyph is gone, the line is
            // the only ink, and a decoration paints one colour across its whole span anyway.
            // Only cells that actually draw a line need this. `line_packed` is a pure function of
            // the same inputs whether or not the attribute bits are set, so skipping it is a cost
            // win with no behaviour change — and it skips a second `ensure_contrast_ratio` luminance
            // loop on every cell in the viewport, which is the bulk of them.
            // #525: the two marks are one ink source split by AUTHORSHIP of the colour. `SGR 58`
            // declares the *underline's* colour and there is no SGR for a strikethrough's, so the
            // declared colour is authoritative over the underline band alone; the strike stays on the
            // follow-fg pipeline whatever the underline does. Computing the pipeline once and forking
            // after it is what keeps the two from drifting apart when nothing declares a colour.
            //
            // The gate is the union of what each band needs, so the #520 cost win survives: an
            // explicitly coloured underline with no strikethrough still skips the contrast loop
            // (`ensure_contrast_ratio`'s luminance work, on every cell in the viewport otherwise).
            let needs_follow_fg = (line_flags & UNDERLINE != 0 && explicit_line.is_none())
                || cell_flags & STRIKETHROUGH != 0;
            let follow_fg_line = if !needs_follow_fg {
                line_fg
            } else if mcr > 1.0 && (!exclude || glyph_taken_by_decoration) {
                let ratio = if dim { mcr / 2.0 } else { mcr };
                match ensure_contrast_ratio(eff_bg, line_fg, ratio) {
                    Some(adjusted) => adjusted,
                    None if dim && !fg_overridden => dim_foreground(line_fg, eff_bg),
                    None => line_fg,
                }
            } else if dim && !fg_overridden {
                dim_foreground(line_fg, eff_bg)
            } else {
                line_fg
            };
            // #520: an explicit SGR 58 colour is authoritative — drawn raw, past dim/contrast and any
            // decoration override (rules 5-7). The fork takes the underline out of the follow-fg
            // pipeline entirely, which is what makes the treatment of an explicit colour uniform (the
            // two-lens verdict) instead of adjusted-by-some-rules.
            let underline_packed = explicit_line.unwrap_or(follow_fg_line);
            // The strike never has a declared colour to be authoritative — `justerm-core` arms
            // `underline_colors` only when UNDERLINE is set (`term.rs::pen_ext_attrs`), so a
            // strike-only cell never carried one even before this split. It is the follow-fg value
            // unconditionally, and on a cell that draws no strike it is unread (same reasoning as the
            // `needs_follow_fg` short-circuit above).
            let strike_packed = follow_fg_line;
            let bg_rgb = gl_rgb(eff_bg);
            let fg_rgb = gl_rgb(fg_packed);

            // A concealed cell points at the blank slot: zero coverage, no decoration bits,
            // so only the (already inverse-swapped) background shows.
            let field = if is_concealed(cell_flags, blink_on) {
                u32::from(BLANK_SLOT)
            } else {
                // #508: a decoration that took the glyph blanks the SLOT and keeps the attribute
                // bits — `ESC[8m` above drops both because the application asked for the whole cell
                // to be hidden; here only the glyph was taken, and an underline is not the glyph.
                //
                // #712: the ink CLASS goes with the glyph, so it is dropped by the same branch.
                // `exclude` is R1's answer for this codepoint, already computed above for the #226
                // contrast exclusion and the #239 re-tint; the shader needs it to order the ink
                // sources, since a background-class glyph's ink joins the background (the underline
                // draws OVER it) while a letter's is TEXT class (the underline draws UNDER it, so a
                // descender is not cut). A taken glyph has no class for the same reason it stands
                // the glyph-only treatments down: the glyph the class is about is gone. Leaving the
                // bit set would be inert — a blank slot has zero coverage — but it would assert
                // something false about the cell, and the five #508 pins say so.
                //
                // A **composed** cell is already right, and only because of how the pass is built:
                // ADR-0028 D2 says every per-cell column owes an answer for one, and this class is
                // derived from `codepoints`, which `preedit_patch` *replaces* — so it describes the
                // preedit's glyph, not the cell underneath. That holds as long as the patch keeps
                // mirroring the codepoint column; a column answered by NEITHER half is the failure
                // `SGR 58` had for one release, and here it would classify a glyph that is no longer
                // on screen — a composition over a `█` would order its underline against the tile.
                let (slot, ink_class) = if glyph_taken_by_decoration {
                    (BLANK_SLOT, false)
                } else {
                    (slots.get(idx).copied().unwrap_or(0), exclude)
                };
                glyph_field(slot, line_flags, ink_class)
            };

            // #455: is this cell's background the pristine DEFAULT backdrop — the one surface #298
            // makes translucent? It is iff NOTHING wrote the bg channel: the ref is Default (tag 0,
            // the file's own predicate, cf. `inverse_default_bg` above), the cell is not inverse (which
            // swaps the fg IN as the bg — content), no decoration painted a bg (bottom or top), and no
            // selection/search/active highlight composited one (`kind`). These are exactly the four
            // sites above that assign the bg channel, so the flag is complete by construction rather
            // than by inference. The shader keys translucency on this instead of comparing the resolved
            // colour to `u_default_bg`, which coincidentally caught any content cell that happened to
            // land on the default RGB (ADR-0019 totality: state, not arithmetic).
            //
            // Both references decide this by provenance too, not by resolved colour — the convergence is
            // the point (ADR-0019: two references agreeing is signal). alacritty's `compute_bg_alpha`
            // (`display/content.rs`) checks `bg == Color::Named(NamedColor::Background)` under the
            // comment *"an RGB color matching the background should not be transparent … computed using
            // the named input color, rather than checking the RGB after its color is computed"* — the
            // #455 bug, guarded at the source. xterm keys on the colour MODE bits (`bg & CM_MASK !=
            // CM_DEFAULT`) and simply draws no background rect for a default cell. The block-cursor cell
            // is the one class neither reaches through these signals; both force it opaque by a dedicated
            // path (alacritty `content.rs` "we must adjust alpha to make it visible"), which here is the
            // shader's separate `!block` term — so it is correctly outside this predicate.
            let bg_is_default_backdrop = (bg_ref >> 24) == 0
                && !is_inverse(cell_flags)
                && !deco_bg
                && top.bg.is_none()
                && kind.is_none();

            out.extend_from_slice(&[
                col as f32,
                row as f32,
                bg_rgb[0],
                bg_rgb[1],
                bg_rgb[2],
                fg_rgb[0],
                fg_rgb[1],
                fg_rgb[2],
                field as f32,
                underline_packed as f32,
                strike_packed as f32,
                if bg_is_default_backdrop { 1.0 } else { 0.0 },
                // `I_neighbour` starts WITHDRAWN and is granted below — the safe default, since a
                // cell with no neighbour and a cell whose neighbour is across a background edge
                // must both end up blank.
                f32::from(BLANK_SLOT),
                f32::from(BLANK_SLOT),
                f32::from(BLANK_SLOT),
                f32::from(BLANK_SLOT),
                0.0,
                0.0,
                0.0,
                0.0,
            ]);
        }
    }

    // #791 / ADR-0019 rule 5: hand each cell its vertical neighbours' ink, and withdraw it where the
    // two cells' **resolved** backgrounds differ. Resolved, not referenced — two cells can hold the
    // identical `Default` reference and still be different colours once a selection covers one of
    // them, and that edge is exactly where crossing ink reads as a fault.
    //
    // Done as a second walk over what was just packed rather than inside the loop above, because the
    // answer needs the *neighbour's* composite and the loop only has its own. Reading it back out of
    // the instance means this shares one resolution with the cell itself — there is no second copy
    // of the bg pipeline here to drift out of step with the first.
    let bg_at = |out: &[f32], i: usize| {
        let b = i * INSTANCE_FLOATS + BG_RGB;
        [out[b], out[b + 1], out[b + 2]]
    };
    // Re-pack the neighbour's resolved foreground into the `0xRRGGBB` float the line inks already
    // use. Exact: these floats were divided by 255 from integers a moment ago.
    let fg_packed_at = |out: &[f32], i: usize| {
        let b = i * INSTANCE_FLOATS + FG_RGB;
        let ch = |v: f32| ((v * 255.0).round() as u32) & 0xFF;
        ((ch(out[b]) << 16) | (ch(out[b + 1]) << 8) | ch(out[b + 2])) as f32
    };
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
            let mine = bg_at(&out, idx);
            if row > 0 {
                let up = idx - cols as usize;
                if bg_at(&out, up) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_UP] =
                        out[up * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_UP_FG] = fg_packed_at(&out, up);
                }
            }
            if row + 1 < rows {
                let dn = idx + cols as usize;
                if bg_at(&out, dn) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_DN] =
                        out[dn * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_DN_FG] = fg_packed_at(&out, dn);
                }
            }
            // The same grant across (#966). A wide pair needs no reconciliation on this axis: each
            // of its outer edges spills into a different receiver, and its inner band is empty by
            // construction (`bitmap::split_wide_bitmap`).
            if col > 0 {
                let lt = idx - 1;
                if bg_at(&out, lt) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_LT] =
                        out[lt * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_LT_FG] = fg_packed_at(&out, lt);
                }
            }
            if col + 1 < cols {
                let rt = idx + 1;
                if bg_at(&out, rt) == mine {
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_RT] =
                        out[rt * INSTANCE_FLOATS + GLYPH_FIELD];
                    out[idx * INSTANCE_FLOATS + NEIGHBOUR_RT_FG] = fg_packed_at(&out, rt);
                }
            }
        }
    }
    // A wide glyph is ONE glyph across two cells, and the grant above is decided per cell — so a
    // background edge under one of the two columns withdraws one half and leaves the other, cutting
    // the pair's overflow down the middle of the letter. That is exactly the failure
    // `docs/map/invariant/a-span-covers-a-wide-pair-whole.md` exists to prevent; a withdrawal gate
    // produces half-covering decisions the same way the ranges that note enumerates do, so it is a
    // fifth producer and the invariant reaches it.
    //
    // Reconciled on the RECEIVERS, because that is where the disagreement lives: the pair is in the
    // source row, and the two cells receiving from it may sit under different backgrounds.
    for row in 0..rows {
        for col in 0..cols {
            let idx = row as usize * cols as usize + col as usize;
            for (field, fg_field, src_row) in [
                (NEIGHBOUR_UP, NEIGHBOUR_UP_FG, row.checked_sub(1)),
                (
                    NEIGHBOUR_DN,
                    NEIGHBOUR_DN_FG,
                    (row + 1 < rows).then_some(row + 1),
                ),
            ] {
                let Some(src) = src_row else { continue };
                let Some(partner_col) = crate::pair::partner_at(flags, cols, src, col) else {
                    continue;
                };
                let mate = row as usize * cols as usize + partner_col as usize;
                let (a, b) = (
                    idx * INSTANCE_FLOATS + field,
                    mate * INSTANCE_FLOATS + field,
                );
                // Both, not just this one. Blanking a single side happens to converge because the
                // partner cell is visited too and reaches the same verdict — measured, a mutation
                // to one-sided blanking still passes — but that makes the result depend on the
                // walk visiting every cell, which is not something this loop should have to promise.
                if out[a] != out[b] {
                    out[a] = f32::from(BLANK_SLOT);
                    out[b] = f32::from(BLANK_SLOT);
                    out[idx * INSTANCE_FLOATS + fg_field] = 0.0;
                    out[mate * INSTANCE_FLOATS + fg_field] = 0.0;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
