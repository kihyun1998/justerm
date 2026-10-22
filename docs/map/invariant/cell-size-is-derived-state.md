# Invariant — the cell size is derived state, and every reader of it can go stale

## The fact

The grid cell is not a constant and is not owned by anyone who reads it. It is derived inside
`justerm-renderer` from four inputs together — glyph box, device pixel ratio, letter spacing, line
height — and re-derived through one funnel, `justerm-renderer/src/webgl/font.rs — bake_config`, whenever
any moves (the funnel was `recompute_cell` until #772, 2026-08-19; same four inputs, the result now
lives on a font configuration). Re-verified at HEAD 7bb31f7, the widget setters that move it are
`JustermRenderer.setFontSize`, `setFontFamily`, `setLetterSpacing`, `setLineHeight` (#578) — each naming
its grid on the backend since #773 — and `setDevicePixelRatio` (#325, wired 2026-08-10; also on
`TerminalSurface`), which moves every cell at once. `setFontWeight` / `setFontWeightBold` (#928) re-bake
through the funnel and do not move it: `rasterizer.rs — Rasterizer::new` measures `█` at `normal`
weight whatever a configuration draws at.

Everything that divides by a cell dimension holds a value with a **lifetime**, which no type states:

- the fit (`cssCellWidth` / `cssCellHeight` → proposed `cols` / `rows`)
- pointer → cell for mouse reporting and selection (`CellGeometry`)
- the hidden textarea's position, where an IME candidate window opens
- the wheel scroller's retained sub-line fraction
- the drawing buffer and the canvas CSS box derived from it

Two consequences, the second the one that gets missed: a cell change obliges every reader to re-read,
and that cannot be discharged by watching the **grid**, because a cell change can leave the grid
identical — routinely once `MINIMUM_COLS` / `MINIMUM_ROWS` bind (#547), or whenever the new cell divides
the same box into the same count.

The value also has a **unit** (the renderer reports device px; every pointer consumer needs CSS px) and,
since #773, a **subject** — whose cell (`cssCellWidth(grid)`). A value read for the wrong grid is
neither stale nor mis-scaled; it is another terminal's, and looks plausible. #775 closed the subject
axis for the widget's readers: a widget still holds exactly one grid (`JustermRenderer` holds its lease
id for its life; the N-grid registry went to `TerminalSurface`, which reads no cell), so the subject is
carried by object identity. #773's prediction that each reader would gain a grid parameter was wrong.
Valid while one widget means one grid.

## Why it is cross-cutting

The cell is produced in one crate and consumed in several modules of another, across five territories,
and the seam is a `number` with no unit, owner or invalidation signal. The readers (`fit.ts`,
`input.ts`, `selection.ts`, `Terminal.positionTextarea`, `Terminal.onWheel`) do not call each other;
each was written against a cell that happened to be stable, because for most of the project's life only
the font setters moved it and one was wired. The two failure modes — stale (wrong time) and mis-scaled
(wrong space) — both present as "clicks land on the wrong cell".

## Territories it holds in

- [cell geometry](../territory/cell-geometry.md) — where the cell is derived (`bake_config`,
  `Rasterizer::new`).
- [fit](../territory/fit.md) — the largest consumer; `justerm-web/src/fit.ts — FitController` keys its
  dedupe on the grid **and the cell** (`last: { cols, rows, cellWidth, cellHeight }`, #632).
- [input encoding](../territory/input-encoding.md) — `CellGeometry`, pointer → cell, re-read per event
  via `getGeometry()`.
- [selection](../territory/selection.md) — the same conversion at its own call sites.
- [widget lifecycle](../territory/widget-lifecycle.md) — `Terminal.positionTextarea`'s cache keyed on
  the viewport cell, which cannot see a cell change; `Terminal.syncTextareaAnchor` re-reads at
  composition start and focus (#631).

## What a violation looks like

Always silent and never in the layer that caused it: the renderer draws correctly; the consumer's
arithmetic is wrong.

- **Stale divisor** — clicks and drags resolve to the wrong cell, error growing with distance from the
  origin; selection appears to drift.
- **Stale cached decision** — `Terminal.positionTextarea` skips the write when the cursor cell is
  unchanged, so a cell change with a stationary cursor left the IME anchor at `row * oldCellHeight`
  until the cursor moved. Measured before #631: row 5, anchor 95px against a correct 150px.
- **Deduped flush** — `FitController` returned early when `cols` / `rows` matched the last pair, so a
  grid-preserving cell change never reached `resize()`, the only place the canvas CSS box is set; the
  browser scaled a mismatched drawing buffer, and the stale pair suppressed a later real resize that
  proposed it. Measured before #632: a 19px → 30px cell change left memory at `88x31` while the renderer
  held `88x20`, and a resize proposing `88x31` never reached the port.
- **Wrong unit** — the published README built `CellGeometry` from `renderer.cellSize()` (device px)
  against CSS-px `clientX` / `clientY`, off by the DPR on every click on a Retina display (#578).

**How a reader discharges it** (#631, 2026-07-30) — the right shape depends on the read rate, not on
how badly it can be wrong:

- **Re-read per use** — `input.ts`, `selection.ts`, `Terminal.onWheel`, `proposeDimensions`,
  `JustermRenderer.resize` (per-event `getGeometry()` / live `cssCellWidth()`); seven of nine readers
  at #325, and what all three references do (xterm.js `_syncTextArea`, ghostty `imePoint`, alacritty
  `update_ime_position` — none caches).
- **Re-read at the point of use** — keep a per-frame cache, re-read where the value is read (#631: the
  OS reads the anchor at composition start, focus steps at `focus()`). The cache is stale between those
  moments on purpose; the claim "nothing reads it there" is falsifiable by a new reader. #649 tested it
  and it held.
- **Key the cache on every input** — #632's key. The stronger form, xterm.js comparing against live
  `this.cols` / `this.rows` in `CoreBrowserTerminal.resize`, is unavailable because `ResizePort` is
  write-only and published; so the widened key is a proxy, and an out-of-band `renderer.resize()` at an
  unchanged cell would still leave it stale. Revisit if `ResizePort` becomes readable or a second
  out-of-band writer appears.

Why not drop the cache as the references do: their cell is a stored field they push to; justerm-web's
arrives through a consumer `getGeometry` callback whose cost (a `getBoundingClientRect()` in the demo
and README) the widget does not control. A validity condition, not a preference. A renderer push signal
was rejected on ownership (no lifecycle owner, spine #605); #579 (2026-08-04) since made one such
channel for context loss, closed by `dispose()`, so "it would be the first" no longer holds — but a loss
is rare and carries no value, while a cell signal would fire on every spacing and DPR move. A
`ResizeObserver` on the canvas was rejected on arithmetic: `cols × cell` can be byte-identical across a
cell change (box 80px, cell 8 → 10, cols 10 → 8). The invalidation-signal question, and the roster of
instances, live on spine #630, not here — #552's measured result that a hand-copied roster in ADR-0025
went stale in five places in three days ([theflow.md](../../agents/theflow.md)).

## Discovery history

Every instance so far was found while doing something else, none by the layer that owns the cell.

- **#417** — wired `setFontSize` / `setFontFamily` and stated the consumer-re-fits contract, pinned to
  two setters rather than to the cell.
- **#547** — floored the fit after a 1-column proposal desynchronised engine and renderer: the same
  silent desync from the box side.
- **#578** — two more setters; the adversarial pass asked "which readers of the cell did you not
  check?" and found two stale readers plus the README unit error.
- **#631 / #632** — the two cache-shaped readers fixed by shape, not by trigger.
- **#325** (landed 2026-08-10) — the re-check ran and all nine readers were already safe, because the
  two cache-shaped fixes were generic: a cell change with no setter call is discharged by the same
  code. `Terminal.onWheel` passes `dpr: 1` into `WheelContext` deliberately — `getGeometry()` already
  reports CSS px. It adds no invalidation signal.
- **#808** (2026-08-24) — `TerminalSurface.onDensityChange` fires after a context restore adopted a
  density that moved while the context was dead. Surface → host, carries density not the cell, and its
  readers are the device-px quantities only the host can re-measure (ADR-0021 D3) — so the membership
  test has a second half on a shared surface: a cell-derived value living in the host can only be told.
- **#249** (landed 2026-08-03, ADR-0028) — inline preedit did not recur the hazard: the drawing is in
  cell coordinates and the renderer converts, so the only new reader was the anchor writer, which
  re-reads. A feature stated in cells is not a reader of this quantity.

Membership test, so instances are derived: does the value reach the rasteriser's measurement or
`device_cell` inside `bake_config` — family, size, letter spacing, line height, or the density every
configuration is baked at? Reaching `bake_config` is not enough: the two weights (#928) and the subpixel
setting (#961, a second coverage on its own canvas) re-bake without moving the measurement.
`setCursorContrast` / `setCursorThickness` (#580) are draw-time scalars and not instances.

## Where it will recur

- **The next reader of the cell added without being asked which shape it is.** The question is not
  "is this important enough to keep fresh" but "at what rate is it read, and what happens between
  reads". A reader that cannot answer the second half defaults to the cache, which has failed here every
  time.
- **A widget holding two grids** (#287's tail): the subject axis reopens exactly as #773 predicted, and
  each reader must say which grid it asks about.
- **A cell-derived value outside the widget**, in a host that gave the renderer device px (#808): no
  shape to fall back on.
- **Any new reader of the textarea anchor between composition start and focus** retests #631's
  "nothing reads it while stale".
