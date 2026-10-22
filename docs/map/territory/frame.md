# Territory — frame (the snapshot)

## What it is

The one value the engine hands a consumer per damage cycle: `Term::frame()` (reached through
`Engine::frame`). It holds only the **viewport**, so anything not in it is — for a frame-mode
consumer — not knowable at all. That makes *"may this go in the frame?"* one of the most
consequential questions in the repo, and it has its own gate (ADR-0020). The *shape* is here; the
bytes are [wire format](wire-format.md), and the JS-side reader is [WASM decoder](wasm-decoder.md).

## Governing decisions

- [**ADR-0020 — what qualifies for the frame snapshot**](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — the gate. A group must pass all three: **R1** state, not occurrence; **R2** not derivable by the
  consumer (derivability, not provenance — an answer and its viewport projection are different
  groups); **R3** `O(1)` or `O(viewport)`. Its amendments admit the v15 marker-index basis and
  record the event-channel obligation (#737, #741).
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  (v5→6) — the overlay group exists at all.
- [ADR-0013 — expose scroll position](../../adr/0013-expose-scroll-position-in-frame.md) (v4→5) ·
  [ADR-0015 — marker primitive + overlay marker group](../../adr/0015-marker-primitive-and-overlay-marker-group.md)
  (v6→7) · [ADR-0016 — mouse wanted-events mask](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  (v7→8) — one record per group added. v9–v18 were admitted with no record of their own; the
  version history lives in the comment on `VERSION` in `serialize.rs`.

## Design model

Source of these rules: ADR-0020 for the qualification gate; the rest from `Frame`/`Overlay`'s
doc-comments and `Term::frame`'s body (read at 7bb31f7).

- **Three layers, three reasons to exist.** Header *scalars* · cell *content* (`spans`, each with
  its sparse `combining` / `links` / `ucolors` maps, plus the frame-local `link_table`; clusters
  inline since v14, #621) · `overlay`. ADR-0020's rules sort a candidate into one of them or refuse
  it.
- **Scalars ride the header because a consumer cannot derive them from cell damage** and they change
  nearly every frame: caret row/col/visible/shape/blink, `display_offset`, `scrollback_len`,
  `mouse_events`, `alt_screen`, `scroll`, since v15 the marker-index basis (`evicted_total`,
  `marker_epoch` — #490), since v16 `marker_count`, and since v18 `modified_keys` (#941 — which
  modified C0-legacy keys the current keyboard modes deliver distinctly; see
  [input encoding](input-encoding.md)). `alt_screen` is the clearest case — buffer-global state that
  viewport damage does not contain, which the a11y announce policy gates on.
- **One `Term::frame` body samples every scalar**, so a frame is internally coherent by
  construction. Two derived values worth knowing: `cursor_visible` is forced false while
  `display_offset > 0` (the live caret is off the frozen viewport, #48), and `cursor_shape` is `None`
  until the application sets DECSCUSR (v17, #927). Cells are read at `display_offset` through
  `abs_row`, not from the live grid (#48), and a row's soft-wrap is *derived* back onto its last
  cell's `WRAPLINE` bit here, since the wire has no per-row slot ([soft wrap](soft-wrap.md)).
- **The basis scalars are the header's first entries that exist to make something *leave* it.**
  Every other scalar describes the terminal; `evicted_total`/`marker_epoch` describe how long a
  consumer's separately pulled marker index stays valid, so the absolute-line marker group could stop
  riding every frame (R3) — which it did in v16, where `marker_count` joined them as the drift check.
  All three pass the gate on their own terms: `O(1)`, state, and not derivable from this frame
  (ADR-0020 amendment, #490). `marker_count` cannot see a create and a dispose inside one frame, so it
  is a net, not the mechanism (`Frame::marker_count` doc).
- **Four overlay groups since v16, and their *ownership* differs** — the part a reader gets wrong:

  | group | owned by | dropped when |
  |---|---|---|
  | `selection` | engine (`selection_range`) | either alt swap; any geometry change on alt (#660); `Term::clear` |
  | `matches` | **consumer** — handed back via `set_search_highlights`, engine only projects (`match_spans`) | any mutation that shifts line coordinates: reflow/resize, eviction, region scrolls, both alt swaps (`invalidate_search_highlights`) |
  | `active_match` | **consumer** designates (`set_active_search_highlight` / `set_active_search_match`); engine projects | every new hand-over and the same invalidation as `matches` |
  | `markers` | engine (`marker_positions`, viewport rows only) | never by mutation — markers re-anchor and survive an alt excursion; death is `TermEvent::MarkerDisposed` |

- **The absolute-line group left in v16 (#490).** It carried every live marker in every frame so the
  overview ruler could place off-screen anchors — measured at 37–70 % of an 80×24 frame at ordinary
  OSC-133 densities (`VERSION` comment; ADR-0020 amendment). Off-screen anchors now come from a
  consumer-held index pulled once (`Engine::marker_index`), kept current by the header basis plus the
  marker events ([marker](marker.md)). What remains is the viewport group, which the a11y command
  announce consumes (`justerm-web/src/command-announce.ts` imports `readMarkers`). That remaining
  group is **not** `O(viewport)` either — several marks share a row and nothing dedups them (70 000
  records measured on 80×24, #721) — so ADR-0020 records two R3 violations, one now gone.
- **`active_match` is usually also present in `matches`.** The overlap is resolved by the renderer's
  highlight *ranking*, not by exclusion here (#424) — an ordering decision pushed to where the pixels
  are. "Usually": a span designation through `set_active_search_match` may sit outside a capped
  hand-over (#436), and then only the active colour paints.
- **Outside input.** A frame is built from engine state, so its size is bounded by the grid
  (`MAX_COLUMNS` × `MAX_ROWS`, both `u16::MAX`), the link table by distinct URIs referenced in the
  shipped spans, and the viewport marker group by `MAX_MARKERS` per buffer (#721).

## Code

- `justerm-core/src/serialize.rs` — `Frame`, `Overlay`, `Span`, `FrameKind`, `MarkerPosition`, `MarkerKind`
- `justerm-core/src/term.rs` — `Term::frame`, `Term::frame_damage`, `Term::scroll_delta`
- `justerm-core/src/term/markers.rs` — `Term::marker_positions`
- `justerm-core/src/term/search.rs` — `Term::match_spans`, `Term::set_search_highlights`, `Term::set_active_search_match`, `Term::invalidate_search_highlights`
- `justerm-core/src/term/selection.rs` — `Term::selection_range`
- `justerm-core/tests/overlay.rs` — the overlay groups end to end

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md` compares what a comparable engine hands its
frontend per cycle, or whether any has an equivalent qualification gate. ADR-0020's context cites
`docs/research/terminal-engine-renderer-architectures.md` §6.2 for the marker-line cost (no prior
art), which is a research essay, not a pinned row.

## Cross-cutting invariants

- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — this is the channel that gets the answer **free**: every scalar is sampled in one `Term::frame`
  body. That is exactly why it makes the other two channels (events, pulled answers) look solved
  when they are not.
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — on the JS
  side the payload is reached through accessors that rebuild a view (or, for the string tables, the
  whole array) on every read, so a reader walks cells from a local and never from `frame.`.
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the span count is `u16`, and one span is emitted per damaged line, so it is bounded by
  `MAX_ROWS`, which is exactly `u16::MAX`: it fits with nothing to spare, a coincidence rather than a
  design, worth re-checking if either number moves.

## Blast radius

Adding a group forces a [wire format](wire-format.md) version bump, and published registries are
immutable; the retired theflow discipline listed `serialize.rs` as an unconditional adversarial-pass
trigger (`docs/agents/theflow.md` § Unconditional triggers).

- [wire format](wire-format.md) — every field here must be encodable at a fixed stride and a width
  the value cannot outgrow.
- [WASM decoder](wasm-decoder.md) — exposes each field as a getter; a field it does not expose does
  not exist for a JS consumer.
- [caret report](caret-report.md) · [viewport](viewport.md) · [damage](damage.md) — own header
  scalars; a scalar sampled anywhere but this one body loses the frame's coherence.
- [selection](selection.md) · [search](search.md) · [active match](active-match.md) — own overlay
  groups; the ownership column decides who refreshes each after a mutation.
- [marker](marker.md) — the header basis and `marker_count` are the contract that keeps a pulled
  index valid; a change to when they move silently stales consumer decorations.
- [frame adapter](frame-adapter.md) · [cell compositing](cell-compositing.md) — consume every group
  and resolve the `active_match` / `selection` ranking.
- [accessibility](accessibility.md) — gates its announce policy on `alt_screen`
  (`accessibility.ts` returns early on `frame.altScreen`).

## Known holes / open

- **The ownership column is stated nowhere else in the code** — only in `Overlay`'s field comments
  and `invalidate_search_highlights`'s doc — and it decides who refreshes each group after a mutation.
  `Overlay::markers`'s comment still says search highlights are "invalidated on output", which is the
  in-place-erase half they are *not* invalidated by.
- **The viewport marker group is an R3 violation the gate still records** (ADR-0020, #721): its
  `u16` count is safe because `MAX_MARKERS` bounds the producer, not because the group is
  viewport-bounded.
- **No reference comparison** for the snapshot model or for ADR-0020's qualification rules.
