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

## Code

- `justerm-core/src/term.rs` — `Term::scroll_up`, `scroll_down`, `scroll_delta`, `scroll_to_bottom`,
  `Term::set_display_offset` (private), `Term::scrollback_len`, `Term::viewport_line`
- `justerm-core/src/serialize.rs` — `Frame`'s `display_offset` / `scrollback_len`

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md` — how the references split scroll ownership
between engine and frontend has never been compared against a pinned tree, although ADR-0013 argues
from that split.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — the
  scrollbar's `dragTo` divides by its track's measured height, so a zero-height track yields
  `±Infinity` that the surrounding clamp turns into a plausible jump to one end
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

- **No reference comparison** for the ownership split that ADR-0013 assumes.
- **The overscan band is described in `architecture.md` and implemented nowhere**, so it is a
  permitted consumer behaviour rather than a supported one — no test asserts the engine stays
  authoritative if a consumer builds it.
- **`scrollback_len` is exposed but its cap is not.** A consumer cannot tell whether history is being
  evicted, only how much currently exists.
