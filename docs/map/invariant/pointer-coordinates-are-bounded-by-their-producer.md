# Invariant — a pointer coordinate is bounded by the converter that produces it

## The fact

A pixel position becomes a grid coordinate by dividing by the cell. The division is total — it answers
for every pixel on the page, including those outside the grid — so the **converter**, not whatever
consumes its answer, owes the bound on both axes and at both ends. Two asymmetric failures:

- **Overshoot** (past the last column / row) yields a cell merely too large; a range-clipping reader
  tends to absorb it.
- **Undershoot** (pointer above or left of the canvas origin) yields a negative cell —
  `Math.floor(-3 / 10)` is `-1` — and the value crosses a boundary (a wire field, an IPC parameter, a
  `usize`) where negative has no representation and becomes enormous.

Out-of-grid input is ordinary: a drag leaves the grid whenever the pointer does (`mousemove` / `mouseup`
are window-scoped in every wiring this repo ships), and `fit.ts — proposeDimensions` floors the grid
from the container, so a box that is not a multiple of the cell keeps a remainder strip resolving one
past the end.

The engine is not the backstop, structurally: some pointer coordinates never reach it —
`SelectionController`'s alt-click cursor move leaves through the consumer's `onMoveCursor` callback. A
second reason once given, that `Term::viewport_to_abs` bounded only the row, was retracted by #671 (it
bounds both at HEAD 7bb31f7); the rule did not rest on it. A reason that expires when another layer
improves was never load-bearing.

[ADR-0026](../../adr/0026-outside-coordinates-are-bounded-once.md) governs the engine side — where a
coordinate arriving from outside is bounded — and explicitly leaves this consumer-side rule alone on the
reason above. Read the record before changing where `justerm-core` bounds something, this note before
changing what `justerm-web` hands it.

