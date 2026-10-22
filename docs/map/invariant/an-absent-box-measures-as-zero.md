# Invariant — an absent element box measures as zero, and zero is in range

## The fact

A DOM element with no box reports every field as `0`, and `0` is a legal value for everything derived
from it. `getBoundingClientRect()` on a `display: none`, detached or not-yet-laid-out element returns
all zeros, so "there is no box" and "the box is at the origin / is empty" reach every downstream site
as the same number. No finiteness guard separates them: zero is finite.

The sibling failure, a `NaN` box, is not this one and is usually handled already: `NaN` propagates and
is refused; zero computes cleanly into a plausible wrong answer.

**The repair is upstream of the derivation, and it belongs to whoever measured the box.** Every site
answers `T | undefined` from the function that took the measurement (#819 completed the set). What
differed per site was only the preparation: #801 carried the extent into the input before it had a
union to return, #810 widened a predicate on a union it had, #814 extracted a pure function to have a
return, #819 reset controller state the others did not own.

It stays a note rather than a helper because "the party that measured" differs: three times justerm,
once (`CellGeometry`) the consumer across a published callback. A helper holds arithmetic; it cannot
hold "ask the party that looked". That is also why #680, #672 and #675 are each excluded: each is about
a value justerm did not measure.

## Why it is cross-cutting

Four sites, four territories, no shared call path, type or test. Each reads a box for its own purpose —
a viewport origin, a column count, a pointer basis, a scroll ratio — through different arithmetic, and
from inside each the question reads as a local numeric edge case whose local answer is usually right
for that site alone (the fit floor gives a small container a usable terminal; geometry is permissive
because a position may be zero or negative). The first two sites were found by one change (#801); the
second pair only by asking which other readers of an element box exist.

It is a sibling, not an instance, of
[pointer coordinates are bounded by their producer](pointer-coordinates-are-bounded-by-their-producer.md),
whose "the fix is usually upstream of the multiplication" is `SelectionController.mouseMove`'s rule
(#680): its zero comes from `getRows() * cellHeight`, a consumer callback times a renderer-derived
cell, neither a DOM box. Same generalisation, different cause (absence vs a lossy product). Reading
#680 as an instance was #801's first hypothesis, and it was wrong; the genuine second site is
`fit.ts`.

## Territories it holds in

Symbols re-verified at HEAD 7bb31f7. The roster of who is left lives in spine
[#815](https://github.com/kihyun1998/justerm/issues/815), not here.

- [multi-viewport](../territory/multi-viewport.md) — `justerm-web/src/terminal-surface.ts — viewportOrigin`.
  Repaired in #801: the overlay's extent is carried with its origin (`OverlayBoxes`) and the return is
  `{x,y} | undefined`. Before, the clamp answered `{0,0}` and re-placed a full-size grid on the canvas
  corner over a sibling. Its repair is the shape the others took.
- [fit](../territory/fit.md) — `justerm-web/src/fit.ts — proposeDimensions` and `gridForBox` (moved
  from `justerm-renderer.ts` in #992). Repaired in #810: both refuse a box with no area as they already
  refused a non-finite one; before, `0x0` floored to `MINIMUM_COLS` x `MINIMUM_ROWS` (a `display: none`
  box proposed `2x1`, a `NaN` box was refused). Two callers: `JustermRenderer.resize` passes a
  consumer-measured box and is live; `applyGrid`'s grant read-back passes the drawing buffer, which
  `justerm-renderer/src/webgl/surface.rs — resize_surface` refuses to set at `<= 0` and an empty
  read-back preserves (`apply_surface_size`), so the guard is defensive there — a completeness pass
  twice claimed it was a live third site, by two different mechanisms, and both fell to reading
  `apply_surface_size`. The one place this invariant diverges from prior art: alacritty refuses a zero
  box, ghostty and xterm.js floor it (`docs/agents/theflow.md` § the box-refusal row, #810).
- [selection](../territory/selection.md) — `justerm-web/src/input.ts — CellGeometry.originX` /
  `originY`, read by `selection.ts — cellAndSide`, `input.ts — cellEvent` and
  `SelectionController.mouseMove`'s drag-scroll term. Repaired in #819 as `getGeometry(): CellGeometry |
  undefined` (`input.ts`, `pointer.ts`) — the only repair crossing a published seam, because the
  consumer took the measurement. These two fields are the only ones of six with no stated precondition
  (a position may be `0` or negative), so `geometryViolations` cannot flag them. A rect-derived cell
  also goes to `0`, fires #672's signal, and #680's `cellHeight > 0` guard suppresses the auto-scroll; a
  renderer-derived cell (`renderer.cellSize() / dpr`, what `demo/shared-surface.ts` and the package
  README do) stays positive, so only the origin moves and nothing fires. Second half: `dragScrollAmount`
  is controller state, so an early return would latch the last speed (#675's shape), and
  `SelectionController.tick` carries its own guard because the tick timer (`Terminal`'s since #902)
  fires whether or not the pointer moves. #672's "signal, do not correct" is untouched (scoped to the
  `NaN` axis). Two `Terminal` readers carry their own reason: `Terminal.onWheel` reads the geometry once
  per notch, since a `0` cell would latch the scroller's accumulator (#675); `writeTextareaAnchor` keeps
  the last anchor, correct because the textarea is positioned relative to the element, which is off
  screen too.
- [viewport](../territory/viewport.md) — `justerm-web/src/scrollbar.ts — dragTrackRatio`, called by
  the private `Scrollbar.dragTo`. Repaired in #814: the ratio takes the track box as data and answers
  `number | undefined`, refusing `height <= 0`; `dragTo` then makes no request and deliberately does not
  end the drag. Before, `(clientY - r.top) / r.height` on a zero track was `±Infinity`, which the
  surrounding clamp turned into a plausible track end (or `NaN` at exactly `r.top`). A finiteness test
  nearly worked: on the un-clamped quotient it is equivalent up to a negative height (a mutation reddens
  exactly one assertion); on the returned value it accepts every zero box, because the clamp makes
  `Infinity` a finite `1`.

## What a violation looks like

A plausible wrong answer, no error, damage usually on a bystander. Measured shapes:

- **#801, real browser, before the fix.** Hiding the second pane of
  `justerm-web/demo/shared-surface.html` with `display: none` moved its viewport from `[500, 40]` to
  `[0, 0]`; a full-size grid was drawn over its neighbour, which went on reporting healthy. The
  renderer's zero-area guard is not on that path: a viewport's extent is derived from `cols * cell`,
  never the measured box.
- **Fit**, against the real module. A hidden pane still being fitted proposes `2x1`, the engine reflows
  through two columns, and showing it again does not undo it. Severity (#810): on the alt screen a
  resize is a re-fit that drops rows with nothing to restore them ([reflow](../territory/reflow.md),
  #567), and `Term::resize` drops the selection on any geometry change while on alt (at HEAD; on the
  primary it re-anchors — corrected from the old note's "any geometry change").
- **A drag that outlives its box (#814, real browser).** `Scrollbar` and `SelectionController` bind
  move/up to `window`, and `Scrollbar.update()` hides the track without clearing `dragging`. With the
  guard off and the host 40 lines up, two moves against an all-zero rect produced two spurious scroll
  requests and left the host's display offset at `NaN`; with it, nothing. Which element is hidden
  decides the harm: a host hiding an ancestor keeps `scrollbackLen`, so the ratio is `1` and the offset
  is slammed to `0` on top of the `NaN`; the widget hiding its own track (`visible` means
  `scrollbackLen > 0`) happens with no host action when RIS (`term.rs — Term::full_reset`), `ED 3` or
  `Engine::clear` (#936) empties scrollback, and only the `NaN` survives. Hiding the track while
  `scrollbackLen` stays positive does not reproduce (the first bad request re-renders, `update()` shows
  it again) — a probe artifact this note briefly mistook for the rule until the RIS route was found.
- **The same drag at the pointer converter (#819, real browser) — the worst.** With a renderer-derived
  cell, three ticks of a hidden pane called `onScroll` 3 times for 45 lines (`3 × DRAG_SCROLL_MAX_SPEED`,
  the maximum) plus a `port.extend(edgeRow, …)` per tick, with zero warnings. A rect-derived cell
  scrolls nothing and warns twice. The silent arm is the recommended one, which is why it outlived three
  passes.

## Discovery history

- **#639** — the renderer reached this first without naming it: "A buffer of no size is not a grant,
  it is the absence of an answer" (`webgl/surface.rs — apply_surface_size`), same `<= 0`, both-axes,
  `||` predicate. The reference-free ground the repairs rest on; found by a completeness pass looking
  for prior art inside the family.
- **#672** (closed 2026-07) — recorded in passing that "a hidden canvas gives `cellWidth === 0` …
  finite and wrong rather than ignored" while answering a `NaN` question. Found by searching the tracker
  by artifact, not feature name.
- **#801** (2026-08-25) — found while making a hidden terminal reachable; repaired the first site and
  enumerated the rest; falsified the #680 hypothesis.
- **#810** (2026-08-25) — repaired fit the day it was filed, found the grant read-back reader, and
  supplied the severity.
- **#814** (2026-08-25) — first site run rather than read; found self-concealment through `update()`.
  Its real ground is a sibling guard: `scroll-control.ts — routeWheel`'s `!Number.isFinite(lines)`
  refusal (#675) already decided a non-finite request must not reach `onScroll`, and
  `TerminalOptions.onScroll`'s doc says its producers funnel to one callback; the scrollbar was the one
  producer off that guard's path. #913 added a producer (user input scrolls to the constant `0`), so the
  claim now reads "the one producer off the guard that computes what it asks for".
- **#819** — the last site, the only one crossing a published seam, with state to unwind.

## Where it will recur

- **Any new reader of an element box.** Test at the signature: can this input distinguish "no box"
  from a legitimate measurement? If not, the repair belongs at the input.
- **Any host that hides a pane.** `display: none` is the ordinary way, `justerm-web`'s README
  documents it, and it fires a `ResizeObserver` — every observer of that element receives the zeros at
  once.
- **A third `box → grid` path**, which `fit.ts` predicts by name. The two existing ones agree and both
  refuse; a copy written from an older version would floor, and a test comparing them stays green.
