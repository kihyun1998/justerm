# Cross-cutting invariant — an absent element box measures as zero, and zero is in range

## The fact

**A DOM element with no box reports every field as `0` — and `0` is a legal value for everything
derived from it.** `getBoundingClientRect()` on a `display: none` element, on a detached node, or on
one not yet laid out returns all zeros rather than anything a reader can recognise as *absent*. So
"there is no box" and "the box is at the origin / is empty" arrive at every downstream site as the
same number, and no guard phrased on finiteness can separate them: zero is finite.

The sibling failure — a `NaN` box — is **not** this one and is usually already handled, which is what
makes this one hard to see. `NaN` propagates, announces itself and is refused; zero propagates,
computes cleanly, and produces a plausible answer that is wrong.

**The repair is always upstream of the derivation, and it is a different repair at every site**,
which is why this cannot live in a helper and has to live here:

| Site | Territory | Absent distinguishable from a legitimate `0`? |
|---|---|---|
| `justerm-web/src/terminal-surface.ts` — `viewportOrigin` | [multi-viewport](../territory/multi-viewport.md) | **Yes, since #801.** The overlay's *extent* is carried alongside its origin and the return is `{x,y} \| undefined`. Before that the clamp answered `{0,0}`, which re-placed a full-size grid on the canvas corner over a sibling |
| `justerm-web/src/fit.ts` — `proposeDimensions`, and `justerm-renderer.ts` `gridForBox`. **Two callers, one of which cannot reach a zero**: `resize` passes a consumer-measured box and is the live one; `applyGrid`'s grant read-back passes the drawing buffer, which `resize_surface` refuses to set at `<= 0` and which an empty read-back preserves rather than zeroes — so the guard is defensive there. Worth the row anyway: a completeness pass claimed that caller was a live third site, twice and with two different mechanisms, and both fell to reading `apply_surface_size` | [fit](../territory/fit.md) | **Yes, since #810.** Both refuse a box with no area, the way they already refused a non-finite one. Before that a `0x0` box floored to `MINIMUM_COLS`x`MINIMUM_ROWS` — measured against the real module, a `display: none` box proposed `2x1` while a `NaN` box was correctly refused — and that function's own comment already stated the intent it was missing: *"a non-finite box means 'not measured', exactly when we should NOT shrink the terminal"* |
| `justerm-web/src/input.ts` — `CellGeometry.originX` / `originY` | [selection](../territory/selection.md) | **It depends on the consumer, which is the part worth carrying.** These are the only two of the six fields with no stated precondition — a position legitimately may be `0` or negative — so `geometryViolations` cannot flag them. What decides exposure is how the *cell* was built: a **rect-derived** cell also goes to `0` when the box does, and that field *is* declared strictly positive, so the signal fires anyway (measured in #672, `demo/main.ts`'s `cellFromEvent`). A **renderer-derived** cell — `renderer.cellSize() / dpr`, which is what `demo/shared-surface.ts` does and what this package's README recommends — stays positive, so only the origin moves and nothing fires. The precondition mechanism itself is #672's, decided and closed as *"signal, do not correct"*; the axis here is the origin's, which that issue did not cover |
| `justerm-web/src/scrollbar.ts` — `dragTo`, via `dragTrackRatio` | [viewport](../territory/viewport.md) | **Yes, since #814.** The ratio step takes the track box as data and answers `number \| undefined`, refusing `height <= 0`; `dragTo` then makes no request at all. Before that, `(clientY - r.top) / r.height` on a zero-height track was `±Infinity`, which the surrounding `clamp` turned into a plausible end of the track — or `NaN` when the pointer was exactly at `r.top`. **This is the site where a finiteness test *nearly* worked, and the near-miss is the row's value.** On the **un-clamped quotient** it is equivalent up to a negative height (a mutation reddens exactly one assertion). On the value the function **returns** it accepts every zero-height box, because the clamp turns `Infinity` into a perfectly finite `1` — the measured slam to the live bottom. `justerm-web/src/terminal.ts`'s `wheelScrollTarget` had already recorded that general form: *"the clamp rescues an infinite request into a finite, wrong one … guarding there would fix half the cases and read as if it had fixed all of them"* |

## Why it is cross-cutting

**Four sites, four territories, and no shared call path, type or test.** Each one reads a box for its
own purpose — a viewport origin, a column count, a pointer basis, a scroll ratio — and each derives a
different quantity through different arithmetic. From inside any one of them the question reads as a
local numeric edge case, and the local answer is usually right for that site alone: the fit path's
floor exists so a small container still gets a usable terminal, the geometry's permissiveness exists
because a position may genuinely be zero or negative.

That is exactly the shape [`docs/map/README.md`](../README.md) exists for. The fact is *invisible from
each territory* while holding in all of them, and the count is doing work: the first two sites were
found by one change (#801) and the second pair only by asking, deliberately, which *other* readers of
an element box exist.

**It is a sibling of, and not an instance of, the product-ambiguity rule.**
[pointer coordinates are bounded by their producer](pointer-coordinates-are-bounded-by-their-producer.md)
already records *"when a derived value is ambiguous, the fix is usually upstream of the
multiplication"*, and `SelectionController.mouseMove` (#680) is that rule's site: its zero comes from
`getRows() * cellHeight`, a consumer callback times a **renderer-derived** cell, neither of which is a
DOM box — `cellHeight` stays positive when the overlay vanishes. Same generalisation (resolve
upstream), different cause (absence versus a lossy product), different noun (a box versus a
coordinate). Reading #680 as an instance of this invariant was the first hypothesis when #801 revealed
the fact, and it was wrong; the genuine second site is `fit.ts`.

## What a violation looks like

**A plausible wrong answer, no error, and the damage usually lands on a bystander.** Nothing throws,
because every value involved is finite and in range. The three measured shapes:

- **#801, measured in a real browser before the fix.** Hiding the second pane of
  `justerm-web/demo/shared-surface.html` with `display: none` moved its viewport from `[500, 40]` to
  `[0, 0]`; a full-size grid was drawn over its neighbour, and the neighbour went on reporting itself
  healthy. The renderer's own zero-area guard is not on that path and cannot be — a viewport's extent
  is derived from `cols * cell` and never from the measured box, so the zeroed box shrinks nothing.
- **The fit path**, measured against the real module. A hidden pane still being fitted proposes `2x1`,
  the engine reflows through two columns, and showing the pane again does not undo it.
- **A drag that outlives its box — measured in a real browser, #814.** Both `scrollbar.ts` and
  `SelectionController` bind their move and up listeners to `window` (`scrollbar.ts:124-125`;
  `demo/main.ts:1187`), and `Scrollbar.update()` hides the track without clearing `dragging`, so a
  drag in flight when the element is hidden keeps computing against zeros rather than ending.
  Driven through the real listener path with the guard off, host scrolled 40 lines up: two mouse
  moves against an all-zero rect produced **two spurious scroll requests** and left the host's
  display offset at **`NaN`**. With the guard, the same moves produce **nothing** and the offset is
  the one the last measured move set.
  **Which element is hidden decides which half of the harm you see**, and a probe that gets it
  wrong reports a milder defect than the one that exists. There are **two real routes**, and they
  are not interchangeable:
  - a host hides an **ancestor** (`display: none`, documented since #801). `scrollbackLen` is
    unchanged, so the ratio is `1` and the offset is driven to `0` — the *slam to the live bottom*,
    on top of the `NaN`;
  - the widget hides its **own** track, because `visible` means `scrollbackLen > 0` and core's
    `full_reset` (RIS, `ESC c` — `tput reset`, a program's `rs1`, a crashed TUI) replaces the whole
    `Term` and takes `scrollback_len()` to `0` (`justerm-core/src/term.rs`). **No host action at
    all.** Here `scrollbackLen` is `0` by construction, so the offset harm is absent — every legal
    offset is `0` — and only the `NaN` survives.

  What does **not** reproduce either is hiding the track while `scrollbackLen` stays positive: the
  first bad request re-renders the host, `update()` sets `display: "block"` again, and the `0/0`
  input never runs (measured: 2 requests, a plausible finite offset, no `NaN`). That is a **probe
  artifact**, not a route — this note said it was the rule for the length of one commit, on the
  strength of that one measurement, until the RIS path was found by asking what *else* empties a
  scrollback. `ED 3` had been checked and is unimplemented; `full_reset` had not.
  `SelectionController`'s half of this bullet is still **read, not run**.

## Territories it holds in

- [multi-viewport](../territory/multi-viewport.md) — where it was found, and the only site currently
  repaired. The repair is the *shape* worth copying: carry the extent so the reader can tell, and
  return a union so the compiler makes every caller decide
- [fit](../territory/fit.md) — the second site, **repaired in #810**, and the one whose own
  doc-comment had already asked for this check: *"When adding a third box→grid path, check **both**
  axes: the floor and the refusal."* A third path was added and the axis checked was neither. It is
  also the one place this invariant runs against the prior art: all three references floor a zero box
  and none refuses it, and the divergence is recorded in `docs/agents/theflow.md`
- [selection](../territory/selection.md) — `CellGeometry`'s two unconstrained fields
- [viewport](../territory/viewport.md) — the scrollbar's track ratio, **repaired in #814**, and the
  one site where a finiteness test *nearly* worked — on the quotient, not on the clamped result

## Discovery history

- **#672** (closed 2026-07) — reached this fact from the other direction without naming it. Its
  subject was `CellGeometry` having no *preconditions*, so a `NaN` poisons every pointer event, and
  its sweep recorded in passing that *"a hidden canvas gives `cellWidth === 0` there (the geometry is
  rect-derived) … finite and wrong rather than ignored"*. That sentence is this invariant, one field
  over, a month early — and it stayed a parenthesis because the question it was answering was about
  `NaN`, where the value announces itself. Found by searching the tracker by artifact rather than by
  feature name, which is the only reason this row exists
- **#639** (before either) — reached this fact in the renderer without naming it, and is the
  reference-free ground the repair actually rests on: *"A buffer of no size is not a grant, it is the
  absence of an answer"* (`justerm-renderer/src/webgl.rs`), with the same `<= 0`, both-axes, `||`
  predicate. Found only by a completeness pass looking for prior art **inside** the family after the
  argument had been built out of the references
- **#810** (2026-08-25) — repaired the second site the same day it was filed, and found a **third
  reader** of an absent box while doing it (the grant read-back above). Its value to this note
  is the **severity**, which the note did not have when it was written: the floor is not a cosmetic
  wrong answer. On the primary screen a reflow preserves logical lines, so re-widening restores the
  content; on the **alt screen** a resize is a re-fit — *"rows dropped or added to reach the new size,
  nothing re-wrapped"* ([reflow](../territory/reflow.md), #567) — so a pane hidden while running a
  full-screen TUI loses rows with nothing to restore them from — and the same resize clears the
  selection on **any** geometry change (`justerm-core/src/term.rs:1516`), whose comment had already
  reasoned about *"a consumer that re-asserts its size every frame (a `fit()` loop)"* and required the
  exact no-op; a `2x1` floor is not one. Absence producing a *plausible* answer is the invariant's
  subject; this is the case where the plausible answer is also irreversible
- **#814** (2026-08-25) — repaired the third site, and is the first of the four to be **run** rather
  than read. Two things it added that reading could not: the defect is **self-concealing** through the
  library's own `update()` when the wrong element is hidden (above), and the repair's real ground is
  not this note at all but a **sibling guard one module over** — `wheelAction`'s
  `!Number.isFinite(lines)` refusal (`justerm-web/src/terminal.ts`, #675) already decided that a
  non-finite scroll request must not reach the consumer's `onScroll`, while
  `TerminalOptions.onScroll`'s own doc says the scrollbar drag *"funnels to the SAME callback"*. The
  scrollbar was the one producer of that callback not on the guard's path — a statement that needs no
  reference in it, which is why it is the one the change rests on
- **#801** (2026-08-25) — found while making a hidden terminal reachable. The first site was repaired
  by widening `OverlayBoxes` and returning a union; the other three were enumerated by a completeness
  pass asking which other readers of an element box exist, and are **not** repaired. The first
  hypothesis — that `SelectionController.mouseMove` (#680) was the second site — was checked and
  falsified, which is why the boundary against the product-ambiguity rule is written above rather
  than left to be re-derived

## Where it will recur

- **Any new reader of an element box.** The tell is one question, and it is answerable at the
  signature: *can this input distinguish "no box" from a legitimate measurement?* If the answer is no,
  the repair belongs at the input, not at the computation
- **Any host that hides a pane.** `display: none` is the ordinary way, `justerm-web`'s README now
  documents it, and it fires a `ResizeObserver` — so every observer watching that element receives the
  zeros at once, and they do not all handle them the same way
- **A third `box → grid` path**, which `fit.ts` predicted by name. The two existing ones agree with
  each other and both floor, so a copy inherits the defect and a test comparing them stays green
