# Territory — frame (the snapshot)

## What it is

The one value the engine hands a consumer per damage cycle: `Term::frame()`. It holds only the
**viewport**, so anything not in it is — for a frame-mode consumer — not knowable at all. That makes
the question *"may this go in the frame?"* one of the most consequential in the repo, and it has its
own gate.

The *shape* is here; the bytes are [wire format](wire-format.md).

## Governing decisions

- [**ADR-0020 — what qualifies for the frame snapshot**](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — the gate. Three rules a candidate must pass: is it *state* or an *event*; does the consumer
  already hold it; is it bounded by the viewport
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  (v5→6) — the overlay group exists at all
- [ADR-0013 — expose scroll position](../../adr/0013-expose-scroll-position-in-frame.md) (v4→5) ·
  [ADR-0015 — marker primitive + overlay marker group](../../adr/0015-marker-primitive-and-overlay-marker-group.md)
  (v6→7) · [ADR-0016 — mouse wanted-events mask](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  (v7→8) — one record per group added

## Design model

- **Three layers, three different reasons to exist.** Header *scalars* · cell *content* (`spans` plus
  the link table; clusters inline since v14/#621) · `overlay`. ADR-0020's rules are how a candidate is sorted into
  one of them or refused.
- **Scalars ride the header because a consumer cannot derive them from cell damage** and they change
  nearly every frame: caret row/col/visible/shape/blink, `display_offset`, `scrollback_len`,
  `mouse_events`, `alt_screen`, `scroll`, and since v15 the marker-index basis (`evicted_total`,
  `marker_epoch` — #490). `alt_screen` is the clearest case — buffer-global state
  that viewport damage simply does not contain, which the a11y announce policy gates on.
- **The basis scalars are the header's first entry that exists to make something *leave* it.** Every
  other scalar describes the terminal; these two describe how long a consumer's separately-pulled
  answer stays valid, so that the two marker groups can stop riding every frame (ADR-0020 R3). They
  pass the gate on their own terms — `O(1)`, state rather than occurrence, and not derivable by a
  consumer that holds only this frame.
- **Four overlay groups since v16, and their *ownership* differs** — the part a reader gets wrong:

  | group | owned by | lifetime |
  |---|---|---|
  | `selection` | engine | cleared on a screen swap |
  | `matches` | **consumer** — handed back via `set_search_highlights`, engine only projects | invalidated on resize |
  | `active_match` | **consumer** designates; engine projects | voided with the set |
  | `markers` | engine | re-anchor through mutation, survive an alt excursion |

- **The absolute-line group left in v16 (#490).** It carried every live marker in every frame so the
  overview ruler could place off-screen anchors — the frame's largest payload, 37–70 % of an 80×24
  frame at ordinary OSC-133 densities, and ADR-0020's R3 violation. Off-screen anchors now come from a
  consumer-held index pulled once (`Engine::marker_index`) and kept current by the header basis plus
  the marker events; `marker_count` in the header is the check that it has not drifted. What remains
  here is viewport-relative and is what the a11y command announce consumes.
- **`active_match` is also present in `matches`.** The overlap is resolved by the renderer's highlight
  *ranking* rather than by excluding it here — an ordering decision pushed to where the pixels are.

## Code

- `justerm-core/src/serialize.rs` — `Frame`, `Overlay`, `Span`, `FrameKind`, `MarkerPosition`,
- `justerm-core/src/term.rs` — `Term::frame`, `Term::frame_damage`

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`. What a comparable engine hands its frontend
per cycle — and whether any of them has an equivalent qualification gate — has never been checked
against a pinned tree.

## Cross-cutting invariants

- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — on the JS
  side the payload is reached through accessors that rebuild a view (or, for the string tables, the
  whole array) on every read, so a reader walks cells from a local and never from `frame.`
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the span count is `u16`, and one span is emitted per damaged line, so it is bounded by
  `MAX_ROWS`. That is exactly `u16::MAX`: it fits with nothing to spare, which is a coincidence
  rather than a design and is worth re-checking if either number moves

## Blast radius

Adding a group is one of theflow's **unconditional** Step 5 triggers: it forces a
[wire format](wire-format.md) version bump, and registries are immutable.

- [wire format](wire-format.md) — every field here has to be encodable at a fixed stride
- [caret report](caret-report.md) · [viewport](viewport.md) · [damage](damage.md) — own header
  scalars
- [selection](selection.md) · [search & active match](search.md) — own overlay groups; the
  ownership column above decides who refreshes each after a resize
- [frame adapter](frame-adapter.md) · [cell compositing](cell-compositing.md) — consume every group, and resolve the `active_match` / `selection`
  ranking
- [accessibility](accessibility.md) — gates its announce policy on `alt_screen`, which exists for that reason

## Known holes / open

- **The ownership column is stated nowhere else** — only in `Overlay`'s field comments — and it is
  what decides responsibility for refreshing each group after a resize.
- **No reference comparison** for the snapshot model or for ADR-0020's qualification rules.
