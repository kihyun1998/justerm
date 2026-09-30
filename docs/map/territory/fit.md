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
- **The #112 `Scrollbar` is an overlay, and the lane for it is opt-in** (#1029). The track is
  `position: absolute; right: 0` with no layout width, so a grid fitted to the whole box puts its last
  column under the track whenever it shows — `trackWidth − (box mod cell)` px of it, all of it when
  the remainder is small. `JustermRendererOptions.scrollbarWidth` (default `0`, the overlay) takes the
  lane off the box on `resize`, and it is the same number `FitInput.scrollbarWidth` takes, so the two
  box→grid paths keep one rule. The `Scrollbar` needs no change: at `right: 0` it lands in the lane,
  and the ruler marks (#199, #500) on its track stop overpainting text with it.
  - *Taken off the box, not off the column count.* `floor((box − lane) / cell)`, xterm.js's shape
    (below, § Reference behaviour). The plausible alternative — whole columns minus the cells the
    lane spans — agrees at some widths and costs an extra column at others (575.67 px, 7.333 px cell,
    8 px lane: 77 against 76); `justerm-web/test/justerm-renderer.test.ts` holds both rows.
  - *Reserved by the option, not by content.* The track shows by content (`scrollbarMetrics().visible`
    is `total > rows`), so a lane that followed it would reflow the grid the moment the first line
    scrolled into history. xterm.js reserves by option too; the renderer never learns the engine's
    scrollback limit, so the opt-in *is* xterm's `scrollback === 0` gate — a terminal with no
    scrollback leaves it `0`. When the track is hidden the lane is empty space.
  - *`FitInput.scrollback` is the limit, not the history length.* `proposeDimensions` keeps the lane
    only when it is non-zero, and `gridForBox` keeps it always, so the two agree only when the fit
    path is handed the configured limit. Handed the current length — which is what the demo passed
    until #1029 — a fresh terminal proposes 79 columns on the fit path against 77 from `resize`
    (579.67 px box, 7.333 px cell, 8 px lane): the #547 shape, engine and renderer on different
    grids, and then the reflow on the first history line the lane exists to avoid. Found by #1029's
    completeness pass; the published docs on both options now name the condition.
  - *Never in the grant read-back.* `applyGrid` re-runs `gridForBox` over the **drawing buffer**
    (`cssWidth()`/`cssHeight()`), which is already `cols × cell` with the lane gone. Taking the lane
    off again shrinks a sole tenant by one more column, measured (the #1029 e2e, 39 → 38). It shows
    only in `terminalSize()`: the canvas display box was written from the grid before the read-back.
  - *Read once, at `create` / `attach`*, with no runtime setter. A setter would carry the same re-fit
    obligation as the spacing setters (the next bullet).
- **A box with no area is refused, not floored** (#810). An element that is `display: none`,
  detached, or not yet laid out reports every metric as `0`, and `0` is finite — so the non-finite
  refusal never saw it while the `MINIMUM_COLS` floor turned it into a plausible `2x1`. Both paths
  now answer `undefined`, and a box that is *measured* and merely tiny still floors, which is the case
  the minimum exists for. This is a **deliberate divergence from all three references**, all of which
  floor; the row in [`theflow.md`](../../agents/theflow.md) carries why, and the short version is that
  ours is the only fit driven automatically by a `ResizeObserver`, so it is the only one handed a
  hidden element's box.
- **The contract runs consumer → CSS box, renderer → `cols`/`rows`.** A consumer that assumes the
  width it asked for is the width it got will be wrong: the engine also clamps `cols` up to
  `MIN_COLUMNS`, silently.
- **This is the frame-mode analog of xterm.js's `FitAddon.proposeDimensions`**, named as such at the
  top of the module.
- **`gridForBox` must agree with `proposeDimensions`** (both in `fit.ts`). The renderer takes a
  *grid* (#331) and pixel→cell is consumer policy (ADR-0017), so the adapter owns this division —
  the same `floor(box / cell)` xterm's FitAddon does, pure so the fractional-DPR rounding is
  testable. Two paths from a pixel box to a grid must not disagree, and each rule below was once
  where they did:
  - *Floored at `MINIMUM_COLS`×`MINIMUM_ROWS`, not at one cell* (#547). "A grid must have a cell"
    under-shot: the engine clamps `resize(1, r)` up to two columns, so a 1-column proposal is a grid
    it can never be in, and driving the engine at 1 while it holds 2 puts every span of the frame
    outside the grid — the surface silently stops updating. The clamp is pull-only on the core side
    (a consumer reads the width back, it is not told), so agreeing with the floor is what keeps the
    two in step.
  - *`undefined` for an unmeasured cell or a non-finite box* (#632). A non-finite box — `NaN` from
    a detached or unlaid-out element, `Infinity` from a degenerate one — means "not measured", exactly
    when the terminal must not be shrunk. This axis was missing:
    `Math.max(2, Math.floor(NaN / 8))` is `NaN`, so `backend.resize(NaN)` coerced to `0` and the
    terminal came back 1×1 — through the path that reaches the renderer, while the guarded path was
    the one nothing called. One `Number.isFinite` check covers both conditions, **measured**: a
    separate `cellCss* === 0` guard, mirroring `proposeDimensions`'s, was written first and a mutation
    showed it could not fail: behind #810's guard only a positive box is divided, so a zero cell makes
    the quotient `+Infinity`, which `Number.isFinite` rejects. It was removed rather than kept for
    symmetry — a branch that cannot change an outcome is untestable by construction; the test
    (`justerm-web/test/justerm-renderer.test.ts`) asserts the zero-cell *behaviour*. `proposeDimensions`
    keeps its `cell === 0` guard, and there it is **not** redundant: its divisor runs after padding and
    the scrollbar are subtracted, so a negative remainder over a zero cell gives `-Infinity`, which the
    `MINIMUM_COLS` floor turns into a finite `2`.
  - *`undefined` for a box with no area* (#810), the same answer `proposeDimensions` gives. Of
    `gridForBox`'s two callers only `resize()` can deliver a zero on an ordinary path (see [an absent
    element box measures as zero](../invariant/an-absent-box-measures-as-zero.md)); for `applyGrid`'s
    grant read-back the guard is defensive, and the one remaining route there is a canvas authored at
    `width="0"` whose surface is never sized. Left unguarded that caller was the worse of the two:
    `{2, 1}` satisfied `granted.cols < cols`, so an empty buffer would clamp **every attached pane's
    grid** to the minimum. The guard borrows the renderer's sentence for the neighbouring fact —
    *"a buffer of no size is not a grant, it is the absence of an answer"* (#639): an unmeasured box
    and an ungranted buffer are different facts with the same shape.
- **The widget re-fits on `resize`, and only there** (`JustermRenderer.setLetterSpacing` and the
  other cell-moving setters). They re-derive the drawing buffer themselves, but not the **grid**,
  which needs a container measurement the widget does not hold — the widget and the consumer own the
  fit (the demo's `setFontSize(); fit(); render();`, #417). Skipping it is not cosmetic: the grid is
  then a column count derived from the old cell, fitted to a box it no longer occupies.
  - *Call `resize`, not `FitController.fit()`.* The reason is a signature: `ResizePort.resize(cols,
    rows)` carries a grid, and the canvas display box is written only on the `applyGrid` →
    `resizeSurface` path, which no `ResizePort` call reaches — a flush stops at the consumer's port. The flush is also debounced (100 ms by default) —
    100 ms of displaying a buffer that no longer exists. Until #632 there was a third reason: the
    controller deduped on `cols`/`rows` alone and dropped a cell change that left the grid identical.
    That is fixed — the key carries the cell — so `FitController` is safe for container resizes across
    a spacing change; it still is not what re-sizes this canvas.
  - *xterm.js draws the same line*, which makes this a shape rather than a preference: an option
    change there re-lays out at the current grid (`RenderService.ts` `handleResize(cols, rows)`) and
    its `FitAddon` registers no listeners. alacritty auto-re-fits, but it owns its OS window; an
    embeddable widget does not.
  - *Read the cell back.* The spacing path can hand back something other than what was asked in three
    ways, none reporting an error: a `lineHeight` whose cell the atlas cannot hold is shrunk; a failed
    re-bake rolls the whole change back; and a change arriving on a lost context depends on the
    renderer. Up to 0.14.x the cell moved at once and the buffer did not — `adopt_spacing` ran
    `recompute_cell()` before its lost-context guard (an earlier comment said the cell "does not move
    at all", corrected in #632). Since #772 neither moves until the restore: the cell belongs to a
    font configuration, the setter advances the selector and defers, and `restore` re-selects. #632's
    conclusion holds under both — `FitController` dedupes on the cell *and* the grid because a cell
    change can leave the grid identical.

## Code

- `justerm-web/src/fit.ts` — `FitPadding`, `ResizePort`, and the proposal arithmetic
- `justerm-web/src/scrollbar.ts` — the overlay track whose width a lane keeps free
- `justerm-web/src/justerm-renderer.ts` — `JustermRendererOptions.scrollbarWidth`, and
  `JustermRenderer.resize`, the `gridForBox` caller that takes the lane off the box
- `justerm-renderer/src/webgl/surface.rs` — `css_cell_width` / `css_cell_height`, the divisor
- `justerm-core/src/lib.rs` — `Engine::resize`, the intent's destination, and `MIN_COLUMNS`

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated**.

- [Who re-fits after a spacing change](../../agents/reference-facts.md) § *#578* — the consumer does,
  and it calls `resize()` rather than the fit; xterm draws the same line, alacritty differs because it
  owns its OS window
- [A scrollbar lane](../../agents/reference-facts.md) § *#1029* — xterm.js's track is an overlay
  too, and its fit keeps the lane by subtracting the width from the box, gated on the `scrollback`
  option rather than on content
- [When is a resize redundant — box, grid, or cell](../../agents/reference-facts.md) § *#632* — the
  three references **do not agree on one shape** (alacritty widens one key to box+cell; ghostty
  dedupes the box and leaves its cell path undeduped; xterm keeps no fit-side memory and dedupes at
  the sink). So "the reference does X" cannot settle a question here; each row carries the constraint
  that makes its shape available, and ours converges with alacritty because `ResizePort` is published
  and write-only

Still unpinned: `proposeDimensions`'s own **rounding** behaviour against `FitAddon`'s, which the module
names as its model — exactly the kind of detail that diverges quietly.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — the
  **second site, repaired in #810**. Both paths floored a `0x0` box at `MINIMUM_COLS`/`MINIMUM_ROWS`
  while refusing a `NaN` one, so the `display: none` box `justerm-web`'s README documents as the way
  to hide a pane proposed `2x1` and the engine reflowed through two columns. This note's own
  doc-comment had asked for the check — *"check **both** axes: the floor and the refusal"* — before
  the path that needed it existed. **This bullet said "and unrepaired" for the length of one commit
  after the repair landed**, because #810 updated the invariant note and the design-model bullet
  above and not this one: two rows in this file stating opposite facts, in the section a reader
  cannot see the need for from inside the territory
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — the largest consumer of the cell. Its dedupe could not express *"the cell moved but the grid did
  not"* (#578) until #632 widened the key to carry the cell alongside the proposal. The residual is
  recorded there: the cell is a **proxy** for "a grid write bypassed this controller", so the key is
  only as complete as that set of writers
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — not a converter, but the *source* of the out-of-range coordinate: `proposeDimensions` floors the
  grid, so a container that is not an exact multiple of the cell keeps a remainder strip outside the
  canvas, and a pointer there resolves one past the end (#667)

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
- **The lane width is written twice and nothing checks the two agree** (#1029): once to
  `Scrollbar`'s `width`, once to `JustermRendererOptions.scrollbarWidth`. Each is written once per
  construction rather than once per `resize`, which is what #1029 asked for; a `Scrollbar` wider than
  the lane still covers up to `trackWidth − lane` px of the last column, silently.
- **The engine's column clamp is invisible to a consumer that bypasses fit.** Fit itself floors at
  `MINIMUM_COLS` (#547, above), but a consumer that sizes the engine directly and asks for one column
  gets two, and nothing tells it — it must read the width back from the frame.
- ~~`setLetterSpacing` / `setLineHeight` are unreachable from the widget.~~ Closed by **#578** — both
  are wired, which is what took the count of setters that can move the cell from two to four and made
  the two stale readers below reachable.
- **A cell change inside the debounce window still proposes against the pre-change cell.** `latest` is
  a snapshot taken when the `ResizeObserver` fired, and the flush replays it 100 ms later — so a
  spacing change landing in between emits a grid derived from the *old* cell and stores that cell as
  the key. It self-heals on the next observer fire, and there may not be one. Found by #632's
  completeness pass; the cure is to read the geometry at flush time rather than replay a snapshot,
  which is what xterm's `fit()` does by construction.
- ~~**The key remembers what was *proposed*, not what the renderer *adopted*.**~~ **Closed by #773**
  (renderer 0.15.0), and by a change of owner rather than by a fix here. A drawing-buffer clamp
  (#339) used to shrink the *grid*, so the remembered pair could describe a grid nobody held — the
  same defect #632 fixed, one axis over. Nothing clamps a grid now: `resizeGrid` records what it was
  told and the **surface** adopts the browser's grant, which `cssWidth`/`cssHeight` report. So
  proposed and adopted are the same pair, and `terminalSize()` is an echo of it.

  What the closure does *not* say is that the clamp went away. It moved: a consumer that asks for
  more than the buffer can hold now gets a grid drawing outside its own rect, clipped by the scissor
  rather than silently reduced. Reading `cssWidth()` back is how that is noticed.
- ~~**No `matchMedia` listener watches for resolution changes.**~~ **Closed by #325** (2026-08-10):
  `JustermRenderer` now owns a resolution watcher that re-bakes at the new density and re-applies the
  canvas display box — and since #773 it also re-derives the **drawing buffer** from the grid it is
  holding, because the renderer stopped doing that (a buffer shared by N grids belongs to none of
  them). Three paths reach the same private step: a density change, a font or spacing change, and a
  GL restore that adopted a density nobody notified. **It still does not re-fit**, deliberately — the
  grid is the consumer's (#417/#578) and the widget holds no container measurement — so this
  territory's job is unchanged and a consumer
  that wants the grid re-derived still calls `resize()` with its own box, as it already must after a
  font or spacing change. xterm.js draws the same line: its `handleDevicePixelRatioChange` calls no
  resize either.
