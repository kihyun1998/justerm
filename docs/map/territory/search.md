# Territory — search

## What it is

Finding every occurrence of a query across screen **and** scrollback, holding the highlight set the
consumer hands back so it rides the frame, and the web controller that drives a search box against a
backend. Which match is *current* is a separate concept — [active match](active-match.md).

The split of labour is deliberate: **the engine finds, the consumer navigates.** The consumer's
backend holds the `Vec<Match>`, caps what it highlights and drives next/prev; the engine scrolls to a
match it is handed (`src/search.rs` module doc, alacritty's "engine finds, frontend navigates").

## Governing decisions

- [ADR-0026 — a coordinate that arrives from outside is bounded once](../../adr/0026-outside-coordinates-are-bounded-once.md)
  governs **one axis only**: a `Match`'s columns are authored by the consumer, so the bound sits at the
  projection (`match_spans`) rather than at a producer the engine does not have.
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why search is in core (it needs the whole buffer, which a frame-mode consumer lacks) and why the
  query policy, cap and navigation are not. Routing only.
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  — the highlights ride the frame as an overlay group. Delivery only.

Nothing decides the match model, the smart-case rule, or the regex dialect.

## Design model

From the source and the doc-comments on `search.rs`, `term/search.rs` and `justerm-web/src/search.ts`.

**The engine half.**
- **A `Match` is inclusive on both ends, in absolute buffer coordinates** — the `[scrollback ++ screen]`
  line index the selection uses. `Term::match_spans` converts one into viewport `SelectionSpan`s,
  bounding both columns at the row's extent (the grid width, #678) and widening onto whole wide pairs
  (#454), because a consumer may author a `Match` and the clamp can land on a trailing spacer.
- **Two entry points.** `search(query)` is literal + smart-case; `search_with(query, opts)` takes
  `SearchOptions { regex, whole_word, case_sensitive }`, whose default is exactly `search`.
- **The haystack is the logical line**, built from `abs_floor` (#144) by joining soft-wrapped rows,
  skipping spacers, including each cell's combining marks and mode-2027 cluster scalars (#304), and
  popping trailing U+0020 padding only (#685) — so a regex `$` anchors at the text's end and a written
  U+00A0 stays findable. Literal matches are non-overlapping; regex empty matches are skipped;
  consecutive identical matches from one cluster are collapsed.
- **Smart-case reads the *raw* pattern** — case-insensitive iff the query has no uppercase, so in regex
  mode an uppercase metacharacter (`\B`, `\D`, `\x1B`) flips it; set `case_sensitive` or use `(?i)`.
  Folding takes the first char of a multi-char case expansion so the haystack stays 1:1 with positions.
- **Whole-word is the `\b` sense, not the selection separator set** (`word_bounded`): alphanumeric,
  `_` or a zero-width mark. #545 made the selection set injectable and deliberately left this one
  alone; xterm.js keeps a separate hardcoded set and alacritty has no whole-word option.
- **The dialect is the `regex` crate, not JS `RegExp`** — no lookaround or backreferences,
  Unicode-aware `\w \d \b`. An invalid pattern yields an **empty result**, not an error (#314); a
  consumer validates with `is_valid_regex` (wasm `isValidRegex`), which compiles case-*insensitively*
  on purpose because Unicode case folding grows the program, so a `true` holds under whichever case
  mode the search picks.
- **The run is not bounded, and that is measured to cost nothing**: 14.1 ms vs 13.9 ms between one
  giant soft-wrap run and the same bytes as short lines, because it scans everything either way
  (`Term::search` doc-comment); xterm's search walk is uncapped too.
- **Highlights are pushed, not pulled.** `set_search_highlights(matches)` puts state into the engine so
  it rides the frame, and resets the active designation. **The engine does not remember a query.**
- **Query-derived state is invalidated; user-authored state is re-anchored.**
  `Term::invalidate_search_highlights` drops the set and the active match wherever a mutation shifts
  line coordinates — cap eviction / `ED 3` / `clear` (through `lines_left_the_front`), region / RI /
  SU / SD / IL / DL scrolls, the top-anchored sub-region accrual (#449), resize, both alt swaps, and
  `Term::clear` — nine call sites. The selection beside it is re-anchored where it can be and cleared
  where it cannot, so the rule is about what the consumer can **reproduce**, not about re-anchoring
  being available.
- **In-place destruction does not invalidate — the consumer heals it.** ICH/DCH, insert-mode print,
  ED/EL/ECH and overwrite leave the set stale in place, because they are output and the web
  controller re-runs the query after a debounce on output. That healing is the whole argument, and is
  why the sibling defect on markers had to be fixed in the engine instead (#750: `command_lines` is
  pulled on a user action, so nothing re-derives it). The comparison to xterm.js's decorations is half
  true: ED there *does* retire search highlights (a decoration dies with its marker); EL, ECH and
  overwrite do not.

**The web half** (`SearchController` over a `SearchPort`).
- **Navigation is by index; matches never cross the wire** — only their viewport spans do. The backend
  owns the `Vec<Match>`, caps the highlight set (PenTerm's backend: 1000, xterm's `highlightLimit` —
  `penterm 7dadc3c95 src-tauri/src/engine/search.rs — SearchState`, `HIGHLIGHT_CAP`) and returns the full count; `showMatch` designates and scrolls, `designateMatch`
  designates without scrolling. Neither selects: the selection channel stays the user's (#429).
- **Re-search on output**: `SearchController.onFrame` re-runs the active query after `DEBOUNCE_MS`
  (200 ms, xterm parity), re-designates on the same **occurrence** via `SearchPort.anchoredIndex`
  ([active match](active-match.md)) without scrolling, and reports through `onResults`. An `epoch`
  counter discards a superseded round-trip so a slow response cannot resurrect a cleared search (#429).
- **Invalid regex**: with an injected `isValidRegex` validator, a failing regex-mode query sets
  `isInvalidRegex()` and drops the previous paint through `SearchPort.clearHighlights` (#687), not
  `clear`, so the anchor survives the invalid intermediate states of typing a pattern.
- **Two consumer-owned a11y seams**: `SearchResult` is the announce seam (#439, the twin of xterm's
  `onDidChangeResults`) and `isInvalidRegex` the attribute seam (#448: set `aria-invalid`, ungated);
  both duties live in published doc-comments.
- **The overview ruler gets every match's line through the port**, not the frame
  (`SearchPort.matchLines`, #440, decided 2026-07-21): the backend already holds the matches in
  absolute coordinates, so no wire group was added. Hand over the pre-cap set. These lines go stale
  silently until the next hand-over — core moves no dating scalar when it drops the set.

**Bounds on outside input.** The query and `SearchOptions` come from the consumer. The query length is
unbounded; a regex is bounded by the `regex` crate's default size limit (`CompiledTooBig` → empty
result). The result `Vec<Match>` is unbounded — a one-character query returns one match per
occurrence across `scrollback_limit + rows` lines; the highlight set handed back is whatever the
consumer passes (the cap is consumer policy), and its wire count is `u32` since #621.

## Code

- `justerm-core/src/search.rs` — `Match`, `SearchOptions`, `is_valid_regex`
- `justerm-core/src/term/search.rs` — `Term::search`, `Term::search_with`, `Term::search_scroll_to`,
  `Term::match_spans`, `Term::set_search_highlights`, `Term::invalidate_search_highlights`,
  `word_bounded` (the active-match setters are [active match](active-match.md)'s). Two files are named
  `search.rs`, so a bare `search.rs:NN` citation is ambiguous.
- `justerm-core/src/term/walk.rs` — `Term::abs_floor`, `Term::abs_line`, `Term::abs_row`,
  `Term::combining_at`
- `justerm-core/src/lib.rs` — `Engine::search`, `Engine::search_with`, `Engine::scroll_to_match`,
  `Engine::set_search_highlights`
- `justerm-wasm-decode/src/lib.rs` — `is_valid_regex` (`isValidRegex`)
- `justerm-web/src/search.ts` — `SearchController` (`search`, `onFrame`, `reSearch`, `next`, `prev`,
  `clear`, `isInvalidRegex`, `rulerLines`, `result`), `SearchPort`, `SearchOptions`, `SearchResult`,
  `StubSearchPort`, `DEBOUNCE_MS`
- Tests: `justerm-core/tests/search.rs`, `match_span_column_bound.rs`

## Reference behaviour

- [Who may hand the engine a match, and what happens to its columns](../../agents/reference-facts.md#search-who-may-hand-the-engine-a-match-and-what-happens-to-its-columns-678-verified-2026-07-31)
  — nobody else lets a consumer supply a match; the guard splits 1–1 (alacritty clamps, xterm hides)
  and justerm clamps because its prior behaviour was neither; the per-row projection converges with
  xterm's.
- [Dropping the paint vs ending the session](../../agents/reference-facts.md#search-dropping-the-paint-vs-ending-the-session-687-verified-2026-08-03)
  — `clearHighlights` vs `clear`.
- [Search on the overview ruler, and how a match position stays valid](../../agents/reference-facts.md#search-on-the-overview-ruler-and-how-a-match-position-stays-valid-440-verified-2026-08-13)
  — the ruler hand-over.
- [What retires a line-anchored mark when the line's content is destroyed in place](../../agents/reference-facts.md#what-retires-a-line-anchored-mark-when-the-lines-content-is-destroyed-in-place-750-verified-2026-08-07)
  — why ED retires xterm.js's search highlights and not justerm's.
- **The dialect is uncompared**: `SearchOptions` "mirrors xterm.js's `ISearchOptions`" is unverified
  doc-comment prose, while the two grammars genuinely differ.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — `match_spans`
  reaches it from two directions the selection does not: a consumer-authored `Match`, and #678's clamp
  landing an out-of-range column on a trailing spacer (#454).
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — the **heal** answer: an in-place erase stales the held highlights and deliberately does not
  invalidate them, because the consumer re-searches on output (#750 is the sibling that answered
  *retire*).
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `search_with` walks
  logical lines by absolute index and floors on alt (#144).
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — the haystack's
  trailing loop is its own implementation; breaking it makes a character **unfindable** and moves `$`
  (#685).
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — its loudest instance: a one-character query over a large viewport made 66 000 spans, wrapping the
  group's `u16` count to 464 while `decode` returned `Ok` (#621).

## Blast radius

- [active match](active-match.md) — designates over this set and inherits its invalidation whole.
- [selection](selection.md) — shares the absolute coordinate space, the pair widening and the overlay
  record shape; the active match paints over it.
- [logical lines](logical-lines.md) · [soft wrap](soft-wrap.md) — the same wrap-joined walk and
  padding trim decide what can match; #144 fixed the floor in both at once.
- [frame](frame.md) · [wire format](wire-format.md) — highlights are an overlay group whose count a
  query can drive to the ceiling first; a new group is an ADR-0020 question.
- [viewport](viewport.md) — matches project through `scrollback.len() - display_offset`, and
  `search_scroll_to` sets the offset.
- [decoration](decoration.md) — match lines become overview-ruler marks ranked beside decoration marks
  (ADR-0024 R3); the first place the consumer holds match coordinates across frames.
- [accessibility](accessibility.md) — the only edge landing in a consumer-owned control: `SearchResult`
  and `isInvalidRegex` change what a screen-reader user perceives (found by #448's completeness pass).
- [tracked points](tracked-points.md) — the anchor a backend keeps between two searches assumes the
  same five line movers this set is invalidated by.

## Known holes / open

- **`search_with` has no error channel.** An invalid regex yields an empty result, so the contract is
  "call two functions or be silently wrong", recorded in no decision.
- **Zero governing records** for the match model, smart-case or the dialect.
- **The invalidate-vs-re-anchor rule is unrecorded** beyond `term/search.rs`'s module doc (which still
  says "called eight times"; there are nine since `Term::clear`).
- **A match handed back after a resize is not rejected.** Invalidation drops what the engine holds and
  says nothing about what the consumer hands back later; `match_spans` bounds it at the projection
  (#678), and whether an intake should reject instead is open — #663 answered "reject" for a
  *geometry*, which has no range to clamp into, so it does not transfer. The web controller does not
  reach this with a stale coordinate (it sends an index into the fresh set); a backend replaying a
  remembered `Match` into `set_active_search_match` does.
- **No bound on the result count or the highlight set.** A one-character query over a full history
  allocates a `Match` per occurrence; the only cap is consumer policy.
- **The ruler's match lines go stale silently** between hand-overs (`SearchPort.matchLines`).
