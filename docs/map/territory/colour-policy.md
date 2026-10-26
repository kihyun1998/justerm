# Territory — colour policy

## What it is

Turning a cell's colour *reference* into an RGB value against the consumer's injected palette, and
applying the rules that change it before compositing: inverse, bold→bright, dim, conceal, the
selection foreground, the tile-glyph re-tint and the minimum contrast ratio. This is where the
family's theme-agnostic promise is cashed — core never resolves a colour, and this is what resolves
it. On the web side it is also the shape of the `Theme` the consumer hands over, and the one piece of
widget chrome a consumer colours (the scrollbar thumb).

## Governing decisions

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) governs the
  *model* these policies feed — rule 1 puts `L0` at "the cell after inverse and bold→bright", rule 4
  sends a background-class glyph's ink through the bg fold — not the policies themselves.
- [ADR-0017 — mechanism in core, policy in the consumer](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  is why every knob here is a consumer-set value over a renderer-owned mechanism (each setter's doc
  says so).
- **The injection principle is ADR-0002's rule, which ADR-0018 keeps** (`palette.rs`'s module doc,
  since 11f1fbb). ADR-0002's Status line names ADR-0018 as its successor; the principle — the consumer
  owns the scheme and injects it — is live and stated as a boundary invariant in `CLAUDE.md`.

## Design model

Status: the composition model is recorded (ADR-0019); the policies themselves are from source,
tests and module docs only.

- **A colour reference is a tagged `u32`** — high byte the tag (`0` Default, `1` Indexed, `2` Rgb),
  low 24 bits the payload. Three implementations: `justerm_core::encode_color` (the source of truth),
  the wasm decoder's `js/colors.js` (held to it by `decode_color_ref_matches_rust_encoding` in the
  wasm job), and this crate's `palette.rs` + `render_policy.rs`, held by convention — the renderer
  does not depend on core.
- **The consumer injects the scheme; the renderer only resolves.** `Palette::from_colors` takes
  exactly 256 colours — the decoder's `buildPalette` output — and rejects any other length rather than
  padding with black. `resolve_indexed_or_rgb` is deliberately not total: a `Default` reference is
  resolved only by `render_policy`'s `resolve_slot`, because which default it means depends on inverse
  (#470).
- **Policies apply in reference and RGB space, before compositing.** Inverse swaps the slots in
  reference space so bold→bright sees the drawn fg's reference (under bold + inverse it brightens the
  original *background* index, as xterm couples them). Bold→bright promotes `Indexed(0..=7)` to
  `8..=15`. Dim fades the foreground *toward the background* by `DIM_BLEND_ALPHA` = `0x80` (xterm's
  `multiplyOpacity(fg, 0.5)` → 128/255, not exact 0.5).
