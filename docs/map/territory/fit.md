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

In `docs/agents/reference-facts.md` — **linked, never restated**.

- [Who re-fits after a spacing change](../../agents/reference-facts.md) § *#578* — the consumer does,
  and it calls `resize()` rather than the fit; xterm draws the same line, alacritty differs because it
  owns its OS window
- [When is a resize redundant — box, grid, or cell](../../agents/reference-facts.md) § *#632* — the
  three references **do not agree on one shape** (alacritty widens one key to box+cell; ghostty
  dedupes the box and leaves its cell path undeduped; xterm keeps no fit-side memory and dedupes at
  the sink). So "the reference does X" cannot settle a question here; each row carries the constraint
  that makes its shape available, and ours converges with alacritty because `ResizePort` is published
  and write-only

Still unpinned: `proposeDimensions`'s own **rounding** behaviour against `FitAddon`'s, which the module
names as its model — exactly the kind of detail that diverges quietly.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — the largest consumer of the cell. Its dedupe could not express *"the cell moved but the grid did
  not"* (#578) until #632 widened the key to carry the cell alongside the proposal. The residual is
  recorded there: the cell is a **proxy** for "a grid write bypassed this controller", so the key is
  only as complete as that set of writers

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
- ~~`setLetterSpacing` / `setLineHeight` are unreachable from the widget.~~ Closed by **#578** — both
  are wired, which is what took the count of setters that can move the cell from two to four and made
  the two stale readers below reachable.
- **A cell change inside the debounce window still proposes against the pre-change cell.** `latest` is
  a snapshot taken when the `ResizeObserver` fired, and the flush replays it 100 ms later — so a
  spacing change landing in between emits a grid derived from the *old* cell and stores that cell as
  the key. It self-heals on the next observer fire, and there may not be one. Found by #632's
  completeness pass; the cure is to read the geometry at flush time rather than replay a snapshot,
  which is what xterm's `fit()` does by construction.
- **The key remembers what was *proposed*, not what the renderer *adopted*.** A drawing-buffer clamp
  (#339) can make the renderer take less than it was asked for, and then the remembered pair describes
  a grid nobody holds — the same defect #632 fixed, one axis over. `terminalSize()` is the documented
  truth; the controller never reads it, because `ResizePort` is write-only.
- **No `matchMedia` listener watches for resolution changes**, so moving a window between displays
  leaves the device-pixel ratio stale until something else triggers a fit. Tracked: #325.
