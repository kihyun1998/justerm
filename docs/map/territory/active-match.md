# Territory — active match

## What it is

The **one** match the user is currently on, painted differently from the rest so "where am I in the
results" is visible. It is a *designation over* a [search](search.md) result set, not a search
concept: which member is active is navigation policy, and navigation is the consumer's.

## Governing decisions

**None.**

- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  governs the delivery mechanism (an overlay group), not the model
- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) governs how a cell
  resolves when several highlights land on it — the ranking that makes an active match visible
  *above* a selection is applied under that model, but the record leaves this specific ordering out
  of its own scope

## Design model

- **The consumer designates; the engine projects.** `set_active_search_highlight(index)` takes an
  index into the set the consumer last handed over; `set_active_search_match(m)` takes the match
  itself. The engine stores a resolved `Match`, not the index.
- **It rides its own overlay group *and* stays in `matches`.** The active match is present in both,
  and the overlap is resolved by **ranking** in the renderer rather than by excluding it from the
  wider set — an ordering decision deliberately pushed to where the pixels are.
- **Painted above selection.** The two can cover the same cell, and the active match wins; the
  foreground channel where they intersect is pinned separately (#430) because "wins" has to mean
  something specific per channel, not per layer.
- **Every path that voids the set voids this.** `set_search_highlights` resets it on hand-over, and
  `invalidate_search_highlights` is the single invalidation site. An active index into a set that no
  longer exists is exactly the failure that discipline prevents.
- **It is the fifth overlay group** (wire v12), added after the other four — which is why its
  interaction rules are the least recorded of them.

## Code

- `justerm-core/src/term/search.rs` — `Term::set_active_search_highlight`,
  `Term::set_active_search_match`
- `justerm-core/src/serialize.rs` — `Overlay::active_match`
- `justerm-renderer/src/overlay.rs` — the `ActiveMatch` overlay kind and its rank

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`. The model is described as following xterm's,
but the specific question — what wins where an active match and a selection overlap, per channel —
has never been compared against a pinned tree, and it is the one that decides pixels.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [search](search.md) — the set this designates over; its lifetime rules are inherited wholesale
- [selection](selection.md) — the overlap partner. A change to either's rank changes the other's
  appearance where they intersect
- [frame](frame.md) — a fifth overlay group, added at wire v12
- **renderer** *(no note yet)* — where the ranking is actually resolved, additively rather than by
  replacement

## Known holes / open

- **Zero governing records** for a feature whose entire content is an interaction rule.
- **The per-channel intersection with selection is pinned in an issue (#430), not in a record.**
  Where two highlights meet, "which wins" has to be answered per channel — background, foreground,
  ink — and only one of those answers is written down.
- **No reference comparison** for the overlap model, which is the part most likely to look wrong to a
  user coming from another terminal.