- **Conceal is one mechanism for two SGR features** (#282). Hidden and blink-off both point the cell
  at `BLANK_SLOT` — zero coverage, no decoration bits — so only the (inverse-swapped) background
  shows. Not alacritty's `fg == bg` model, because a colour emoji samples the texture, not `fg`, and
  would leak (`attrs.rs`). The blink *phase* is the consumer's (`blink_on`).
- **Minimum contrast is a faithful port** of xterm's `ensureContrastRatio` via the deleted
  justerm-web `contrast.ts` (`contrast::ensure_contrast_ratio`): nudge the foreground's luminance
  away from the background in 10 % steps until the WCAG ratio is met, fall back to the other
  direction, keep the better. It is `f64`, separate from `color::contrast` (`f32`, alacritty's cursor
  metric, #368), and pinned byte-exact by vectors computed from the web algorithm. It works on
  packed `0xRRGGBB`, so under `setBgAlpha` < 1 it corrects against the *nominal opaque* background —
  and both references that have the feature do the same (xterm.js shifts the alpha byte off, ghostty
  composites first and still reads `bg.rgb`; pinned below). None of the three can know what is behind
  the window, so this is a limit of the idea, not a gap in the port.
- **The bit positions mirror `justerm_core::CellFlags`** (`attrs.rs`) — the renderer decodes the
  word the engine packed, so a flag added on one side is a silent no-op on the other until both move.
  The underline style's 3-bit field (`USTYLE_SHIFT`) is the recorded duplication: this crate
  re-declares the position because it does not depend on core, and `justerm-wasm-decode`'s
  `underline_style` export exists so no *other* consumer has to repeat it.
- **Input bound:** the palette is exactly 256 entries (rejected otherwise); the contrast ratio and
  cursor contrast are clamped to `[1, 21]`; each contrast nudge loop terminates because every step
  moves a non-saturated channel by at least one unit.

### A live palette swap is a re-pack

`setPalette` replaces the grid's `Palette` and sets `needs_repack`; nothing else is re-pushed. The
palette *values* are the consumer's and re-resolving every retained cell against them is the
renderer's (ADR-0017), so the next `render` re-packs every cell's colour, and the render's clear reads
`default_bg` fresh. Translucency used to need a uniform re-push here too; it no longer does, because
its trigger is the packer's per-cell `bg_default` provenance flag (#455,
[cell compositing](cell-compositing.md)), which does not depend on the palette.

### `Theme` is colours plus the two colour policies

Every other renderer policy knob is a `JustermRendererOptions` field (`justerm-web/src/justerm-renderer.ts`).
The theme contract is renderer-neutral, carried over from the beamterm adapter.

- *A theme is a complete description, not a patch* (#580). `JustermRenderer.setTheme` pushes
  **every** member, so an unset one resets to its default. A field added to `Theme` therefore needs an
  unconditional push in `setTheme` — the opposite of the options' wiring (read once at `create`,
  pushed only when present), close enough to swap by accident. Nothing but a test catches it: the
  wrong shape type-checks and misbehaves only on the *second* `setTheme`. PenTerm honours the
  contract by handing over its whole theme whenever any of it differs (penterm 7dadc3c95
  `src/blocks/terminal/lib/nativePaneView.ts` — `applyNativePaneView`).
- *`DEFAULT_CURSOR_CONTRAST` restates the renderer's default, and `cursorThickness`'s does not.* A
  `Theme` field must reset to a named value, so the web file names one (`1.5`); an option read once at
  `create` can simply not call the setter. The cost is a second copy that can drift — the renderer
  publishes no getter for its default, so nothing checks the two. Out-of-range values are the
  renderer's to clamp and are not re-clamped here.
- *Both contrast knobs are on `Theme`, though both references keep contrast out of their colour
  scheme* — xterm.js's `minimumContrastRatio` is an option, alacritty's `MIN_CURSOR_CONTRAST` is not
  configurable ([cursor policy knobs](../../agents/reference-facts.md#cursor-policy-knobs--where-each-reference-puts-them-580-verified-2026-08-10)).
  This API's own coherence governs: `cursorColor`, which `cursorContrast` defends, is on `Theme`, and
  so is `minimumContrastRatio`. They are separate knobs on purpose: one corrects a cell's *text*
  against its background, the other rescues an *overlay* against the cell it covers.
- *Everything else is an option, and where a reference has the knob, it agrees.* A blink is motion
  (`cursorBlink`); a duration is not a colour (`cursorBlinkTimeout`, `textBlinkInterval`,
  `contextRestoreTimeout`); a thickness is geometry (`cursorThickness`); and `bgAlpha` changes no
  palette entry — xterm.js's `allowTransparency` is an option and alacritty's `opacity` sits under
  `window` (#577). `setTheme` leaves `setBgAlpha` alone for the same reason.
- *Why each knob is shaped as it is* (each attributed to its issue; the xterm.js line references are
  from those issues, unpinned). `cursorBlinkTimeout` is exposed because the references disagree by
  60x (#593). `textBlinkInterval` defaults to off and is an interval rather than a boolean because
  only xterm.js animates blinking text, defaulting to `0` (#576). `contextRestoreTimeout` is exposed
  because the renderer declares it consumer policy (`context_loss.rs` `DEFAULT_RESTORE_TIMEOUT_MS`),
  and epic #583 settled that every knob the renderer declares consumer policy is reachable through
  the widget. `activeMatchBg` defaults to a dark orange (`0x995200`), warmer than the selection and
  the other matches — the Chrome find-in-page model, with alacritty's `focused_match` gold agreeing.
  `cursorThickness` is alacritty's fraction rule (default `0.15`, chosen at #270 over xterm.js's
  `cursorWidth` in CSS px, which gives a 32px font the same hairline as a 12px one; ADR-0023 does not
  apply, a fraction carrying no unit). `onContextLoss` fires at most once per loss and never after
  dispose, matching xterm.js.

### Where the policies meet the highlight

All in `frame.rs` `pack_instances`, per cell.

- **The fg channel is keyed on selection coverage, not on the winning highlight** (#430, xterm's
  model). The bg composite keys off `Overlay::highlight_at`; the selection-only fg rules (#224, #227,
  #239) key off `Overlay::is_selected`, so they survive on a cell whose bg the active match outranks.
  `selectionForeground` (#227) forces a selected cell's fg to the injected colour — never a match's —
  over the cell's own or a bottom decoration's fg; a tile glyph discards it, and being selection-only
  it never triggers the #230 re-dim.
- **A tile glyph under a selection fuses into the band** (#239, #241). It is re-tinted toward the
  *raw* selection colour from the cell's own undimmed fg, discarding `selectionForeground`. An inverse
  cell with a Default bg is "treated as transparent" (#241): its fg becomes the band over whatever is
  beneath — the raw selection colour, or (#453) the selection over a bottom decoration's bg when one
  painted. That band is recomputed with the cell taken out of the stack, **not** read from `eff_bg`,
  which for an inverse cell carries the cell's own colour (probe `0x97AFDF` against the raw
  `0x3060C0`, `frame/tests.rs`). The decoration folds in only when the *selection* is the layer over
  it: a match paints solid (#400). The code tests `deco_bg && kind == Selection` as the collapsed form
  of "did the bg channel blend" — equal here because this arm requires inverse, which makes the blend
  unconditional; valid as long as no future `HighlightKind` both outranks `Selection` and blends.
- **The re-tint starts from `cell_fg`, after bold→bright** (#223), which is ADR-0019 rule 1's `L0`.
  xterm re-tints a bold + ANSI 0–7 + tile + selection cell from the *base* ANSI colour — a
  sub-perceptible corner under the `0x80` blend. #398 asked for the xterm value and was closed
  won't-fix on that rule; do not "restore parity" without amending the ADR.
- **The fg policy applies once, against the effective bg, on the undimmed fg** — xterm's model
  (`TextureAtlas._getMinimumContrastColor`), which the renderer can follow because the highlight is
  already folded into `eff_bg`. Minimum contrast (#225) is checked first: if it fires the corrected fg
  wins and DIM is skipped; a dim cell that already clears the *halved* ratio is dimmed instead (#232).
  A selected cell's DIM is cleared (#224). A decoration fg on a dim, unselected cell keeps the DIM
  (#230) and is re-dimmed before contrast; the base fg's dim is the `!fg_overridden` arm, so exactly
  one path dims. A dim cell's underline is dim too (#513 rule 6).
- **A tile glyph is excluded from the contrast demand, and the exclusion is scoped** (#226).
  `ensure_contrast_ratio` is a function of `eff_bg`, so two cells of one tiling run over different
  backgrounds would be nudged differently and the run would seam. Once a bg-only top decoration has
  taken the glyph (#508) there is no tile left and the exclusion must not reach the remaining line ink
  (`minimum_contrast_reaches_the_line_on_a_taken_tile`). The line inks run the two policies again with
  the glyph's contrast gate verbatim: an underline is as continuous across cells as a tile is
  (correcting #513's first shape). Where the line's ink forks: [cell compositing](cell-compositing.md).
- **What counts as a tile** is `glyph_class::treat_glyph_as_background_color`: xterm.js's Powerline +
  box/block list, unioned since #507 with what `builtin::owns` draws
  ([built-in block glyphs](builtin-block-glyphs.md)); a combined cell classifies from its base (#495,
  a declared divergence from xterm's two disagreeing call sites).

### Widget chrome takes its colour through the CSS cascade, not the palette

(#926.) The web scrollbar thumb's inline `background-color` is a `var()` chain —
`--justerm-scrollbar-thumb-active` → `-hover` → `--justerm-scrollbar-thumb` → `rgba(255,255,255,0.25)`
(`thumbBackground`) — so a consumer that sets nothing sees no change and one that sets only the rest
colour gets it in every state. The longhand, not the `background` shorthand: the shorthand resets
`background-clip` and the rest inline, which a stylesheet reaching the thumb by
`SCROLLBAR_THUMB_ATTRIBUTE` could not undo. PenTerm sets the rest and hover properties
(penterm 7dadc3c95 `src/global.css`).

- **No `<style>` is injected, and that is the point.** xterm.js injects one per `Terminal` with an
  unscoped selector (`browser/Viewport.ts`, the `_styleElement` block @ 699f553), so with N terminals
  on a page the last one created paints every pane's slider — PenTerm measured that (#926). A custom
  property inherits from the pane it is set on, so each pane follows its own scheme.
- **The widget holds the state, because an inline style cannot say `:hover`.** `thumbState` is
  `active` for as long as a drag the thumb started is held, wherever the pointer is; xterm.js holds
  `xterm-active` the same way.
- **A drag ends on a buttonless move as well as on `mouseup`**, and starts on the primary button only
  (`startsThumbDrag`, `dragStillHeld`). Before, a release the page never received — a context menu
  opened on press, a window switch mid-drag — left a visibly stuck active thumb. xterm.js ends its drag
  on a move whose `buttons` no longer match. It does not undo #814's inert drag through a hidden pane:
  that pointer still has its button down.

## Code

- `justerm-renderer/src/palette.rs` — `Palette`, `Palette::from_colors`, `resolve_indexed_or_rgb`
- `justerm-renderer/src/render_policy.rs` — `ColorPolicy`, `resolve_cell`, `resolve_slot`,
  `dim_foreground`, `DIM_BLEND_ALPHA`
- `justerm-renderer/src/contrast.rs` — `ensure_contrast_ratio`
- `justerm-renderer/src/color.rs` — `gl_rgb`, `contrast` (the cursor metric)
- `justerm-renderer/src/attrs.rs` — the `CellFlags` mirror, `is_inverse`, `is_dim`, `is_concealed`,
  `glyph_field`, `BLANK_SLOT`, `USTYLE_SHIFT`
- `justerm-renderer/src/glyph_class.rs` — `treat_glyph_as_background_color`
- `justerm-renderer/src/frame.rs` — `pack_instances` (where the policies meet the highlight)
- `justerm-renderer/src/webgl/grid_state.rs` — `set_palette`, `set_bg_alpha`, `set_bold_to_bright`,
  `set_minimum_contrast_ratio`, `set_selection_foreground`, `set_cursor_contrast`
- `justerm-web/src/justerm-renderer.ts` — `Theme`, `JustermRendererOptions`, `setTheme`,
  `DEFAULT_CURSOR_CONTRAST`
- `justerm-web/src/scrollbar.ts` — `thumbBackground`, `thumbState`, `SCROLLBAR_THUMB_ATTRIBUTE`,
  `startsThumbDrag`, `dragStillHeld`
- `justerm-wasm-decode/js/colors.js` — `resolveRgb`, the JS mirror of the encoding

## Reference behaviour

- [Background transparency — the shape of the knob](../../agents/reference-facts.md#background-transparency--the-shape-of-the-knob-577-verified-2026-07-29)
  — where each reference puts the alpha knob, and that xterm.js and ghostty both compute minimum
  contrast ignoring the background's alpha, by two routes.
- [Cursor policy knobs](../../agents/reference-facts.md#cursor-policy-knobs--where-each-reference-puts-them-580-verified-2026-08-10)
  — where each reference puts the contrast and thickness knobs (the `Theme` placement above).
- [Text blink — SGR 5](../../agents/reference-facts.md#text-blink--sgr-5-576-verified-2026-07-29)
  — who animates blinking text, the reason `textBlinkInterval` defaults to off.
- **Unpinned:** the port claim itself — nothing checks `ensure_contrast_ratio`'s step behaviour
  against xterm's source (its vectors pin it to the deleted `contrast.ts`). And the glyph-class
  exception is xterm.js's `treatGlyphAsBackgroundColor` (`src/browser/renderer/shared/RendererUtils.ts:63`),
  passed down as `excludeFromContrastRatioDemands` (`addons/addon-webgl/src/TextureAtlas.ts:423-424`)
  and painted in the selection background under a selection (`CellColorResolver.ts:132-139`) — cited
  in `glyph_class.rs`, not a `reference-facts.md` row.
- **Provenance (#504):** these modules cite justerm-web siblings they were ported from —
  `render-policy.ts`, `render-core.ts`, `glyph-class.ts`, `contrast.ts` — which no longer exist; the
  widget's compositing half went when the renderer took it over (#273). The module docs say so.

## Cross-cutting invariants

- [the renderer's copies of core encodings are held by convention](../invariant/renderer-encodings-are-held-by-convention.md)
  — `resolve_slot` / `resolve_indexed_or_rgb` re-declare `encode_color`'s tag and `attrs.rs` the flag bits, with no dependency and no parity test

## Blast radius

- [cell compositing](cell-compositing.md) — every policy here runs inside the same per-cell pass as
  the layer stack; the two share `eff_bg`, so a policy that reads the bg before the highlight folds
  in corrects against a colour no pixel shows.
- [wire format](wire-format.md) — the tagged-`u32` encoding is implemented three times; a new tag
  or payload layout is a three-site change with only one parity test.
- [pen](pen.md) — core writes the references and `CellFlags` bits this resolves; a new attribute bit
  is a no-op here until `attrs.rs` mirrors it.
- [selection](selection.md) · [active match](active-match.md) — the fg channel keys on selection
  coverage while the bg keys on the winner; a new highlight kind must say which channel it joins.
- [decoration](decoration.md) — a decoration's fg/bg override enters the same dim and contrast
  passes; changing their order changes what a decorated dim cell looks like.
- [caret drawing](caret-drawing.md) — the cursor contrast guard uses the resolved per-cell RGB this
  territory produces; a palette swap must reach the caret in the same pack.
- [built-in block glyphs](builtin-block-glyphs.md) — the tile class is asked of `builtin::owns`; a
  range added there is exempted from contrast here automatically.

## Known holes / open

- **A "faithful port" with no pinned row for the port itself.** `ensure_contrast_ratio` is pinned
  byte-exact to the deleted web port, not to xterm's source.
- **The renderer's copy of the colour encoding has no parity test.** Core and `colors.js` are held by
  one; `palette.rs`/`render_policy.rs` only by convention (the renderer does not depend on core), and
  only the wire version would announce a change.
- **`DEFAULT_CURSOR_CONTRAST` can drift from the renderer's default** — the renderer exposes no
  getter, so nothing compares them.
- **`unicode-width` is pinned twice** — `justerm-core` and `justerm-renderer` each declare `"0.2"` in
  separate lockfiles (both resolve 0.2.2 at 7bb31f7), and nothing checks they stay equal; see
  [renderer encodings are held by convention](../invariant/renderer-encodings-are-held-by-convention.md).