Reference: 3/3 references bound both axes at both ends in the consumer-side converter, before anything
reaches their engine, and xterm.js serves mouse reporting, selection and linkification from one
converter; the two details that deliberately did not transfer (xterm's `colCount + 1`, alacritty's
`end_of_grid` arm) are in
[reference-facts](../../agents/reference-facts.md#who-bounds-a-pointer-coordinate--the-producer-not-the-engine-667-verified-2026-07-31).

## Why it is cross-cutting

The obligation belongs to a shape — pixels in, cell out — not a feature. Every territory accepting a
pointer grows its own converter, written by whoever needed it, sharing no call graph, type or test
(the two in `input.ts` / `selection.ts` share `clampTo` since #667; the a11y bridge, the link converter
and the engine do not). A grep for the fix at one site (`clampTo`, `min`, `clamp`) cannot find the site
that never heard of it — the reason [the alt-screen floor](alt-screen-buffer-floor.md) needed a note
rather than a helper.

## Territories it holds in

Symbols re-verified at HEAD 7bb31f7.

- [selection](../territory/selection.md) — `justerm-web/src/selection.ts — cellAndSide` turns a
  press/drag into an anchor and a `Side`; the clamp comes before the side, so overshooting right lands
  on the last column's `Right` and left on column 0's `Left`. `SelectionController.tick` is the one row
  producer not going through it.
- [input encoding](../territory/input-encoding.md) — `justerm-web/src/input.ts — cellEvent` with
  `clampTo`, the mouse-reporting converter, bounded because an unbounded value would wrap in core's
  `encode_mouse` (#266). It clamps `px` / `py` too, for `?1016` SGR-pixel reporting, and since #907
  floors them (fractional CSS px are ordinary from `getBoundingClientRect()` / `clientX`, and a consumer
  mirroring core's `usize` refused every report) to the last pixel inside the grid, `ceil(extent) − 1`
  (xterm.js's `getMouseReportCoords` rule). Unit fixtures with integer geometry could not see it; the e2e
  proof gets its fraction from a sub-pixel canvas origin. `CellGeometry`'s field docs carry its
  preconditions; `geometryViolations` / `checkGeometry` signal violations (#672).
- [accessibility](../territory/accessibility.md) — `justerm-web/src/a11y-selection.ts — clamp`
  converts DOM text offsets, not pixels; same obligation, discharged (out-of-tree endpoints resolve to
  the tree's edges).
- [hyperlinks](../territory/hyperlinks.md) — `justerm-web/src/pointer.ts — cellAt`, the link converter
  (#934). It **refuses** rather than clamps: outside `0..cols` × `0..rows` it answers `undefined`, so
  the remainder strip hovers no link; clamping, right for a report or an anchor that must name some
  cell, would hover the edge link. A zero-height box resolves to `Infinity` and is refused by the same
  test.
- [fit](../territory/fit.md) — the source of the remainder strip, not a converter.
- [viewport](../territory/viewport.md) — `justerm-core/src/term/viewport.rs — Term::viewport_to_abs`,
  the engine-side clamp on both axes, explicitly not a substitute. Added here; this territory already
  linked back.

Not a member: `scrollbar.ts` computes a ratio of `rows` / `scrollbackLen`, never a cell from a pixel;
it clamps by coincidence of range arithmetic.

## What a violation looks like

Nothing throws and the widget looks fine. A drag past the left edge selects from a negative column; a
press in the remainder strip anchors one row past the last. Then it depends on the consumer: a
range-clipping reader returns an empty selection (a gesture that silently does nothing); an
absolute-index walk reads the wrong region; a transport reinterpreting the sign turns `-1` into its
unsigned maximum, and `usize::MAX + 1` overflows in the `Side::Right` arms of
`justerm-core/src/term/selection.rs`'s resolve (on wasm32 `usize` is 32 bits, so `-1` maps exactly onto
`usize::MAX`).

Review tell: a bare `Math.floor(px / cellWidth)` with no bound on the same expression, or a converter
whose siblings in the same package clamp while it does not.

## Discovery history

Found twice at sibling sites months apart by two routes:

- **#266** — renderer S6 pixel → cell, then `input.ts`, reached through core's `encode_mouse` wrapping;
  the fix's comment named that mechanism, so it read as a mouse-reporting fact.
- **#667** — the sibling converter in `selection.ts`, never given the bound; surfaced as a
  "deliberately not fixed" note while working #660, whose engine row clamp made the gap look closed
  when only half was. #660's doc comment named the sibling by file and line, the gap's only durable
  record.
- **#671** — the engine clamp gained the column; the second reason above was retracted.
- **#672** — `NaN`, which no bound catches: `clampTo` propagates it through `Math.max` and `Math.min`.
  Settled as a precondition: `CellGeometry` states what its six fields may be (finite; positive finite
  cell; non-negative integer counts) and the converters signal rather than refuse — xterm's
  `hasValidCharSize` → `undefined` guard is half a repair loop whose re-measure half this widget cannot
  have (geometry is the consumer's, ADR-0017). #819 retired one clause: a refused gesture does resume
  on the right cell when the box returns, because `getGeometry` is pulled per event; #819 refuses on a
  different axis, an absent box ([an absent box measures as zero](an-absent-box-measures-as-zero.md)).
- **#934** — the demo's unbounded fourth converter (`demo/main.ts — cellFromEvent`, feeding
  `LinkController`) went away when link hover moved into the widget as `cellAt`. Its inertness had been
  conditional and unstated (#819): it held only while the page derived its cell from the measured box
  (a zero `cellHeight` sends the quotient to `Infinity`); with a renderer-derived cell, as the README
  recommends, it answered an in-range cell against a hidden pane on bare motion. (The old note's "now
  answers `[number, number] | undefined`" described a function that no longer exists.)

The sibling rule one layer out, from the same work — **a producer owes its consumer a value the
consumer's type can mean** — is not this invariant (those producers compute ratios and line counts,
not cells), but it was found here and its sites are:

- **#675** — `WheelScroller` latched a non-finite delta in `wheelPartialScroll` (`Infinity % 1` is
  `NaN`, cleared only by `reset()`); `dragScrollSpeed` emitted `NaN` at `py === 0`. Fixed by making each
  producer total. `wheelScrollTarget` (exported, so it owes its own totality) answers `null` and checks
  its **inputs**, because the clamp rescues an infinite request into a finite wrong one
  (`Math.max(0, Math.min(100, 10 - Infinity))` is `0`). `routeWheel` refuses a non-finite line count
  first, because `direction` from `lines < 0` is false for `NaN` and the application branch fabricated
  a `down`; it does not assert `wheelScrollTarget`'s result non-null.
- **#814** — `dragToDisplayOffset` measured across four poisoned inputs: three surfaced `NaN`,
  `rows: Infinity` surfaced `0` (a plausible jump to the live edge); moving the guard to the result
  reddens exactly that assertion. `scrollbarMetrics` divided after a relational `visible` check and
  emitted `"NaN%"` into the thumb styles; #463 had fixed the same in `decorations.ts` with the comment
  "`Number.isFinite` is the check", on the marks half only. Found by grepping the closed backlog by
  artifact.
- **#680** — a drag outliving the canvas box auto-scrolled at maximum speed because
  `SelectionController` builds its viewport height as `getRows() * geom.cellHeight`, and a product loses
  which factor was zero. Guarding the product would have re-decided #667 (a 0-row viewport is
  load-bearing for `tick()`'s edge-row floor; its test caught that attempt). The guard sits at the
  caller still holding the factors apart, because `cellHeight` has a precondition and `getRows()` does
  not. Generalised: when a derived value is ambiguous, the fix is upstream of the multiplication.
- **#908** — the integer axis: line and page wheel modes returned `deltaY × scrollSensitivity` as-is, so
  a fractional sensitivity gave `onScroll` a fractional offset while the scrollbar rounded. Every mode
  now uses one accumulator in lines (alacritty and ghostty share one across modes; lines, not pixels,
  because LINE mode must work with an unmeasured cell, #675), emitting whole lines toward zero, and a
  `deltaMode` change drops the remainder. Carrying created a sub-line case; of three options shown
  2026-09-15 the maintainer chose to consume only a LINE / PAGE notch carried below a whole line, leaving
  the PIXEL half of `Terminal.onWheel`'s #133 native-scroll policy untouched by decision. The retained
  fraction's staleness across a cell change is spine #630's third instance
  ([cell size](cell-size-is-derived-state.md)); since #908 only its pixel-mode part can go stale.

## Where it will recur

- **Any new pointer affordance** — hover, link click, a context-menu target. Per-decoration
  hit-testing (#502) was retired 2026-08-18: ADR-0024 R1 makes decorations object-less, so there is
  nothing to hit; it depended on a model question that resolved the other way.
- **#287 multi-viewport** — N grids under one context means N sets of bounds, and a pointer inside the
  canvas but outside this viewport becomes ordinary.
- **A side-from-raw-pixel refactor.** The clamp doubles as the overshoot rule for `Side`; computing the
  side from the unclamped pixel (alacritty's shape) would need alacritty's `end_of_grid → Right` arm
  restored with it.
