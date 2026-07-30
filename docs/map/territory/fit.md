# Territory — fit

## What it is

Turning a container's pixel size into `cols` / `rows`. Pure geometry: given the parent box, the
element padding, the cell size and the scrollbar width, propose the grid that fills the space.

The **direction** is the interesting part and it is the reverse of a grid-first API — a consumer sets
a CSS box and reads the grid back, rather than asking for 80×24 and being given a size.

## Governing decisions

**None.**

- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) and
  [ADR-0023 — a spacing setting is CSS pixels](../../adr/0023-spacing-settings-are-css-pixels.md)
  supply the cell size this divides by, and the unit it is expressed in. Neither decides the fit
  contract

## Design model

- **Pure geometry, no DOM.** The caller reads the box; this proposes the grid. That split is what
  makes the arithmetic testable without a browser, and it mirrors how every other pure/browser seam in
  the family is drawn.
- **The resize *intent* stays with the caller.** Fitting proposes; driving `Engine::resize` and the
  PTY `SIGWINCH` is the consumer's, through its own port. Proposing and applying are deliberately not
  the same call.
- **The scrollbar width is an input.** A grid fitted without subtracting it overflows its container by
  exactly one scrollbar — which is why the parameter exists rather than being derived.
- **The contract runs consumer → CSS box, renderer → `cols`/`rows`.** A consumer that assumes the
  width it asked for is the width it got will be wrong: the engine also clamps `cols` up to
  `MIN_COLUMNS`, silently.
- **This is the frame-mode analog of xterm.js's `FitAddon.proposeDimensions`**, named as such at the
  top of the module.

## Code

- `justerm-web/src/fit.ts` — `FitPadding`, `ResizePort`, and the proposal arithmetic
- `justerm-web/src/scrollbar.ts` — supplies the width this subtracts
- `justerm-renderer/src/webgl.rs` — `css_cell_width` / `css_cell_height`, the divisor
- `justerm-core/src/lib.rs` — `Engine::resize`, the intent's destination, and `MIN_COLUMNS`

## Reference behaviour

**None** in `docs/agents/reference-facts.md`, although the module names
`FitAddon.proposeDimensions` as its model. An analog claim about a named reference, unpinned — and
`proposeDimensions` is a function whose rounding behaviour is exactly the kind of detail that
diverges quietly.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — the largest consumer of the cell, and the one whose `cols`/`rows` dedupe cannot express "the
  cell moved but the grid did not" (#578)

## Blast radius

- [cell geometry](cell-geometry.md) — the divisor. A change to the cell/glyph box split changes every
  proposal, and `cssCellWidth()` is a float precisely so this arithmetic can be undone
- [reflow](reflow.md) — a proposal that reaches `Engine::resize` re-lays the whole buffer, so fitting
  is the entry point to the widest blast radius in the engine
- [viewport](viewport.md) — `rows` decides how much of the buffer is visible, which changes what
  damage means
- [widget lifecycle](widget-lifecycle.md) — resize observation is ambient work with a disposer and no
  stated owner
- [wide glyph](wide-glyph.md) — `MIN_COLUMNS` exists because a width-2 glyph needs two columns, so
  the silent clamp is a pair-model consequence surfacing in a layout API

## Known holes / open

- **Zero governing records** for a contract that inverts the usual direction of a terminal API.
- **The silent `MIN_COLUMNS` clamp is invisible here.** Fit can propose one column; the engine
  returns two, and nothing in this territory says so — a consumer must read the width back from the
  frame.
- **`setLetterSpacing` / `setLineHeight` are unreachable from the widget**, so two inputs that change
  the cell size cannot be driven by the consumer that owns the box. Tracked: #578.
- **No `matchMedia` listener watches for resolution changes**, so moving a window between displays
  leaves the device-pixel ratio stale until something else triggers a fit. Tracked: #325.
