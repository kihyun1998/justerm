# Territory — caret drawing

## What it is

The renderer's half of the cursor: turning the scalars on the frame into rectangles, in device
pixels. The engine reports state and owns no pixels ([caret report](caret-report.md)); everything
about *how the caret looks* is decided here.

## Governing decisions

- [ADR-0028 — each surface a composition touches has one writer](../../adr/0028-composition-surfaces-have-one-writer-each.md)
  — **D5 decides this territory's column**: while a composition is open the caret's *position* is the
  composition's end, re-asserted on every frame (a frame carries the engine's cursor, which cannot know
  about a preedit). Position only — `DECTCEM` still decides whether a caret is drawn at all, which is
  the boundary #592 set and this record carries forward rather than reopens

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) — the caret is
  drawn as an **overlay**, so it sits outside the per-cell resolution rather than inverting a cell

## Design model

- **The caret is a native overlay, not a cell inversion.** The previous renderer had no cursor
  concept at all and left the caller to swap a cell's fg/bg; drawing it as its own geometry is why
  the engine only has to report scalars.
- **Geometry is device pixels and pure.** Stroke thickness is `(frac * cell_w).round().max(1)` —
  alacritty's rule, with its default fraction (`0.15`) as the starting value. The consumer may
  override per-renderer via `setCursorThickness` — **reachable through the widget since #580**, as a
  `JustermRendererOptions` field plus a runtime setter.
- **Shapes are rectangles, and a wide lead changes them.** A block caret over a width-2 glyph covers
  the pair, which is why the geometry module reaches for `is_wide_lead` — the caret is one of the few
  renderer concerns that has to know about pair structure.
- **Contrast is a separate knob** (`setCursorContrast`). A caret that inverts under a theme can
  become invisible, so its legibility is adjusted independently of the text contrast policy. It
  compares the **caret's own colour against the cell background**, which alacritty does not — it
  tests the cell's `fg` against its `bg`, because its caret can take a cell reference and justerm's
  never does. On the consumer side it rides `Theme` (#580), beside the colour it defends.
- **Blink phase arrives as state, not as a timer.** The consumer resolves the policy and pushes the
  phase; nothing here animates. That is the same split as the engine's, one layer further out.
  **The phase's origin is not the moment anything asked for it**, though: a cursor *move* re-anchors
  it too (the phase-only `CursorBlink.restart()`, which is application output, distinct from the
  input-path `restartFromInput`), so the origin a caller observes trails the call it made by however
  long the frame carrying that move takes to arrive. Measured at tens of ms and drifting with
  unrelated work on the composition path (#706, #707) — which is why anything timing the caret has to
  observe the phase rather than compute when it should have flipped.

## Code

- `justerm-renderer/src/cursor.rs` — the geometry: `THICKNESS`, stroke computation, the rect
  builders (pure, host-testable)
- `justerm-renderer/src/webgl.rs` — `set_cursor`, `clear_cursor`, `cursor_rects_js`,
  `set_cursor_contrast`, `set_cursor_thickness` — five of the crate's wasm exports
- `justerm-web/src/cursor.ts` — the consumer half that resolves the blink policy and owns the clock

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Cursor blink — who decides](../../agents/reference-facts.md#cursor-blink--who-decides-575-verified-2026-07-28)
  — the policy resolution this territory receives the result of
- [Cursor policy knobs — where each reference puts them](../../agents/reference-facts.md#cursor-policy-knobs--where-each-reference-puts-them-580-verified-2026-08-10)
  — the thickness and contrast constants, pinned, plus what each reference exposes. Its sharpest row
  is a *negative* one: alacritty's contrast guard is a compile-time constant with no config path, so
  the number this territory borrowed is prior art and the **knob** is not

## Cross-cutting invariants

- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — the caret stops blinking while composing (#592), and the only place that can know is a browser
  event handler: there is no VT sequence for a preedit and no frame field carrying one, so this rule
  cannot be derived from the five caret scalars this territory otherwise runs on. It arrived as a
  `setComposing` notification from the widget rather than as frame state, which is why it reads like an
  exception here and is a consequence of the invariant there.

This section said *"(none identified yet)"* until 2026-07-30, while #592 had already shipped exactly
such a behaviour into this territory. That silence is cited in the invariant note as the evidence for
its own existence — the fact held here and was invisible from here.

## Blast radius

- [caret report](caret-report.md) — the engine-side half; everything drawn here comes from those five
  scalars and nothing else
- [cell geometry](cell-geometry.md) — thickness and rects are expressed in cell dimensions, so a
  change to the cell/glyph box split moves the caret
- [wide glyph](wide-glyph.md) — a block caret over a wide lead must cover the pair
- [cell compositing](cell-compositing.md) — the caret deliberately does **not** participate; keeping
  it an overlay is what stops it becoming a compositing special case

## Known holes / open

- **Zero governing records** for the overlay-vs-inversion choice, which is the decision that lets the
  wire stay scalar-only.
- ~~**The alacritty default is unpinned**~~ — **closed by #580**, which pinned both borrowed
  constants (thickness `0.15`, contrast `1.5`) to `file:line` at the recorded SHA while wiring the
  consumer half. Kept struck through rather than deleted because the *shape* recurs: this territory
  borrows numbers from a named source, and a path in a doc comment reads like a citation without
  being one.
- **Nothing states what happens when the caret and a selection cover the same cell.** The highlight
  ranking answers selection versus active match; the caret sits outside that stack entirely and no
  document says how the two read together.
