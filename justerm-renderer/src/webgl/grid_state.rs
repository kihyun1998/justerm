//! Per-grid state — damage, the overlays, the palette, the caret, the preedit and suggestion runs, and the colour and cursor policies.

use wasm_bindgen::prelude::*;

use super::JustermRenderer;

#[wasm_bindgen]
impl JustermRenderer {
    /// Consume a decoded **damage** frame directly (the damage adapter): scatter its span-ordered
    /// cells into the persistent grid, then resolve + pack the full viewport. A Full frame wipes
    /// the grid first, a scroll op shifts it before spans. Grapheme clusters ride the `extra` column
    /// + `side_table` and are resolved to text at scatter (the index is frame-local).
    ///
    /// `header` carries the frame's scalars, `[cols, rows, kind, has_scroll, scroll_top,
    /// scroll_bottom, scroll_count, blink_on]` (kind `0` = Full / `1` = Partial; `scroll_count`
    /// reinterpreted as `i16`; `blink_on` `0`/`1`). `spans` is the span directory
    /// (`SPAN_STRIDE` `u32`s each);
    /// `codepoints`/`fg`/`bg`/`flags`/`extra` are the span-ordered cell columns.
    // Eight column views at the wasm-bindgen boundary, one argument each
    // (`docs/map/territory/frame-adapter.md` § The damage entry point's arguments).
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
        // Optional and trailing; omitted ⇒ all Default.
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
    /// **A composition reaches no frame and no wire**, so the consumer is the only source and must
    /// re-push on every `compositionupdate` ([ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md)).
    /// An update whose data is unchanged may be skipped.
    ///
    /// The run may extend past the anchor's row end: it shifts left to stay whole rather than
    /// clipping. Width is per codepoint, the same `unicode-width` answer the engine gives, so a VS16
    /// emoji measures narrow here exactly as it does there.
    ///
    /// Returns the column the **caret and the IME anchor** belong at: one past the run's last cell,
    /// clamped to the grid — not `col + len`, because the run shifts left at the right edge. Feed it
    /// to `setCursor` (the caret rides the composition's end, while DECTCEM still decides whether it
    /// is drawn at all) and to the hidden textarea. Why: [`docs/map/territory/input-encoding.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/input-encoding.md)
    /// § The preedit run's caret column.
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
    /// constructor takes); `default_fg`/`default_bg` the theme's defaults. The palette *values* are
    /// the consumer's; re-resolving every retained cell against them is the renderer's.
    ///
    /// Marks the buffer dirty so the next `render` re-packs, re-resolving every cell's colour, and
    /// the change shows with no new frame — like `setOverlay` (a no-op until the first
    /// `apply_damage`; the direct `apply_frame` path reflects the new palette on its next call).
    /// Why nothing else is re-pushed: [`docs/map/territory/colour-policy.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/colour-policy.md)
    /// § A live palette swap is a re-pack.
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
    /// highlights the wrong cells until the next call; the renderer never refreshes them itself.
    /// Why: [`docs/map/territory/cell-compositing.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-compositing.md)
    /// § The highlight spans are consumer-pushed.
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
    /// `blink_on` is — call `clearCursor` for the off phase. Why a block is not an instance:
    /// [`docs/map/territory/caret-drawing.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/caret-drawing.md)
    /// § Every shape lives in one uniform.
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

    /// Set the background cell opacity: `0` = fully transparent, `1` = opaque (default). The
    /// consumer injects this policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) to make the terminal background see-through to the
    /// page/desktop behind the canvas, while glyph pixels stay opaque. Clamped to `[0, 1]`; takes
    /// effect on the next `render`.
    ///
    /// A translucent background contributes to a cell's colour in proportion to how translucent it
    /// is, so at `0` an antialiased glyph edge shows no background colour at all.
    ///
    /// **A non-finite value falls back to `1.0` (opaque).** Why, and what `NaN` did before the
    /// guard: [`docs/map/territory/cell-compositing.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-compositing.md)
    /// § `setBgAlpha`'s two fixes.
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
    /// bar, an underline, or a hollow block's outline, in device pixels as
    /// `(frac * cell_w).round().max(1)` — so it tracks both dpr and font size, alacritty's rule. A
    /// **block** ignores it — a block recolours its cell and draws no stroke.
    ///
    /// Default `0.15` (alacritty's `cursor.thickness`); clamped to `[0, 1]` (alacritty's
    /// `Percentage`). Even `0`, and `NaN`, leave a one-pixel stroke rather than an invisible
    /// cursor. Takes effect on the next `render` — like a stroke's shape, it is a shader uniform, so
    /// changing it costs no upload. Why: [`docs/map/territory/caret-drawing.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/caret-drawing.md)
    /// § The stroke thickness is a clamped fraction of the cell.
    #[wasm_bindgen(js_name = setCursorThickness)]
    pub fn set_cursor_thickness(&mut self, grid: u32, frac: f32) -> Result<(), JsValue> {
        let at = self.slot(grid)?;
        self.grid_at_mut(at).set_cursor_thickness(frac);
        Ok(())
    }
}
