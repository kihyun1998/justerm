# Territory — search

## What it is

Finding every occurrence of a query across screen **and** scrollback, and handing the consumer the
match list. Which match is *current* is a separate concept — see [active match](active-match.md).

The split of labour is deliberate: **the engine finds, the consumer navigates.** The consumer holds
the `Vec<Match>` and drives next/prev; the engine only scrolls to a match it is handed.

## Governing decisions

**None.**

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

**None.** `docs/agents/reference-facts.md` has no entry for search. The dialect is the sharp gap:
`SearchOptions` is described as mirroring xterm.js's `ISearchOptions`, but that is unverified prose
in a doc comment while the two grammars genuinely differ — exactly the shape of difference a pinned
comparison exists to catch.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `search_with` walks
  logical lines by absolute index and must floor on alt (#144). Site 2 of 3 in that note's discovery
  history

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
