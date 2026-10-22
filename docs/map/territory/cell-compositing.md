# Territory — cell compositing

## What it is

Turning one decoded cell into the numbers a GPU draws, and those numbers into pixels: resolve its
colour references and policies ([colour policy](colour-policy.md)), composite every layer that claims
the same pixel (decorations, the selection/search highlight, a neighbour's spilled ink, the block
cursor), fold the underline and strikethrough into the glyph field, pack a flat per-cell instance
buffer for **one** instanced draw, and run the fragment chain that composites ink over a possibly
translucent background. It also holds the two *passes* that supply glyphs the engine never wrote —
the IME preedit and the consumer's suggestion run — because both work by taking cells out of this
stack. It is the renderer's hot path and the first place in the family where a colour reference
becomes an actual colour.

## Governing decisions

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) — a layered,
  per-channel, **total** resolution: a combination is answered by construction, and one it cannot
  answer is an amendment, not a new decision. Its amendments carry R1.1 (#317 §2), R1.2 and rule 5
  (#791), rule 4's ink sources and classes (#525, #712), the preedit pass (#249) and the suggestion
  pass (#972) — the dates are on the record.
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the "A-ii"
  hot-path-in-wasm decision the crate exists to execute.
- [ADR-0024 — decoration projection and precedence](../../adr/0024-decoration-projection-and-precedence.md)
  — the axis ADR-0019 put out of its own scope; what arrives at the Bottom/Top layers.
- [ADR-0028 — composition surfaces have one writer each](../../adr/0028-composition-surfaces-have-one-writer-each.md)
  — D2 makes the preedit a pass that removes its cells from the stack; D4 keeps the suggestion a
  separate writer sharing nothing but per-codepoint width.
- [ADR-0033 — subpixel coverage](../../adr/0033-subpixel-coverage-is-one-mask-and-a-measured-curve.md)
  — what a text glyph's coverage is when it is three numbers.

## Design model

Status: the model, the passes and the subpixel layout are recorded (ADR-0019, ADR-0028, ADR-0033);
the shader's mark geometry and most packer details are from source, tests and measurements on issues.

- **Back-to-front, and decorations sit on *both* sides of the highlight:**
  `base < bottom-decoration < highlight < top-decoration`. A decoration chooses a side of the
  selection/search layer — that is what `DecorationLayer::{Bottom, Top}` means.
- **Per-channel, not per-layer.** A layer may claim the background and leave the foreground alone,
  which is what makes "which wins" answerable for an active match over a selection without either
  winning outright. Colour policies run before the layers and transform the cell's own colours
  ([colour policy](colour-policy.md) § Where the policies meet the highlight).
- **Which ink source ends up on top is a question about CLASS, not draw order** (#712, ADR-0019
  rule 6) — with one source whose position is not class-derived: `I_neighbour` sits above the
  receiver's own tile and below everything else the receiver owns, whatever its owner's class,
  because the thing prevented is one row's letter amputating the next row's. Background-channel ink
  cannot occlude a `TEXT`-class source, so an underline draws *over* a tile and *under* a letter; the
  glyph field carries the class (bit 16, `GLYPH_BG_CLASS`) because the shader sees a slot, never a
  codepoint. This is the one place a background-class glyph's ink is *ordered*, and the question is
  invisible unless something declared a colour — with equal inks both orders give the same bytes,
  which is why it survived until `SGR 58`.
- **The two marks are one ink source split by authorship of the colour** (#525, rule 4). They share
  the follow-fg pipeline and separate only where something *declared* a colour: `SGR 58` declares the
  underline's, and no SGR declares a strikethrough's. Without `SGR 58` both inks are equal. Underline
  and strikethrough fold into the glyph field, and their ink is a channel separate from the
  foreground (#513), so a coloured underline needs no second draw.
- **Alpha is a channel of the composite, not a postscript** (#317 §2, R1.1). No GL blending is
  enabled, so the fragment *is* the composite: ink accumulates premultiplied from nothing, the
  background's surviving weight `w_bg` is the product of every ink source's complement, and the two
  recombine against `u_bg_alpha` — `a = 1 - w_bg(1 - A)`, `rgb = (ink + bg·A·w_bg) / a`. The failure
  this replaced — colour composed against a fully present background while alpha declared it mostly
  absent — is ADR-0019's Coherence clause one axis over. Invisible at `A = 1`.
- **Text coverage may be three numbers** (#961, ADR-0033). In a subpixel configuration a text-class
  glyph's coverage is the slot's RGB mask, per channel of the ink — as-is where that channel is at or
  above 0.75, raised to `u_lcd_gamma` below — so `w_bg` is a `vec3`; `I_neighbour` reads its owner's
  mask the same way. `a` reads one channel of `w_bg`, which is exact: over a translucent default
  background coverage stays scalar, and over an opaque one `A = 1`. Background-class ink and colour
  emoji stay scalar.
- **A cell's ink can come from the cell above, below or beside it** (#791 vertical, #966 across,
  ADR-0019 R1.2). The slot carries a bleed band on every side ([cell geometry](cell-geometry.md)), and
  the receiving fragment reads the four adjacent slots and folds the strongest one into the rule-6
  chain. Reader-side, deliberately: the quad stays one cell, the composite one evaluation per pixel,
  and no GL blending — the writer-side shape every reference uses would force a premultiplied buffer
  and put foreign-vs-own occlusion back under instance order
  ([whether a glyph's ink may leave its cell](../../agents/reference-facts.md#whether-a-glyphs-ink-may-leave-its-cell--30-and-the-single-guard-that-withdraws-it-791-verified-2026-08-20)).
- **Foreign ink is withdrawn where the two cells' *resolved* backgrounds differ** (rule 5). Two cells
  can both hold `Default` and differ once a selection covers one. Two of the three producers of that
  difference are invisible to the packer, so the rule is enforced in two more places: the **block
  cursor** is a per-fragment background and is withdrawn in the shader, and a **wide pair** is one
  glyph whose receivers must reach the same verdict.
- **A colour emoji's overflow keeps the atlas's colours, and since #794 something proves it.** The
  shader mixes `tex.rgb` under the *owner's* emoji bit (R1.2: foreign ink keeps its owner's ink).
  `demo/neighbour-ink.html`'s probe comes from a 2D-canvas `fillText`, which is not the path a wide
  glyph takes; taken from the renderer's own bake, a colour emoji on the **wide** path deposits
  nothing on its neighbour while the same codepoint on the **narrow** path spills 6 device px at dpr 1
  and 10 at dpr 2 (#794). So `demo/emoji-neighbour-ink.html` uses the narrow path and states the claim
  as two hypotheses — the overflow's mean colour is nearer the owner's ink than the foreground every
  cell wears — because the defect produced that foreground exactly.
- **The packer is pure and host-testable** (`frame.rs`); slot resolution and rasterisation are
  stateful and browser-only, and the packer takes already-resolved slots. Since #989 `shader.rs`
  compiles on the host too.
- **Every pack covers the whole viewport; not every render packs.** There is no incremental repaint
  — a pack re-resolves every cell, and the #263 upload diff re-sends only changed instances, which is
  why the engine's damage model targets the *wire* rather than this renderer. But a `render` re-packs
  a grid only when it is marked dirty (`needs_repack`, #421) or its atlas evicted under it (#772); an
  idle grid keeps its instances.
- **Input bound:** a frame is bounded by the data it carries (`resolve_frame` refuses a grid its
  slices cannot back, #355); the instance reservation is `checked_mul`ed because `usize` is 32 bits on
  wasm32. Highlight and decoration spans are flat `u32` lists the consumer pushes, re-issued each
  frame, and an out-of-range span highlights nothing. A suggestion run is bounded by the row's width
  (it clips at the right edge).

### The highlight spans are consumer-pushed

`setOverlay` and `setActiveMatch` take viewport-relative spans the decoder re-projects every frame,
so the consumer re-issues them on every viewport or selection change, as it re-issues the cursor.
beamterm's spans ride each decoded frame; this renderer is not handed them with the frame, so it
cannot refresh them itself — the split mirrors the cursor's, and the widget wires both. A stale
in-range span highlights the wrong cells until the next call.

### `setBgAlpha`'s two fixes

- *A translucent background contributes in proportion to how translucent it is* (2026-08-18, #317 §2).
  It used to contribute in full: at `0` a half-covered glyph pixel came out half background — a
  colour the caller asked to be absent. At `1` the two forms agree, which is why it shipped.
- *A non-finite alpha falls back to `1.0`* (`GridTier::set_bg_alpha`). `f32::clamp` passes `NaN`
  through, and `NaN` reached every fragment's alpha, so glyph pixels went transparent too and the
  terminal vanished with no error — measured at boot: `[30,30,46,0]` on a background cell and
  `[205,214,244,0]` inside a glyph, against alpha 128/255 for a valid `0.5`. The guard sits in the
  renderer because the input is reachable from type-correct consumer code (`Number(configValue)` is a
  `number`). Finite out-of-range values were never the problem (`-3` and `9` measured fully
  transparent and opaque). Not every float setter has the guard: `set_cursor_contrast` is a bare clamp
  (a `NaN` there disables the guard, as `1.0` does), and `set_cursor_thickness`'s `NaN` is handled a
  layer deeper ([caret drawing](caret-drawing.md)).

### An IME preedit is not a layer in this stack

It is a *pass* that removes cells from it (ADR-0019's 2026-08-03 amendment, #249, ADR-0028 D2).
Nothing in the stack can *supply* a glyph — every layer recolours a channel or blanks a slot — and
rule 5's authorship axis has no value for content the browser owns and the application never
declared. So the composed cells leave the stack at resolve time and come back with bg, fg and glyph
together (`preedit::patch`, `preedit_patch` in `webgl/draw.rs`), and `pack_instances` stands every stage
below glyph resolution down inside the run (`Frame::preedit`). Replacing only the resolver's inputs
was measured not to be enough: a selection covering the run still tinted it. The run draws in the
terminal's default fg over its default bg — ghostty's choice
([drawing the preedit](../../agents/reference-facts.md#drawing-the-preedit-two-draw-into-the-grid-one-draws-a-dom-box-249-verified-2026-08-03)).

- **Every per-cell column owes an answer for a composed cell; which half gives it is free**
  (ADR-0028 D2, #711). Five columns are re-supplied by the patch and `underline_colors` is stood down
  in the packer — a gate artifact rather than a rule, since `0` already means *follow the fg the pass
  supplied*. What bites is a column answered by **neither**: `SGR 58` was, for one published release,
  so a composition drew its underline in the colour of the text it had erased. The obligation is on
  whoever writes a pass, not only whoever adds a column.
- **The mirror hazard is a *cell* answered by the wrong half** (#715, same release). The pass also
  writes the pair-repair beside its run — a cell it un-pairs but does not take — and the patch
  re-supplied that cell's colours too, so the application's background vanished while composing. The
  rule both halves now share: a repair blanks the glyph, never the pen (`WriteKind`).

### The consumer's suggestion is a second glyph-supplying pass

It inverts the preedit's answers (#972, ADR-0019's 2026-09-28 amendment). It shares only the
per-codepoint width with the preedit and no state (ADR-0028 D4): the preedit is browser state the
engine cannot know, the suggestion hangs off the engine's cursor. It is renderer state only — no
engine cell holds it, so it never reaches copy or search (`suggestion.rs` module doc).

- **Anchor: the engine cursor, re-sent by justerm-web on every frame**, mapped through the display
  offset and cleared off screen (`Terminal.paintSuggestion`). Not the renderer's retained caret —
  cleared on every blink-off phase and under DECTCEM, so the pass would blink — and not a latched
  point, which drifts on output scroll and resize. The cost is one frame at the wrong cell when output
  moves the cursor before the consumer clears it.
- **Geometry: keep the head, clip on the right.** `preedit::range` shifts left to keep its tail, which
  here would overdraw the text typed. A wide codepoint that does not fit is dropped whole and ends the
  run. No underline.
- **"Blank" means *draws nothing but its background*** (`suggestion::is_blank`): a space or empty
  codepoint, no grapheme, not half of a pair, and none of `INVERSE`, any underline or strikethrough —
  each puts ink on an empty cell. This also covers pending wrap, which the wire does not carry: at the
  right margin the cursor cell still holds the character just typed. The codepoint half is
  [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md): an NBSP or U+3000 was
  printed by the application, so it stops the run.
- **Withheld, not stood down.** A cell under a highlight, either decoration layer or the hovered link
  keeps the engine's cell (`suggestion_patch` in `webgl.rs`), and a wide suggestion glyph with either
  half withheld is skipped whole; the run still counts that cell. Link hover belongs here because the
  suggestion's flags carry no underline, so a hover would draw one in the covered cell's `SGR 58`
  colour.
- **Colour: a tagged reference, not RGB.** A Default or Indexed colour re-resolves on `setTheme` with
  no push from the consumer. `DIM` is the 50 % blend toward the cell's background, so over a coloured
  row it reads as a mix — measured `[128,0,127]` for red over blue in `demo/suggestion.html`. On the
  web surface the colour is a union (`"default" | { indexed } | { rgb }`, `SuggestionColor`) encoded
  by `suggestionColorRef`, because every other widget colour is a plain `0xRRGGBB` and a bare number
  here would read as RGB and silently draw as Default.
- **Composition: hidden from `compositionstart` to `compositionend`, by the widget**
  (`suggestionCell` keys on `composing`). The renderer also yields to a drawn preedit, but that alone
  misses the start of a composition and an update to `""`; the consumer cannot do it, since keys an IME
  owns never reach its key hook.
- **Cost: the patch borrows the clusters and backgrounds.** A written cell has no cluster (a cell with
  one stops the run) and keeps its background, so only codepoints, flags and fg are copied — measured
  on a 200×60 grid, release host build: 44 µs per pack against `pack_instances`' ~620 µs, down from
  ~180 µs copying all five columns (#972). The copy runs on every pack while a suggestion is set.
- **Alternate screen: dropped by justerm-web on entry and refused while there** (`Terminal.setSuggestion`
  returns `false`). The renderer's damage header carries no alt flag, and masking would bring a stale
  suggestion back after `:q`, because the consumer's ranker answers asynchronously after Enter.

### The fragment stage (`shader.rs` `FRAG_SRC`), step by step

- **The instance's inks travel as floats, exactly.** `a_underline_fg` / `a_strike_fg` carry
  `0xRRGGBB` one per float (#513, split by #525); a colour is below 2^24, so an `f32` carries it —
  measured with a standalone WebGL2 probe. The vertex stage unpacks them once per instance. The glyph
  field is read as `uint(a_glyph)`, which is why its ink class (bit 16, #712) and underline style
  (bits 17..19, #829) may sit above the `u16` the rest fits in. `a_bg_default` is provenance packed by
  the Rust side, not re-inferred from the colour (#455).
- **`I_neighbour` in the fragment** (#791, #966). A slot holds `bleed | cell | bleed` on each axis,
  so the band that spilled toward this cell sits exactly one *cell* from where its own texel reads —
  `± u_cell_uv.w` for rows, `± u_cell_uv.z` across — with no padding arithmetic. `metrics::ink_rows`
  states the same mapping in device px and is where it is tested. The four neighbours are sampled
  **unconditionally and masked**: an implicit-LOD fetch under non-uniform control flow is undefined in
  GLSL ES 3.00, which is also why `slot_texel` passes an explicit LOD. One neighbour supplies a
  fragment's foreign ink — the one laying down the most, earlier in up-down-left-right order on a tie.
- **Foreign ink carries its owner's emoji rule, not the receiver's.** Reading only coverage drew a
  colour emoji's overflow as a silhouette in the receiver's foreground — measured, a brown pile spilling
  down arrived as 11 device px of pure red. One rule-6 position serves every direction because the
  rule never asks the neighbour's class. A shader comment once claimed a background-class glyph cannot
  overflow; false — Powerline `U+E0A4..=U+E0D6` is background-class, drawn by the font, and spills.
- **The block cursor completes rule 5's withdrawal in the fragment** (#791; any column since #966). A
  block replaces the background at fragment time, so it never reaches the packed instance where rule 5
  is applied. Without the shader half, a neighbour's descender drew *over* the block, and a cursor
  cell's own glyph — painted in `u_cursor_text_color` — spilled into the next row in the pre-cursor fg.
- **A colour emoji samples the atlas RGB, a text glyph the packed foreground** — beamterm's
  `cell.frag` `mix(base_fg, glyph.rgb, emoji_factor)`.
- **The two bands carry separate coverages because they carry separate inks** (#525). A `max()` fold
  was free while one colour served both and total loss once `SGR 58` makes them differ. The line draws
  in its own ink, resolved without the glyph-only rules (`I_line` is `TEXT` class), and is still
  overridden by a block cursor: under a block the line bases follow `base_fg`.
- **Composite in steps, not once.** Folding a line into `fg` first and compositing once applies the
  band's coverage twice (`mix(bg, mix(fg, line, L), L)`), leaving `L(1-L)` of the glyph's ink in the
  line — up to 25 % at half coverage, proportional to exactly the divergence #513 exists to create.
- **The strikethrough goes last**, so where a thick band overlaps the underline the strike wins —
  xterm's band order (`TextureAtlas.ts` strokes the underline at :565-688 and the strike at :762), and
  all three references put the strike over the glyph. Blanket "underline first", which ghostty
  (`generic.zig:2932`) and xterm (`fillText` at :735) take, would trade the #712 defect for its mirror
  — measured here, a red underline over `█▄▓░` goes from 66 red px per cell to 0. `bg_class` splits
  the glyph's coverage between the band's two sides; exactly one side is ever non-zero, so it is one
  `mix`, not a branch. The cursor's strokes draw last and opaque.
- **Band-vs-band overlap is out of reach for the single underline, and #830's second band moves the
  threshold.** The single's centre is 0.38 of the glyph box from the strike's while
  `u_line_thickness / char_height` stays near 0.06, so reaching it needs a ~3 device px box. A double's
  second band sits a fixed number of device *pixels* above 0.88, so the two approach at roughly
  `0.38 * H < sep_px + T`, about ten device px. `demo/underline-marks.html` mounts a struck double
  (`aStruckDoubleKeepsThreeSeparateBands`) and reads three bands at every dpr it sweeps; a mutation
  walking the strike toward the underline reddens that check and only it.
- **A double's two bands merge as coverage (`max`), not as two composites** — both share one colour by
  rule 4, and compositing them separately would apply `v_underline_fg` twice where they overlap.
- **The #317 premultiplied chain, with its measurement.** The old chain seeded with `base_bg` and
  computed alpha separately; at `u_bg_alpha = 0`, `cov = 0.5` a pixel came out `0.5*bg + 0.5*fg` at
  alpha 0.5. Measured before the fix (white `A` on a Default blue, dpr 2, alpha 0): of 174 pixels with
  any alpha, 35 were the foreground and the rest carried blue that was not there — `a = 126` read
  `rgb(150,175,207)`, `mix(blue, white, 0.494)` to the byte. It is not inherent to one-pass
  compositing, the premise #317 had taken from beamterm: straight-alpha source-over of opaque ink onto
  a background of opacity `A` is available in one pass (the references reach it with separate passes
  and hardware blend — alacritty `BlendFuncSeparate` at `renderer/mod.rs:252`, ghostty an
  `AlphaBlending` mode). Accumulating rather than subtracting `base_bg * w_bg` back out is a
  **precision** choice under `mediump`: every term stays positive.
- **`w_bg` is a product, and it replaced an approximation.** `max(coverage, max(ul_band, st_band))`
  disagreed with the colour chain wherever two sources overlapped — a descender crossing its
  underline, #712's own geometry — and never showed while alpha was the only consumer.
- **Only the default background is translucent, keyed on provenance** (#298, #455). An explicit
  `SGR 48`, an inverse, a selection or a cursor background is content and stays opaque; ink is always
  opaque, background-class glyphs included (R1.1). Keying on `base_bg == u_default_bg` went
  translucent on any content cell whose composite landed on the default RGB — a pinhole in opaque
  content. A block cursor is forced opaque (the shader's `!block` term) even where its colour equals
  the default background, as alacritty forces `bg_alpha = 1.` for the cursor cell
  (`display/content.rs:175`).
- **`u_cell_size` is `highp` in the fragment stage by necessity.** It is the one uniform both stages
  declare, so its precision must match the vertex stage's default `highp` or the program fails to link
  ("Precisions of uniform 'u_cell_size' differ"). The fragment-only glyph-box uniforms are `highp` too,
  with no link constraint behind them.
- **Subpixel coverage is gated twice** (#961): only over an opaque background — one alpha cannot carry
  three coverages — and never for a colour emoji. For foreign ink, also never for a background-class
  owner, whose slot RGB is not a mask.

### The underline and strikethrough marks (`hline`, `xgate`, the style chain)

- **A band is a solid fill, not a tent** (#515). `hline` gives full coverage between its edges with a
  half-pixel ramp (`0.5 * fwidth(gy)`) at each. It used to be `1 - smoothstep`, a beamterm port (#267)
  that peaks only at the exact centre, so a sub-pixel band read grey (measured 118/255 at dpr 1). Every
  GPU terminal (kitty, ghostty, wezterm) draws a solid pixel-snapped fill. The centre is pulled inside
  `[0,1]` so it never spills into the next row. **The band's position is not rounded to a pixel row**:
  `top` is only clamped, so a band at a fractional position covers its two edge rows partially.
- **A mark's antialiasing ramp is part of its geometry** (#830). The ramp makes a band's visible
  support one pixel wider than its thickness, so a double underline separated by the references'
  `2 × thickness` **merges into one band** at one-pixel thickness, and a one-pixel dotted dot is erased.
  None of the references ramps (xterm.js strokes onto a canvas that snaps a 1 px line, ghostty fills
  rects into a sprite, alacritty emits rects), so a geometry constant imported from any of them must be
  re-derived against this ramp.
- **The thickness is a device-px rule from the font size** (#517), computed host-side by
  `metrics::line_thickness` ([cell geometry](cell-geometry.md)). `hline` divides by `char_h` only to
  reach glyph-box space, so `lineHeight` or font family cannot distort it.
- **Positions are fixed fractions of the glyph box** — underline centre 0.88, strikethrough 0.5.
  Canvas 2D exposes no `underline_position` (#517). They are glyph-local, not cell-local: with
  `lineHeight = 1.5` a cell-local 0.88 would drop the underline far below its text, and glyph-box space
  keeps `hline`'s centre-clamp inside the cell without alacritty's cell-relative `max_y`. The glyph's
  own coverage no longer needs the glyph-box uniforms (#359 bakes the offset); its decorations do.
- **The style displaces or gates the one band; only double adds a band** (#829, #830 — the maintainer
  settled double as the one structural addition). Curly displaces the centre; dotted and dashed gate
  the band along x (`xgate`). So everything #513 / #525 / #712 settled about the channel keeps holding.
- **The style chain is total over all eight representable values.** `attrs.rs` forwards the raw 3-bit
  field and `apply_frame` takes any `u16`, so 0, 1, 6 and 7 fall through to a straight band. For 1, 6
  and 7 that is what `UnderlineStyle::from_bits` normalises them to in core; 0 is reconciled by the
  `underline` bit, which core's one writer (`set_underline_style`) arms with the style. An `else if`
  ending at 5 with no fall-through would make a mark vanish on a malformed input.
- **`xgate` takes its ramp as an argument**, because a caller gates a *wrapped* coordinate: the
  derivative of `fract(x)` spikes at the seam, and the caller passes the unwrapped coordinate's ramp.
- **Curly: one cycle per cell.** `sin` is 2π-periodic, so a per-cell x agrees with a row-continuous one
  at every boundary; a first draft's column phase term was proven dead by mutation. Per-cell is what
  ghostty (a sprite) and xterm.js (a stroke into the atlas) do. The curl oscillates **upward** from
  0.88, so its lowest point sits where the straight band does, and its amplitude has a device-px floor
  for #515's reason. **Its antialiasing is vertical, measured rather than assumed**: `fwidth(gy)` is
  exact for a horizontal edge and narrower by `cos θ` across the curl's diagonal (max slope
  `2π·max(thickness,1) / cell_width`, ~0.79 px/px at an 8×16 cell). `steepOverFlatExtent` at font 48
  reads 1.00 / 0.90 / 1.00 / 0.93 over dpr 1 / 1.1 / 1.5 / 2 against a straight control of 1; a
  perpendicular-correct shader would read ~1.27. **Deliberately not asserted**: correcting the ramp
  would redden a test pinning 1.00, so the ratio is published in the proof's `measured` block.
- **Double: the lower band where the single sits, the second above it.** The separation is xterm.js's
  and ghostty's `2 × thickness` (`TextureAtlas.ts:590-591`, `special.zig:57-70`); alacritty straddles
  the descent, a metric this renderer lacks. Upward because `top` is clamped into the glyph box, so a
  pair placed downward collapses into one line with no error; xterm.js, which also restricts to the
  cell, shifts the pair up the same way. **`2 × thickness` does not survive this rasteriser**: at the
  default 16 px font and dpr 1 the ramps close the 1 px gap (`doubleBandHistogram: {"1": 96}`), so the
  separation is `max(2 * thickness, thickness + 2px)` — one device pixel of clear air where the
  reference rule has no room. The floor binds at one-pixel thickness only. A deliberate divergence
  with a measurement behind it, on the retired tie-breaker table's "Renderer cell composition — justerm's
  own model (ADR-0019)" row (frozen in `docs/agents/theflow.md`).
- **Dotted: a whole number of dots per cell, computed host-side** (#830, `metrics::dots_per_cell` →
  `u_dots_per_cell`), so the pattern is cell-periodic and the cell-local `v_tex.x` needs no cross-cell
  phase — ghostty's route; xterm.js (`variantOffset`) and alacritty (every-two-cells inversion) pay with
  cross-cell state. Rust, not GLSL, also because GLSL ES 3.00 leaves `round()` implementation-dependent
  at 0.5. **At or above a 2 px dot** the dot is centred in its period so `fract`'s seam falls in the
  gap. **Below 2 px it is pixel-aligned and hard**: antialiasing a 1 px dot erased the mark
  (`dottedDuty: 1.0`, one run, every "is drawn" check green); alacritty splits `draw_dotted` /
  `draw_dotted_aliased` at the same 2 px. The hard gate is **half-open from the period start**, since a
  centred gate on a 2 px period admits both fragment centres or neither (`dottedRuns: 0`). It must not
  be a pixel index modulo the period: ours is `cell_w / n`, whole only on an even cell, and on an 11 px
  cell `mod(pixel, period)` lit columns 0, 2, 4 and nothing after — identical in every cell, so
  invisible to a cross-cell check. The branch is chosen from uniforms, so the two branches' quarter-period
  phase difference is grid-wide.
- **Dashed: one period per cell, the dash at the two outer quarters**, so adjacent cells' dashes join —
  alacritty's construction (`rect.f.glsl`, `draw_dashed`). All three references are cell-periodic here.

### The packer (`frame.rs` `pack_instances`), stage by stage

The decoration stages are in [decoration](decoration.md), the selection, contrast and dim policies in
[colour policy](colour-policy.md).

- **The instance is flat and fixed-width**: `col, row, bg(3), fg(3), glyph_field, underline_fg,
  strike_fg, bg_default`, then the four neighbours' glyph fields `up, dn, lt, rt` and their four inks —
  `INSTANCE_FLOATS` = 20. Each group of four is one `vec4` attribute, so the instance spends 10 of
  WebGL2's guaranteed 16 attribute locations (#792 had recorded that ceiling as a reason against a
  horizontal band); `shader.rs` asserts the four stay consecutive. **The offsets are named**
  (`BG_RGB`, `GLYPH_FIELD`, `NEIGHBOUR_UP`…, #791): `INSTANCE_FLOATS - 1` and a literal stride both read
  the wrong float the moment a field is appended, and appending four is how that was found.
- **The line inks and neighbour inks ride as packed floats, measured exact.** A standalone WebGL2 probe
  round-tripped ten values including `0x000000`, `0xFFFFFF`, `0x010101` and both sides of the sign-bit
  boundary. Packed because a line is rare: every cell would otherwise pay three floats per band, and
  ADR-0021 keeps one instance buffer resident per grid. With no `SGR 58` the second line float is
  redundant (`sgr58_colours_the_underline_only_and_the_strike_keeps_the_follow_fg_ink`). Drawing the
  strike from `v_fg` instead is wrong on exactly #513's cells: a glyph-only rule (the #239 re-tint)
  moves `fg` and must not reach `I_line`. `bg_default` costs its own float because a colour fills all
  24 f32-safe bits.
- **`Frame` carries only what resolution left for it to read.** A short or missing column entry
  resolves as `Default` / slot `0` / no flags, and the cell is still emitted (#255). `codepoints` is the
  **base** scalar only, kept to classify the glyph (#226, #239/#241, #494); a cluster stops at
  `glyph_resolve` — coverage is set by the base and a mark only adds ink (#495, `glyph_class`).
  `underline_colors` (SGR 58, #520) is a tagged reference; core arms it only when `UNDERLINE` is set
  (`Term::pen_ext_attrs`).
- **Colours stay packed through the whole cell pipeline** and unpack to GL floats (`gl_rgb`) only at
  the end, so the blends are integer math matching the reference to the byte. A cell that gains or
  loses a highlight re-uploads through the upload diff with no bookkeeping (unlike beamterm's overlay
  delta). A failed capacity reservation falls back to growing on demand rather than aborting.
- **An open composition stands every stage below glyph resolution down** (ADR-0028 D2, #249): the
  highlight lookup, selection coverage, both decoration lookups, and the underline colour (#711). The
  pass writes `UNDERLINE` itself, and both grid-drawing references give that mark the run's **own** fg
  (alacritty `renderer/mod.rs:225`, ghostty `generic.zig:3299`, `:3335`). **Zeroing the reference *is*
  the declaration**: `0` is `Default`, "follow the fg", and the fg is the pass's. Mirroring the column
  into `preedit_patch` writes the same `0`; the packer side was chosen because `webgl.rs` is wasm32-only
  and a patch-side fix is unreachable by the renderer's `cargo test`. A composed cell's ink class is
  right only because `codepoints` is a column the patch replaces.
- **Every span lookup asks about the cell and its wide-pair partner** (#454, `pair::partner_at`), from
  the frame's flags — the *patched* ones inside a preedit. The partner's `composed` state is not
  re-checked, safe because `preedit::range` counts a wide codepoint as two cells, so a pair the run
  writes lies wholly inside the span; if that counting changes, the lookup gains a `!partner_composed`
  term.
- **A hovered link draws a single underline only where the cell has none** (#934). The underline and
  glyph field read these `line_flags`; the colour stages keep reading `cell_flags`.
- **The line's ink forks from the glyph's at exactly one point** (#513): after the cell's own ink, a
  bottom decoration's fg and `selectionForeground`, and before the tile re-tint — the one rule about
  *the glyph*. Top decoration, dim and contrast then run over both. A top decoration's fg reaches a
  **follow-fg** line only; an explicit `SGR 58` colour (#520) is authoritative, drawn raw and immune to
  decoration fg, DIM and minimum contrast — xterm's rule (`TextureAtlas` strokes the raw
  `getUnderlineColor()`). The follow-fg pipeline is computed only where a follow-fg underline or a
  strike draws (`needs_follow_fg`), so most cells skip the second `ensure_contrast_ratio` loop; the
  strike is the follow-fg value unconditionally.
- **A concealed cell points at the blank slot and drops the attribute bits too** — `ESC[8m` hides the
  whole cell, as does a blink cell in the off phase (`blink_on` is the consumer's phase, #282). A glyph
  a decoration took (#508) keeps the bits, because an underline is not the glyph; its ink class goes
  with the glyph (#712).
- **`bg_default` is complete by construction** (#455, ADR-0019 totality): `1.0` iff the reference is
  Default, the cell is not inverse, no decoration painted a bg (bottom or top), and no highlight
  composited one — the four sites that assign the bg channel. Both references decide by provenance too
  (alacritty `compute_bg_alpha`, xterm's colour-mode bits). The block cursor is the one class neither
  reaches through these signals; here it is the shader's `!block` term, outside this predicate.
- **`I_neighbour` is granted in a second walk, withdrawn by default.** A cell starts with four blank
  handles and is granted a neighbour's glyph field and ink where the two **resolved** backgrounds are
  equal (rule 5). The walk is separate because the answer needs the neighbour's composite, and reading
  it back out of the instance shares one resolution — no second bg pipeline to drift. Across, a wide
  pair needs no reconciliation (each outer edge spills into a different receiver, the inner band is
  empty by construction, `bitmap::split_wide_bitmap`). Up and down it does
  ([a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)), and the
  reconciliation blanks **both** receivers: one-sided blanking happens to converge (the mutation still
  passes) only because the walk visits every cell, which the loop should not have to promise.

## Code

- `justerm-renderer/src/frame.rs` — `pack_instances`, `Frame`, `INSTANCE_FLOATS`, the named offsets
- `justerm-renderer/src/shader.rs` — `FRAG_SRC` (the composite chain, `hline`, `xgate`,
  `slot_texel`), `VERT_SRC` (instance unpacking)
- `justerm-renderer/src/preedit.rs` — `patch`, `writes`, `range`, `caret_col`, `WriteKind`, `Span`
- `justerm-renderer/src/suggestion.rs` — `is_blank`, `writes`, `patch`
- `justerm-renderer/src/overlay.rs` — `Overlay`, `HighlightKind`, `composite_bg`, `blend_over`,
  `should_blend_kind`
- `justerm-renderer/src/decoration.rs` — `DecorationLayer`, `DecorationRect`,
  `decoration_override_at`
- `justerm-renderer/src/pair.rs` — `partner_at`
- `justerm-renderer/src/attrs.rs` — `glyph_field`, `GLYPH_BG_CLASS`, `GLYPH_USTYLE_SHIFT`
- `justerm-renderer/src/metrics.rs` — `ink_rows`, `dots_per_cell`
- `justerm-renderer/src/webgl/draw.rs` — `apply_frame`, `resolve_and_pack`, `preedit_patch`,
  `packs`
- `justerm-renderer/src/webgl.rs` — `suggestion_patch`, `GridTier::set_bg_alpha`
- `justerm-renderer/src/webgl/grid_state.rs` — `set_overlay`, `set_active_match`, `set_bg_alpha`,
  `set_suggestion`
- `justerm-web/src/terminal.ts` — `Terminal.setSuggestion`, `paintSuggestion`
- `justerm-web/src/suggestion.ts` — `suggestionCell`, `suggestionColorRef`, `SuggestionOptions`,
  `SuggestionColor`

## Reference behaviour

- [Renderer ink channels](../../agents/reference-facts.md#renderer-ink-channels)
- [How a translucent background composites](../../agents/reference-facts.md#how-a-translucent-background-composites-317-2-verified-2026-08-18)
- [Whether a glyph's ink may leave its cell](../../agents/reference-facts.md#whether-a-glyphs-ink-may-leave-its-cell--30-and-the-single-guard-that-withdraws-it-791-verified-2026-08-20)
- [Drawing the preedit](../../agents/reference-facts.md#drawing-the-preedit-two-draw-into-the-grid-one-draws-a-dom-box-249-verified-2026-08-03)
- [The underline marks — how each reference draws double, dotted and dashed](../../agents/reference-facts.md#the-underline-marks--how-each-reference-actually-draws-double-dotted-and-dashed-830-verified-2026-09-04)
- [A span, a highlight and a caret over a wide pair](../../agents/reference-facts.md#a-span-a-highlight-and-a-caret-over-a-wide-pair-454-verified-2026-08-10)
- **Read ADR-0019's framing before comparing to xterm.js here:** it is a design input, not a
  validator (ADR-0031 D1 states the same for the bake). The mark citations above in prose
  (`TextureAtlas.ts`, `generic.zig`, `rect.f.glsl`) are unpinned unless they appear in those rows.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — every span in
  the family meets the flags here: the three overlay lookups, both decoration layers and the
  neighbour-ink reconciliation resolve their pair through `pair::partner_at` (#454).
- [composition is browser-owned state](../invariant/composition-is-browser-owned-state.md) — the
  preedit pass draws state the engine never sees, and the suggestion is hidden for a whole composition,
  which only the widget can see (#972).
- [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md) — the suggestion's "blank"
  test is the by-cell form of it, plus the flags that ink an empty cell (#972).
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the renderer is outside the root workspace; the packer's host tests run only where a gate names it
  by `--manifest-path`.
- [the renderer's copies of core encodings are held by convention](../invariant/renderer-encodings-are-held-by-convention.md)
  — the packer reads the flag bits and the underline style field through `attrs.rs`'s re-declared constants

## Blast radius

- [decoration](decoration.md) — its precedence rules decide what arrives at the Bottom/Top layers;
  the packer assumes a decoration's override is per property (bg and fg merged independently, #452).
- [active match](active-match.md) · [selection](selection.md) · [search](search.md) — the bg channel
  keys on the winning highlight and the fg on selection coverage; a highlight that changes which
  channel it claims changes every rule downstream.
- [colour policy](colour-policy.md) — the policies run inside this pass against `eff_bg`; moving a
  layer before or after them changes what the contrast correction sees.
- [caret drawing](caret-drawing.md) — the block cursor replaces the background in the fragment, so
  rule 5's withdrawal and the forced-opaque alpha both depend on the shader knowing the caret cell.
- [wire format](wire-format.md) — the packer reads the decoded columns as given; a new per-cell column
  owes an answer inside a composition and a suggestion, or it leaks the erased cell's state.
- [cell geometry](cell-geometry.md) — the neighbour reads assume the slot is `bleed | cell | bleed` at
  the `u_cell_uv` origin, and the marks assume glyph-box space; a slot layout change breaks both.
- [glyph atlas](glyph-atlas.md) — the packer assumes every slot it names holds the same glyph until
  the frame is drawn, and the foreign-ink rule assumes the owner's emoji bit travels with its slot.
- [emoji classification](emoji-classification.md) — a glyph's emoji bit decides how its own ink and its
  overflow are coloured; a reclassification changes what spills onto a neighbour.
- [input encoding](input-encoding.md) — the composition the preedit pass draws, and the
  `composing` gate the suggestion keys on, are owned there.

## Known holes / open

- **A band at a fractional position draws a full row plus a partial neighbour** — derived from
  `hline`'s arithmetic during #989, not measured. A 1 px band whose top sits at 0.3 px covers the row
  centred at 0.5 fully and the row at 1.5 at 0.6. #515's substance holds (a full-coverage core row
  always exists), but "snaps the band to whole device pixels", which a shader comment and
  `demo/cursor.html` said, was never what the code does. Whether the partial row is visible softness
  at the default size is unmeasured.
- **`preedit::caret_col`'s right-edge step-back has lost its stated reason.** At the edge it puts the
  caret on the last glyph's lead rather than its spacer, because a block caret on a spacer inverted
  the right half of the glyph being composed (measured on a 106-column grid). Since the caret applies
  the pair rule (#454, [caret drawing](caret-drawing.md)), a caret on either half covers the whole
  glyph, so the step-back is redundant. Harmless; whether to keep it is undecided.
- **A suggestion's zero-width codepoint takes a cell of its own** (#972). Width is per codepoint, as
  for the preedit, so a combining mark measures narrow while core attaches it to the previous cell; an
  NFD suggestion draws one cell per mark and disagrees with what the shell echoes on accept. ADR-0028
  excuses this for the preedit because it re-renders every keystroke; a suggestion lives longer. Reach
  unmeasured.
- **The policy setters have no records.** `set_bg_alpha`, `set_minimum_contrast_ratio`,
  `set_bold_to_bright`, `set_selection_foreground` each change what a cell resolves to; ADR-0019
  governs the model, not the knobs.
- **A mark's z-order is only observable where a declared colour splits it from the glyph's ink.** A
  strikethrough has no declared-colour regime (#525), so its position relative to the glyph cannot be
  asserted on an ordinary cell — only where a glyph-only treatment moves `fg` away from the line inks
  (a selected tile). The natural next test here is one that cannot fail.
- **"Every pack covers the whole viewport" is load-bearing and unrecorded.** It is why the previous
  renderer's incremental repaint was deliberately not ported, and it lives in no record.
- **The curly ramp is ~21 % thin perpendicular to the steepest slope** — measured and deliberately
  unasserted (above); a correction would move the published ratio to ~1.27.
