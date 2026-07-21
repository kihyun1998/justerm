# ADR-0022: The grid cell is the ink box of the font's `█`, and everything geometric follows from it

Status: **proposed** (2026-07-21). Records an implemented decision (#288/#361) whose grounds turned out
to be inherited rather than established, and states the invariant it creates. Scoped to **cell geometry
and what derives from it**; cell *composition* is ADR-0019, resource *ownership* is ADR-0021.

## Context

### What we do

`Rasterizer::new` draws `U+2588 █` with the browser's text engine into a generous square buffer and takes
the tight bounding box of pixels with **alpha ≥ 128** as the cell (`rasterizer.rs:92-96`,
`bitmap.rs::ink_bounds` / `cell_metrics`). Width and height are the inclusive ink span; `ascent` is
`draw_offset − min_y`. The atlas cell adds a `PADDING` guard band; the on-screen grid cell **is** that
physical box.

### Everything geometric derives from it — and, since #507, so does something that is not geometric

```
ink-scan of the font's █  (rasterizer.rs:92-96)
  └─ cell_size (device px)
      ├─ cssCellWidth = cell / dpr → the consumer's fit → cols × rows → the whole grid
      ├─ the atlas cell (+ PADDING)
      ├─ #338: letterSpacing / lineHeight grow the CELL while the glyph box stays → char_size / char_offset
      │    └─ every glyph is masked to its ink box
      │         └─ tiling glyphs stop meeting at the cell seam
      │              └─ builtin.rs exists: ~300 codepoints drawn to the CELL, bypassing the font (#359/#361)
      │                   └─ #507: builtin's range now also decides which glyphs are BACKGROUND ink (ADR-0019 rule 4)
      └─ webgl.rs: grid cell, glyph quad, cursor box
```

The last edge is the one worth pausing on: a decision about **how to measure a font** is, four steps
later, deciding **what colour a cell paints**. That chain lives in three module doc-comments today.

### Where the decision came from

`bitmap.rs:37` records it: the approach "mirrors beamterm `canvas_rasterizer::measure_cell_metrics`".
Verified — beamterm's `beamterm-renderer/src/gl/canvas_rasterizer.rs:233-292` is the line-for-line
ancestor, including the 128 alpha threshold, the draw offset, `ascent = draw_offset − min_y` and
`+ 2 * PADDING`. justerm inherited it while reimplementing beamterm's renderer (ADR-0002 → ADR-0018), and
kept it after the reason for it — beamterm — was gone. It has not been re-examined since.

### The references both do something else, and neither is silent by choice

- **alacritty** sizes the cell from **font metrics**: `compute_cell_size` (`alacritty/src/display/mod.rs:1608-1615`)
  is `(metrics.average_advance + offset.x).floor().max(1.)` × `(metrics.line_height + offset.y).floor().max(1.)`,
  and those metrics come from the font tables — `crossfont` takes the **horizontal advance of `'0'`**
  (fallback `max_advance`) and `line_height = max(height, ascender − descender)` (`crossfont/src/ft/mod.rs:420-430`,
  `:145-147`; macOS `darwin/mod.rs:326-343` agrees). No part of alacritty measures ink to size the cell.
  *(`average_advance` is a misnomer — it is one reference character's advance.)*
- **xterm.js** sizes it from `CharSizeService` (`src/browser/services/CharSizeService.ts:104-126`):
  `measureText('W').width` plus `fontBoundingBoxAscent + fontBoundingBoxDescent`, falling back to a DOM
  `<span>` of `'W'.repeat(32)`. That becomes the device cell in `addons/addon-webgl/src/WebglRenderer.ts:646-671`.
  xterm *does* scan pixels (`TextureAtlas.ts:974` `_findGlyphBoundingBox`) but only to trim the atlas
  region of an already-drawn glyph, strictly downstream of a cell that is already fixed.
- **Neither documents why not to measure ink.** Their doc-comments carry no rationale for the choice at
  all. The only stated justification anywhere is beamterm's own comment — *"This is more accurate than
  text metrics which can have rounding issues"* — asserted, with no measurement attached.

So the ink scan is **1 of 3**, and its ground is a sentence in the crate we replaced.

### The hazard is ours alone, and it is novel

Because our measurement channel is *rasterisation of a glyph* rather than a metrics table, measurement
and drawing share a mechanism, and the two can close a loop: if the **builtin** `█` (the one this crate
draws to the cell) ever entered measurement, the cell would be defined by the glyph the cell defines.
`rasterizer.rs:16-21` names this and states why it is safe today — `block_glyph` is called from
`Rasterizer::builtin` alone, never from `Rasterizer::new`.

Checked against both references: the loop is **structurally impossible** for them. alacritty loads
metrics once from the face and passes them *into* `builtin_glyph` (`glyph_cache.rs:112`, `:214-216`);
xterm's `tryDrawCustomGlyph` takes `deviceCellWidth/Height` as **input parameters**
(`CustomGlyphRasterizer.ts:15-29`) and `CharSizeService` never touches custom glyphs. They are silent
about the hazard because their architecture forecloses it — not because they weighed it. The obligation
is created by our own choice, and it is therefore ours to enforce.

## Decision

**The grid cell is the ink box of the font's `█`, rasterised at `font_size × dpr` with an alpha
threshold of 128.** Everything geometric derives from it: the atlas cell (plus the guard band), the glyph
quad, the cursor box, and the CSS cell the consumer divides its box by.

**The glyph box is nested inside the cell, not equal to it** (#338). `letterSpacing` and `lineHeight`
grow the cell while the glyph keeps its measured size; `char_size` / `char_offset` place it. This part
*is* the prior-art consensus — xterm carries `device.char.*` beside `device.cell.*` and centres with
`device.char.{top,left}` (`WebglRenderer.ts:646-671`), and alacritty adds a user `offset` to the cell and
positions with a separate `glyph_offset`.

**Glyphs that must tile are drawn to the cell, not to their ink box** (`builtin.rs`, #359/#361/#364-#367).
This follows from the two rules above rather than standing on its own: once every glyph is masked to its
ink box and the cell can be larger than the glyph, a run of `█` stops meeting. Both references intercept
the same families ahead of the font for the same reason, so this is convergence, not divergence.

**Invariant — nothing this crate draws may enter measurement.** `Rasterizer::new` measures the *font's*
`█` via `fill_text`; `block_glyph` is reachable only from `Rasterizer::builtin`. Any future path that
measures, re-measures or validates the cell must take the same care. This is the price of measuring by
rasterisation, and it is not optional.

### Grade of evidence, stated because it is uneven

The **derived structure** (nested glyph box, tile glyphs drawn to the cell) is well attested — both
references do it, for stated reasons. The **measurement method** is not: it is inherited from beamterm,
justified by one unmeasured comment, and diverges from the two mature implementations. This ADR records
it as **the current decision with an open validity question**, not as an established one.

**What would settle it:** measure, on real fonts at real sizes, whether `fontBoundingBox`-derived metrics
actually round badly enough to matter — i.e. whether the grid ends up a different size, and whether that
size is worse. Until then, "more accurate" is inherited hearsay and this ADR does not repeat it as fact.

## Consequences

- **Our grid can differ from alacritty's and xterm's for the same font and size.** If a font's `█`
  under- or over-fills its advance, the cell differs, and with it cols × rows for a given pixel box. This
  is a real, user-visible divergence with no test pinning it; it has simply never been compared.
- **The invariant is enforced by call-site discipline only.** Nothing fails if someone calls
  `block_glyph` during measurement — the cell would silently drift, and the symptom ("the grid is subtly
  the wrong size with some font settings") is far from the cause. A test that asserts the measurement
  path never reaches `builtin` would make the invariant load-bearing rather than documentary.
- **#507's classification now rides this chain.** `builtin::owns` decides both what this crate draws and
  what counts as background-shaped ink (ADR-0019 rule 4). A change to *why* builtin exists is therefore a
  change to cell composition, which is not obvious from either module.
- **Two citations in the code were wrong and are corrected with this ADR.** `rasterizer.rs:15` cited
  alacritty's `builtin_font.rs:51` for cell sizing; that line is the *consumer* of the metrics, and the
  decision lives in `display/mod.rs:1608`. Cited-but-unverified references are how an inherited
  assumption acquires the appearance of prior art.

## Alternatives considered

- **(A) Size the cell from font metrics, as both references do.** **Not rejected — deferred pending
  measurement.** It is the majority practice and it removes the feedback hazard outright by making
  measurement a metrics read rather than a rasterisation. What stops it being adopted here is that
  switching would move every existing grid's size with no evidence that the new size is better; the
  comparison in "what would settle it" is the precondition.
- **(B) Keep the ink scan and make the invariant executable.** Compatible with (A) as a stopgap and worth
  doing regardless: a guard so the loop cannot be closed by accident.
- **(C) Measure both and reconcile.** Rejected as a design: two sources of truth for one number invites
  the disagreement to be resolved differently in different call sites, which is the shape #507 was just
  fixed for.
- **(D) Leave it undocumented (status quo).** Rejected: the chain from measurement to colour crosses
  three modules, the hazard is stated in only one of them, and the grounds are inherited from a
  dependency that no longer exists — exactly the combination that reads as intentional design to the
  next person.

## Out of scope

- **Rasterisation itself** — shaping, fallback, colour emoji (#268/#284/#297).
- **DPR handling** — re-baking on a density change (#322) is a consequence of this cell, not a decision
  about it.
- **Whether `█` is the right reference glyph** — it is the widest guaranteed-full-cell glyph in the
  block range; nothing in the code or the references suggests an alternative, and none was considered.
