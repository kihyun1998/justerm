# Territory — viewport

## What it is

Which window of the `[scrollback ++ grid]` buffer the consumer is looking at, who moves it, and how a
user's wheel, scrollbar drag and typing become a request to move it. The engine owns the position
(`display_offset`) and resolves scroll intents against the buffer; the web widget turns DOM events
into those intents (`scroll-control.ts`) and draws the scrollbar thumb from the two header scalars
(`scrollbar.ts`); the consumer carries the request across its transport.

## Governing decisions

- [ADR-0013 — expose scroll position in the frame](../../adr/0013-expose-scroll-position-in-frame.md)
  — `display_offset` and `scrollback_len` ride the frame header so a consumer can draw a scrollbar.
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why the position is engine-side at all: "new output while scrolled up — follow or stay?" is bound
  to an output event, which only the engine sees.
- [ADR-0016 — mouse-mode wanted-events mask in the frame](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  — the WHEEL bit the widget's wheel routing reads.
- `docs/architecture.md` §Viewport / scrollback / scroll holds the ownership statement.

## Design model

From the ADRs above, `docs/architecture.md`, and the source; the web half is held by this note and the
`scroll-control.ts` doc-comments only.

**The engine half.**
- **Relative intents in, window out.** `Engine::scroll_up` / `scroll_down` / `scroll_to_bottom`
  (and `scroll_to_match`, through [search](search.md)) are intents clamped to `[0, scrollback_len]`;
  `Term::set_display_offset` is `pub(super)`. There is **no absolute setter**, so a consumer whose UI
  speaks absolute offsets composes one: PenTerm's `scroll_to` is `scroll_to_bottom` then
  `scroll_up(n)` inside one `Session::edit` (`penterm 7dadc3c95 src-tauri/src/engine/native.rs —
  NativeEngine::scroll_to`).
- **`display_offset` counts lines scrolled up from the bottom.** `0` follows the live screen; the
  addressable height is `scrollback_len + rows`; the top visible line is
  `scrollback.len() - display_offset` (`Term::viewport_line`, `Term::viewport_to_abs`,
  `Term::viewport_link_at`). `viewport_to_abs` clamps both axes because a pointer past the grid is
  ordinary input (#660, #671, ADR-0026 D1).
- **Follow-bottom is "stay"** (`Term::linefeed_inner`): an accrual while scrolled up bumps the offset
  so the same lines stay in view; a cap eviction while scrolled up lowers it and forces a full repaint
  ([grid & scrollback](grid-and-scrollback.md)).
- **A move of the window sets full damage**, and while scrolled up content damage is not reported at
  all (`Term::damage`, `Term::frame_damage` answer an empty `Partial`) and `scroll_delta` is `None` —
  the frozen view must not be shifted by a content scroll ([damage](damage.md)).
- **The alt screen has no scrollback to view**: `set_display_offset` is a no-op there and entering or
  leaving alt zeroes the offset (`vt_compliance.rs` — `entering_alt_resets_scroll_position`).
- **The consumer may cache an overscan band** (viewport ± a screen) for instant small scrolls — a
  cache, not ownership (`docs/architecture.md`). Nothing implements it.

**The web half: three producers of a scroll request, all through `TerminalOptions.onScroll` or a
sibling callback.**
- **The wheel is routed in three pure steps, the way xterm.js routes it.** `WheelScroller` turns the
  event into whole lines, `routeWheel` picks the destination, `wheelScrollTarget` computes the offset
  for local scrollback.
  - **Whole lines first, for every destination and every `deltaMode`** (#908). Every mode carries its
    sub-line remainder to the next event, so the application never gets per-pixel wheel reports and a
    fractional sensitivity still emits whole lines; xterm.js returns LINE and PAGE amounts unrounded.
    A change of `deltaMode` (a different device) starts the remainder from zero. A LINE/PAGE notch
    carried below a whole line is consumed (`preventDefault`); a sub-line PIXEL delta is left to native
    scroll. xterm.js's wheel *report* goes through the same accumulator as its local scroll
    (`src/browser/services/MouseService.ts:146` @ `699f553`).
  - **Precedence: the application, then the alt screen, then scrollback.** An application tracking the
    wheel wins even on the alt screen; otherwise the alt buffer gets cursor keys (xterm.js's
    `_handlePassiveWheel`, `MouseService.ts:252-290`); otherwise the view scrolls locally.
  - **"Tracks the wheel" is per category** (#129): `wheelGoesToApp` tests the WHEEL bit of
    `mouseWantedEvents`, so an X10 application (`?9`, presses only) keeps the wheel local, as in
    xterm.js (`MouseStateService.ts:28-29`, `MouseService.ts:420-427`). A frame that omits the field
    routes locally.
  - **Each step is total** (#675): a non-finite amount returns `0` before the accumulator, which would
    otherwise keep `NaN` forever; `wheelScrollTarget` checks its inputs, not its result
    ([pointer coordinates are bounded by their producer](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)).
  - **A buffer switch drops the remainder** (`Terminal.track` calls `WheelScroller.reset`) — ours, not
    xterm.js's, whose `MouseService.reset()` runs only on a terminal reset
    (`src/browser/CoreBrowserTerminal.ts:1109` @ `699f553`).
- **Wheel sensitivity is the one option a mounted `Terminal` changes** (#959): `setScrollOptions`.
  The scroller is built in the constructor (one `readonly` instance for the widget's life, so no
  before-mount state); the setter is named per option group because no other option can change at
  runtime; a field left out keeps its value, and the carried remainder survives (xterm's
  `_wheelPartialScroll` is cleared only by `reset()`). That last call is the **maintainer's
  (2026-09-22)**, chosen over "a field left out resets to the constructor default".
- **The scrollbar is a DOM overlay the consumer mounts** (`Scrollbar`, #112): the GPU canvas has no
  native overflow bar, so like xterm.js's VS Code scrollable element the track is
  `position: absolute` and takes no layout width; the lane is the fit's subtraction
  (`ScrollbarOptions.width`, default 14 CSS px, matched by `JustermRendererOptions.scrollbarWidth`,
  #1029 — [fit](fit.md)). `scrollbarMetrics` mirrors xterm's `Viewport` (thumb = `rows / total`, top =
  `(scrollbackLen - displayOffset) / total`); the bar shows only when history exists, and a non-finite
  position hides it rather than writing `"NaN%"` (#463's lesson). A drag is absolute-position:
  `dragTrackRatio` then `dragToDisplayOffset`, each refusing what it cannot answer (#814), and a
  refused move makes no `onScroll` call while the drag stays alive, so a pane shown again mid-gesture
  resumes. The track carries `SCROLLBAR_ATTRIBUTE` so a `Terminal` around it does not route a press on
  it as one on the grid (#902); the thumb's colour is three inheritable custom properties, one per
  `ThumbState` (#926). The ruler marks on the same track are [decoration](decoration.md)'s.
- **User input asks for the live edge, and the widget decides that** (#913). `Terminal.onUserInput`
  wraps the consumer's sink (`scrollOnUserInput`), so it sees only what survived the IME gate and
  `beforeKey`, and fires before the intent is forwarded, as xterm.js fires its scroll request before
  `onData` (`src/common/services/CoreService.ts:82-94` @ `699f553`). The predicate
  `scrollsToBottomOnInput` reads only `displayOffset` — not `altScreen`, because core already zeroes
  the offset on alt entry and a second copy would be weaker — and `isUserInput` counts a key,
  committed IME text, a paste and an IME-swallowed keydown, bare modifiers excluded on both key paths.
  A non-finite offset snaps (the request is the constant `0`). `onWheel`'s application report and
  alt-screen cursor keys go straight to `TerminalOptions.input`, past the wrapper; unreachable for the
  snap since an alt screen is always at offset 0.
- **The widget's offset is a mirror, deduplicated optimistically.** `Terminal.displayOffset` is
  rewritten by `track()` on each frame and advanced ahead of the echo by the two optimistic sites (the
  wheel's scroll case and `onUserInput`), because a frame-mode echo is an async round-trip and a burst
  of keystrokes would otherwise each request one. A consumer that echoes synchronously cannot observe
  this, which is why the demo needs `__deferScrollEcho` to make it testable. The scrollbar drag, the
  selection drag auto-scroll and the accessible view's line nav move the viewport through their own
  callbacks without touching the mirror, so between one of those and its echo the snap and
  `routeWheel` read a stale offset — one frame wide, unbounded if a consumer coalesces frames.
- **What the screen shows is the last frame applied, not the requested scroll.** `Terminal.frameOffset`
  is the offset of the frame the renderer was given; anything mapping a grid coordinate onto the
  viewport uses it, not the optimistic mirror. `cursorRow`/`cursorCol` are grid coordinates that do
  not move with `display_offset` — the reason a preedit drawn during an excursion landed
  `display_offset` rows off its cell until it was mapped and re-asserted per frame (#921 and its
  follow-up; [caret drawing](caret-drawing.md) owns the run).

**Bounds on outside input.** Engine intents take a `usize` and clamp to `[0, scrollback_len]`. Wheel
deltas, sensitivities and drag coordinates are DOM numbers: non-finite values are refused at each
producer step; finite sensitivities are not validated (Known holes).

## Code

- `justerm-core/src/term/viewport.rs` — `Term::viewport_line`, `Term::scroll_up`, `Term::scroll_down`,
  `Term::scroll_to_bottom`, `Term::set_display_offset`, `Term::viewport_to_abs`,
  `Term::screen_link_at`, `Term::screen_underline_color_at`, `Term::viewport_link_at`
- `justerm-core/src/term.rs` — `Term::linefeed_inner` (follow-bottom), `Term::scrollback_len`
- `justerm-core/src/lib.rs` — `Engine::scroll_up`, `Engine::scroll_down`, `Engine::scroll_to_bottom`,
  `Engine::scrollback_len`
- `justerm-core/src/serialize.rs` — the header's `display_offset` / `scrollback_len`
- `justerm-web/src/scroll-control.ts` — `WheelScroller` (`consumeWheelEvent`, `setOptions`, `reset`),
  `wheelGoesToApp`, `wheelScrollTarget`, `routeWheel`, `WheelAction`, `scrollsToBottomOnInput`
- `justerm-web/src/scrollbar.ts` — `Scrollbar` (`update`, `dragTo`, `dispose`), `scrollbarMetrics`,
  `dragTrackRatio`, `dragToDisplayOffset`, `thumbState`, `thumbBackground`, `startsThumbDrag`,
  `dragStillHeld`, `SCROLLBAR_ATTRIBUTE`
- `justerm-web/src/terminal.ts` — `Terminal.track`, `Terminal.onWheel`, `Terminal.onUserInput`,
  `Terminal.scrollOnUserInput`, `Terminal.setScrollOptions`
- `justerm-web/src/input.ts` — `isUserInput`, `InputScrollSignal`

## Reference behaviour

- [Scroll-on-user-input — both references snap](../../agents/reference-facts.md#scroll-on-user-input--both-references-snap-and-xtermjs-does-it-at-two-sites-913-verified-2026-09-16)
  — the input snap (#913).
- [What a scrollbar drag reads on every move](../../agents/reference-facts.md#what-a-scrollbar-drag-reads-on-every-move--1-comparable-reference-and-it-reads-nothing-814-verified-2026-08-25)
  — VS Code's snapshot-plus-delta drag; ours stays absolute-position and goes inert while hidden.
- [A scrollbar lane — an overlay track](../../agents/reference-facts.md#a-scrollbar-lane--an-overlay-track-and-a-fit-that-leaves-its-width-free-1029-verified-2026-09-30)
  — overlay track, width 14, lane reserved by the fit.
- [Who guards a wheel accumulator](../../agents/reference-facts.md#who-guards-a-wheel-accumulator-675-verified-2026-07-31)
  — #675's totality.
- The wheel routing and the snap's ordering are cited inline against xterm.js at `699f553` (above),
  with no reference-facts row. The ownership split ADR-0013 assumes is uncompared.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — the
  scrollbar's track ratio, repaired in #814: `dragTrackRatio` takes the box as data and answers
  `undefined` for `height <= 0`, where the old division yielded `±Infinity` that the clamp turned into
  a plausible jump to one end.
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the window is expressed
  in the same concatenated coordinate space every buffer walk uses.
- [an IME composition is browser-owned state](../invariant/composition-is-browser-owned-state.md) — a
  preedit is drawn from a coordinate the engine gave for something else (the cursor's **grid** row), so
  the grid → viewport mapping through `frameOffset` is where a composition meets scrolling (#921, #917).
- [pointer coordinates are bounded by their producer](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — `wheelScrollTarget` and `dragToDisplayOffset` guard their inputs because a clamp rescues an
  infinite request into a finite, wrong one.

## Blast radius

- [damage](damage.md) — while scrolled up content damage is suppressed by definition, so a change to
  what counts as "the view moved" changes when a full redraw is issued.
- [caret report](caret-report.md) — the caret is reported invisible whenever `display_offset > 0`.
- [frame](frame.md) · [wire format](wire-format.md) — two header scalars, added under ADR-0013.
- [selection](selection.md) · [search](search.md) · [hyperlinks](hyperlinks.md) — each projects onto
  viewport rows via `scrollback.len() - display_offset`, which underflows if the offset outlives its
  history.
- [caret drawing](caret-drawing.md) — the preedit and suggestion runs map grid coordinates onto the
  viewport through `frameOffset`, which assumes the renderer shows the last frame applied.
- [input encoding](input-encoding.md) — the alt-screen wheel becomes cursor keys whose DECCKM form core
  decides; the web only picks the direction.
- [fit](fit.md) — the scrollbar lane's width is subtracted by the fit and must match the scrollbar's.
- [decoration](decoration.md) — ruler marks live on the scrollbar's track and assume its geometry.
- [accessibility](accessibility.md) — its line nav is one of the three requests that bypass the mirror.

## Known holes / open

- **No reference comparison** for the ownership split ADR-0013 assumes (#913 compared one moment —
  input — not the split).
- **The input snap has no decision record**, only this note; the maintainer's call that an
  IME-swallowed keydown counts (2026-09-16) is recorded on #913.
- **The mirror is stale across three producers** (scrollbar drag, selection auto-scroll, accessible
  line nav), so typing right after one of them may not snap. Pre-existing; #913 made a second reader
  depend on it.
- **`scrollsToBottomOnInput` excludes bare modifiers, not "keys that write nothing".** `keyOf` maps
  every unrecognised DOM key name to a `char`, so `ContextMenu`, `Pause`, `BrowserBack` and the rest of
  the non-writing tail still snap. Only core knows whether bytes result.
- **The overscan band is described in `architecture.md` and implemented nowhere**; no test asserts the
  engine stays authoritative if a consumer builds one.
- **Wheel sensitivities are not validated**, through the constructor or `setScrollOptions`. `0`
  scrolls nothing while LINE/PAGE notches are still consumed; a negative value inverts the direction;
  `NaN` makes the wheel go dead rather than latch (#675's guard). xterm.js rejects `<= 0` with a throw.
  #959 widened the exposure (a settings field is a likely source of `parseFloat("")`); throw, clamp or
  keep is an unmade policy call.
- **No absolute offset setter in core**, while the web port speaks absolute offsets; each consumer
  composes one from two intents (PenTerm does).
- **`scrollback_len` is exposed but its cap is not** ([grid & scrollback](grid-and-scrollback.md)).
