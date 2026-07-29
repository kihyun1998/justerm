# Cross-cutting invariant — the cell size is derived state, and every reader of it can go stale

## The fact

**The grid cell is not a constant and not owned by anyone who reads it.** It is derived, inside
`justerm-renderer`, from four inputs together — the glyph box, the device pixel ratio, the letter
spacing and the line height — and re-derived through a single funnel (`webgl.rs` `recompute_cell`)
whenever any of them moves. Five widget-exposed setters can move it: `setFontSize`, `setFontFamily`,
`setLetterSpacing`, `setLineHeight` (#578) and `setDevicePixelRatio` (#325, still unwired).

Everything downstream that divides by a cell dimension therefore holds a value with a **lifetime**, and
nothing in the type system says so:

- the fit (`cssCellWidth`/`cssCellHeight` → proposed `cols`/`rows`)
- pointer → cell for mouse reporting and for selection (`CellGeometry`)
- the hidden textarea's position, which is where an IME candidate window opens
- the wheel scroller's retained sub-line fraction
- the drawing buffer itself, and the canvas CSS box derived from it

**Two things follow, and the second is the one that gets missed.** A cell change obliges every reader
to re-read — *and* the obligation cannot be discharged by watching the **grid**, because a cell change
can leave the grid identical. That happens routinely, not exceptionally: once `MINIMUM_COLS` /
`MINIMUM_ROWS` bind (#547), or whenever the new cell divides the same box into the same count, the grid
is unchanged while every pixel quantity under it has moved.

## Why it is cross-cutting

The cell is produced in one crate and consumed in five modules of another, across four map
territories, and the seam between them is a `number` — a quantity with no unit, no owner and no
invalidation signal. Each consumer was written against a cell that happened to be stable at the time,
because for most of this project's life only the font setters could move it and only one of them was
wired.

It also crosses a **unit** boundary in the same step, which is why the two failure modes look
unrelated: the renderer reports the cell in **device** px, while every pointer consumer needs **CSS**
px. So a reader can be stale (read the wrong *time*) or mis-scaled (read the wrong *space*), and both
present as "clicks land on the wrong cell".

## Territories it holds in

- [cell geometry](../territory/cell-geometry.md) — where the cell is derived. Its `## Code` names the
  renderer setters; it does not name the widget as a place a cell change *arrives*
- [fit](../territory/fit.md) — the largest consumer, and the one with a dedupe that this invariant
  makes unsafe to route a cell change through
- [input encoding](../territory/input-encoding.md) — `CellGeometry`, pointer → cell
- [selection](../territory/selection.md) — the same conversion, its own call sites
- [widget lifecycle](../territory/widget-lifecycle.md) — the textarea-position cache is state that
  outlives the geometry it was computed from

## What a violation looks like

**Always silent, and never in the layer that caused it.** Nothing throws, nothing logs, and the
renderer keeps drawing correctly — it is the consumer's arithmetic that is wrong.

- **Stale divisor.** Clicks and drags resolve to the wrong cell; the further from the origin, the
  larger the error. Selection appears to "drift".
- **Stale cached decision.** `Terminal.positionTextarea` returns early when the cursor *cell* is
  unchanged (`terminal.ts`), so a cell change with a stationary cursor leaves the IME anchor at
  `row * oldCellHeight`. The candidate window opens in the wrong place until the cursor moves. The
  cache key is the cell coordinate; the thing that went stale is the geometry.
- **Deduped flush.** `FitController` returns early when the proposed `cols`/`rows` match the last
  pair, so a cell change that leaves the grid identical never reaches `resize()` — and `resize()` is
  the only place the canvas CSS box is set. The browser then scales a drawing buffer that no longer
  matches its display box.
- **Wrong unit.** The published README built `CellGeometry` from `renderer.cellSize()` (device px) and
  fed it CSS-px `clientX`/`clientY`, so every click was off by the device pixel ratio on a Retina
  display (#578). This one is not even time-dependent — it was wrong from the first click.

## Discovery history

Recorded because the shape of the discovery is the argument for writing it down: **each instance was
found while doing something else, and none of them was found by the layer that owns the cell.**

- **#417** wired `setFontSize`/`setFontFamily` and established the consumer-re-fits contract in a
  doc-comment. The contract was right; it was pinned to two setters rather than to the cell.
- **#547** floored the fit at `MINIMUM_COLS`/`MINIMUM_ROWS` after a 1-column proposal desynchronised
  the engine from the renderer — the same silent-desync failure, reached from the box side.
- **#578** added two more setters and found the stale-cache and deduped-flush instances *while
  looking for something else* — the adversarial pass asked "which readers did you not check?", not
  "is there a bug in the textarea".
- **#325** is the fifth setter and is still unwired, so it will arrive with the same obligation.

## Where it will recur

- **#325** (`setDevicePixelRatio`) — the cell's fourth input, and the only one that moves without a
  consumer asking. A resolution change is the case where nobody calls a setter at all.
- **#580** (cursor contrast / thickness) — *not* an instance, and worth saying so: those are draw-time
  scalars that leave the cell alone. The tell for membership is whether the value reaches
  `recompute_cell`.
- **#579** (context-loss surface) — a spacing change while the context is lost stores the policy and
  defers the cell move to `restore()`, so the consumer's re-fit runs against the *old* cell and the
  correction arrives with no signal.
- **#287** (multi-viewport) — one context serving N grids means N cells; a per-grid cell change would
  need this invariant expressed per viewport rather than per renderer.
