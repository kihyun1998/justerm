# Territory — search

## What it is

Finding every occurrence of a query across screen **and** scrollback, and handing the consumer the
match list. Which match is *current* is a separate concept — see [active match](active-match.md).

The split of labour is deliberate: **the engine finds, the consumer navigates.** The consumer holds
the `Vec<Match>` and drives next/prev; the engine only scrolls to a match it is handed.

## Governing decisions

- [ADR-0026 — a coordinate that arrives from outside is bounded once](../../adr/0026-outside-coordinates-are-bounded-once.md)
  governs **one axis only**: a `Match`'s columns are authored by the consumer, so the bound sits at the
  projection (`match_spans`) rather than at a producer the engine does not have. Nothing about the
  match model, smart-case or the dialect — those are still unrecorded, below

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  supplies *why search is in core* — it needs the whole buffer, which a frame-mode consumer
  physically lacks — and why the query *policy* is not. Routing only, not the model

Nothing decides the match model, the smart-case rule, or the regex dialect.

## Design model

- **A `Match` is inclusive on both ends, in absolute buffer coordinates** — the same
  `[scrollback ++ screen]` line index the selection model uses. `Term::match_spans` converts one into
  viewport `SelectionSpan`s for painting.
- **Two entry points.** `search(query)` is literal + smart-case; `search_with(query, opts)` takes
  `SearchOptions { regex, whole_word, case_sensitive }`, and its default is exactly `search`.
- **Smart-case reads the *raw* pattern** — case-insensitive iff the query has no uppercase. In regex
  mode an uppercase metacharacter (`\B`, `\D`, `\x1B`) therefore flips case-sensitivity; set
  `case_sensitive` explicitly or use an inline `(?i)`.
- **The dialect is the `regex` crate, not JS `RegExp`** — no lookaround, no backreferences,
  Unicode-aware `\w \d \b`. A consumer must validate with `search::is_valid_regex`, because a
  JS-side check misjudges patterns the engine will reject and vice versa.
- **`is_valid_regex` compiles case-*insensitively* on purpose.** Unicode case folding grows the
  compiled program, so a `true` there holds under whichever case mode the search later picks; a
  case-sensitive-only check could pass a near-limit pattern that then exceeds the size limit.
- **Highlights are pushed, not pulled.** `set_search_highlights(matches)` puts state *into* the
  engine so it rides the frame. **The engine does not remember a query** — it cannot re-run one.
- **Query-derived state is invalidated; user-authored state is re-anchored.** On resize the engine
  calls `invalidate_search_highlights()` and the consumer re-searches at the new width, while the
  *selection* is re-anchored instead. The engine can recompute neither, but only one of them is
  reproducible by the consumer — and that asymmetry decides the coordinate-drift question for every
  piece of pushed state, not just this one.

## Code

- `justerm-core/src/search.rs` — `Match`, `SearchOptions`, `is_valid_regex` (the types)
- `justerm-core/src/term/search.rs` — `Term::search`, `search_with`, `search_scroll_to`,
  `match_spans`, `set_search_highlights`, `invalidate_search_highlights`, and the whole-word
  predicate `word_bounded`. Extracted from `term.rs` in #586. **The crate has two files named
  `search.rs`** — this one holds the mechanism, the one above holds the types, so a bare
  `search.rs:NN` citation is ambiguous
- `justerm-core/src/term/walk.rs` — the shared floor search reaches cells through (`abs_line` /
  `abs_row` / `abs_floor`, `line_in`, `extract_lines`)

## Reference behaviour

