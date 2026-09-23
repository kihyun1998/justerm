# Territory — built-in block glyphs

## What it is

The glyphs the renderer draws itself rather than asking the font for, **to the cell instead of to
the ink box**: box drawing `U+2500`–`U+257F`, the block elements `U+2580`–`U+259F`, and Symbols for
Legacy Computing `U+1FB00`–`U+1FB9F` except the reserved `U+1FB93` (sextants, smooth-mosaic wedges,
one-eighth and extra-eighth blocks, regional shades, pattern fills, diagonal hatches, triangular
halves). They are the characters TUIs use to tile a region, and tiling is exactly what breaks when a
glyph is drawn to its own bounds.

The module is large and mostly tests; it holds four things under one name — block and eighth fills,
box drawing, the Legacy Computing polygons and fills, and a general raster primitive
(`fill_polygon` + `fill`) that four of those drawers call.

## Governing decisions

- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md#coordinate-spaces-device-pixels-are-the-source-of-truth)
  decides this module, in two paragraphs of that section: "Block elements `U+2580`–`U+259F` never
  reach the font…" (the interception, the geometry mirrored from alacritty, flat-alpha shades rather
  than a dither) and "**Shades and pattern fills (#367).**" (the regional shades and the pattern
  fills, settled by measurement). The reasons are there, not restated here.
- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) is
  the *cause* — it establishes the cell/glyph box split this module exists to escape — but it decides
  nothing about intercepting a character range

## Design model

- **The problem, stated as the module states it.** `U+2580`–`U+259F` are meant to tile: a region of
  `█` is one solid fill, `▀▄▌▐` halve the cell exactly, `▖▗▘▝` quarter it. The browser draws them as
  glyphs and the renderer masks every glyph to its **ink box** — so as soon as `letterSpacing` or
  `lineHeight` moves the cell away from the ink box, the fills stop meeting.
- **And it is worse than a gap.** The renderer *measures* its cell's height by ink-scanning `█`. At
  `lineHeight = 1.5` the very glyph that defines the cell no longer fills it — the measurement and
  the drawing disagree about the same character.
- **Both references do the same thing, and that is unusually strong agreement**: xterm.js intercepts
  the range ahead of the font with `CustomGlyphRasterizer` at `deviceCellWidth × deviceCellHeight`;
  alacritty's `builtin_font::builtin_glyph` draws at `average_advance + offset.x` by
  `line_height + offset.y`. Both draw at **cell** size.
- **So the range is intercepted before the font is consulted** and drawn geometrically to the cell.
  The font's own version of these characters is never used.

- **A builtin bitmap does not go through the font, so it does not go through the font's placement
  either** — `pad()` lays it into the slot directly, and since #791 the slot's cell band starts past
  a bleed band as well as past the guard band. That origin was wrong for exactly one commit and the
  symptom was this territory's whole reason for existing: a run of `█` stopped meeting its
  neighbour. Anything that changes the slot's shape has to move `pad()` with it; a grep for the
  geometry helper's name will not find this site, because it does not call it.

- **`owns` is a hand-written range, not `block_glyph(cp, 1, 1).is_some()`.** The call site runs once
  per cell per frame, and the drawing version allocates and rasterises a bitmap: ~190 ns measured,
  ~1.9 ms per frame at 10 000 cells. The two are held equal by a test that walks the whole plane
  (`owns_is_exactly_what_block_glyph_draws`, which runs in well under a second because ownership is
  size-independent), so the cost of the equivalence is paid in the test rather than per frame.

### Block, sextant and eighth fills

- **Sextant masks are derived, not transcribed.** Unicode enumerates 60 of the 64 six-bit
  combinations — `000000` is a space, `111111` is `█`, and `010101` / `101010` would duplicate
  `▌` / `▐` — so a codepoint is a plain index into that filtered list. The six thirty-literal lists
  alacritty spells out were cross-checked against the derivation once (all 180, zero mismatches), and
  VTE and kitty derive the masks the same way, skipping at the same two masks.
- **Sextant rows are equal thirds**, the first two `round(h/3)` and the last the remainder, so the six
  cells tile exactly for any `h >= 3`. Below 3 a 2×3 mosaic cannot show three rows: the lower bands
  come out empty, and no pixel is ever lit twice. xterm alone divides `3/8, 2/8, 3/8`, landing the
  row boundaries on the eighth-block grid at the cost of a thinner middle row; its source states the
  fractions and never says why. These are Teletext 2×3 mosaic glyphs, so a uniform mosaic pixel is
  held to matter more than agreeing with `▄`.
