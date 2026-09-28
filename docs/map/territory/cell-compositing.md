# Territory — cell compositing

## What it is

Turning one decoded cell into the numbers a GPU draws: resolve its colour *references* against a
palette, apply the colour policies, composite every layer that claims the same pixel, fold the
underline into the glyph field, and pack it into a flat per-cell instance buffer for **one** instanced
draw call.

This is the renderer's hot path, and it is the first place in the family where a colour reference
becomes an actual colour — the engine never does that by identity.

## Governing decisions

- [**ADR-0019 — the cell composition model**](../../adr/0019-cell-composition-model.md) — a layered,
  per-channel, **total** resolution. The model answers a combination *by construction* rather than
  case by case, and a combination it cannot answer is an **amendment**, not a new decision
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — this is the "A-ii"
  hot-path-in-wasm decision the whole crate exists to execute
- [ADR-0024 — decoration projection and precedence](../../adr/0024-decoration-projection-and-precedence.md)
  — the axis ADR-0019 deliberately put out of its own scope (check its `Status:` line; it is not
  copied here)

## Design model

- **An IME preedit is not a layer in this stack — it is a *pass* that removes cells from it**
  (ADR-0019's 2026-08-03 amendment, #249/ADR-0028). Nothing here can *supply* a glyph: every layer
  recolours a channel or blanks a slot, and rule 5's authorship axis has no value for content the
  browser owns and the application never declared. So the composed cells leave the stack at resolve
  time and come back with bg, fg and glyph together, and `pack_instances` stands every stage below
  glyph resolution down inside the run. Replacing only the resolver's *inputs* is not enough and was
  measured not to be: a selection covering the run still tinted it.
  **Every per-cell column owes an answer for a composed cell; which half gives it is free**
  (ADR-0028 D2, #711). Five columns are re-supplied by the patch and `underline_colors` is stood down
  in the packer, and that split is a gate artifact rather than a rule — `0` already means *follow the
  fg the pass supplied*, so either half writes the same value. What bites is a column answered by
  **neither**: `SGR 58` was, for one published release, so a composition drew its underline in the
  colour of the text it had erased. The pass shipped *after* that column existed, so the obligation
  is on whoever writes a pass, not only on whoever adds a column.
  **The mirror of that hazard is a *cell* answered by the wrong half** (#715, same release). The pass
  also writes the pair-repair beside its run — a cell it does not take, only un-pairs — and the patch
  re-supplied that one's colours too, so the application's background vanished for as long as the
  composition stayed open. `preedit::Span` had drawn the line correctly the whole time; the patch had
  not, and the two halves of one pass are what disagreed. The rule both now share: a repair blanks the
  glyph, never the pen.
- **The consumer's suggestion is a second glyph-supplying pass, and it inverts the preedit's answers**
  (#972, ADR-0019's 2026-09-28 amendment). It shares only the per-codepoint width with the preedit and no
  state (ADR-0028 D4), because the two refer to different things: the preedit is browser state the engine
  cannot know, the suggestion hangs off the engine's cursor. So:
  - **Anchor: the engine cursor, re-sent by justerm-web on every frame**, mapped through the display
    offset and cleared when off screen. Not the renderer's retained caret — that is cleared on every
    blink-off phase and under DECTCEM, so a pass anchored there would blink with the caret; and not a
    latched point, which drifts on output scroll and on resize. The cost is one frame at the wrong cell
    when output moves the cursor before the consumer clears it.
  - **Geometry: keep the head, clip on the right.** `preedit::range` shifts left to keep its tail, which
    here would draw over the text already typed. No underline.
  - **"Blank" means *draws nothing but its background*.** A space or empty codepoint, no grapheme, not
    half of a pair, and none of `INVERSE`, any underline or strikethrough — each of those puts ink on an
    empty cell. This also covers pending wrap, which the wire does not carry: at the right margin the
    cursor cell still holds the character just typed, so "draw from the cursor cell" would overwrite it.
    The codepoint half is [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md): an
    NBSP or U+3000 was printed by the application, so it stops the run although it draws no ink.
  - **Withheld, not stood down.** A cell under a highlight, either decoration layer or the hovered link
    keeps the engine's cell, and a wide suggestion glyph with either half covered is withheld whole. The
    run still counts that cell. Link hover belongs here too: the suggestion's flags carry no underline,
    so a hover would otherwise draw one in the covered cell's `SGR 58` colour.
  - **Colour: a tagged reference, not RGB.** A Default or Indexed colour re-resolves on `setTheme` with
    no push from the consumer, which an absolute colour would need. `DIM` is the 50 % blend toward the
    cell's background, so over a coloured row it reads as a mix of the two — measured `[128,0,127]` for
    red over blue in `demo/suggestion.html`.
  - **Composition: hidden from `compositionstart` to `compositionend`, by the widget** (`suggestionCell`
    keys on `composing`). The renderer also yields to a drawn preedit, but that alone misses the start
    of a composition and an update to `""`. The consumer cannot do it: keys an IME owns never reach
    its key hook.
  - **Colour on the web surface is a union** (`"default" | { indexed } | { rgb }`), encoded to the
    tagged reference in the widget. Every other colour the widget takes is a plain `0xRRGGBB`, so a
    bare number here would read as RGB and silently draw as Default.
  - **Cost: the patch borrows the clusters and backgrounds.** A written cell has no cluster to clear
    (a cell with one stops the run) and keeps its background, so only codepoints, flags and fg are
    copied. Measured on a 200×60 grid, release host build: 44 µs per pack against `pack_instances`'
    ~620 µs, down from ~180 µs when all five columns were copied. The copy happens on every pack while
    a suggestion is set, which at a prompt is every output frame.
  - **Alternate screen: dropped by justerm-web on entry and refused while there.** The renderer's damage
    header carries no alt flag. Masking would bring a stale suggestion back after `:q`, because the
    consumer's ranker answers asynchronously after Enter.
- **Back-to-front, and decorations sit on *both* sides of the highlight:**
  `base < bottom-decoration < highlight < top-decoration`. A decoration is not simply "above" or
  "below" content — it chooses a side of the selection/search layer, which is what
  `DecorationLayer::{Bottom, Top}` means.
- **Alpha is a channel of the composite, not a postscript to it** (#317 §2). The shader emits one
  straight-alpha RGBA per cell — this renderer enables no GL blending, so the fragment *is* the
  composite. The ink accumulates premultiplied from nothing, the background's surviving weight is
  the product of every ink source's complement, and the two are recombined against `u_bg_alpha`:
  `a = 1 - w_bg(1 - A)`, `rgb = (ink + bg·A·w_bg) / a`. The failure this replaced is the one to
  recognise if it reappears anywhere: the colour was composed against a **fully present**
  background while the alpha declared that background mostly absent, so the two channels described
  different cells. That is ADR-0019's Coherence clause, one axis over from where it is usually read.
  Invisible at `A = 1`, where the two expressions agree exactly.
- **Text coverage may be three numbers, not one** (#961, ADR-0033). In a subpixel configuration a
  text-class glyph's coverage is the slot's RGB mask, chosen per channel of the ink — as-is where that
  channel is at or above 0.75, raised to `u_lcd_gamma` where it is below — so the background's
  surviving weight `w_bg` is a `vec3`. `I_neighbour` reads its owner's mask the same way, with the
  owner's ink. `a` reads one channel of `w_bg`, which is exact: over a translucent default background
  the coverage stays the scalar alpha (the three are equal), and over an opaque one `A = 1` makes
  `a = 1` whatever they are. Background-class ink and colour emoji stay scalar.
- **Per-channel, not per-layer.** A layer may claim the background and leave the foreground alone.
  This is what makes "which wins" answerable for an active match over a selection without either
  layer having to win outright.
- **Colour references resolve here, against an injected palette.** `Indexed` and `Rgb` become
  concrete through `Palette`; the engine hands over references precisely so this step is the
  consumer's and the theme stays out of core.
- **Colour policies apply before compositing** — inverse, bold→bright, dim, and the minimum contrast
  ratio. They transform the *cell's own* colours; compositing then layers other things over that.
- **Underline and strikethrough fold into the glyph field**, and their ink is a separate channel
  from the foreground (#513) rather than the foreground itself — so a coloured underline is
  expressible without a second draw.
- **Which ink source ends up on top is a question about their CLASS, not about draw order** (#712,
  ADR-0019 rule 6) — with one source whose position is *not* class-derived: `I_neighbour` sits above
  the receiver's own tile and below everything else the receiver owns, whatever class its owner had,
  because the thing being prevented is one row's letter amputating the next row's. Background-channel ink cannot occlude a `TEXT`-class source, so an underline draws
  *over* a tile and *under* a letter — and the glyph field carries the class (bit 16) because the
  shader sees an atlas slot, never a codepoint. Two things here are easy to miss: this is the one place
  a background-class glyph's ink is *ordered* rather than merely recoloured, and the whole question is
  **invisible unless something declared a colour** — with the inks equal both orders composite to the
  same bytes, which is why it survived undetected until `SGR 58`.
- **A mark's antialiasing ramp is part of its geometry, and a separation computed without it is
  wrong** (#830). `hline` spends half a device pixel on a `fwidth` ramp at each edge, so a band's
  visible support is one pixel wider than its thickness. Two consequences arrived together and both
  read as green: a double underline separated by the references' `2 x thickness` **merges into one
  band** at a one-pixel thickness (measured: one band at every column, the merged centre exactly a
  pixel above a single's), and a **one-pixel dotted dot is erased entirely** because a half-pixel
  ramp on each side fills the whole gate (measured: duty 1.0, one run - a solid line). None of the
  three references meets either, because none of them ramps: xterm.js strokes onto a canvas that
  snaps a horizontal 1px line, ghostty fills whole rects into a sprite, alacritty emits rects. So a
  geometry constant imported from any of them has to be re-derived against this ramp before it is
  believed - which is what put a `thickness + 2px` floor under the double's separation and a hard,
  unramped path under sub-2px dots.
- **The two marks are one ink source split by authorship of the colour** (#525, ADR-0019 rule 4).
  They share the follow-fg pipeline and separate only where something *declared* a colour: `SGR 58`
  declares the underline's and there is no SGR for a strikethrough's. A cell with no `SGR 58` has
  both inks equal, which is what keeps the split from inventing a divergence of its own.
- **The instance is flat and fixed-width**: `col, row, bg(3), fg(3), glyph_field, underline_fg,
  strike_fg, bg_default`, then the four neighbours' slots `up, dn, lt, rt` and their four inks in
  the same order (#791 the vertical pair, #966 the horizontal) — 20 floats. Each group of four is
  one `vec4` attribute, so the instance spends 10 of WebGL2's guaranteed 16 attribute locations
  rather than the 16 one-float-per-location would have (#792 recorded that ceiling as a reason
  against a horizontal band); `shader.rs` asserts at compile time that the four stay consecutive. One buffer, one instanced draw call — the same fixed-stride
  reasoning the wire format uses, for the same reason. **The offsets are named** (`frame.rs`), not
  arithmetic: `INSTANCE_FLOATS - 1` for "the last field" and a literal stride both read the *wrong*
  float the moment a field is appended, and appending four is how that was found.
- **A cell's ink can come from the cell above, below or beside it** (#791 vertical, #966
  horizontal, ADR-0019 **R1.2**). A glyph whose
  ink exceeds its cell used to have the excess destroyed — at bake, by a slot the size of the cell,
  and again at sample, by a texcoord inset that never reached past it. The slot now carries a
  **bleed band** on every side, derived per font configuration — above and below from what that face
  overshoots its own `█` vertically, left and right from what its `█` overhangs the glyph box — and
  the receiving cell's fragment reads the four adjacent slots and folds the strongest one's coverage
  into the same rule-6 chain. Reader-side, deliberately: the quad stays exactly one cell, so nothing
  overlaps, the composite stays one evaluation per pixel, and no GL blending is involved — the
  writer-side shape every reference uses would have forced a premultiplied buffer and put
  foreign-vs-own occlusion back under instance order.
- **A colour emoji's overflow keeps the ATLAS's colours, and since #794 something proves it.** The
  shader mixes `tex.rgb` under the *owner's* emoji bit, because R1.2 says foreign ink keeps its
  owner's ink and an emoji's ink lives in the texture. The guard could not be built on
  `neighbour-ink.html`'s instrument, which picks its probe from a 2D-canvas `fillText` — that is not
  the path a wide glyph takes. Taking the precondition from the renderer's own bake instead shows
  why: a colour emoji on the **wide** path deposits nothing on its neighbour, while the same
  codepoint on the **narrow** (width-1) path spills 6 device px at dpr 1 and 10 at dpr 2. So
  `demo/emoji-neighbour-ink.html` is built on the narrow path, and states the claim as two
  hypotheses — the overflow's mean colour is nearer the owner's own ink than the foreground every
  cell wears — because the defect it guards produced that foreground exactly.
- **Foreign ink is withdrawn where the two cells' backgrounds differ**, and *resolved* backgrounds:
  two cells can both hold `Default` and still differ once a selection covers one. Two of the three
  producers of that difference are invisible to the packer, so the rule is enforced in two places —
  the **block cursor** is a per-fragment background and is withdrawn in the shader, and a **wide
  pair** is one glyph across two cells whose receivers must reach the same verdict.
- **The packer is pure and host-testable.** Glyph-slot resolution and rasterisation are stateful and
  browser-only; this function takes already-resolved slots, which is what lets the hot path be tested
  without a GPU.
- **Every frame re-packs entirely.** There is no incremental repaint here, which is why the
  engine's damage model targets the *wire* rather than this renderer.

### The fragment stage (`shader.rs` `FRAG_SRC`), step by step

What the shader's own comments carried before #989, by the step each one explains.

- **The instance's inks travel as floats, exactly.** `a_underline_fg` / `a_strike_fg` carry
  `0xRRGGBB` one per float (#513, split by #525); a colour is below 2^24, so an `f32` carries it
  exactly — measured with a standalone WebGL2 probe. The vertex stage unpacks them once per
  instance rather than per fragment. The glyph field is read the same way (`uint(a_glyph)`), which is
  why its ink class (bit 16, #712) and underline style (bits 17..19, #829) may sit above the `u16`
  the rest fits in: the field is full, the transport is not. `a_bg_default` is **provenance**
  packed by the Rust side, not re-inferred from the resolved colour (#455).
- **I_neighbour, in the fragment** (#791, #966). A slot holds `bleed | cell | bleed` on each axis,
  so the band that spilled toward this cell sits exactly one *cell* away from where its own texel
  reads — `± u_cell_uv.w` down the slot for the rows above and below, `± u_cell_uv.z` across it for
  the cells beside — and no arithmetic about padding or band depth is needed. `metrics::ink_rows`
  states the same mapping in device px and is where it is tested, on both axes; the shader holds its
  texcoord form. The four neighbours are sampled **unconditionally and masked**, not branched: an
  implicit-LOD fetch under non-uniform control flow is undefined in GLSL ES 3.00, which is also why
  `slot_texel` passes an explicit LOD. One neighbour supplies a fragment's foreign ink — the one
  laying down the most, the earlier in up-down-left-right order on a tie, so a fragment only rows
  reach behaves as it did before #966.
- **Foreign ink carries its owner's emoji rule, not the receiver's.** Reading only coverage drew a
  colour emoji's overflow as a monochrome silhouette in the receiving cell's SGR foreground —
  measured, a brown pile spilling into the row below arrived as 11 device px of pure red. **One rule-6
  position serves every direction, and that is a property of the rule, not of what can spill**: the
  clause is "above the receiver's own tile, below everything else the receiver owns", which never
  asks the neighbour's class. A shader comment once justified it by claiming a background-class glyph
  cannot overflow; that is false — Powerline `U+E0A4..=U+E0D6` is background-class by `glyph_class`,
  is drawn by the font rather than by `builtin`, and spills like any other font glyph.
- **The block cursor completes rule 5's withdrawal in the fragment** (#791; any column since #966).
  A block replaces the cell's background at fragment time, so it never reaches the packed instance —
  and the packer is where ADR-0019 rule 5 withdraws `I_neighbour` at a background edge. Both
  directions were wrong without the shader-side half: a neighbour's descender drew *over* the block,
  and a cursor cell's own glyph — painted in `u_cursor_text_color` — spilled into the next row in the
  pre-cursor `fg`, one glyph in two colours split at the cell boundary.
- **A colour emoji samples the atlas RGB, a text glyph the packed foreground** — beamterm's
  `cell.frag` `mix(base_fg, glyph.rgb, emoji_factor)`.
- **The two bands carry separate coverages because they carry separate inks** (#525). Folding them
  with `max()` first was free while one colour served both; it is lossy the moment `SGR 58` makes
  them differ, and the loss is total — the underline's colour would paint the strike. The line draws
  in its **own** ink, resolved without the glyph-only rules (#513, ADR-0019 rule 4 — `I_line` is
  `TEXT` class), and is still overridden by a block cursor, because the cursor recolours the whole
  cell: under a block the line bases follow `base_fg`.
- **Composite in steps, not once.** Folding a line into `fg` first and compositing once applies the
  band's coverage twice (`mix(bg, mix(fg, line, L), L)`), leaving `L(1-L)` of the glyph's ink in the
  line — up to 25% at half coverage. Invisible while the two inks were equal; an error the moment
  #513 made them differ, proportional to exactly the divergence the channel exists to create: at the
  default font size an underline on a selected tile was never the cell's ink, only mostly it.
- **The strikethrough goes last**, so where a thick band makes the two overlap the strike wins —
  xterm's band order (`TextureAtlas.ts` strokes the underline at :565-688 and the strike at :762),
  and all three references put the strike over the glyph. The underline's place is the #712 bullet
  above; blanket "underline first", which ghostty (`generic.zig:2932`, the descender reason) and
  xterm (`fillText` at :735, between the two bands) take, would trade that defect for its mirror —
  measured on this renderer, a red underline over `█▄▓░` goes from 66 red px per cell to 0. Neither
  reference has a background ink class driving occlusion, so neither faced the choice. In the chain,
  `bg_class` splits the glyph's coverage between the two sides of the band — exactly one side is ever
  non-zero, so it is one `mix` more than the chain before #712, not a branch. The cursor's
  strokes draw last and opaque, over the glyph — both references append the cursor rects after the
  text pass.
- **Band-vs-band overlap is out of reach for the single underline, and #830's second band moves
  the threshold.** The single's centre is 0.38 of the glyph box from the strike's while
  `u_line_thickness / char_height` stays near 0.06 at every font size, so reaching it needs a glyph
  box of about three device px. A double's second band sits a fixed number of device *pixels* above
  0.88 rather than a fixed fraction, so its distance to the strike shrinks with the box — the two
  approach at roughly `0.38 * H < sep_px + T`, about ten device px. A completeness pass raised it;
  `demo/underline-marks.html` mounts a struck double (`aStruckDoubleKeepsThreeSeparateBands`) and
  reads three separate bands at every dpr it sweeps, and a mutation walking the strike toward the
  underline reddens that check and only it.
- **A double's two bands merge as coverage (`max`), not as two composites**: ADR-0019 rule 4 splits
  marks by authorship of the colour, both halves share one, and compositing them separately would
  apply `v_underline_fg` twice where they overlap.
- **The #317 premultiplied chain, with its measurement.** The old chain seeded with `base_bg` and
  computed alpha separately; at `u_bg_alpha = 0`, `cov = 0.5` a pixel came out `0.5*bg + 0.5*fg` at
  alpha 0.5, where a fully transparent background can contribute nothing and the answer is `fg`.
  Measured before the fix (white `A` on a Default blue, dpr 2, `bg_alpha = 0`): of 174 pixels with
  any alpha, **35** were the foreground and the rest carried background blue that was not there —
  `a = 126` read `rgb(150,175,207)`, which is `mix(blue, white, 0.494)` to the byte. It is **not**
  inherent to compositing in one pass — the premise #317 recorded from beamterm and nobody had
  re-derived for this shader: straight-alpha source-over of opaque ink onto a background of opacity
  `A` is `a = 1 - w_bg*(1-A)` and `rgb = (ink + base_bg*A*w_bg) / a`, both available in one pass (the
  references reach the same result with separate passes and hardware blend: alacritty
  `BlendFuncSeparate` at `renderer/mod.rs:252`, ghostty a whole `AlphaBlending` mode). Accumulating
  rather than subtracting `base_bg * w_bg` back out is a **precision** choice under `mediump`: every
  term stays positive and numerator and denominator shrink together, where the subtraction form
  cancels two near-equal quantities exactly where `a` is smallest.
- **`w_bg` is a product, and it replaced an approximation.** `max(coverage, max(ul_band, st_band))`
  approximated the ink's total weight and disagreed with the colour chain wherever two sources
  overlapped — a descender crossing its underline is the reachable case, #712's own geometry. The two
  agreed only where at most one source was partial, which is why it never showed while alpha was the
  only consumer.
- **Only the default background is translucent, keyed on provenance** (#298, #455). An explicit
  `SGR 48`, an inverse, a selection or a cursor background is *content* and stays opaque, or a
  highlight would vanish on a translucent terminal; ink is always opaque, a background-class glyph's
  included (ADR-0019 R1.1 carries why). Keying on `base_bg == u_default_bg` instead went translucent on
  any content cell whose composite coincidentally landed on the default RGB — an `SGR 48` set to the
  theme bg, an `Indexed` slot resolving to it, a decoration painting it: a pinhole in opaque content.
  A block cursor is forced opaque even where its colour equals the default background — alacritty
  forces `bg_alpha = 1.` for the cursor cell unconditionally (`display/content.rs:175`, "we must
  adjust alpha to make it visible"). The strokes no longer need `max(bg_a, cur)`: `cur` is in `w_bg`,
  so a stroked pixel has no background left. `w_bg` is per channel since #961 and one channel serves
  `a`: the three are equal wherever `bg_alpha < 1` (`text_cov` is scalar there), and `bg_alpha == 1`
  makes `a` 1 whatever they are.
- **`u_cell_size` is `highp` in the fragment stage by necessity.** It is the one uniform both stages
  declare (`u_projection` is vertex-only), one per program, so its precision must match: the
  fragment stage is `mediump float` and the vertex stage defaults to `highp`, and an unqualified
  `vec2` fails to link ("Precisions of uniform 'u_cell_size' differ"). The fragment-only glyph-box
  uniforms (`u_char_size`, `u_char_offset`, `u_line_thickness`, `u_dots_per_cell`) are `highp` too,
  with no link constraint behind them.
- **Subpixel coverage is gated twice** (#961): only over an opaque background — one alpha cannot
  carry three coverages — and never for a colour emoji, whose RGB is its own colour. For foreign ink,
  also never for a background-class owner, whose slot RGB is not a mask (a builtin glyph keeps white
  there).

### The underline and strikethrough marks (`hline`, `xgate`, the style chain)

- **A band is a solid fill, not a tent** (#515). `hline` gives the band full coverage between its
  edges, with a half-pixel ramp (`0.5 * fwidth(gy)`) at each. It used to be `1 - smoothstep`, a beamterm port
  (#267): the tent peaks at 1 only at the exact centre and has no plateau, so a sub-pixel band
  integrates below 1 and the line read grey at small cells (measured 118/255 at dpr 1). Every GPU
  terminal (kitty, ghostty, wezterm) draws a straight line as a solid pixel-snapped fill. The band's
  centre is pulled inside `[0,1]` so it never spills into the next row — the invariant alacritty
  holds with `max_y` and this renderer did not — and the half-pixel ramp keeps the edge crisp rather
  than stair-stepped at fractional DPR. **The band's position is not rounded to a pixel row**: `top`
  is only clamped, so a band at a fractional position covers its two edge rows partially. The shader
  comment this came from said the band is "snapped to the pixel grid", which the code does not do.
- **The thickness is a device-px rule from the font size** (#517): `u_line_thickness =
  max(1, round(font_size * dpr / 15))`, computed host-side — xterm.js's rule (`TextureAtlas.ts`,
  `max(1, floor(fontSize*dpr/15))`), the right reference because it is a Canvas renderer under this
  one's constraint: no font file, so no `underline_thickness` metric. The old `0.05 * box`
  half-thickness was a beamterm inheritance (#267), about twice too heavy (11.3% of the cell against
  xterm's 6.2%). `hline` works in device px and divides by `char_h` (`u_char_size.y`) only to reach
  glyph-box space, so the thickness tracks the font size and `lineHeight` or font family cannot
  distort it.
- **Positions are fixed fractions of the glyph box, in glyph-box space** — underline centre 0.88,
  strikethrough 0.5. Deriving them from font metrics is not available: Canvas 2D exposes no
  `underline_position` (#517), so a better fraction is a later refinement. They are glyph-local, not
  cell-local: with `lineHeight = 1.5` a cell-local 0.88 would drop the underline far below its text.
  That space is also what keeps the band inside the cell under a tall `lineHeight` — `gy` is bounded
  to the box, so `hline`'s centre-clamp holds without the cell-relative `max_y` alacritty needs. The
  glyph's own coverage no longer needs the glyph-box uniforms (#359 bakes the offset into the bitmap);
  its decorations still do. The two spaces coincide at the default (#338).
- **The style displaces or gates the one band; it never adds a draw except for double** (#829,
  #830). Curly displaces the centre; dotted and dashed gate the same band along x (`xgate`); double
  is the only one that adds a band — what the maintainer settled when #830's "make no new structural
  decision" rule met it. So a curl is the same band, ink and thickness, and everything #513 / #525 /
  #712 settled about the channel keeps holding.
- **The style chain is total over all eight representable values, not six.** `attrs.rs` forwards
  the raw 3-bit field and names none of them on purpose, and that crate is published separately —
  `apply_frame` takes any `u16` from any caller. So 0, 1, 6 and 7 fall through to a straight band.
  For 1, 6 and 7 that is what `UnderlineStyle::from_bits` normalises them to one crate away; **0 is
  reconciled by a different mechanism** — `from_bits(0)` is `None`, not a single, and what makes the
  two agree is the `underline` bit, which core's one writer (`set_underline_style`) arms with the
  style. An earlier comment said `from_bits` normalises all four to a band, which a refuting pass
  measured false. An `else if` ending at 5 with no fall-through would break the agreement in silence:
  a mark that vanishes on a malformed input.
- **`xgate` takes its ramp as an argument** because a caller gates a *wrapped* coordinate: the
  derivative of `fract(x)` spikes at the seam, and a ramp derived from it (`fwidth(t)`) draws a
  visible line there. The caller passes the ramp of the unwrapped coordinate (`aa`).
- **Curly: one cycle per cell.** `sin` is 2π-periodic, so a row-continuous x and a per-cell x agree
  at every boundary and everywhere else. A first draft added the cell's column to the phase to "join
  the curls" and a mutation proved the term dead — removing it changed no pixel. Per-cell is what the
  two references that draw a curl do, each baking one per cell (ghostty a sprite codepoint, xterm.js
  a stroke into the glyph atlas). The curl oscillates **upward** from 0.88 so its lowest point sits
  where the straight band does and `hline`'s centre-clamp holds at the bottom of the box. The
  amplitude carries a device-px floor for #515's reason: a curl a fraction of a pixel tall *is* a
  straight line, and every pixel assertion would still pass.
  **Its antialiasing is vertical, measured rather than assumed**: `hline`'s ramp comes from
  `fwidth(gy)`, a function of the vertical coordinate alone, which is exact for a horizontal edge and
  narrower by `cos(theta)` across the curl's diagonal one. Max slope is
  `2*pi*max(line_thickness,1) / cell_width` — `char_h` cancels, so it is fixed by the cell's
  *aspect*: about 0.79 px/px at an 8x16 cell (theta ~38°, cos ~0.79). The vertical extent stays
  constant across the curl (browser proof, `steepOverFlatExtent` at font 48: 1.00 / 0.90 / 1.00 /
  0.93 over dpr 1 / 1.1 / 1.5 / 2, against a straight-band control of exactly 1); a
  perpendicular-correct shader would read about 1.27. So the band is about 21% thinner
  perpendicularly where the sine is steepest — 0.2–0.4 px at the default font, sub-pixel, reaching
  ~1 px only at very large fonts. **Deliberately not asserted**: the ratio is the current answer, and
  correcting the ramp for slope would move it to ~1.27, so a test pinning 1.00 would redden on the
  fix. The measurement is published in the proof's `measured` block instead.
- **Double: the lower band where the single sits, the second above it.** The separation is 2 of 3:
  xterm.js `yBotDefault = yTopDefault + lineWidth * 2` (`TextureAtlas.ts:590-591`) and ghostty "one
  above ... and one below by one thickness" (`special.zig:57-70`) agree; alacritty straddles the
  descent at 0.25 / 0.75 of it (`rects.rs:82-83`), a metric this renderer does not have — no font
  file, #517's reason. The **direction** is `hline`'s clamp, not a preference: `top` is clamped into
  the glyph box, so a pair placed downward has both bands pulled to the same `top` and collapses into
  one line, with no error and a pixel assertion that still sees an underline. Going up is free, which
  is also why the curl oscillates upward. ghostty centres the pair and can, because it bakes into a
  canvas with padding below the cell; xterm.js, which does restrict to the cell height, shifts the
  pair up so the bottom band lands where the single would — the same answer under the same
  constraint, and justerm is permanently in that regime.
  **`2 * thickness` does not survive this rasteriser**: at the default 16px font and dpr 1 the thickness is one device
  pixel, so the bands sit 2px apart with a 1px gap, and `hline`'s half-pixel ramp on each edge closes
  it — the proof read `doubleBandHistogram: {"1": 96}`, one band at every column, the merged run's
  centre exactly 1px above the single's — so following the references' number would ship a double
  underline that is a slightly thicker single one at the size almost every user runs. So the separation is `max(2 * thickness, thickness + 2px)`:
  the reference rule wherever it has room, and a floor of **one** device pixel of clear air where it
  does not (the nominal gap is two, and the two ramps spend half a pixel each — a refuting pass caught
  an earlier sentence claiming two). The floor binds at one-pixel thickness only: `max(2,3) = 3`,
  `max(4,4) = 4`, `max(6,5) = 6`. A deliberate divergence with a measurement behind it, on the
  tie-breaker's "renderer cell composition is justerm's own model" row.
- **Dotted: a whole number of dots per cell, computed host-side** (#830). `u_dots_per_cell` comes
  from `metrics::dots_per_cell`, so the pattern is cell-periodic by construction and the cell-local
  `v_tex.x` needs no cross-cell phase — the term #829 proved inert stays deleted. The two references
  whose period is not a whole cell pay with cross-cell state (xterm.js's `variantOffset`,
  alacritty's every-two-cells inversion); ghostty quantises as this does. A shader comment used to
  send the reader to xterm.js's `variantOffset` as *the* answer; #830 took ghostty's route instead.
  The count is computed in Rust, not in GLSL, and not only to keep it testable: GLSL ES 3.00 leaves
  `round()` implementation-dependent at exactly 0.5 (`roundEven` is the defined one), so an odd cell
  width could give a different dot count on a different GPU.
  **At or above a 2px dot** the dot is centred in its period, so `fract`'s seam falls inside the gap
  where coverage is zero on both sides; anchoring it at [0, 0.5) would put a hard edge on the
  discontinuity, a seam at every dot. **Below 2px it is pixel-aligned and hard**, and both halves of
  that were measured wrong first. Antialiasing a 1px dot erases the mark: at 16px the cell is 8 device
  px and `dots_per_cell` gives 4, so a half-pixel ramp on each side fills the whole 0.5-wide gate —
  the proof read `dottedDuty: 1.0`, `dottedRuns: 1`, a solid line with every "is drawn" check green.
  alacritty splits on exactly this and is where the threshold comes from: `draw_dotted` is a hard
  per-pixel on/off below a 2px thickness and `draw_dotted_aliased` is used at or above it
  (`rect.f.glsl`); `aa = 0` makes `xgate` a hard step, the same split. And the hard gate is
  **half-open from the start of the period**, not centred: with a 2px period the fragment centres land
  on `fract == 0.25` and `0.75`, exactly a centred gate's two edges and symmetric about the dot, so a
  symmetric test admits both or neither — measured `dottedRuns: 0`. Half-open breaks the symmetry,
  the asymmetry alacritty gets from a parity test on a pixel index. **It must not be that pixel index,
  and a refuting pass measured why**: alacritty's period is the integer 2, ours is `cell_w / n`, a
  whole number of pixels only when the cell width is even. On an odd cell `mod(pixel, period)` drifts
  across the cell and the residue walks out of the gate — computed over the shipped arithmetic, an
  11px cell lit columns 0, 2, 4 and nothing from 5 to 10, and a 17px cell lit 0, 2, 4, 6 and nothing
  from 7 to 16 — invisible to the cross-cell check by construction, because the drift is identical in
  every cell. The normalised coordinate tiles the cell exactly whatever `n` is. The two branches
  differ in phase by a quarter period, which is safe because the branch is chosen from
  `u_cell_size.x` and `n`, both uniform over the grid.
- **Dashed: one period per cell, the dash at the two outer quarters**, so adjacent cells' dashes join
  into one half-cell dash separated by a half-cell gap — alacritty's construction, whose comment
  states the reason: "since dashes of adjacent cells connect with each other our dash length is half
  of the desired total length" (`rect.f.glsl`, `draw_dashed`). All three references are cell-periodic
  for dashed, so unlike dotted this needed no decision.

## Code

- `justerm-renderer/src/frame.rs` — `pack_instances`, the instance layout
- `justerm-renderer/src/preedit.rs` — `patch`, `WriteKind`, `Span`: the composition's copy-on-write,
  applied before anything here resolves, and the one place that decides which cells are the pass's
- `justerm-renderer/src/suggestion.rs` — `is_blank`, `writes`, `patch`: the consumer's suggestion run
  (#972), applied only when no composition is open
- `justerm-web/src/terminal.ts` — `Terminal.setSuggestion`, `paintSuggestion`: the anchor and the
  alternate-screen drop
- `justerm-renderer/src/render_policy.rs` — `ColorPolicy`, `resolve_cell`, `dim_foreground`
- `justerm-renderer/src/overlay.rs` — `HighlightKind`, `composite_bg`, `blend_over`,
  `should_blend_kind`
- `justerm-renderer/src/decoration.rs` — `DecorationLayer`, `DecorationRect`,
  `decoration_override_at`
- `justerm-renderer/src/contrast.rs` — `ensure_contrast_ratio`
- `justerm-renderer/src/palette.rs` · `attrs.rs` · `color.rs` — the reference→colour step and the
  attribute decode
- `justerm-renderer/src/webgl.rs` — `packs`, and the policy setters that feed it
- `justerm-renderer/src/shader.rs` — `FRAG_SRC`, the composite chain and the marks; `VERT_SRC`, the
  instance unpacking (both browser-consumed, host-compiled since #989)

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Renderer ink channels](../../agents/reference-facts.md#renderer-ink-channels)
- [How a translucent background composites](../../agents/reference-facts.md#how-a-translucent-background-composites-317-2-verified-2026-08-18)

**Read ADR-0019's own framing before comparing to xterm.js here:** it is a *design input*, not a
validator. In the four decisions before 0019 it was silent, self-contradictory across its own call
sites, the outlier, or demoted — so a difference from it is not by itself a defect.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — this is where every span in the family finally meets the flags: the three overlay lookups, both
  decoration layers and the caret all resolve their pair through one helper (#454)
- [composition is browser-owned state](../invariant/composition-is-browser-owned-state.md) — the
  suggestion is hidden for a whole composition, which only the widget can see (#972)
- [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md) — the suggestion's
  "blank" test is the by-cell form of it, plus the flags that ink an empty cell (#972)
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — this crate is outside the root workspace, so no `--workspace` or `--all` command reaches it;
  every gate it has is named for it by `--manifest-path`

## Blast radius

- [decoration](decoration.md) — its R1–R6 decide what arrives here; the Bottom/Top split is the
  handshake between the two
- [active match](active-match.md) · [selection](selection.md) · [search](search.md) — the highlight
  layer, and where their overlap is finally resolved
- [caret report](caret-report.md) — a cell-invert caret would be a compositing step; this renderer
  draws it as an overlay instead, so the two stay separable
- [wire format](wire-format.md) — consumes decoded cells; a colour-encoding change lands here first
- [cell geometry](cell-geometry.md) — supplies the box each instance is drawn into
- [glyph atlas](glyph-atlas.md) — supplies the resolved slot this packer assumes; since #791 a slot
  is taller than its cell, and this packer hands each cell its neighbours' slots as well as its own

## Known holes / open

- **`preedit::caret_col`'s right-edge step-back has lost its stated reason.** At the edge it puts the
  caret on the last glyph's *lead* rather than its spacer, because "a block caret spans one column on
  a spacer and inverts the right half of the glyph being composed" — measured then on a 106-column
  grid, where asking for column 104 or 105 returned the spacer, 105. Since `cursor_span` applies the
  pair rule (#454, [caret drawing](caret-drawing.md)), a caret on either half covers the whole glyph,
  so the step-back is now redundant rather than necessary. The behaviour is unchanged and harmless;
  whether to keep it is undecided.
- **A suggestion's zero-width codepoint takes a cell of its own** (#972). The width is per codepoint, as
  for the preedit, so a combining mark measures narrow here while core attaches it to the previous cell;
  a decomposed (NFD) suggestion therefore draws one cell per mark and disagrees with what the shell echoes
  on accept. ADR-0028 excuses this for the preedit because it re-renders on every keystroke; a suggestion
  lives longer. Reach unmeasured.
- **The policy setters have no records.** `set_bg_alpha`, `set_minimum_contrast_ratio`,
  `set_bold_to_bright`, `set_selection_foreground` each change what a cell resolves to, and ADR-0019
  governs the *model* rather than the individual knobs.
- ~~**The record feeding the Bottom/Top layers may not be accepted yet**~~ — **accepted 2026-08-18
  (#502)**. ADR-0024's `Status:` line remains the place to check and is still not restated here;
  what changes is that the layers' feed is no longer governed by a proposal.
- **A mark's z-order is only observable where a *declared* colour splits it from the glyph's ink.**
  A strikethrough has no declared-colour regime at all (#525), so its position relative to the glyph
  cannot be asserted by a proof on an ordinary cell — rule 6 states it, and only a cell where a
  glyph-only treatment moves `fg` away from the line inks (a selected tile, #513's own case) could
  see it. Recorded because the natural next test to write here is one that cannot fail.
- **The "every frame re-packs" property is load-bearing and unrecorded.** It is why incremental
  repaint work from the previous renderer was deliberately not ported, and it lives in no record.
