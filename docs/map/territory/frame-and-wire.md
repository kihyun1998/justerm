# Territory — frame & wire

## What it is

The one snapshot the engine hands a consumer, and the byte format it travels in. `Term::frame()`
builds it; `encode` / `decode` round-trip it. Everything a frame-mode consumer knows about the
terminal arrives here — it holds only the viewport, so anything not in the frame is, for that
consumer, not knowable.

The map's most-referenced unwritten area until now: six other notes point at it, because almost every
territory eventually has to answer *"and how does this reach the consumer?"*

## Governing decisions

The most heavily recorded territory in the map — the contrast case against
[cursor](cursor.md) and [selection](selection.md), which have none.

- [**ADR-0005 — binary reference-based serialization**](../../adr/0005-binary-reference-based-serialization.md)
  — the format itself: little-endian, fixed-width 18-byte cell records, colour *references* not RGB
- [**ADR-0020 — what qualifies for the frame snapshot**](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — the **gate**. Three rules a candidate must pass before a wire group is added
- [ADR-0013 — expose scroll position](../../adr/0013-expose-scroll-position-in-frame.md) (v4→5) ·
  [ADR-0014 — carry interaction overlays](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  (v5→6) · [ADR-0015 — marker primitive + overlay marker group](../../adr/0015-marker-primitive-and-overlay-marker-group.md)
  (v6→7) · [ADR-0016 — mouse wanted-events mask](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  (v7→8) — one record per version bump
- [ADR-0008 — wasm-decode as a separate crate](../../adr/0008-wasm-decode-binding-separate-crate.md)
  — who decodes it on the other side
- `docs/architecture.md` §Serialization holds the field-by-field byte arithmetic

## Design model

- **Three layers in one struct.** Header *scalars* (cursor row/col/visible/shape/blink,
  `display_offset`, `scrollback_len`, `mouse_events`, `alt_screen`, `scroll`) · cell *content*
  (`spans` + `side_table` for grapheme clusters + `link_table` for OSC 8 URIs) · `overlay`.
  Each layer has a different reason to exist, and ADR-0020's rules are how a candidate is sorted
  into one or rejected.
- **Scalars ride the header because they change nearly every frame** and a consumer cannot derive
  them from cell damage. `alt_screen` is the clearest case: buffer-global state that viewport damage
  simply does not contain, which the a11y announce policy gates on.
- **Five overlay groups**, and their *ownership* differs, which is the part a reader gets wrong:

  | group | owned by | lifetime |
  |---|---|---|
  | `selection` | engine | cleared on a screen swap |
  | `matches` | **consumer** — handed back via `set_search_highlights`, engine only projects | invalidated on resize |
  | `active_match` | **consumer** designates; engine projects | voided with the set |
  | `markers` | engine | re-anchor through mutation, survive an alt excursion |
  | `marker_lines` | engine | superset of `markers` by id, in a **different frame of reference** |

- **`marker_lines` is absolute, `markers` is viewport-relative.** The overview ruler needs anchors
  for lines that are off-screen, which a viewport-only group cannot supply. Two groups for one
  concept, deliberately.
- **`active_match` is also present in `matches`.** The overlap is resolved by the renderer's
  highlight *ranking*, not by excluding it here — an ordering decision pushed to where the pixels
  are.
- **Grapheme clusters and hyperlinks are side-tables with frame-local indices**, which is what keeps
  the cell record fixed-width at `CELL_RECORD_LEN = 18`. Fixed stride is the whole point: the
  consumer takes one contiguous typed-array view with no per-field parse.
- **The engine provides the format and *both directions*; transport is the consumer's.** `encode`
  and `decode` both live here so the round-trip is testable without a consumer.

## Code

- `justerm-core/src/serialize.rs` — `WIRE_VERSION`, `Frame`, `Overlay`, `Span`, `FrameKind`,
  `MarkerId` / `MarkerKind` / `MarkerPosition` / `MarkerLine`, `DecodeError`, `encode`, `decode`,
  `encode_cell_record`, `encode_color`, `CELL_RECORD_LEN`
- `justerm-core/src/term.rs` — `Term::frame`, `Term::frame_damage`
- `justerm-wasm-decode/` — the JS-side decoder (ADR-0008), its own crate on the same version lockstep
- `justerm-web/src/types.ts` — `DecodedFrame`, a **hand-written mirror** of the wasm getters

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`. ADR-0005 reasons about Mosh's protobuf
baseline-diff and xterm.js's escape-sequence re-emit, but as argument rather than as pinned rows —
so the comparison that decided this format has never been re-checked against the sources.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the grapheme and link side-tables are
  the wire's version of the same escape hatch, and the cell's presence bits gate both ends

## Blast radius

Adding or changing a wire group is the highest-consequence change in the repo — one of theflow's
**unconditional** Step 5 triggers, because registries are immutable and a consumer decoding a wrong
layout gets garbage cells, not an error.

- [release & published surface](release-and-published-surface.md) — a `WIRE_VERSION` bump ships on
  `v*`, which fires **two** publishes at once (crates.io + npm)
- [selection](selection.md) · [search & active match](search.md) · [cursor](cursor.md) ·
  [damage & viewport](damage-and-viewport.md) — each owns state that rides here; a change to what
  the frame carries changes what those territories are allowed to keep private
- **renderer** *(no note yet)* — the consumer of every group, and where the `active_match` /
  `selection` ranking is resolved
- **a11y** *(no note yet)* — gates its announce policy on `alt_screen`, a header scalar that exists
  for that reason
- `justerm-web/src/types.ts` — a hand-written mirror, so a new getter is `undefined` in TypeScript
  until someone adds it there **and** the wasm package is republished

## Known holes / open

- **`types.ts` is a hand-maintained copy of the wasm surface**, with nothing gating the two against
  each other. A field can exist in core, ship in the wasm binding, and be invisible to the web widget
  with no error anywhere.
- **No reference comparison** (§Reference behaviour) for a format whose alternatives were argued
  from prior art.
- **The ownership column above is not stated anywhere else.** That `matches` is consumer-owned while
  `selection` is engine-owned is documented only in `Overlay`'s field comments, and it decides who is
  responsible for refreshing each after a resize.
