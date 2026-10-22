# Territory — caret drawing

## What it is

The drawing half of the cursor. The engine reports the caret as scalars on the frame and owns no
pixels ([caret report](caret-report.md)); this territory turns those scalars into what is on screen.
In the renderer: the cursor uniform, the shape geometry in device pixels, the stroke thickness, the
contrast guard, the wide-pair rule, and the caret column during an IME composition. In the widget:
the blink — whether the caret blinks at all, its phase clock, its focus, composition, idle and
reduced-motion gates — and the redraw that pushes each phase to the renderer.

## Governing decisions

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) — the caret is
  composited last in the fragment stage (rule 6), outside the per-cell resolution the packer does.
- [ADR-0028 — each surface a composition touches has one writer](../../adr/0028-composition-surfaces-have-one-writer-each.md)
  — D5: while a composition is open the caret's *position* is the composition's end, re-asserted on
  every frame, because a frame carries the engine's cursor, which cannot know about a preedit.
  Position only — `DECTCEM` still decides whether a caret is drawn, the boundary #592 set.
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — makes a blink tick a
  no-output event the renderer must draw without a re-feed, which is why every shape is a uniform.
- [ADR-0017 — mechanism in core, policy in the consumer](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  — the contrast and thickness numbers and the blink policy are the consumer's; the contrast
  mechanism is the renderer's because only it has the resolved cell RGB.

## Design model

Status: the decisions above plus the source and its tests; the overlay-versus-inversion choice itself
has no record (Known holes).

**Drawing.**

- **The caret is a native overlay, not a cell inversion** (#270). The previous renderer had no cursor
  concept and left the caller to swap a cell's fg/bg; drawing it as its own geometry is why the engine
  reports only scalars.
- **Every shape lives in one uniform, not in the instance buffer** (#270). `u_cursor` is
  `(col, row, span, shape)`, shape `0` = no cursor and otherwise `shape_id + 1` (1 block, 2 underline,
  3 bar, 4 hollow block), with `u_cursor_color`, `u_cursor_text_color` and `u_cursor_thickness`
  beside it. Moving, blinking or reshaping costs one uniform and no upload. A block could have been an
  instance — it is a colour override, not geometry — and is not because ADR-0018 makes that the
  contract: a blink tick produces no terminal output, so a block packed into instances could not blink
  off without the consumer re-feeding the frame; an early draft did exactly that.
- **A block is a colour override on the cell**, and both references draw it that way: xterm's
  `RectangleRenderer.ts:251` emits no vertices and alacritty's `display/cursor.rs:33` no rects
  (unpinned here). In `FRAG_SRC` a block replaces the cell's background and every ink colour
  (`u_cursor_color` / `u_cursor_text_color`) *before* anything composites, so it lands after the
  packer's inverse swap and conceal — the order alacritty gets by overwriting `cell.fg`/`cell.bg`
  (`display/content.rs:167`). It is opaque even when the default background is translucent, and it
  replaces whatever background the packer composited, a selection's included. What it costs
  `I_neighbour` — a block is a background edge the packer cannot see — is in
  [cell compositing](cell-compositing.md) (`block_cursor_at`, #791, #966).
- **The strokes are hard-edged rectangles in device pixels, with two statements of one spec.**
  `cursor::cursor_rects` is the pure geometry; the shader's `stroke_coverage` mirrors it per fragment
  and `cursor_dx` mirrors `cursor::covers`. Antialiasing a pixel-aligned stroke would only blur it
  onto its own boundary. Both clamp a stroke to the box it outlines, and a bar's width by its own
  cell, not the cell's height. A `Block` yields no rect, and a stroke with no area is dropped rather
  than emitted degenerate — alacritty lets a too-short cell produce a negative `f32` height, which
  would wrap to `u32::MAX` here. `cursorRects` exposes the pure geometry to `demo/cursor.html`, which
  holds the shader to it: a drift between the two is the bug it catches. `covers` is
  `#[cfg(test)]` (#465): the real test runs in GLSL, and its `checked_sub` is where
  `col < c.col + span` would overflow at the far edge.
- **The stroke thickness is a clamped fraction of the cell** (`setCursorThickness`):
  `cursor_thickness` is `(frac.max(0.0) * cell_w).round().max(1)`, alacritty's rule and its default
  `THICKNESS = 0.15`, so it tracks dpr and font size — unlike xterm's `cursorWidth` in CSS px, which
  gives a 32-px font the same hairline as a 12-px one. The setter added configurability the mechanism
  already had. The `[0, 1]` clamp is load-bearing: an unclamped `f32::INFINITY` saturates the cast to
  `u32::MAX` device pixels. `NaN` passes the clamp and is caught a layer down — `frac.max(0.0)` returns
  `0.0` for it — so it gets the one-pixel floor, as `0` does. Reachable through the widget since #580
  (`JustermRendererOptions.cursorThickness` and a runtime setter).
- **Shapes are rectangles, and a wide pair changes them** (#454). The caret is a span and obeys the
  pair rule every span this crate paints obeys (`pair::partner_at`): on either half of a width-2 glyph
  it covers the whole glyph, and on the trailing spacer it moves back onto the lead — which is why
  `cursor_cells_at` returns an origin as well as a span (a width alone once forced "one cell"). It is
  re-resolved when a frame arrives (the flags may change under a still cursor) and when the cursor
  moves (`resolve_cursor_cells`). The references split three ways on the spacer, so this crate's own
  rule decides rather than a tally.
- **The contrast guard compares the caret's own colour against the cell's resolved background**
  (#368, `draw_grid`, `guarded_cursor_colors`). Below the threshold (default
  `DEFAULT_CURSOR_CONTRAST = 1.5`, clamped to `[1, 21]`, `1.0` disables it) the caret inverts to the
  default fg/bg. alacritty tests the cell's fg against its bg instead, because its caret can take a
  cell reference and justerm's never does. On the widget side the threshold rides `Theme`
  (`cursorContrast`, #580), beside the colour it defends.
- **The contrast guard reads the packed background**, looked up in the packed instances at
  `row * last_cols + col`, bounded only by `get()` and not by `col < last_cols`, so a cursor with
  `col >= last_cols` reads a *different* row's cell. Harmless because the shader paints the caret only
  on fragments of a real cell inside `[col, col + span)`, so a mis-guarded colour is never sampled —
  valid only while `cursor_dx` keeps that gate.
- **A block caret at a prompt with a suggestion shows the first suggested character** (#972). The
  suggestion is drawn from the cursor cell and the caret composites over it, so the block covers that
  character in the caret's text colour, as fish's in-band suggestion looks. Nothing here changed for
  it: the caret neither knows about nor moves for the suggestion.
- **During a composition the caret rides the preedit's end** (ADR-0028 D5). `setPreedit` returns
  `preedit::caret_col` — one past the run, except at the right edge, where there is no cell past it
  and the caret steps back onto the last glyph's lead so a block covers the pair. The widget retains
  it (`preeditCaret`) and `updateCursor` re-asserts it over every frame's engine cursor, because the
  engine never sees the composition ([input encoding](input-encoding.md) holds the column rule).

**The blink** (`justerm-web/src/cursor.ts`, `CursorBlink`).

- **Blink phase arrives as state, not as a timer.** The widget resolves the policy and pushes the
  phase with `setCursor` / `clearCursor`; nothing in the renderer animates — the engine/consumer split,
  one layer out. The blink loop that drives it is the widget's
  ([widget lifecycle](widget-lifecycle.md)).
- **Whether it blinks**: the consumer's three-state override, else the application's mode from the
  frame's `cursorBlink` (DECSCUSR and `?12`) — alacritty's placement
  (`blinking_override().unwrap_or(...)`). The default is solid, as both references and core default
  (#575). The override exists because the application's half is not authoritative: on a real PTY
  (RHEL 9.2, 2026-07-28) none of six programs emitted DECSCUSR while vim, htop and top emit `?12l` as
  part of terminfo's `cnorm`, so quitting vim would otherwise pin the caret steady for the session.
- **The gates, in `isVisible`**: not blinking, reduced motion, unfocused or composing → solid (solid
  means shown); idle past `BLINK_IDLE_TIMEOUT` (5 min, xterm.js's value rather than alacritty's 5 s,
  #593; `0` disables) → solid; otherwise the `BLINK_INTERVAL` (600 ms) phase. Reduced motion outranks
  both the application and the override because it can only subtract motion. Composing suppresses the
  blink and nothing else (#592), and is a `setComposing` notification because no frame carries it.
- **Two restarts: phase and idle are separate clocks.** A cursor *move* is application output and
  restarts the phase only (`restart`, in `updateCursor`); a keystroke or committed IME text restarts
  both (`restartFromInput`, via `rendererNotifyingSink` → `restartCursorBlink`, #107/#116), so a live
  TUI never idles out the blink. The input path, not the output, because the echo frame that would
  move the caret arrives a round-trip later. The same sink drives the renderer's focus from a focus
  intent. **The phase origin a caller observes trails its call** by however long the frame carrying
  the move takes — measured at tens of ms and drifting with unrelated work on the composition path
  (#706, #707) — so anything timing the caret must observe the phase rather than compute it.
- **Focus-in re-anchors the blink phase, because an unfocused caret is parked rather than stopped**
  (#912). The gate returns solid while blurred but the phase clock keeps running underneath, so a
  focus that did not re-anchor landed on whichever half of the 600 ms cycle it hit — half the time
  OFF, the caret vanishing as the user arrives. `setFocused` calls the phase-only `restart`, never the
  idle clock: focus is not typing. `CursorBlink` starts unfocused (#912): before that the renderer
  assumed focus, so a first focus was not a transition at all. The pointer path hid the defect
  (`onDown` restarts the blink right after focusing); Tab and the public `Terminal.focus()` — the
  documented way back after a dialog or the accessible view — did not. Reachable only where something
  asked the caret to blink, which PenTerm does from its view setting
  (`penterm 7dadc3c95 src/blocks/terminal/lib/nativeRenderer.ts — acquireNativeRenderer`).
- **The caret's redraw is also the overlay's present, and that is a trap rather than a design**
  (#912). `redrawCursor` is `pushCursor` + `backend.render()`, the only thing on the no-new-frame paths
  that draws; `issueOverlay` only retains spans and marks a re-pack. Every policy setter may skip its
  redraw when there is no caret — **except `setFocused`, which also moves the selection tint**, so it
  renders on a focus change even with no caret. Guarding it like its neighbours adopts the tint and
  never paints it, and with the caret hidden (`DECTCEM` off, where a full-screen TUI sits) there is no
  next present: `updateCursor`'s clear branch returns before `startBlinkLoop`. Caught by an adversarial
  pass, not a test; the tint had no coverage until #912 added `__unfocusedTintProbe`
  ([selection](selection.md) owns the tint).
- **The shape drawn** is `resolveCursorShape`: the application's DECSCUSR shape, else the consumer's
  `cursorStyle` (#927; the default's placement is [caret report](caret-report.md)'s).
- **Input bound.** The inputs are five scalars per frame plus the consumer's numbers: shape is
  rejected outside `0..=3` (`set_cursor`), thickness and contrast are clamped, and a cursor off the
  grid draws nothing and keeps the consumer's colours.
- **The preedit and suggestion runs are placed in viewport rows through `Terminal.frameOffset`**, the
  display offset of the last frame applied, via `viewportCell` (`terminal.ts`). That is where a
  composition meets scrolling (#921, #917); [viewport](viewport.md) holds why the mapping uses the
  frame's offset rather than the requested one.

## Code

- `justerm-renderer/src/cursor.rs` — `THICKNESS`, `cursor_thickness`, `cursor_cells_at`,
  `CursorShape`, `shape_id`, `shape_from_id`, `Cursor`, `DEFAULT_CURSOR_CONTRAST`,
  `guarded_cursor_colors`, `covers`, `Rect`, `cursor_rects` (pure, host-tested)
- `justerm-renderer/src/webgl/grid_state.rs` — `JustermRenderer::set_cursor`, `clear_cursor`,
  `set_cursor_contrast`, `set_cursor_thickness`, `set_preedit`
- `justerm-renderer/src/webgl.rs` — `cursor_rects_js` (`cursorRects`), `GridTier::set_cursor`,
  `GridTier::resolve_cursor_cells`, `GridTier::preedit_caret_col`
- `justerm-renderer/src/webgl/draw.rs` — `JustermRenderer::draw_grid` (the `u_cursor` uniform and the
  contrast guard)
- `justerm-renderer/src/shader.rs` — `FRAG_SRC`: `cursor_dx`, `block_cursor_at`, `stroke_coverage`
- `justerm-renderer/src/preedit.rs` — `caret_col`
- `justerm-web/src/cursor.ts` — `CursorBlink` (`isVisible`, `restart`, `restartFromInput`,
  `setFocused`, `setComposing`, `setReducedMotion`, `setBlinkOverride`, `setIdleTimeout`),
  `BLINK_INTERVAL`, `BLINK_IDLE_TIMEOUT`, `cursorCommand`, `resolveCursorShape`
- `justerm-web/src/justerm-renderer.ts` — `updateCursor`, `pushCursor`, `redrawCursor`,
  `restartCursorBlink`, `setFocused`, `setComposing`, `setPreedit`, `setCursorThickness`
- `justerm-web/src/terminal.ts` — `rendererNotifyingSink`

Proofs: `justerm-renderer/demo/cursor.html` (shader strokes against `cursorRects`);
`justerm-web/demo/shared-surface.ts`, whose unfocused-tint probe #912 added.

## Reference behaviour

- [Cursor blink — who decides](../../agents/reference-facts.md#cursor-blink--who-decides-575-verified-2026-07-28)
  — the application/consumer resolution `CursorBlink` follows.
- [The INITIAL focus state — who establishes it](../../agents/reference-facts.md#the-initial-focus-state--who-establishes-it-912-verified-2026-09-16)
  — where the focus flag starts, the gap #912 fell into; ghostty starts focused on purpose, so the
  shared rule is a correction path, not a value.
- [Idle timeout — both stop blinking, 60x apart](../../agents/reference-facts.md#idle-timeout--both-stop-blinking-60x-apart-593-verified-2026-07-28)
  — one rule, two thresholds; the reset set decided the 5-minute default.
- [Cursor policy knobs — where each reference puts them](../../agents/reference-facts.md#cursor-policy-knobs--where-each-reference-puts-them-580-verified-2026-08-10)
  — thickness `0.15` and contrast `1.5` pinned; alacritty's contrast guard is a compile-time
  constant, so the number is borrowed and the knob is not.
- [A span, a highlight and a caret over a wide pair](../../agents/reference-facts.md#a-span-a-highlight-and-a-caret-over-a-wide-pair-454-verified-2026-08-10)
  — the references split three ways on the spacer; this crate's pair rule decides.
- [Drawing the preedit](../../agents/reference-facts.md#drawing-the-preedit-two-draw-into-the-grid-one-draws-a-dom-box-249-verified-2026-08-03)
  — where the references put the caret during a composition (ghostty draws none).

## Cross-cutting invariants

- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — the caret stops blinking while composing (#592) and rides the preedit's end (ADR-0028 D5); only a
  browser event handler can know either, since no VT sequence or frame field carries a preedit. Both
  arrive as widget notifications (`setComposing`, `setPreedit`), not frame state. This section said
  "(none identified yet)" until 2026-07-30 while #592 had already shipped such a behaviour — the
  silence the invariant note cites as evidence for its own existence.
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — the caret is a
  span: `cursor_cells_at` draws a pair as two cells from its lead even when the engine's cursor sits on
  the spacer.
- [a screen change no frame carries owes its own present](../invariant/an-unframed-screen-change-owes-a-present.md)
  — `setFocused` presents with no caret when the tint moved, because `redrawCursor` is the only present on the no-new-frame paths (#912)

## Blast radius

- [caret report](caret-report.md) — everything drawn here comes from its scalars; a field added or
  reinterpreted there changes what `cursorCommand` and `updateCursor` assume.
- [widget lifecycle](widget-lifecycle.md) — the initial focus value and the blink loop are mount-time
  and loop questions answered there (#912); a caret blinking in a pane nobody clicked is this
  territory's symptom of a rule one layer out.
- [selection](selection.md) — the focus redraw is the selection tint's only present when no caret is
  drawn; a guard added here for the caret's sake drops the tint.
- [cell geometry](cell-geometry.md) — thickness and rects are fractions and multiples of the device
  cell, so a change to the cell/glyph-box split moves the caret.
- [wide glyph](wide-glyph.md) — the caret's span assumes the pair flags the frame carries; a pair
  representation change breaks `cursor_cells_at`.
- [cell compositing](cell-compositing.md) — the block recolours before the ink stack and is a
  background edge `I_neighbour` must see; a new layer must decide whether it sits above or below it.
- [GL context lifecycle](gl-context-lifecycle.md) — the four cursor uniforms belong to the program a
  loss destroys; a new one must be added to `Pipeline` so the restore carries it.

## Known holes / open

- **No record decides the overlay-versus-inversion choice**, the decision that lets the wire stay
  scalar-only.
- **Nothing states whether a block caret should hide a selection.** By reading `FRAG_SRC`, a block
  replaces the composited background, selection included; the highlight ranking answers selection
  versus active match, and no document says how the caret reads against either.
- **A constant borrowed with a path in a doc comment reads like a citation without being one.** The
  thickness default was exactly that until #580 pinned it; xterm's and alacritty's block-as-recolour
  sites above are still unpinned.
