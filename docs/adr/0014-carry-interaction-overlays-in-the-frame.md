# ADR-0014: Carry interaction overlays (selection + search-match spans) in the frame (wire v6)

Status: accepted (2026-06-29, #108) — bumps WIRE_VERSION 5 → 6.

## Context

S8 (#109, selection) and S9 (#110, search) paint **highlights** — the selected run, the search-match
runs. To draw them justerm-web needs the highlighted cell ranges as *viewport* spans. The engine already
computes exactly that shape:

- `Engine::selection_range() -> Vec<SelectionSpan>` (`lib.rs:283`) — the live selection projected onto
  visible rows, inclusive columns, off-screen rows dropped.
- `Engine::match_spans(&Match) -> Vec<SelectionSpan>` (`lib.rs:309`) — one match projected the same way.

But **neither crosses the wire.** `serialize.rs` carries cells, cursor, scroll op/position, and the
side/link tables — no selection or match reference. In frame mode (engine = backend native, web = decode)
the web therefore has no spans to highlight. xterm.js has no such gap because its `SelectionService` /
`DecorationService` query the model **in-process** (`SelectionService.ts`, `DecorationService.ts`, read at
source) — no serialization. The boundary is what forces a wire path here; this gap is frame-mode-specific.

### The ownership asymmetry (what the code forced)

`Engine::frame()` is `&self`, so it can only emit state the engine *holds*. The two highlight sources are
not symmetric:

- **Selection is engine-owned** — `selection_range()` reads `self`. `frame()` can project it directly.
- **Search matches are consumer-owned** — search.rs documents the split *"the engine finds, the consumer
  drives next/prev navigation, holding the `Vec<Match>`"* (mirroring Alacritty). The engine does not know
  which matches are the *active* highlight set, so `frame()` cannot project them on its own.

Resolving this is the crux: the consumer hands the active set back via a new
`Engine::set_search_highlights(Vec<Match>)`, which the engine holds (like the selection) so `frame()` can
project it. Without this, search highlights could never ride a `&self` frame.

## Decision

Add an **overlay section** to the wire after the link table, and bump `WIRE_VERSION` 5 → 6:

- The section is two groups, each a `u16` count then that many `(row, left, right)` `u16` viewport
  triples — the same triple the cell spans already use. Group 1 = selection spans, group 2 = search-match
  spans.
- `Frame.overlay: Overlay { selection, matches }` is the logical form; `frame()` populates it by
  projecting `selection_range()` and the held search highlights.
- `Engine::set_search_highlights(Vec<Match>)` holds the consumer's active highlight set (empty clears).
- The decoder exposes `selectionSpans` / `matchSpans` getters (flat `Uint32Array`, `OVERLAY_STRIDE = 3`),
  the same structure-of-arrays treatment the cell `spans` directory gets.

### Why the frame, and viewport coordinates

**First principles.** In frame mode the model is across an IPC boundary, so any state the consumer needs
must be *sent* — the same reasoning ADR-0013 used for scroll position. The overlay is per-frame viewport
state, not cell content; it rides the frame for the same reason the cursor and scroll position do.

**Viewport coordinates, re-projected by `frame()`.** Selection/match anchors live in *absolute* buffer
coordinates (`selection.rs:3`, `search.rs:3`), but the wire carries them as *viewport* spans, because
`frame()` already re-projects the cells against `display_offset` (`term.rs:448`). Projecting the overlay
in the same place keeps **one anchoring authority**: on every scroll the engine re-emits the spans at
their new viewport rows and drops what scrolled off-screen. Absolute coordinates can't cross alone — the
consumer does not know the scroll offset to resolve them.

**Theme-agnostic.** The overlay carries *positions only*; the highlight colour is the consumer's
(S12 / #115). The engine stays colour-blind, consistent with the cell colour-reference model.

**Named prior art.** xterm's `SelectionService` / `DecorationService` read viewport ranges from the live
model each frame; the frame-mode equivalent of that read is putting the projected spans on the wire. The
convergence (viewport spans, re-derived per frame) is the non-arbitrary shape.

## Consequences

- **WIRE_VERSION 5 → 6.** `encode` writes the two count-prefixed groups; `decode` reads them; the
  `justerm-wasm-decode` `DecodedFrame` gains `selectionSpans` / `matchSpans`; `wireVersion()` tracks the
  constant in lockstep (ADR-0008), so a stale binding fails the version gate. A v5 buffer is rejected
  (existing behaviour).
- **Live drag stays web-local.** During a drag the web paints the highlight locally for instant feedback;
  the frame's overlay span is the authority for *scroll re-anchoring* and copy. So the wire is not touched
  per mouse-move — only when a frame is already being sent.
- **Selection and search highlights have different lifecycles — by design, not by oversight.** The
  selection is *user-authored*: `Term` re-anchors it through cap eviction, region/RI scroll, reflow, and
  the alt-screen swap so it keeps pointing at its content (or clears when it cannot). Search highlights are
  *query-derived*: at those same points the absolute coordinates shift or the buffer is swapped (and the
  match set itself can change — new output adds matches, reflow re-wraps them), so the held `Vec<Match>`
  is no longer authoritative. The engine invalidates the highlights at exactly the set of points the
  selection already tracks (RIS resets them via full reconstruction; a soft reset leaves content untouched
  and so leaves them alone). The engine holds matches, not the query, so it
  cannot re-derive; it **invalidates** the highlights at those mutation points (`set_search_highlights`'s
  set is cleared) and the consumer re-searches on output (xterm/alacritty do the same). Pure scrollback
  scrolling (`display_offset`) leaves absolute coordinates valid, so it preserves the highlights — the
  "search, then scroll through the hits" path stays lit. Re-anchoring the matches like the selection would
  faithfully carry a now-incomplete, mis-shaped set: correct-looking but wrong.
- **Cheap and append-only.** Empty overlays cost 4 bytes (two zero counts). The section is append-only:
  a future group (decoration markers, #118) adds a third count at the next version bump without disturbing
  the layout.
- **Generalises the "viewport state on the frame" group.** Cursor, scroll op, scroll position, and now
  interaction overlays are all per-frame viewport state the consumer needs but isn't cell content.

## Alternatives considered

- **(B) A separate `encode_overlay` side channel.** Rejected — the overlay must be re-projected against
  the scroll offset on every scroll, which `frame()` already does for cells; a second transport would
  duplicate that projection and the ack-gated cadence, and any frame/overlay desync would mis-place
  highlights. The selection/cursor precedent already makes the frame the carrier for viewport state.
- **Absolute buffer coordinates on the wire.** Rejected — the consumer does not track the scroll offset,
  so it could not resolve absolute lines to viewport rows; the engine is the single anchoring authority
  and must project before sending.
- **Bundle search highlights into the selection group / infer them.** Rejected — search matches are
  consumer-owned and semantically distinct from the selection (a renderer may colour them differently);
  they need their own group and the explicit `set_search_highlights` hand-back.
- **Re-anchor stored matches through eviction/reflow like the selection.** Rejected — it conflates
  query-derived data with user-authored data. After the mutation the *correct* match set may differ (new
  matches, merged/split by reflow); re-anchoring old endpoints preserves a stale, incomplete set and a
  wrong match count. The engine invalidates and the consumer re-searches (the completeness authority,
  per the "engine finds, consumer navigates" split in search.rs).
- **Let the engine own the query and re-search every frame.** Rejected as an architecture change beyond
  this slice — it would duplicate the consumer's navigation `Vec<Match>` (two diverging sources), cost
  O(scrollback) per frame, and reverse the deliberate consumer-driven-search split. Could be revisited as
  its own ADR if dogfood shows the per-output re-search burden is too high.
- **Defer markers (#118) into this version.** Deferred, not rejected — #118 needs a core `Marker`
  primitive (a stable line handle) first; its viewport position becomes a third overlay group at a later
  version bump. The section is shaped to grow.
