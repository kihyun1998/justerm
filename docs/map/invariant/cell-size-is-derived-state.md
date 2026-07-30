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

## The roster lives in the spine, not here

**Which issues are instances is tracked on spine #630, deliberately.** This note holds the rule; the
roster is a different kind of fact and wants a different home.

That split is not a preference — it is #552's measured result, recorded in
[theflow.md](../../agents/theflow.md): a hand-copied roster inside ADR-0025 *went stale in five places
within three days* while the rule itself (D1–D4) needed no edit. A roster wants a mutable home and a
rule wants an immutable one, so they separate **even after the rule exists**. Copying the instance list
into this file would reproduce exactly the failure that observation is about.

What the spine carries and this note deliberately does not: the current instance list, the recurrence
sites, and the open question (*what should an invalidation signal look like?*).

## Discovery history

Kept here because it is about the *rule*, not about who is currently on the list: **every instance so
far was found while doing something else, and none was found by the layer that owns the cell.**

- **#417** wired `setFontSize`/`setFontFamily` and established the consumer-re-fits contract in a
  doc-comment. The contract was right; it was pinned to two setters rather than to the cell — which is
  why adding two more setters found readers nobody had re-checked.
- **#547** floored the fit at `MINIMUM_COLS`/`MINIMUM_ROWS` after a 1-column proposal desynchronised
  the engine from the renderer — the same silent-desync failure, reached from the box side instead of
  the cell side.
- **#578** added two setters and found two stale readers *while looking for something else*: the
  adversarial pass asked "which readers of the cell did you not check?", not "is there a bug in the
  textarea". It also found the unit half — a published README example that had been dividing CSS-px
  pointer coordinates by a device-px cell.

The membership test, so the list can be derived rather than remembered: **does the value reach
`recompute_cell`?** `setCursorContrast`/`setCursorThickness` (#580) do not — they are draw-time scalars
and are not instances, which is worth stating because they look like near neighbours.
