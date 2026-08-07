# Cross-cutting invariant — the write path funnels motion and does not funnel destruction

## The fact

Several structures in `justerm-core` sit **outside** the cells and assert something about the
content at a buffer position: a marker's line, a search highlight's span, a tracked point, a
selection's endpoints. Every one of them can stop being true in two unrelated ways.

1. **The content moved.** The write path has a *complete, enforced funnel* for this. Every mover
   calls a fixup trio (a quartet since #691) on adjacent lines — `selection_*`, `markers_*`,
   `tracked_*` for `shift_below_margin` / `evict_oldest` / `rotate_region` — and
   `invalidate_search_highlights` sits beside them. A new mover that forgets one is visible as a
   missing call next to three present ones.
2. **The content died where it stood.** There is **no funnel at all.** An erase or an overwrite
   leaves the row exactly where it is, so no mover runs, nothing is announced, and every one of
   these structures keeps asserting a fact about characters that are gone.

The invariant is not *"destruction must retire"* — that is false for two of the four. It is:

> **A structure that asserts a fact about content at a position owes an answer for destruction as
> well as for motion, and it must say which of the three answers it takes: retire, heal, or be
> positional. There is nothing to inherit, because no funnel decides it for you.**

The three answers, each with the site that takes it:

| Answer | Structure | Why it is the right one there |
|---|---|---|
| **retire** | OSC-133 command marks (#750) | the mark asserts *a command happened here*; nothing re-derives `command_lines`, so a wrong answer reproduces on every ask, forever |
| **heal** | search highlights | the consumer re-runs the query after a debounce on output, and an erase *is* output — so the staleness window is one debounce (`invalidate_search_highlights` owns this decision and its grounds) |
| **be positional** | tracked points, selection | the structure never claimed the characters. `anchoredIndex` asks "which occurrence was I on" and resolves by nearest position; a selection is a region of the screen and showing what is now under it is the semantics |

## Why it is cross-cutting

**Four structures, three answers, and no two of them arrived together.** Markers (#118), search
highlights (#108), selection and tracked points (#691) were built years and epics apart, and share
only the constraint that they name a position they do not own. Nothing in the code brings the four
into contact, so each one's destruction answer was decided — or, in the marker's case, never
decided — in isolation.

The motion half is the reason this stays invisible. It is so thoroughly funnelled that "the anchor
fixups" reads as *the* maintenance story, and
[marker](../territory/marker.md)'s own module doc said exactly that for three years: *"the three
fixups that keep a mark on its content while the buffer moves under it."* Every clause of that
sentence is true, and it is why a whole class of staleness had no home.

Distinct from [row-keyed side maps](row-keyed-side-maps.md), which is the same shape one layer
down and gets the answer for free: those maps are gated by a presence bit **inside the cell**, so
`cell.reset()` retires the fact whether the author thought about it or not (rule 3 there). None of
the four structures here rides in a cell, which is precisely why each owes an explicit answer.

## Territories it holds in

- [marker](../territory/marker.md) — `dispose_markers_on_row`, the *retire* answer, added by #750
  after a `clear` was measured reporting four commands that did not exist and then duplicating
  every later one
- [search](../territory/search.md) — the *heal* answer, recorded in
  `invalidate_search_highlights`'s doc-comment. Its reference clause was measured half false by
  #750 and corrected there
- [selection](../territory/selection.md) — the *positional* answer, and the discriminator that
  keeps this note honest: a selection over rewritten cells showing the new text is correct
- [logical lines](../territory/logical-lines.md) — the read surface where a retained-but-false
  assertion becomes a wrong string rather than a wrong colour, which is what makes the marker case
  the severe one

## What a violation looks like

- **A query answers about content that is gone**, with no error and no event. The marker case:
  `command_lines()` returned `[(0,""),(1,""),(2,""),(3,"")]` after `ESC[H ESC[2J` with **zero**
  `MarkerDisposed` events and `marker_count` unchanged — so a consumer holding an index had no
  channel telling it anything had happened.
- **A query answers with somebody else's content.** The same marks, after the shell redrew its
  prompt onto the columns they bound: `(0, "grepo")` for a command nobody ran. A screen reader
  announces it as a command.
- **An answer is duplicated rather than wrong.** The residue of the above: two real commands plus
  their four corpses is `n=4`, the corpses at the same document lines, so the list grows by one
  dead pair per `clear` and never shrinks.
- **A structure that should have healed did not, because its consumer changed.** The *heal* answer
  is a claim about the consumer, not about core — if the re-search is removed or debounced past
  usefulness, the search highlights silently join the first bullet.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| Motion funnel established | selection anchors + marker anchors, called side by side | #118 |
| Motion funnel extended, still motion-only | `invalidate_search_highlights` | #108 / #449 |
| Destruction answered *deliberately* for one structure — recorded in a doc-comment, and nowhere a neighbour would read it | search highlights: heal, on the consumer's debounced re-search | #108 |
| Motion funnel became a quartet, destruction still unasked | tracked points | #691 |
| Destruction found unanswered for the one structure that needed *retire* | OSC-133 marks: `command_lines` phantoms + re-borrowed text | #750 |

The tell is in row three. The decision was made, correctly, with its grounds — inside
`invalidate_search_highlights`, where only somebody already working on search will read it. Two
files over, the marker surface never asked the question, and the field comment that enumerates the
motion funnel reads as the complete rule.

## Where it will recur

The next structure that names a buffer position it does not own — a bookmark, a fold, a
diagnostic range, an OSC 8 hyperlink promoted out of the row map, a shell-integration span richer
than a mark. Two questions at the moment it is designed, not after:

1. Which of the three answers does it take, and *why that one* — the reason is always about the
   structure's consumer, not about core.
2. Is the answer written where the next author will hit it? A doc-comment on the invalidation
   helper is where the answer goes; a link from the territory note is what makes it findable.

It also recurs on any **new destroyer**. `DECALN` and `ED 3` are both unimplemented and both blank
content in place; `term.rs`'s `ED 3` arm already records an anchor-fixup obligation naming
selection, markers and tracked points — and, consistent with this note's history, does not name
search highlights.
