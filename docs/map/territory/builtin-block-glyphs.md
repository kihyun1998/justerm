# Territory — built-in block glyphs

## What it is

The glyphs the renderer draws itself instead of asking the font, **to the cell rather than to the
ink box**: box drawing `U+2500`–`U+257F`, the block elements `U+2580`–`U+259F`, and Symbols for
Legacy Computing `U+1FB00`–`U+1FB9F` except the reserved `U+1FB93` (sextants, smooth-mosaic wedges,
one-eighth and extra-eighth blocks, regional shades, pattern fills, diagonal hatches, triangular
halves). They are the characters TUIs tile a region with, and tiling is what breaks when a glyph is
drawn to its own bounds. The module is large and mostly tests; it holds four things under one name —
block and eighth fills, box drawing, the Legacy Computing polygons and fills, and a raster primitive
(`fill_polygon` + `fill`) those drawers share — plus `owns`, the range answer other modules ask.

## Governing decisions

- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md#coordinate-spaces-device-pixels-are-the-source-of-truth)
  decides this module in two paragraphs of that section: "Block elements `U+2580`–`U+259F` never
  reach the font…" (the interception, geometry mirrored from alacritty, flat-alpha shades rather than
  xterm's dither) and "**Shades and pattern fills (#367).**" (regional shades and pattern fills,
  settled by measurement). The reasons are there, not restated here.
- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) is
  the *cause* — it sets up the cell/glyph-box split this module exists to escape, and records the
  "builtin `█` never re-enters measurement" invariant — but decides nothing about intercepting a
  range.

## Design model

Status: the interception, ranges and shade rule are recorded in ADR-0018; everything below the first
three bullets is from source and tests only.

- **The problem.** `U+2580`–`U+259F` are meant to tile: a region of `█` is one solid fill, `▀▄▌▐`
  halve the cell, `▖▗▘▝` quarter it. A font glyph is drawn into the slot at the glyph box, so once
  `letterSpacing` or `lineHeight` moves the cell away from that box the fills stop meeting. Until
  #986 the cell's height was itself measured by ink-scanning the font's `█`, so at `lineHeight = 1.5`
  the glyph that defined the cell no longer filled it (`rasterizer.rs` module docs).
- **Both references draw this range at cell size** — xterm.js `CustomGlyphRasterizer` at
  `deviceCellWidth × deviceCellHeight`, alacritty `builtin_font::builtin_glyph` at
  `average_advance + offset.x` by `line_height + offset.y` — so the range is intercepted before the
  font is consulted and the font's version is never used.
- **Interception site.** `Rasterizer::rasterize` asks `Rasterizer::builtin` first, for a lone scalar
  in a non-wide cell; a cluster (`█` + a mark) goes to the font. `Rasterizer::finish` returns a
  builtin bitmap without an LCD mask (ADR-0033).
- **`pad()` places a builtin bitmap, not the font's placement.** It lays the cell-sized bitmap past
  the guard band and the bleed band (`margin_x` across, `PADDING + bleed_y` down) — the same origin
  `metrics::cell_uv` gives the shader. That origin was wrong for one commit during #791 and the
  symptom was this territory's reason to exist: a run of `█` stopped meeting its neighbour. `pad()`
  does not call the slot-geometry helper, so a grep for that helper's name does not find this site;
  anything that reshapes the slot has to move `pad()` with it.
