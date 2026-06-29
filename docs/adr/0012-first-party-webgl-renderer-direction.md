# ADR-0012: Replace beamterm with a first-party WebGL renderer (direction; implementation deferred)

Status: accepted as **direction** (2026-06-29, #107) — implementation deferred until sub-cell needs
justify it. Supersedes nothing yet; ADR-0002 (adopt beamterm) stays in force until this is built.

## Context

`justerm-web` renders through **beamterm** (ADR-0002), a third-party WebGL2 text renderer
(`github.com/junkdog/beamterm`). Its drawing unit is **one styled glyph per cell** — `batch.cell(x, y,
glyph+fg+bg+bold/italic/underline/strikethrough)`. There is no sub-cell geometry: you cannot colour an
arbitrary rectangle or line *within* a cell.

That granularity blocks a class of features. S5 (#107, cursor) hit the first one: distinct cursor shapes.

- **Block** cursor works (cell invert — the glyph survives in reverse).
- **Bar / underline / hollow-outline** do not: each needs a thin rectangle at a sub-cell position (bar =
  left edge, underline = bottom edge, hollow = a border *around* the glyph). xterm's webgl draws these as
  sub-cell quads (`cursorWidth`/`dpr`/`cell.width`, read at source this session); a hollow cursor needs a
  frame **and** the glyph in the same cell — two visual layers — which "one glyph per cell" cannot do.

The same wall is ahead for **undercurl**, **ligature shaping**, **images / sixel**, and **smooth
pixel-scroll / overscan** — all sub-cell.

Two facts make this tractable to record now and defer:

- The **`Renderer` port** (S1) already hides the renderer behind a small interface (`applyFrame` /
  `render` / cursor + focus hooks). The renderer is **swappable without touching the widget** — the same
  seam that lets a fake renderer unit-test the wiring lets a different real renderer drop in.
- beamterm is **external**: its API cannot be extended in-repo, and upstream changes depend on its
  maintainer's roadmap.

## Decision

The `-term` family will **replace beamterm with a first-party WebGL2 renderer**, built behind the
existing `Renderer` port so justerm-web consumers are unaffected.

**Named prior art to study** (CLAUDE.md: 1-principle derivation + named prior-art cross-check, each read
at the appropriate fidelity):

- **xterm.js** — `addon-webgl` (GlyphRenderer, RenderModel, the cursor render path) + `common/buffer`
  (CircularList / BufferLine). Source-readable (npm bundle + sourcemaps), already used as reference in
  ADR-0009/0011.
- **alacritty** — `alacritty_terminal` grid (`Storage`/`Grid`, read for ADR-0009/0011) + its GPU
  renderer (glyph atlas, instanced quads). Source-readable (Rust).
- **warp** — block model + GPU text rendering as an *architectural* reference (public engineering
  writing); not open to read line-by-line like the other two, so it informs shape, not code.

The win is **sub-cell control**: free cursor shapes (block / bar / underline / hollow), undercurl, and a
path to ligatures and inline images — none of which beamterm's cell-only model can serve.

## Timing — deferred, dogfood-driven

Not now. A production WebGL2 text renderer (font atlas, instancing, shaders, sub-millisecond throughput)
is a **project-sized** effort — comparable to justerm-core. Building it for today's cosmetic gaps (a
hollow cursor) is the *maximal* grain, not the *correct* one (CLAUDE.md: "perfect = the correct grain").
The trigger is an **accumulated** set of sub-cell needs that beamterm structurally cannot meet — surfaced
by dogfooding, not by a single missing shape ("bones correct from day one, the tail grows by dogfood").

Until then: beamterm + cell-level approximations (block-invert cursor; bar/underline approximated;
hollow → solid), each logged as it is hit.

## Consequences

- **The `Renderer` port is the load-bearing seam.** Keep it sufficient and renderer-agnostic; resist
  leaking beamterm specifics (CellStyle, batch) through it, so the eventual swap stays a drop-in.
- **Sub-cell limitations accrue here as motivation.** Each feature beamterm blocks gets noted, so the
  "enough to justify a renderer" trigger is visible when it arrives rather than argued from one case.
- **The swap is non-disruptive by construction** — justerm-web's public API is the port, not beamterm.
- **justerm's engine boundary is unchanged.** "The engine does not render" (CLAUDE.md) still holds — the
  renderer remains a separate, renderer-side package; this only makes it first-party instead of an
  external dependency.

## Alternatives considered

- **Keep beamterm + approximate (status quo).** Correct *until* sub-cell needs accumulate; this ADR is
  the trigger to revisit, not a reason to act today.
- **Contribute sub-cell primitives upstream to beamterm** (e.g. a cursor-overlay quad). Far cheaper than
  a whole renderer and preserves the dependency — a sensible **interim bridge** if the maintainer is
  receptive. Not chosen as the *end state* because it leaves the family's rendering on an external
  roadmap, which is the dependency this ADR exists to end.
- **Fork beamterm.** Rejected — the maintenance burden of a fork without the benefit of a design built
  for justerm's exact needs (the wire format, the cell mirror, the cursor/selection model).