- **The last sextant row uses `saturating_sub` on purpose.** alacritty computes `height - 2*y_third`
  in `f32` with no floor — `-1` at `h = 1`, which wraps when cast to `usize`. The saturating form is
  hardening, not a transcription slip; do not "fix" it back to the reference's arithmetic.
- **One-eighth blocks place each rectangle from its two boundaries** (`edge * w / 8`), not from a
  width, so adjacent eighths tile with no cumulative rounding gap. On a cell narrower than 8 px an
  interior eighth can round to zero width and vanish. (The sextant rows are *not* sized this way —
  they are sized by a rounded width with the remainder last.)
- **`U+1FB90` INVERSE MEDIUM SHADE is a flat `128` over the whole cell**, the same as `▒`: it is the
  `▒` dither phase-flipped, still 50 % coverage, so under the flat-alpha rule it is visually
  identical to `▒`.
- **`fill` clips the far edge** of a rectangle to the bitmap, as alacritty's `draw_rect` clamps it, so
  a rounded-up extent on an odd cell never wraps onto the next row (pinned by
  `a_rounded_up_extent_is_clipped_rather_than_wrapping_onto_the_next_row`).

### Box drawing

- **`BOX_ARMS` was generated mechanically** from alacritty's four stroke-arm match arms and is never
  hand-edited: copying ~200 literals by hand invites a plausible-forever typo (#363's lesson). The
  tests re-check it against each character's meaning.
- **Strokes snap to whole pixels.** A 1 px line on a fractional midline would blur under the atlas's
  texture filtering. Each arm runs from the cell edge to the **far** side of the perpendicular
  strokes, so a corner's two arms overlap at the centre, a junction is one connected shape, and a run
  of `─` is unbroken across the cell seam.
- **No glyph is blank on a 1 px cell — a module invariant.** The left/up arm length is the far edge of
  the perpendicular strokes, which collapses to `floor(centre) = 0` on a 1 px cell with no
  perpendicular arm; a left/up terminal (`╴ ╸ ╵ ╹`) would vanish where its right/down mirror
  (`╶ ╷`, sized from `w - x` / `h - y`) shows. The `.max(1)` on those two arm lengths makes a present
  arm light at least one pixel, as the block glyphs' `.max(1)` and dash `max(1)` already do. Only
  `w` or `h` = 1 is affected.
- **Diagonals `╱ ╲ ╳` overshoot their corners by half a stroke and are clipped back**, meeting the
  diagonally-adjacent cell's band at the shared corner; alacritty instead draws Xiaolin Wu lines on a
  canvas grown into the neighbouring cells. Their band is a **true perpendicular** stroke of width
  `stroke` at any aspect — a deliberate divergence from alacritty, whose Wu loop offsets the line
  vertically so its diagonals thin to `stroke·cosθ` and read lighter than `─`/`│` on a tall cell.
  Constant perpendicular weight matters more for a line-drawing family than reproducing that
  artefact.
- **The rounded-corner X-mirror runs `1..=h`, not alacritty's `1..h`.** A horizontal flip must reach
  the last row too, or a `╰`/`╭` on a wide-short cell keeps the base `╯`'s ink on its bottom row.
  alacritty's cells are always tall enough that the last row is blank, so its off-by-one never shows;
  a `letterSpacing`-widened cell here can reach it.

### Legacy Computing polygons

- **`WEDGES` and `OCTANT_BLOCKS` come from xterm only** (`CustomGlyphDefinitions.ts` `PATH` /
  `VECTOR_SHAPE` and `SOLID_OCTANT_BLOCK_VECTOR` entries); alacritty draws none of them. They are read
  as vertex / rectangle **lists**, not as a coverage spec — the fill rule is `fill_polygon`'s, not
  xterm's Canvas2D. Unlike the sextant masks or the box arms there is no bit-rule to derive them from,
  so they are genuine lookup tables guarded by independent, name-derived oracle tests rather than by a
  derivation.
- **The diagonal hatch is a band over `fill_polygon`, not a new stroke primitive** (the choice recorded
  on #366: a Wu stroke would only sharpen endpoint anti-aliasing, invisible at cell scale). Its lines
  are a **one-device-pixel** hairline (xterm's `strokeWidth: 1`), not the box-line stroke the solid
  diagonals use: the hatch is a fill texture, and a heavier line at quarter-cell spacing would merge
  into a solid block. Below about 8 px a hatch goes near-solid regardless, because it can no longer
  resolve its lines.

### `fill_polygon` — the one primitive with no reference

- **Why it exists:** a diagonal at cell scale must be anti-aliased, which a rectangle table cannot do.
  **Neither reference supplies it** — alacritty anti-aliases its diagonals with Xiaolin Wu *lines* and
  fills only rectangles; xterm.js fills polygons through Canvas2D `ctx.fill()`, delegating the
  coverage rule to the browser — so the area-coverage rule is this module's own: a scanline fill,
  exact in x (analytic span overlap) and supersampled in y. Vertices are `f32` so a slope need not
  land on a pixel boundary.
- **`POLY_SS = 4` sub-rows, y only.** Horizontal coverage is analytic, so only the vertical axis is
  sampled; four sub-rows suffice at cell scale, and each glyph rasterises once into the atlas, so the
  cost never reaches a hot path. Each sub-scanline samples at its sub-row centre — a midpoint rule,
  which integrates a linear span length exactly, so a triangle's coverage is unbiased.
- **The crossing test is half-open** (`<=` on both endpoints): an edge counts iff the scanline
  separates its endpoints, so a vertex shared by two edges is crossed once, never twice, and a
  horizontal edge is skipped.
- **Even-odd is correct for every shape it backs** — not because those shapes avoid
  self-intersection (xterm draws `1FB9A`/`1FB9B` as single-ring bowties touching at the centre), but
  because none has two overlapping loops of equal winding (a pentagram), the one topology where
  even-odd and non-zero diverge. Concave rings (`1FB68`–`1FB6B`) and self-touching ones are fine.
- **Pass a seamless shape as ONE ring; never split it across calls.** Coverage is max-combined into
  alpha (alacritty's brighter-wins `put_pixel`), which bounds genuine overlaps (two crossing strokes)
  at 255 but does **not** merge two polygons that *abut* at a fractional edge in one buffer: each
  paints about half the boundary pixel and `max` keeps one half, so a seam remains (pinned as
  intended by `abutting_polygons_in_one_buffer_seam_by_design`). Complementary halves reassemble only
  across *separate* cells, where their coverage sums optically over the cell boundary — the tiling
  the wedges and diagonals rely on, proven by
  `complementary_triangles_partition_the_cell_with_no_gap_or_overlap`.
- **Keep `fill` and `fill_polygon` to disjoint regions of one buffer.** `fill` overwrites where
  `fill_polygon` max-combines, so mixing them over the same pixels is draw-order dependent.

### Test discipline

- **A test asserts from the character's name or meaning, never by recomputing what the code
  computes.** `picture` is read by eye against what the character means, and `sample_frac` samples a
  polygon glyph at a point the character's name implies — an oracle independent of how
  `fill_polygon` computes it. The rule recurs across the test module; it is the reason the tables
  above are guarded at all.

## Code

- `justerm-renderer/src/builtin.rs` — the whole territory: `owns`, `block_glyph`, `box_glyph`,
  `wedge_glyph`, `octant_block`, `shade_glyph`, `diagonal_hatch`, `fill_polygon`, `fill`
- `justerm-renderer/src/rasterizer.rs` — the interception site: `Rasterizer::rasterize` asks
  `Rasterizer::builtin` (which calls `block_glyph` for a lone, non-wide character) before the font is
  touched, and lays the result into the slot with `pad()`; `finish` skips the LCD pass for a builtin
- `justerm-renderer/src/glyph_class.rs` — `treat_glyph_as_background_color` unions `builtin::owns`
  (#507)

## Reference behaviour

**None** in `docs/agents/reference-facts.md` — the comparisons below were each made once, in prose,
with a `file:line` and no SHA. That is the shape this map treats as most fragile: a comparison in a
place nothing re-checks, describing upstream code that can move.

- Interception at cell size: xterm.js `CustomGlyphRasterizer` (`deviceCellWidth × deviceCellHeight`);
  alacritty `builtin_font::builtin_glyph` (`average_advance + offset.x` × `line_height + offset.y`)
- Block geometry mirrors alacritty `builtin_font.rs:394-499`; shade alphas are its
  `COLOR_FILL_ALPHA_STEP_*` / `COLOR_FILL` (`builtin_font.rs:10-15`)
- Sextant masks: alacritty's literals `builtin_font.rs:509-572`; the same derivation in VTE
  `minifont.cc:1682` and kitty `decorations.c:2171`; bit order is xterm's `sextant(0b000001)`
  (`CustomGlyphDefinitions.ts:465`)
- Sextant rows: equal thirds in alacritty (`builtin_font.rs:505-507`), VTE (`minifont.cc:678`),
  wezterm (`customglyph.rs:5368`) and kitty (`decorations.c:1591`); `3/8, 2/8, 3/8` in xterm
  (`CustomGlyphDefinitions.ts:888-889`)
- Box drawing: `BOX_ARMS` from alacritty `builtin_font.rs:162-216`; stroke width
  `builtin_font.rs:53,977`; arm joins `:226-242`; dashes `:111-152`; doubles `:247-348`; rounded
  corners `:350-393` + `draw_rounded_corner` `:890-954`; diagonals as Wu lines `:60-106`
- Wedges, one-eighth blocks, pattern fills: xterm only (`CustomGlyphDefinitions.ts`); vertex
  scaling confirmed against xterm's `CustomGlyphRasterizer.ts` transform. The diagonal hatch is xterm's
  `strokeWidth: 1` `PATH_FUNCTION`s — nine parallel segments overshooting the cell, a quarter cell
  apart in intercept, the density this module keeps
- `fill_polygon`: alacritty's Wu line `builtin_font.rs:818` and `put_pixel` `:807`; xterm.js
  Canvas2D fill `CustomGlyphRasterizer.ts:287`
- The ring diff of all 44 smooth-mosaic wedges against xterm's source was done once at review time
  and is not pinned by any test (the tests pin the named corner and the third-grid line per family)

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — this crate is outside the root workspace, so no `--workspace` or `--all` command reaches it;
  every gate it has is named for it by `--manifest-path`
- **The drawer owns the "background-shaped ink?" answer** (#507, see
  [colour policy](colour-policy.md)): `glyph_class` *asks* `builtin::owns` instead of copying its
  ranges, so a codepoint added here is classified correctly the day it is added. `emoji.rs`'s
  `1FB00..=1FBFF` is deliberately **wider** than `owns` — it answers a different question (text
  presentation, whoever draws it) — and must not be narrowed to match

## Blast radius

- [cell geometry](cell-geometry.md) — the cell/glyph box split is the *cause* of this territory. If
  the nesting rule changes, this module's reason to exist changes with it
- [glyph atlas](glyph-atlas.md) — these glyphs occupy atlas slots like any other, but are produced
  rather than rasterised from a font
- [cell compositing](cell-compositing.md) — a built-in glyph is still a glyph field in the instance

## Known holes / open

- **Most of the module has no public surface.** Nothing in it is `#[wasm_bindgen]`-exported, so none
  of it is addressable from outside the crate; ADR-0018 records the ranges and the fill rules, but
  box drawing's own choices (the diagonal overshoot, the 1 px `.max(1)`, the `1..=h` mirror) are
  recorded only here.
- **The reference agreement is quoted, not pinned.** Every citation under `## Reference behaviour`
  is a `file:line` with no SHA behind it.
- **No published document tells a user these characters are intercepted.** The ranges are a constant
  in the code and a paragraph in an internal ADR, so a font that draws these characters well is
  overridden anyway, and a user cannot learn that from the published docs.
- **The diagonal overshoot-and-clip has lost its stated reason.** It was justified as "an atlas glyph
  cannot spill past its cell"; since #791/#966 the slot carries bleed bands for ink that leaves the
  cell ([cell geometry](cell-geometry.md)). `block_glyph` still draws at cell size, so the choice
  still stands in the code, but nothing now says why it is preferred over spilling into the bleed
  band as alacritty's grown canvas does.
- **"Real cells are 16–33 device px" is unverified.** The comments cited that range three times (the
  sextant `h < 3` degeneracy, the one-eighth `< 8 px` vanish, and `POLY_SS = 4` sufficing); nothing
  owns it and #962 changed how the cell width is sized, so it has not been carried here as a fact.