- [Who may hand the engine a match, and what happens to its columns](../../agents/reference-facts.md#search-who-may-hand-the-engine-a-match-and-what-happens-to-its-columns-678-verified-2026-07-31)
  — the first entry for this territory (#678). Three separable questions. **Nobody else lets a
  consumer supply a match** (xterm's addon takes a *term*; alacritty's and ghostty's are built
  internally). **The guard is arbitrated and splits 1–1** — alacritty clamps, xterm hides with a
  commented arm for exactly this input, ghostty cannot represent it; justerm took alacritty's side
  because its own prior behaviour was *neither* (it dropped the start row and painted the rest), and
  the section records what clamping costs that hiding would not. **The projection converges exactly**
  — xterm's per-row split is `match_spans`'s model — and is itself unguarded there, emitting spurious
  full-width rows for an out-of-range column

**Still open — the dialect.** `SearchOptions` is described as mirroring xterm.js's `ISearchOptions`,
but that is unverified prose in a doc comment while the two grammars genuinely differ (`regex` crate
vs JS `RegExp`) — exactly the shape of difference a pinned comparison exists to catch, and the entry
above does not touch it.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `search_with` walks
  logical lines by absolute index and must floor on alt (#144). Site 2 of 3 in that note's discovery
  history
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — the haystack's
  trailing loop is its own implementation of that rule, and this is the territory where breaking it
  is worst: the character is not merely missing from a copy, it is **unfindable**, and a regex `$`
  anchors one column early (#685)
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — this territory produced that note's loudest instance: a **one-character query** over a large
  viewport made 66 000 match spans, wrapping the group's `u16` count to 464 while `decode` returned
  `Ok` (#621). Search is the cheapest way to generate a viewport-scaled count, so it reaches a new
  group's ceiling before anything else does

## Blast radius

- [active match](active-match.md) — designates over this set and inherits its lifetime rules whole
- [selection](selection.md) — shares the absolute coordinate space and the viewport-span conversion
- [logical lines](logical-lines.md) — the same wrap-joined walk; #144 fixed the floor in both at once
- [soft wrap](soft-wrap.md) — matches cross wrapped rows, so the join rule decides what can match
- [frame](frame.md) — highlights are an overlay group; adding one is an ADR-0020 question
- [viewport](viewport.md) — matches are projected onto viewport rows, so the window decides what is
  emitted

## Known holes / open

- **`search_with` has no error channel.** An invalid or unsupported regex yields an **empty result**,
  not an error, so a consumer cannot distinguish a bad pattern from a genuine no-match.
  `is_valid_regex` exists as the workaround — the contract is effectively "call two functions or be
  silently wrong", recorded nowhere as a decision.
- **Zero governing records** for the match model, smart-case, or the dialect — all of which a
  consumer must know exactly and can currently learn only from doc comments.
- **The invalidate-vs-re-anchor rule is unrecorded.** It decides the coordinate-drift question for
  every future piece of pushed state and exists only as a comment inside `Term::resize`.
- **…and it leaves a gap on the way back in, which #678 measured.** Invalidation drops what the
  engine holds; it says nothing about what the consumer hands back *after*. A consumer that
  re-designates by **position** — the shape `set_active_search_match` exists for — can return a
  coordinate from before a resize, and nothing upstream re-clamps it — invalidation is not a bound,
  because the value re-enters through a public intake rather than surviving inside the engine.
  `match_spans` now bounds it at the projection (#678); what stays unrecorded is whether an intake
  should *reject* such a match instead, which is #663's question one seam over. **#663 answered its
  own seam with "reject" and that does not settle this one**: it rejects a *geometry*, which has no
  range to be clamped into, while a stale match column does — so ADR-0026 D1/D2 still govern here and
  this stays open.
  **A cleared concern, with its condition:** the web widget's own re-designation (#437/#441) does
  **not** reach that intake with a stale coordinate — it resolves the remembered position against
  the *fresh* match list backend-side and sends an index into that set, so the coordinate that
  crosses into the engine was produced by the search it belongs to. This holds while
  `SearchPort.anchoredIndex` is the only path carrying an emphasis across a hand-over; a backend
  that instead replays a remembered `Match` into `set_active_search_match` (the past-cap path, #436)
  is squarely the case above.
  **Re-read against #687, which added a second verb to that lifetime.** `SearchPort.clearHighlights`
  drops the paint while *sparing* the anchor, so the anchor now survives strictly longer — across a
  regex-mode query the engine refused. The condition still holds: `clearHighlights` carries no
  coordinate anywhere, and the anchor it spares is still resolved backend-side against a fresh match
  list before an index crosses the wire. What lengthened is the window in which the anchor can go
  stale by other means, which is [active match](active-match.md) § Known holes, not this one.
