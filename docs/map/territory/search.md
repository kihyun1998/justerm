# Territory — search & active match

## What it is

Finding every occurrence of a query across screen **and** scrollback, handing the consumer the match
list, and carrying the highlight — plus the one *active* match — out on the frame so the renderer can
paint the current hit differently from the rest.

The split is deliberate: **the engine finds, the consumer navigates.** The consumer holds the
`Vec<Match>` and drives next/prev; the engine only scrolls to a match it is handed.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  supplies *why search is in core* (it needs the whole buffer, which a frame-mode consumer physically
  lacks) and *why the query policy is not* — routing only, not the model
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  governs the *delivery* of highlights as an overlay group, not what a match is

Nothing decides the match model, the smart-case rule, the regex dialect, or the active-match
precedence against selection.

## Design model

- **A `Match` is inclusive on both ends, in absolute buffer coordinates** — the same
  `[scrollback ++ screen]` line index the selection model uses. `Term::match_spans` converts one to
  viewport `SelectionSpan`s for painting.
- **Two entry points**: `search(query)` (literal + smart-case) and `search_with(query, opts)`.
  `SearchOptions { regex, whole_word, case_sensitive }` mirrors xterm.js's `ISearchOptions`; the
  default is exactly `search`.
- **Smart-case** = case-insensitive iff the query contains no uppercase. It reads the **raw** pattern,
  so in regex mode an uppercase metacharacter (`\B`, `\D`, `\x1B`) flips case-sensitivity — set
  `case_sensitive` explicitly or use inline `(?i)`.
- **The regex dialect is the `regex` crate, not JS `RegExp`** — no lookaround, no backreferences,
  Unicode-aware `\w \d \b`. A consumer must validate with `search::is_valid_regex`, not with JS, or it
  will misjudge patterns.
- **`is_valid_regex` compiles case-*insensitively* on purpose** — Unicode case folding grows the
  compiled program, so a `true` there holds under whichever case mode the search later picks. A
  case-sensitive-only check could pass a near-limit pattern that then exceeds the size limit.
- **Highlights are pushed, not pulled**: `set_search_highlights(matches)` and
  `set_active_search_match(m)` / `set_active_search_highlight(index)` put state *into* the engine so
  it rides the frame. The engine does not remember a query.
- **Query-derived state is invalidated; user-authored state is re-anchored.** On resize the engine
  calls `invalidate_search_highlights()` and the consumer re-searches at the new width, while the
  *selection* is re-anchored instead — because the engine can recompute neither, but only one of them
  is reproducible by the consumer. This is the line that decides the whole coordinate-drift question
  for pushed state.
- **The active match is voided by every path that voids the set** — the `set_search_highlights`
  hand-over resets it, and `invalidate_search_highlights` is the single invalidation site. An active
  index into a set that no longer exists is the failure this guards.

## Code

- `justerm-core/src/search.rs` — `Match`, `SearchOptions`, `is_valid_regex`
- `justerm-core/src/term.rs` — `Term::search`, `search_with`, `search_scroll_to`, `match_spans`,
  `set_search_highlights`, `set_active_search_highlight`, `set_active_search_match`,
  `invalidate_search_highlights`
- `justerm-core/src/term/walk.rs` — the shared floor search reaches cells through (`abs_line` /
  `abs_row` / `abs_floor`, `line_in`, `extract_lines`). Extracted in #585; `walk.rs`'s module doc
  names search as one of the read surfaces standing on it

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `search()` walks
  logical lines by absolute index and must floor on alt (#144). This is site 2 of 3 in that note's
  discovery history

## Blast radius

- [selection](selection.md) — shares the absolute coordinate space and the viewport-span conversion;
  the active match is painted **above** selection, so their precedence is a single decision touching
  both (the active ∩ selected fg channel is pinned separately)
- [logical lines](logical-lines.md) — the same wrap-joined walk; #144 fixed both in one change
- **frame / wire** *(no note yet)* — highlights and the active match are overlay groups; adding one is
  an ADR-0020 question and a wire-version bump
- **renderer** *(no note yet)* — `ActiveMatch` is its own overlay kind with its own rank, applied
  additively

## Known holes / open

- **`search_with` has no error channel.** An invalid or unsupported regex yields an **empty result**,
  not an error, so a consumer cannot distinguish a bad pattern from a genuine no-match. `is_valid_regex`
  exists as the workaround, which means the API's contract is "call two functions or be silently
  wrong" — recorded nowhere as a decision.
- **Zero governing records** for the match model, the smart-case rule, or the dialect choice — all of
  which a consumer must know exactly and can currently learn only from doc comments.
- **The invalidate-vs-re-anchor rule is unrecorded.** *"Query-derived state is invalidated and the
  consumer re-searches; user-authored state is re-anchored"* is a genuine design principle — it decides
  the coordinate-drift question for every future piece of pushed state, not just highlights — and it
  exists only as a comment inside `Term::resize`. The next pushed overlay will re-derive it or get it
  wrong.
