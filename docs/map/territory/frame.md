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
  the grapheme and link side-tables) · `overlay`. ADR-0020's rules are how a candidate is sorted into
  one of them or refused.
- **Scalars ride the header because a consumer cannot derive them from cell damage** and they change
  nearly every frame: caret row/col/visible/shape/blink, `display_offset`, `scrollback_len`,
  `mouse_events`, `alt_screen`, `scroll`. `alt_screen` is the clearest case — buffer-global state
  that viewport damage simply does not contain, which the a11y announce policy gates on.
- **Five overlay groups, and their *ownership* differs** — the part a reader gets wrong:

  | group | owned by | lifetime |
  |---|---|---|
  | `selection` | engine | cleared on a screen swap |
  | `matches` | **consumer** — handed back via `set_search_highlights`, engine only projects | invalidated on resize |
  | `active_match` | **consumer** designates; engine projects | voided with the set |
  | `markers` | engine | re-anchor through mutation, survive an alt excursion |
  | `marker_lines` | engine | superset of `markers` by id, in a **different frame of reference** |

- **`marker_lines` is absolute, `markers` is viewport-relative.** Two groups for one concept,
  deliberately: the overview ruler needs anchors for lines that are off-screen, which a viewport-only
  group cannot supply.
- **`active_match` is also present in `matches`.** The overlap is resolved by the renderer's highlight
  *ranking* rather than by excluding it here — an ordering decision pushed to where the pixels are.

## Code

- `justerm-core/src/serialize.rs` — `Frame`, `Overlay`, `Span`, `FrameKind`, `MarkerPosition`,
  `MarkerLine`
- `justerm-core/src/term.rs` — `Term::frame`, `Term::frame_damage`

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`. What a comparable engine hands its frontend
per cycle — and whether any of them has an equivalent qualification gate — has never been checked
against a pinned tree.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

Adding a group is one of theflow's **unconditional** Step 5 triggers: it forces a
[wire format](wire-format.md) version bump, and registries are immutable.

- [wire format](wire-format.md) — every field here has to be encodable at a fixed stride
- [caret report](caret-report.md) · [viewport](viewport.md) · [damage](damage.md) — own header
  scalars
- [selection](selection.md) · [search & active match](search.md) — own overlay groups; the
  ownership column above decides who refreshes each after a resize
- **renderer** *(no note yet)* — consumes every group, and resolves the `active_match` / `selection`
  ranking
- **a11y** *(no note yet)* — gates its announce policy on `alt_screen`, which exists for that reason

## Known holes / open

- **The ownership column is stated nowhere else** — only in `Overlay`'s field comments — and it is
  what decides responsibility for refreshing each group after a resize.
- **No reference comparison** for the snapshot model or for ADR-0020's qualification rules.
