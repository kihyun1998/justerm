//! Per-grid state — damage, the overlays, the palette, the caret, the preedit and suggestion runs, and the colour and cursor policies.

use crate::glyph_resolve::Cells;
use crate::preedit::{Patch as PreeditPatch, Span as PreeditSpan};
use wasm_bindgen::prelude::*;

use super::JustermRenderer;

#[wasm_bindgen]
impl JustermRenderer {
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

    /// Re-resolve [`GridTier::cursor_cells`](super::GridTier::cursor_cells) against the last frame's flags. Called when a frame arrives
    /// (its flags may have changed under a still cursor) *and* when the cursor moves (onto either
    /// half of a wide char, with no new frame).
    pub(super) fn resolve_cursor_cells(&mut self, at: usize) {
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
    pub(super) fn preedit_span(&self, at: usize, cols: u32, rows: u32) -> Option<PreeditSpan> {
        self.grid_at(at).preedit_span(cols, rows)
    }

    pub(super) fn preedit_patch(
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
}
