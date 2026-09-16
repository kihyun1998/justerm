# Territory — viewport

## What it is

Which window of the buffer the consumer is looking at, and who decides when it moves. The engine owns
the scroll position and resolves the consumer's scroll *intents* — the consumer never sets an offset
directly.

## Governing decisions

- [**ADR-0013 — expose scroll position in the frame**](../../adr/0013-expose-scroll-position-in-frame.md)
  — `display_offset` and `scrollback_len` ride the frame header so the consumer can draw a scrollbar
  (wire version 4 → 5)
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why the *position* is engine-side at all: "new output while scrolled up — follow or stay?" is bound
  to an output event, which only the engine sees
- `docs/architecture.md` §"Viewport / scrollback / scroll" holds the ownership statement

## Design model

- **Intents in, window out.** The consumer sends wheel / page / jump-to-bottom; `Term` resolves them
  against the buffer. `scroll_up` / `scroll_down` / `scroll_delta` / `scroll_to_bottom` are intents,
  not setters — `set_display_offset` is private.
- **`display_offset` counts lines scrolled *up* from the bottom.** `0` means following the live
  screen. Total addressable height is `scrollback_len + rows`.
- **Follow-bottom is engine state**, because the decision it drives ("output arrived while the user
  is scrolled up — move or stay?") is triggered by an event the consumer does not see.
- **A scroll that moves the viewport sets `full_damage`.** Moving the window changes everything the
  consumer can see, so the frame after a scroll is a full redraw rather than a translated set of
  spans — which is also why no scroll op has to be suppressed for a scrolled-up view.
- **The consumer may cache an overscan band** (viewport ± a screen) for instant small scrolls. That
  is a cache and not ownership; the engine stays authoritative.
- **Alt-screen is transparent here.** The engine emits whichever screen is current; the viewport does
  not know which it is looking at.
- **User input asks for the live edge, and the widget is what decides that** (#913). Typing while
  scrolled up funnels to the consumer's `onScroll` with `0`, alongside the wheel and the scrollbar
  drag. It is the terminal layer's call rather than the embedding app's because the state the
  decision needs is only here: whether the key survived the IME gate and `beforeKey`, and how far up
  the view actually is. The predicate is `scrollsToBottomOnInput`, kept pure and exported like
  `routeWheel`, and it takes **only** `displayOffset` — deliberately not `altScreen`, because core
  zeroes the offset on entering alt (`term.rs`, asserted by core's own
  `entering_alt_resets_scroll_position`), so an alt-screen branch here would be a second, weaker copy
  of an invariant the engine already holds. What counts as input is a key, committed IME text, a
  paste, and a keydown the IME gate swallowed — bare modifiers excluded on **both** key paths.
  - **The gate is a mirror, and three siblings write past it.** The predicate reads the widget's
    `displayOffset`, refreshed only by `track()` on each frame and by the two optimistic sites. The
    scrollbar drag, the selection drag auto-scroll and the accessible-view line nav all move the
    viewport through the consumer directly, so between one of those and the frame that echoes it the
    mirror reads stale and typing does not snap. One frame wide, unbounded if a consumer coalesces
    frames. Pre-existing — `routeWheel` reads the same mirror — and **not** introduced by #913, which
    merely made a second thing depend on it.
  - **Requesting is deduplicated by the widget, not by the consumer.** `requestBottom` sets its own
    tracked offset to `0` before calling out, because a frame-mode echo is an async round-trip and
    without it every keystroke in a burst is another round-trip. This is invisible to a consumer that
    echoes synchronously, which is why the demo needs `__deferScrollEcho` to make it observable at
    all — a test written against the demo's default timing passes whether or not the line exists.

## Code

- `justerm-core/src/term.rs` — `Term::scroll_up`, `scroll_down`, `scroll_delta`, `scroll_to_bottom`,
  `Term::set_display_offset` (private), `Term::scrollback_len`, `Term::viewport_line`
- `justerm-core/src/serialize.rs` — `Frame`'s `display_offset` / `scrollback_len`
- `justerm-web/src/terminal.ts` — `scrollsToBottomOnInput` (the input→bottom predicate),
  `InputScrollSignal`, and `Terminal`'s `scrollOnUserInput` / `requestBottom` wiring

## Reference behaviour

**One section** in
[`reference-facts.md` § scroll-on-user-input](../../agents/reference-facts.md#scroll-on-user-input--both-references-snap-and-xtermjs-does-it-at-two-sites-913-verified-2026-09-16)
(#913), which is also the
first time this territory was read against the pinned trees rather than argued from. It covers one
moment only: what the references do to the viewport when the user provides input. The ownership split
ADR-0013 assumes — who holds the scroll position at all — is still uncompared.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — the
  third site, **repaired in #814**. `dragTo` divided by its track's measured height, so a zero-height
  track yielded `±Infinity` that the surrounding clamp turned into a plausible jump to one end, or
  `NaN` at exactly the track's top. The ratio step is now `dragTrackRatio`, which takes the box as
  data and answers `undefined` for `height <= 0`; a refused move makes **no** `onScroll` call, and the
  drag deliberately survives so a pane shown again mid-gesture keeps following the pointer
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the viewport's window
  is expressed in the same concatenated coordinate space every buffer walk uses

## Blast radius

- [damage](damage.md) — while scrolled up, damage is empty by definition; a change to what counts as
  "moved" changes when a full redraw is issued
- [caret report](caret-report.md) — the caret is reported invisible whenever `display_offset > 0`
- [frame](frame.md) — two header scalars, added under ADR-0013
- [selection](selection.md) · [search & active match](search.md) — both project onto **viewport**
  rows, so the window decides what is emitted at all

## Known holes / open

- **No reference comparison** for the ownership split that ADR-0013 assumes. (#913 compared one
  *moment* — input — not the split.)
- **The input snap has no decision record**, only this note. It was routed to the terminal layer on
  two references converging plus a first-principles argument about which layer holds the state; the
  maintainer's calls inside it (that an IME-swallowed keydown counts, 2026-09-16) are recorded on the
  issue and nowhere else.
- ~~**A composition that begins while scrolled up draws its preedit at a stale cell.**~~ **Closed by
  #921 (2026-09-16), and the correction to the hole is the part worth keeping.** Traced from #913's
  lens, it needed a *third* condition nobody had stated: the cursor must have **moved** while the
  view was away. `cursor_row`/`cursor_col` are grid coordinates that do not move with
  `display_offset`, so with a stationary cursor the frozen cell is still the right cell — which is
  why "scroll up, then type Korean" is not on its own a reproduction, and why the repro has to move
  the cursor during the excursion for the reading to mean anything. The fix was not a scroll
  question at all: the widget was
  gating a *retained coordinate* on `cursorVisible`, a bit about **drawing**, so the retention moved
  to `Terminal.track` where every other frame-derived value already lives ungated. Adjacent to #917,
  which is untouched — a different root (two surfaces disagreeing at the latch, not a stale source).
  **Its follow-up found the sharper half**: the origin is a *grid* row and the renderer draws the
  *viewport*, so a run drawn during an excursion landed `display_offset` rows above its own cell
  (measured: drawn on row 33 while row 35 showed the cell) and, written once, kept that row while
  the view moved. Now mapped and re-asserted per frame — ADR-0028 D5's clause reaching the surface
  it always covered.
- **`scrollsToBottomOnInput` excludes bare modifiers, not "keys that write nothing".** `keyOf` maps
  every unrecognised DOM key name to a `char`, so `ContextMenu`, `Pause`, `BrowserBack`, `Copy` and
  the rest of the non-writing tail still snap. The widget cannot ask the real question — core owns
  encoding, so only it knows whether bytes result — and a denylist is the most it can honestly hold.
- **The overscan band is described in `architecture.md` and implemented nowhere**, so it is a
  permitted consumer behaviour rather than a supported one — no test asserts the engine stays
  authoritative if a consumer builds it.
- **`scrollback_len` is exposed but its cap is not.** A consumer cannot tell whether history is being
  evicted, only how much currently exists.
