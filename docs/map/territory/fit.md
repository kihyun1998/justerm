# Territory — fit

## What it is

Turning a container's pixel size into `cols` / `rows`. Pure geometry: given the box, the element
padding, the cell size and the scrollbar lane, propose the grid that fills the space. The **direction**
is the reverse of a grid-first API — a consumer sets a CSS box and reads the grid back, rather than
asking for 80×24 and being given a size.

`justerm-web` has two box→grid paths, and most of this territory is keeping them in agreement:
`proposeDimensions` (driven by `FitController` / `observeResize`, ending at a consumer's
`ResizePort`) and `gridForBox` (inside `JustermRenderer.resize`, ending at the renderer's grid). The
one consumer uses only the second: PenTerm calls `renderer.resize(box)` from its own `ResizeObserver`,
reads `terminalSize()` back and sends exactly that grid to the session
(penterm 7dadc3c95 `src/blocks/terminal/lib/nativeTerminalCache.ts` — `build`, its `fit` closure).

## Governing decisions

**None.**

- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) and
  [ADR-0023 — a spacing setting is CSS pixels](../../adr/0023-spacing-settings-are-css-pixels.md)
  supply the cell this divides by and the unit it is in; neither decides the fit contract
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  puts pixel→cell on the consumer side, which is why the adapter owns this division at all; it does not
  decide how

## Design model

Status: every rule below comes from the source (`fit.ts`, `justerm-renderer.ts`) and the issues cited;
no record decides them.

- **Pure geometry, no DOM.** The caller reads the box; this proposes the grid. `observeResize` is the
  module's only DOM touch, and lazy, so the arithmetic is unit-tested in `node`.
- **Proposing and applying are different calls.** Fitting proposes; driving `Engine::resize` and the
  PTY `SIGWINCH` is the consumer's, through `ResizePort` (write-only and published). The engine clamps
  `cols` up to `MIN_COLUMNS`; `Engine::resize` returns nothing, while `Session::resize` returns the
  adopted `(cols, rows)` — so a host on the session API is told, and one driving `Engine` directly must
  read the width back from the frame.