- **`owns` is a hand-written range, not `block_glyph(cp, 1, 1).is_some()`.** It is called per cell
  per frame (`frame.rs` `pack_instances` → `treat_glyph_as_background_color`), and the drawing
  version allocates and rasterises: ~190 ns, ~1.9 ms per frame at 10 000 cells (#507, 2026-08). The
  two are held equal by `owns_is_exactly_what_block_glyph_draws`, which walks every codepoint below
  `U+20000` — cheap because ownership is size-independent.
- **Input bound:** a codepoint and the cell size. Each glyph allocates one `w × h` RGBA buffer, and a
  degenerate size (`w` or `h` = 0) returns `None`; the cell is bounded by the rasteriser's geometry,
  not here. Each glyph rasterises once into the atlas, so none of this is on a per-frame path except
  `owns`.

### Block, sextant and eighth fills

- **Sextant masks are derived, not transcribed** (`sextant_mask`). Unicode enumerates 60 of the 64
  six-bit combinations — `000000` is a space, `111111` is `█`, `010101` / `101010` would duplicate
  `▌` / `▐` — so a codepoint is a plain index into the filtered list. alacritty's literal lists were
  cross-checked against the derivation once (all 180, zero mismatches, #361); VTE and kitty derive
  the same way and skip the same two masks.
- **Sextant rows are equal thirds**: the first two `round(h/3)`, the last the remainder, so the six
  cells tile for any `h >= 3`. Below 3 the lower bands come out empty and no pixel is lit twice. xterm
  alone divides `3/8, 2/8, 3/8`, landing rows on the eighth grid at the cost of a thinner middle row,
  and its source never says why; these are Teletext 2×3 mosaic glyphs, so a uniform mosaic pixel is
  held to matter more than agreeing with `▄` (#361).
- **The last sextant row uses `saturating_sub` on purpose.** alacritty computes `height - 2*y_third`
  in `f32` with no floor — `-1` at `h = 1`, which wraps when cast to `usize`. The saturating form is
  hardening; do not "fix" it back to the reference's arithmetic.
- **One-eighth blocks (`octant_block`) place each rectangle from its two boundaries**
  (`edge * w / 8`), not from a width, so adjacent eighths tile with no cumulative rounding gap; on a
  cell narrower than 8 px an interior eighth can round to zero width and vanish. The `U+2581`–`U+258F`
  eighths and the extra eighths in `block_glyph` are sized from a rounded width (`round().max(1)`), as
  are the sextant rows.
- **`U+1FB90` INVERSE MEDIUM SHADE is a flat `128` over the whole cell**, the same as `▒`: the `▒`
  dither phase-flipped is still 50 % coverage, so under the flat-alpha rule it is identical to `▒`.
- **`fill` clips the far edge** of a rectangle to the bitmap, as alacritty's `draw_rect` clamps it,
  so a rounded-up extent on an odd cell never wraps onto the next row
  (`a_rounded_up_extent_is_clipped_rather_than_wrapping_onto_the_next_row`).

### Box drawing

- **`BOX_ARMS` was generated mechanically** from alacritty's four stroke-arm match arms and is not
  hand-edited: copying ~200 literals by hand invites a plausible-forever typo (#363's lesson, applied
  in #365). The tests re-check it against each character's meaning.
- **Strokes snap to whole pixels**; a 1 px line on a fractional midline would blur under texture
  filtering. Each arm runs from the cell edge to the **far** side of the perpendicular strokes, so a
  corner's two arms overlap at the centre, a junction is one shape, and a run of `─` is unbroken
  across the cell seam.
- **No glyph is blank on a 1 px cell.** The left/up arm length is the far edge of the perpendicular
  strokes, which collapses to `floor(centre) = 0` on a 1 px cell with no perpendicular arm, so a
  left/up terminal (`╴ ╸ ╵ ╹`) would vanish where its right/down mirror (`╶ ╷`) shows. The `.max(1)`
  on those two lengths lights a present arm at least one pixel, as the block glyphs' `.max(1)` and the
  dash `max(1)` do. Only `w` or `h` = 1 is affected.
- **Diagonals `╱ ╲ ╳` overshoot their corners by half a stroke and are clipped back** (`box_diagonal`),
  meeting the diagonal neighbour's band at the shared corner; alacritty instead draws Xiaolin Wu
  lines on a canvas grown into the neighbouring cells. The band is a **true perpendicular** stroke of
  width `stroke` at any aspect — a divergence from alacritty, whose Wu loop thins its diagonals to
  `stroke·cosθ` on a tall cell; constant weight matters more for a line-drawing family than
  reproducing that artefact.
- **The rounded-corner X-mirror runs `1..=h`, not alacritty's `1..h`** (`box_rounded`). A horizontal
  flip must reach the last row, or a `╰`/`╭` on a wide-short cell keeps the base `╯`'s ink on its
  bottom row. alacritty's cells are always tall enough that the last row is blank; a
  `letterSpacing`-widened cell here can reach it.

### Legacy Computing polygons

- **`WEDGES` and `OCTANT_BLOCKS` come from xterm only** (`CustomGlyphDefinitions.ts` `PATH` /
  `VECTOR_SHAPE` and `SOLID_OCTANT_BLOCK_VECTOR` entries); alacritty draws none of them. They are
  read as vertex / rectangle **lists**, not as a coverage spec — the fill rule is `fill_polygon`'s,
  not Canvas2D's. With no bit rule to derive them from, they are lookup tables guarded by independent,
  name-derived oracle tests.
- **The checker fills `U+1FB95`/`U+1FB96` are solid quarter-cell squares, not a dither**, and
  `U+1FB97` two solid bands (ADR-0018, #367, measured bimodal at dpr 1 / 1.5 / 2).
- **The diagonal hatch is a band over `fill_polygon`, not a new stroke primitive** (#366: a Wu stroke
  would only sharpen endpoint anti-aliasing, invisible at cell scale). Its nine lines are a
  **one-device-pixel** hairline (xterm's `strokeWidth: 1`), not the box stroke: a heavier line at
  quarter-cell spacing would merge into a solid block. Below about 8 px a hatch goes near-solid
  regardless.

### `fill_polygon` — the one primitive with no reference

- **Why it exists:** a diagonal at cell scale must be anti-aliased, which a rectangle table cannot do,
  and neither reference supplies an area-coverage rule — alacritty anti-aliases only Wu *lines* and
  fills rectangles; xterm.js delegates to Canvas2D `ctx.fill()`. So the rule is this module's own: a
  scanline fill, exact in x (analytic span overlap) and supersampled in y. Vertices are `f32`.
- **`POLY_SS = 4` sub-rows, y only**, each sampled at its sub-row centre — a midpoint rule that
  integrates a linear span length exactly, so a triangle's coverage is unbiased.
- **The crossing test is half-open** (`<=` on both endpoints): a vertex shared by two edges is crossed
  once, and a horizontal edge is skipped.
- **Even-odd is correct for every shape it backs** — not because none self-intersects (xterm draws
  `1FB9A`/`1FB9B` as bowties touching at the centre) but because none has two overlapping loops of
  equal winding, the one topology where even-odd and non-zero diverge.
- **Pass a seamless shape as ONE ring.** Coverage is max-combined into alpha (alacritty's
  brighter-wins `put_pixel`), which bounds a genuine overlap at 255 but does not merge two polygons
  that *abut* at a fractional edge in one buffer — each paints about half the boundary pixel and
  `max` keeps one half (`abutting_polygons_in_one_buffer_seam_by_design`). Complementary halves
  reassemble only across separate cells, where coverage sums optically
  (`complementary_triangles_partition_the_cell_with_no_gap_or_overlap`).
- **Keep `fill` and `fill_polygon` to disjoint regions of one buffer**: `fill` overwrites where
  `fill_polygon` max-combines, so mixing them is draw-order dependent.

### Test discipline

- **A test asserts from the character's name or meaning, never by recomputing what the code
  computes.** `picture` renders a glyph as text to be read against what the character means, and
  `sample_frac` samples a polygon glyph at a point the name implies — oracles independent of how
  `fill_polygon` computes coverage.

## Code

- `justerm-renderer/src/builtin.rs` — `owns`, `block_glyph`, `sextant_mask`, `box_glyph`,
  `box_diagonal`, `box_dash`, `box_rounded`, `box_double`, `wedge_glyph`, `octant_block`,
  `shade_glyph`, `diagonal_hatch`, `fill_polygon`, `fill`; tables `BOX_ARMS`, `WEDGES`,
  `OCTANT_BLOCKS`
- `justerm-renderer/src/rasterizer.rs` — the interception: `Rasterizer::rasterize` asks
  `Rasterizer::builtin` before the font and lays the result with `Rasterizer::pad`;
  `Rasterizer::finish` skips the LCD mask for a builtin
- `justerm-renderer/src/glyph_class.rs` — `treat_glyph_as_background_color` unions `builtin::owns`
  (#507)

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. Each comparison below was made once, in prose or a
source comment, with a `file:line` and no SHA — a comparison nothing re-checks.

- Interception at cell size: xterm.js `CustomGlyphRasterizer`; alacritty `builtin_font::builtin_glyph`
- Block geometry mirrors alacritty `builtin_font.rs:394-499`; shade alphas are its
  `COLOR_FILL_ALPHA_STEP_*` / `COLOR_FILL` (`builtin_font.rs:10-15`)
- Sextant masks: alacritty's literals `builtin_font.rs:509-572`; the same derivation in VTE
  `minifont.cc:1682` and kitty `decorations.c:2171`; bit order is xterm's `sextant(0b000001)`
  (`CustomGlyphDefinitions.ts:465`)
- Sextant rows: equal thirds in alacritty (`builtin_font.rs:505-507`), VTE (`minifont.cc:678`),
  wezterm (`customglyph.rs:5368`), kitty (`decorations.c:1591`); `3/8, 2/8, 3/8` in xterm
  (`CustomGlyphDefinitions.ts:888-889`)
- Box drawing: `BOX_ARMS` from alacritty `builtin_font.rs:162-216`; stroke width `:53,977`; arm joins
  `:226-242`; dashes `:111-152`; doubles `:247-348`; rounded corners `:350-393` +
  `draw_rounded_corner` `:890-954`; diagonals as Wu lines `:60-106`
- Wedges, one-eighth blocks, pattern fills: xterm only (`CustomGlyphDefinitions.ts`); vertex scaling
  checked against `CustomGlyphRasterizer.ts`'s transform. The hatch is xterm's `strokeWidth: 1`
  `PATH_FUNCTION`s — nine parallel segments a quarter cell apart in intercept
- `fill_polygon`: alacritty's Wu line `builtin_font.rs:818` and `put_pixel` `:807`; xterm.js Canvas2D
  fill `CustomGlyphRasterizer.ts:287`
- The ring diff of all 44 smooth-mosaic wedges against xterm's source was done once at review time
  (#366) and is not pinned by a test (the tests pin the named corner and the third-grid line per family)
- Adjacent, pinned: [whether a glyph's ink may leave its cell](../../agents/reference-facts.md#whether-a-glyphs-ink-may-leave-its-cell--30-and-the-single-guard-that-withdraws-it-791-verified-2026-08-20)
  — the bleed band that undercuts the diagonal overshoot's original reason (Known holes)

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the renderer is outside the root workspace, so no `--workspace` or `--all` command reaches this
  module; every gate it has names it by `--manifest-path`.
- [every atlas-slot writer and the shader agree on one origin](../invariant/an-atlas-slot-has-one-origin.md)
  — `pad()` places a builtin bitmap at `cell_uv`'s origin with its own arithmetic; #791 broke it for one commit

## Blast radius

- [colour policy](colour-policy.md) — `treat_glyph_as_background_color` asks `owns` rather than
  copying its ranges, so a codepoint drawn here is excluded from the contrast demand and re-tinted
  under a selection the day it is added; a change to `owns` changes which cells the policy treats as
  background (#507).
- [emoji classification](emoji-classification.md) — `emoji.rs` carves `1FB00..=1FBFF` out as text,
  deliberately wider than `owns`; the two answer different questions and must not be aligned.
- [cell geometry](cell-geometry.md) — the cell/glyph-box split is this module's cause, and the slot
  origin `pad()` hard-codes is the one `metrics::cell_uv` owns; a change to the band layout must move
  both. The builtin `█` must also never feed the ink scan that sizes `bleed_x`.
- [glyph atlas](glyph-atlas.md) — a builtin glyph occupies an ordinary slot and is keyed like a font
  glyph; a re-bake on a cell change is what redraws these at the new size.
- [cell compositing](cell-compositing.md) — the tiling class this module answers is what lets a
  foreign-ink and selection-tint rule skip these cells; a builtin glyph that drew past its cell would
  meet the neighbour-ink rules it currently never triggers.

## Known holes / open

- **Most of the module's choices are recorded only here.** Every module in the crate is
  `pub(crate)`; ADR-0018 records the ranges and the fill rules, but box drawing's own choices (the
  diagonal overshoot, the 1 px `.max(1)`, the `1..=h` mirror) and `fill_polygon`'s rule live only in
  this note and the module's comments.
- **The reference agreement is quoted, not pinned.** Every citation under `## Reference behaviour`
  except the adjacent row is a `file:line` with no SHA behind it.
- **No published document tells a user these characters are intercepted**, so a font that draws them
  well is overridden anyway and a user cannot learn that from the published docs.
- **The diagonal overshoot-and-clip has lost its stated reason.** It was justified as "an atlas glyph
  cannot spill past its cell"; since #791/#966 the slot carries bleed bands for ink that leaves the
  cell ([cell geometry](cell-geometry.md)). `block_glyph` still draws at cell size, so the choice
  stands in the code, but nothing says why it is preferred over spilling into the band as alacritty's
  grown canvas does.
- **"Real cells are 16–33 device px" is unverified.** Comments cited that range for the sextant
  `h < 3` degeneracy, the one-eighth `< 8 px` vanish and `POLY_SS = 4` sufficing; nothing owns it and
  #962 changed how the width is sized, so it is not carried as a fact.
- **The builtin-`█`-never-measured invariant is held by call-site discipline only** (ADR-0022;
  `rasterizer.rs` module docs): `block_glyph` is reachable from `Rasterizer::builtin` alone, and
  nothing enforces that it is never called from `Rasterizer::new`.
