# Cross-cutting invariant — the cell size is derived state, and every reader of it can go stale

## The fact

**The grid cell is not a constant and not owned by anyone who reads it.** It is derived, inside
`justerm-renderer`, from four inputs together — the glyph box, the device pixel ratio, the letter
spacing and the line height — and re-derived through a single funnel (`webgl.rs` `recompute_cell`)
whenever any of them moves. Five widget-exposed setters can move it: `setFontSize`, `setFontFamily`,
`setLetterSpacing`, `setLineHeight` (#578) and `setDevicePixelRatio` (#325, **wired 2026-08-10**).

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
  unchanged (`terminal.ts`), so a cell change with a stationary cursor left the IME anchor at
  `row * oldCellHeight`. The candidate window opened in the wrong place until the cursor moved. The
  cache key is the cell coordinate; the thing that went stale is the geometry. Fixed by **#631**, and
  the shape it chose is in the next section — measured before the fix: at row 5, an anchor of 95px
  against a correct 150px.
- **Deduped flush.** `FitController` returned early when the proposed `cols`/`rows` matched the last
  pair, so a cell change that leaves the grid identical never reached `resize()` — and `resize()` is
  the only place the canvas CSS box is set (`justerm-renderer.ts`, the only two `canvas.style.width/
  height` writes in the package). The browser then scaled a drawing buffer that no longer matched its
  display box. Worse and less obvious, the stale pair also **suppressed a later real resize**: once it
  described a grid nobody held, any container resize that happened to propose it was dropped.
  Fixed by **#632** — the key now carries the cell as well. Measured before the fix: a cell change
  from 19px to 30px left the memory at `88x31` while the renderer held `88x20`, and a resize to a
  viewport proposing `88x31` again never reached the port.
- **Wrong unit.** The published README built `CellGeometry` from `renderer.cellSize()` (device px) and
  fed it CSS-px `clientX`/`clientY`, so every click was off by the device pixel ratio on a Retina
  display (#578). This one is not even time-dependent — it was wrong from the first click.

## How a reader discharges the obligation

Three shapes are available, and **which one is right depends on how often the value is read, not on
how badly it can be wrong** (#631, 2026-07-30):

- **Re-read per use.** What `input.ts`, `selection.ts`, `Terminal.onWheel`, `proposeDimensions` and
  `JustermRenderer.resize` already do — a per-event `getGeometry()` / live `cssCellWidth()`. Seven of
  the nine readers in `justerm-web` are this shape, and it is what all three references do
  (xterm.js `_syncTextArea`, ghostty `imePoint`, alacritty `update_ime_position` — **none of them
  caches**). Correct whenever the read is event-rate rather than frame-rate.
- **Re-read at the point of use.** For a value that is *written* per frame but *read* at a few
  discrete moments, keep the per-frame cache and re-read where the read happens. #631's answer for
  the IME anchor: the OS reads it at composition start, and the browser's focus steps read it at
  `focus()`, so those two are the only places that must be fresh. **Note what this does not do** —
  the cache is still stale between those moments, deliberately; the claim is that nothing reads it
  there. That makes the claim falsifiable by a *new reader*, which is the thing to check before
  adding one.
- **Key the cache on everything the value derives from.** A cache over a derived quantity must carry
  *all* of its inputs, not the half the output happens to expose. `FitController` kept only the
  proposal (`cols`/`rows`) and not the cell it came from, so the same pair from a different cell read
  as "nothing happened" (#632). The key now carries both.
  **The stronger form, and why it was not taken:** the reference's executed dedupe prior art holds no
  copy at all — xterm.js's `CoreBrowserTerminal.resize` compares against `this.cols`/`this.rows`,
  i.e. authoritative live state, because the dedupe lives *at* the thing being resized. Ours cannot:
  `ResizePort` is write-only and published, so moving the dedupe there would hand every consumer a new
  idempotency obligation over `Engine::resize` + SIGWINCH. **So the widened key is a proxy** — what
  really invalidates it is any grid write that bypasses the controller, and the cell is merely the one
  such write that exists today (#578's setters). An out-of-band `renderer.resize()` at an *unchanged*
  cell would still leave it stale. Revisit if `ResizePort` ever becomes readable, or a second
  out-of-band writer appears.

**Why "just drop the cache" is not universally right here, even though every reference does it.**
Their cell is a **stored field they push to** (`dimensions.css.cell`, `size.cell`, `size_info`);
justerm-web's arrives through a consumer-supplied `getGeometry` callback whose cost the widget does
not control, and both the demo's and the README's do a `getBoundingClientRect()`. Per-frame is free
for them and a forced layout read per output flush for us. That is a **validity condition, not a
preference**: the day the widget holds a pushed cell, the reference's no-cache shape becomes the
right one.

**A push signal from the renderer was rejected**, not on cost but on ownership: it would be the first
renderer→consumer push channel, i.e. new ambient work in a layer that has no lifecycle owner
([widget lifecycle](../territory/widget-lifecycle.md), spine #605).

**That ground is now partly gone, and the rejection is worth re-reading rather than re-applying**
(#579, 2026-08-04). Wiring the renderer's context-loss surface made exactly such a channel: the
renderer holds a JS function for the life of the widget and calls it from a timer armed in wasm. So
"it would be the first" is no longer true, and the ownership question it deferred to has an answer at
one site — the widget registers an indirection it can close, and `dispose()` closes it, because the
renderer's own teardown of that slot runs at `free()` and the widget never gets there.
**What did not change is the reason the rejection was right here.** A loss notification is a rare,
terminal event that carries no value; a cell-change signal would fire on every spacing and DPR move
and carries a quantity every reader has to re-derive from. The precedent settles *whether a push
channel may exist*, not *whether this quantity should ride one* — and the invalidation question this
note holds is still open on spine #630.

A `ResizeObserver` on the canvas
was rejected on arithmetic — `resize()` sets the CSS box to `cols × cell`, and a cell change can
leave that product byte-identical (box 80px, cell 8→10, cols 10→8), so it is incomplete for the same
reason a grid-keyed dedupe is.

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

## Where it will recur

**The next reader of the cell that is added without being asked which of the three shapes it is.** The
question is never *"is this value important enough to keep fresh"* — every one of them looked
unimportant at the time — but *"at what rate is it read, and what happens between reads"*. A reader
that cannot answer the second half is defaulting to the cache, which is the shape that has failed here
every time.

Three named places it is already queued to recur:

- ~~**#325 (`matchMedia` → `set_device_pixel_ratio`).**~~ **Landed 2026-08-10, and the re-check it
  asked for was run: all nine readers were already safe.** Kept because the *result* is the useful
  part, not the prediction. Seven are "re-read per use" and were never at risk; the two cache-shaped
  ones had both been fixed generically rather than for the setter that found them — #631's anchor
  re-reads `getGeometry()` on every write, #632's fit key carries the cell — so a cell change with
  **no setter call at all** is discharged by the same code as one with. That is the argument for
  fixing this class at the shape rather than per trigger, and it is the first time it has been
  tested by a trigger that did not exist when the fixes were written.
  One reader looked like an instance and is not: `Terminal.onWheel` passes `dpr: 1` into
  `WheelContext`, which is a **deliberate neutral** — `getGeometry()` already reports CSS px, so
  `cellHeight / dpr` is CSS-px-per-cell, and the reason is stated at the call site.
  **What #325 did not settle is this note's open question.** It adopts the new ratio and re-applies
  the canvas box; it adds no invalidation *signal*, so a consumer's own readers are still on the
  honour system. That remains spine #630's.
- ~~**#249 (inline preedit).**~~ **Landed 2026-08-03 (ADR-0028) and it did not recur, for a reason
  worth keeping.** The predicted mismatch was two readers of the cell on two refresh policies — the
  drawing at frame rate, the IME anchor at points of use. It did not arise because the *drawing* never
  reads the cell at all: it is expressed in cell **coordinates** and the renderer owns the conversion,
  so the only consumer-side reader added was the anchor writer, which calls `getGeometry()` afresh on
  every write. The general form: a feature stated in cells rather than pixels is not a reader of this
  quantity, however visual it looks.
- **#287 (multi-viewport).** N grids under one GL context means the cell stops being a single value.
  Every "the cell" in this note becomes "which cell", and a reader that closed over the wrong one
  fails silently — the same failure mode, one dimension wider.

The falsifier for the middle shape is stated where it is chosen and repeats here because it is the one
that decays without anyone touching it: **"nothing reads the cache while it is stale" is a claim about
the current set of readers, not a property of the design.** #649 tested it and it held — the browser's
focus steps really are a reader, and they were already served — but the next reader added anywhere in
the family retests it without meaning to.
