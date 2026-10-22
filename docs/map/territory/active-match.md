# Territory — active match

## What it is

The **one** match the user is currently on, painted differently from the rest so "where am I in the
results" is visible. It is a *designation over* a [search](search.md) result set, not a search
concept: which member is active is navigation policy, and navigation is the consumer's. The engine
stores and projects it; the renderer ranks it above the selection; the web search controller decides
which match it is and keeps that decision on the same occurrence across a re-search.

## Governing decisions

- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) governs the overlap:
  rule 1 orders the highlight layer `ActiveMatch > Selection > Match`, rule 3 makes a match paint
  solid (replace), and rule 2 — layers declare `bg` and `ink` separately — derives #430 (an
  `ActiveMatch` declares no ink, so the selection's ink treatment passes through).
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  governs delivery (an overlay group), not the model.

Nothing records the designation model itself — the two setters, the lifetime, or the occurrence
anchor.

## Design model

From the source, ADR-0019, and the `SearchPort` doc-comments.

- **The consumer designates; the engine projects.** `Term::set_active_search_highlight(index)` takes an
  index into the set last handed over; `Term::set_active_search_match(m)` takes a `Match` — the
  past-cap path, for a backend that caps its hand-over and still wants the current match emphasised,
  as xterm creates its active decoration from the found result outside the capped list (#436). Both
  converge on one stored representation, the resolved span (`active_search_highlight: Option<Match>`).
  Past the cap the span paints the active colour alone, with no plain highlight beneath.
- **It rides its own overlay group *and* stays in `matches`** (`Overlay::active_match`, the fifth
  group, wire v12, #428). The overlap is resolved by ranking in the renderer (`Overlay::highlight_at`,
  `HighlightKind::ActiveMatch`), not by excluding it from the wider set.
- **Every path that voids the set voids this.** Because the stored span is not structurally tied to the
  set, `set_search_highlights` resets it on every hand-over (#428 — a stale index could be in range and
  light wrong content) and `invalidate_search_highlights` clears it in the same funnel (#436).
- **An index is how it is requested and not how it is remembered.** After every hand-over the consumer
  re-designates into a re-derived set, where the same ordinal can name different text — measured: at
  the scrollback cap the emphasis walked forward one occurrence per evicted line with no user input.
  What survives a re-search is the **occurrence**: the controller asks its backend
  (`SearchPort.anchoredIndex`, #437/#441), which answers the same occurrence in the new set, else the
  first at or after it (wrapping to 0), else `undefined`. The anchor is written when the emphasis is
  *placed* (`showMatch` / `designateMatch`), not sampled per hand-over — sampling was measured wrong in
  two orderings (a superseded debounced re-search; a `next()` during a round trip).
- **The backend keeps that anchor as a [tracked point](tracked-points.md)** (#691), because the engine
  renumbers absolute lines in the holder's absence. PenTerm's backend does exactly this:
  `SearchState::designate` → `anchor_at` (`Engine::track_point` at the match start, untracking the
  previous one), `anchored_index` resolves through `Engine::tracked_point`, and `clear` untracks
  (`penterm 7dadc3c95 src-tauri/src/engine/search.rs — SearchState`).
- **The anchor outlives the paint, and only one verb takes both** (#687). `SearchPort.clearHighlights`
  drops the match set and the designation while the session and its anchor continue; `SearchPort.clear`
  ends it. The highlight set is query-derived and invalidated, the anchor user-authored and carried.
  Conflating them put #441's viewport yank back on ordinary typing, because a regex is invalid for as
  long as it takes to type a group's closing paren. One deliberate divergence: alacritty's designation
  survives an invalid pattern, justerm's does not.
- **Designating a match paints it; it does not make its text available — a decision (#438).**
  `selection_text` is bound to the selection and there is no `Match` → text call, so *copy the current
  match* is consumer policy over two mechanisms core already exposes: `viewport_logical_lines`'
  per-char viewport map beside its wrap-joined text, and `match_spans` projecting a `Match` into the
  same space. The answer is viewport-scoped. A core getter was rejected on three counts: the
  `extract_lines` trim would end the text at the *range's* end, where a match's `' '` is written
  content by construction (the haystack pops the padding before matching, so
  [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md)'s structural excuse does not
  apply here); `match_spans`' clamp of a consumer-authored column would turn wrong paint into wrong
  **content**; and a copy is pulled on a user action, so in-place staleness the paint survives by
  re-search would reach the clipboard (#750's discriminator).

**Bounds on outside input.** One `Option<Match>`; its columns are consumer-authored and bounded at
projection by `match_spans` (#678, ADR-0026 D2); its line is not bounded at intake.

## Code

- `justerm-core/src/term/search.rs` — `Term::set_active_search_highlight`,
  `Term::set_active_search_match`, `Term::set_search_highlights` (the reset),
  `Term::invalidate_search_highlights` (the clear)
- `justerm-core/src/lib.rs` — `Engine::set_active_search_highlight`, `Engine::set_active_search_match`
- `justerm-core/src/serialize.rs` — `Overlay::active_match`
- `justerm-renderer/src/overlay.rs` — `HighlightKind::ActiveMatch`, `Overlay::highlight_at`,
  `HighlightColors`, `should_blend_kind`
- `justerm-renderer/src/webgl.rs` — `set_active_match`
- `justerm-web/src/justerm-renderer.ts` — `setActiveMatch`, `activeMatchBg`
- `justerm-web/src/search.ts` — `SearchController`, `SearchPort` (`showMatch`, `designateMatch`,
  `anchoredIndex`, `clearHighlights`, `clear`)

## Reference behaviour

- [What carries the current match across a re-search](../../agents/reference-facts.md#search-what-carries-the-current-match-across-a-re-search-437441-verified-2026-08-03)
  — 3–0 on keeping the occurrence, by three mechanisms (xterm: the selection it puts on each result;
  alacritty: a stored `Point`; ghostty: a tracked pin beside the index). None keeps a bare ordinal.
  While typing they split 2–1, and the outlier designates nothing.
- [Dropping the paint vs ending the session](../../agents/reference-facts.md#search-dropping-the-paint-vs-ending-the-session-687-verified-2026-08-03)
  — 3–0 that a query change may drop the paint without ending the session; alacritty answers
  justerm's exact invalid-pattern case.
- **The overlap is uncompared**: the per-channel answer where an active match meets a selection is
  derived from ADR-0019's model, never checked against a pinned tree.

## Cross-cutting invariants

**None.**

The active match is projected through `match_spans`, so the wide-pair rule reaches it through
[search](search.md)'s link to [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md),
not through a site of its own.

## Blast radius

- [search](search.md) — the set this designates over; every invalidation of that set voids this, so a
  new invalidation path that forgets the active span leaves it painting arbitrary text.
- [selection](selection.md) — the overlap partner; a change to either's rank or declared channels
  changes the other's appearance where they intersect.
- [cell compositing](cell-compositing.md) — where ADR-0019's ranking is actually resolved, per channel.
- [frame](frame.md) · [wire format](wire-format.md) — the fifth overlay group, added at wire v12.
- [tracked points](tracked-points.md) — the backend's anchor; its "`None` once the line left" answer is
  what `anchoredIndex` falls back from.
- [logical lines](logical-lines.md) — the mechanism a consumer composes to copy the current match.

## Known holes / open

- **No record for the designation model** — two setters, one stored span, the reset on hand-over, the
  occurrence anchor. The last lives in a port doc-comment on the consumer side.
- **The engine accepts any `Match`, including one authored before a resize**
  ([search](search.md) § Known holes). The anchor's drift is answered by `Engine::track_point`, wired in
  PenTerm's backend; the in-repo search backend is a browser-side fake (`justerm-web/demo/fake-search.ts`)
  that does not run the engine, so nothing in this repo exercises the wired path end to end.
- **No reference comparison** for the overlap model, the part most likely to look wrong to a user
  coming from another terminal.
