# ADR-0031: Glyph bake geometry answers to our own model — prior art here is a mechanism catalogue, never a validator

Status: **accepted** (2026-09-04). Promoted from the tie-breaker table's row, added **2026-08-21
(#792)** — which had to state this by hand in its own lens brief because no *record* held it. The
maintained copy of that table went with the generated `thegraph` build when it was retired; a frozen
copy survives in [`../agents/theflow.md`](../agents/theflow.md) as a cited corpus, and that copy now
defers to this record.

**This record is what ADR-0019's `I_neighbour` alternative (E) means by *"the tie-breaker for this
layer"***. That sentence referred to the retired table; it resolves here now.

**The layer this governs**: what the **bake** may do to a glyph before the composite ever sees it —
place it, scale it, refuse it. Not where ink lands once baked (ADR-0019), and not what a cell *is*
(ADR-0022).

## Context

**The gap is narrow and real, and it is between two records rather than in either.** ADR-0019's
*Out of scope* declines *"glyph rasterisation"*; ADR-0022's declines *"Rasterisation itself"*. And
ADR-0019's R1.2 marks the seam from its own side — the horizontal residue it once recorded *"is no
longer a residue of this model — **it is answered before the model sees it**"*. What answers it is the
bake, and the bake had no record.

**What justerm actually does, so this record does not repeat a mistake its first draft made:** since
**#791 (2026-08-20)** a glyph's ink **does** leave its cell. `I_neighbour` is a first-class ink source
in ADR-0019's rules 4 and 6, produced **reader-side** — the receiving cell's fragment samples the
adjacent slots — which is precisely how bleed and *one evaluation per pixel with no GL blending*
coexist. Any argument here that starts *"this renderer cannot let ink leave a cell"* is false.

**The asymmetry is the whole subject**, and it is a property of the bake, not of the model:

- **Vertical** — a band exists (the bake's bleed, a per-configuration quantity). Ink inside it travels
  and lands as `I_neighbour`; ink beyond it is destroyed, because a face can overshoot even its own
  declared line box.
- **Horizontal** — **there is no band**, and the Canvas API exposes no counterpart to the vertical one.
  So no `I_neighbour` contribution is ever produced on this axis, and the real choice is
  **condense-or-destroy** rather than condense-or-overflow.

## Decision

### D1 — The references' **defaults** are unimportable, and a lens reporting one is not reporting a defect

All three let a glyph overflow, because their quad *is* the glyph's bounding box — xterm.js
`addons/addon-webgl/src/GlyphRenderer.ts:53` (`a_cellpos + (a_unitquad * a_size)`: the quad is the
glyph's own size and the cell contributes only an origin), alacritty
`alacritty/src/renderer/text/glsl3.rs:355-358` (`width: glyph.width, height: glyph.height`), ghostty
`src/renderer/generic.zig:3172-3178`. Re-verified against the pinned trees on 2026-09-04.

**That is writer-side placement, which ADR-0019 alternative (E) rejected with three costs stated** —
hardware blending onto a non-premultiplied buffer, splitting a fragment that owns both background and
glyph, and column-order occlusion. Their defaults therefore rest on a model that never contained rules
4 and 6. Reading one across that gap imports a conclusion whose premise did not travel with it, and a
finding of the form *"the reference makes this optional / lets it overflow"* is `DELIBERATE` here.

**And the converse binds equally**: their *agreement* is not an argument either. Three references
sharing a default they can afford and we cannot is one fact, not three votes.

### D2 — Their **mechanisms** are readable, and are the reason to open the trees at all

The catalogue is genuinely useful; only the ranking is not. On file: xterm.js's **opt-in quad squeeze**
(`src/browser/renderer/shared/RendererUtils.ts:47` `allowRescaling(...)`, gated on `width === 1`, behind
`rescaleOverlappingGlyphs: false` at `src/common/services/OptionsService.ts:51`) and ghostty's
**`Constraint` with `.fit` for symbols only** (`generic.zig:3175-3178` — ordinary letters get `.none`).
Read them as design inputs; never as a vote.

### D3 — The worked example (#792), which is what the rule was extracted from

justerm **condenses a single-cell glyph the font draws wider than its box, at bake, by default, with no
setting** — `justerm-renderer/src/metrics.rs`'s `horizontal_fit`, whose two treatments are a property of
the ink rather than a mode: ink wider than the box is condensed to exactly the box; ink that fits but
sits outside is moved in by the least that puts it inside.

Against the references' overflow default that reads like difference for its own sake. It is not: on
this axis there is no band to overflow into (Context above), so the alternative to condensing is
**cutting**. Measured before the change: **252** clipped codepoints on DejaVu Sans Mono and **1153** on
the demo face, **35 to 629** of them losing over 30 % of their ink, with `Ǆ` reaching the screen as `D`.

## Consequences

- A question on this axis is answered from the bake's own constraints plus a measurement; the trees are
  consulted for *how something could be done*, never for *what should be done*.
- **This record widens neither neighbour.** ADR-0019 keeps where ink lands, ADR-0022 keeps what the cell
  is, and both keep their exclusions. This covers only what happens to a glyph before either applies.
- **Falsifier**: a horizontal bleed band — a mechanism that lets ink travel sideways the way the vertical
  band lets it travel up and down — removes the condense-or-destroy premise and reopens D3. ADR-0022's
  still-open alternative (A) (size the cell from the font's declared line box) does **not** do this: it
  was measured on 2026-08-20 and Cascadia Mono still loses 168 glyphs, Lucida Console 263, because a
  bigger cell is still a scissor.