- **The two paths must agree, on both axes** (`gridForBox` must agree with `proposeDimensions`, both
  in `fit.ts`). Each rule below was once a place they disagreed:
  - *Floored at `MINIMUM_COLS`×`MINIMUM_ROWS`, not at one cell* (#547). The engine clamps `resize(1, r)`
    to two columns, so a 1-column proposal is a grid it can never be in; driving the engine at 1 while it
    holds 2 puts every span outside the renderer's grid and the surface stops updating. `MINIMUM_COLS`
    is a hand mirror of core's `MIN_COLUMNS` (Rust; it does not cross the wire).
  - *`undefined` for an unmeasured cell or a non-finite box* (#632). `Math.max(2, Math.floor(NaN / 8))`
    is `NaN`, so `backend.resize(NaN)` used to coerce to `0` and the terminal came back 1×1 — through the
    path that reaches the renderer. One `Number.isFinite` check covers both conditions in `gridForBox`:
    behind the zero-box guard only a positive box is divided, so a zero cell makes `+Infinity`, which
    the check rejects. A separate `cell === 0` guard was written first and a mutation showed it could not
    fail, so it was removed (#632); the test asserts the zero-cell *behaviour*
    (`justerm-web/test/justerm-renderer.test.ts`, `gridForBox(800, 240, 0, 16)`). `proposeDimensions`
    keeps its `cell === 0` guard and there it is **not** redundant: its divisor runs after padding and
    the lane are subtracted, so a negative remainder over a zero cell gives `-Infinity`, which the floor
    would turn into a finite `2`.
  - *`undefined` for a box with no area* (#810). `display: none`, detached or not-yet-laid-out reports
    `0`, and `0` is finite — so the non-finite refusal never saw it and the floor turned it into a
    plausible `2x1`. A box that is measured and merely tiny still floors. The guard is on the *parent*
    box, not on what padding leaves. This is a **deliberate divergence from all three references**,
    which floor (the reasoning sits on `proposeDimensions`' own comment): ours is the only fit driven
    automatically by a `ResizeObserver`, so the only one handed a hidden element's box. Of `gridForBox`'s
    two callers only `resize()` can deliver a zero on an ordinary path; for `applyGrid`'s grant read-back
    the guard is defensive (a canvas authored at `width="0"`), but there `{2, 1}` would have satisfied
    `granted.cols < cols` and clamped every attached pane to the minimum.
- **The scrollbar lane is opt-in and taken off the box** (#1029). The `Scrollbar` is an overlay
  (`position: absolute; right: 0`, no layout width), so a grid fitted to the whole box puts its last
  column under the track. `JustermRendererOptions.scrollbarWidth` (default `0`) takes the lane off the
  box on `resize`, and it is the same number `FitInput.scrollbarWidth` takes.
  - *Off the box, not off the column count*: `floor((box − lane) / cell)`, xterm.js's shape. Whole
    columns minus the cells the lane spans would cost an extra column at some widths (575.67 px box,
    7.333 px cell, 8 px lane: 77 against 76); `justerm-renderer.test.ts` holds that row and PenTerm's
    measured 579.67 px row.
  - *Reserved by the option, not by content.* The track shows when `total > rows`, so a lane that
    followed it would reflow the grid when the first line scrolls into history. The renderer never learns
    the scrollback limit, so the opt-in *is* xterm's `scrollback === 0` gate. PenTerm sets it exactly
    that way (`scrollback > 0 ? NATIVE_SCROLLBAR_WIDTH : 0`, penterm 7dadc3c95
    `src/blocks/terminal/lib/nativePaneView.ts`).
  - *`FitInput.scrollback` is the limit, not the history length.* `proposeDimensions` keeps the lane only
    when it is non-zero and `gridForBox` keeps it always, so they agree only when the fit path is handed
    the configured limit. Handed the history length (what the demo passed until #1029), a fresh terminal
    proposes 79 columns against 77 from `resize` — the #547 shape. Both published docs name it.
  - *Never in the grant read-back.* `applyGrid` re-runs `gridForBox` over the drawing buffer
    (`cssWidth()`/`cssHeight()`, already `cols × cell`) with no lane; taking it again shrank a sole
    tenant by one more column (measured in #1029, 39 → 38).
  - *Read once, at `create` / `attach`*, with no runtime setter — a setter would carry the re-fit
    obligation below.
- **The widget re-fits on `resize`, and only there.** The cell-moving setters (`setFontSize`,
  `setFontFamily`, `setLetterSpacing`, `setLineHeight`; the weights do not move the cell, which is
  measured at `"normal"`) re-derive the drawing buffer but not the **grid**,
  which needs a container measurement the widget does not hold (#417, #578); skipping the re-fit leaves a
  column count from the old cell in a box it no longer occupies. PenTerm does it: a view change that moved
  the metrics calls its `fit` (penterm 7dadc3c95 `nativeTerminalCache.ts` — `setNativeTerminalView`).
  - *Call `resize`, not `FitController.fit()`* — `ResizePort.resize(cols, rows)` carries a grid, and the
    canvas display box is written only on the `applyGrid` path, which no `ResizePort` call reaches; the
    flush is also debounced (100 ms default). Since #632 the controller's key carries the cell, so it is
    safe for container resizes across a spacing change; it still is not what re-sizes the canvas.
  - *Read the cell back.* A spacing change can hand back something other than what was asked without an
    error: a `lineHeight` the atlas cannot hold is shrunk, a failed re-bake rolls back, and on a lost
    context the cell does not move until the restore (since #772 the setter advances the font selector
    and defers). That is why `FitController` dedupes on the cell *and* the grid.
  - *A density change does not re-fit either* (#325). The watcher is now the surface's `DprWatcher`
    (#775), which re-bakes and re-derives each terminal's drawing buffer from the grid it holds; the grid
    stays the consumer's. xterm.js's `handleDevicePixelRatioChange` calls no resize either.
- **Outside input and its bounds.** The box comes from the DOM (a `ResizeObserver`, or the host's
  `getBoundingClientRect`); its values are bounded by the refusals above (non-finite, zero-area), and the
  result is floored below. Nothing bounds it above: a huge box proposes a huge grid, and the renderer's
  drawing-buffer clamp (#339) is what limits what is drawn.

## Code

- `justerm-web/src/fit.ts` — `proposeDimensions`, `gridForBox`, `FitInput`, `FitPadding`,
  `ResizePort`, `FitController`, `observeResize`, `MINIMUM_COLS`, `MINIMUM_ROWS`
- `justerm-web/src/justerm-renderer.ts` — `JustermRendererOptions`, `JustermRenderer.resize`,
  `applyGrid`, `terminalSize`
- `justerm-web/src/scrollbar.ts` — `Scrollbar`, the overlay track a lane keeps free
- `justerm-web/src/dpr-watcher.ts` — `DprWatcher`
- `justerm-renderer/src/webgl/surface.rs` — `css_cell_width`, `css_cell_height`, the divisor
- `justerm-core/src/lib.rs` — `Engine::resize`, `MIN_COLUMNS`; `justerm-core/src/session.rs` —
  `Session::resize`, which reports the adopted size
- Tests: `justerm-web/test/fit.test.ts`, `justerm-web/test/justerm-renderer.test.ts` (the two-path
  agreement rows), `justerm-web/e2e/demo.spec.ts` and `shared-surface.spec.ts` (the #1029 lane)

## Reference behaviour

- [Minimum screen size](../../agents/reference-facts.md#minimum-screen-size-547)
  — the floor `MINIMUM_COLS` mirrors
- [Who re-fits after a spacing change](../../agents/reference-facts.md#who-re-fits-after-a-spacing-change-578-verified-2026-07-29)
  — the consumer does, and calls `resize()`; xterm draws the same line, alacritty differs because it
  owns its OS window
- [When is a resize redundant — box, grid, or cell](../../agents/reference-facts.md#when-is-a-resize-redundant--box-grid-or-cell-632-verified-2026-07-30)
  — the references **do not agree on one shape**, so "the reference does X" cannot settle a dedupe
  question here; ours converges with alacritty's because `ResizePort` is published and write-only
- [A scrollbar lane](../../agents/reference-facts.md#a-scrollbar-lane--an-overlay-track-and-a-fit-that-leaves-its-width-free-1029-verified-2026-09-30)
  — xterm.js's track is an overlay too, and its fit subtracts the width from the box, gated on the
  `scrollback` option

Still unpinned: `proposeDimensions`' **rounding** against `FitAddon`'s, which the module names as its
model.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — the
  second site, repaired in #810: both paths floored a `0x0` box while refusing a `NaN` one, so the
  `display: none` box the README documents for hiding a pane proposed `2x1` and the engine reflowed
  through two columns. `fit.ts`' own comment had asked for the check — *"check **both** axes: the
  floor and the refusal"* — before the path that needed it existed
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md) — `FitController`'s
  dedupe could not express *"the cell moved but the grid did not"* (#578) until #632 put the cell in the
  key. The cell is a **proxy** for "a grid write bypassed this controller", so the key is only as
  complete as that set of writers
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — not a converter but the *source* of the out-of-range coordinate: the grid is floored, so a box that
  is not a multiple of the cell keeps a remainder strip outside the canvas, and a pointer there resolves
  one past the end (#667)
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — `FitController.dispose` and `observeResize`'s disposer are the host's to call; PenTerm uses neither and the demo writes `void disposeFit;`

## Blast radius

- [cell geometry](cell-geometry.md) — the divisor; `cssCellWidth()` is a float precisely so this
  arithmetic can be undone, and a change to how the cell is rounded changes every proposal
- [reflow](reflow.md) — a proposal that reaches `Engine::resize` re-lays the whole buffer, so a fit that
  oscillates (two paths disagreeing by a column) is a reflow per oscillation
- [multi-viewport](multi-viewport.md) — `applyGrid`'s grant read-back assumes the drawing buffer is
  `cols × cell` with the lane already gone; a change to who sizes the shared buffer changes what that
  read-back may shrink
- [widget lifecycle](widget-lifecycle.md) — the resize observer and the controller's debounce are
  host-ended ambient work; the widget never sees them
- [wide glyph](wide-glyph.md) — `MIN_COLUMNS` exists because a width-2 glyph needs two columns; the
  floor here is that pair model surfacing in a layout API
- [viewport](viewport.md) — `rows` decides how much of the buffer is on screen, so a fit that changes
  rows changes what a display offset and a damage row mean

## Known holes / open

- **The renderer may adopt less than was proposed, and `FitController` cannot see it.** The old note
  called this closed by #773; at HEAD `JustermRenderer.applyGrid` reads the browser's grant back and
  shrinks a sole tenant's grid (#339), so a `ResizePort` driven from `proposeDimensions` can hand the
  engine a grid the renderer does not hold. `FitController.last`'s own doc still lists it as hole 1.
  PenTerm sidesteps it by fitting through the renderer and sending `terminalSize()`.
- **A cell change inside the debounce window proposes against the pre-change cell** (#632's completeness
  pass). `FitController.latest` is a snapshot taken when the observer fired; the flush replays it and
  stores the old cell as the key. It self-heals on the next observer fire, and there may not be one. The
  cure is reading the geometry at flush time, which xterm's `fit()` does by construction.
- **The lane width is written twice and nothing checks the two agree** (#1029): once to `Scrollbar`'s
  `width`, once to `JustermRendererOptions.scrollbarWidth`. A `Scrollbar` wider than the lane still
  covers up to `trackWidth − lane` px of the last column. PenTerm keeps them equal by one constant
  (`NATIVE_SCROLLBAR_WIDTH`); nothing in this package does.
- **The engine's column clamp is invisible on the `Engine` API.** A consumer that sizes the engine
  directly and asks for one column gets two with no return value; only `Session::resize` reports it.
- **No governing record** for a contract that inverts the usual direction of a terminal API.
