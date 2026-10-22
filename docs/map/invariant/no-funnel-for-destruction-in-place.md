# Invariant — the write path funnels motion and does not funnel destruction

## The fact

Several structures in `justerm-core` sit outside the cells and assert something about the content at a
buffer position: a marker's line, a search highlight's span, a tracked point, a selection's endpoints,
the settled-cell paint change record. Each can stop being true two unrelated ways.

1. **The content moved.** The write path has a complete funnel. Every mover calls the per-structure
   fixups side by side — `selection_*`, `markers_*`, `tracked_*` for `shift_below_margin` /
   `evict_oldest` / `rotate_region`, with `invalidate_search_highlights` beside them. Eviction goes
   through one function, `justerm-core/src/term.rs — Term::lines_left_the_front`, which at HEAD 7bb31f7
   calls five: `selection_evict_oldest`, `invalidate_search_highlights`, `markers_evict_oldest`,
   `tracked_evict_oldest`, `watches_evict_oldest` (the last added by #967; the old note said "a
   quartet"). A new mover that forgets one shows as a missing call beside present ones.
2. **The content died where it stood.** There is no funnel. An erase or overwrite leaves the row in
   place, no mover runs, nothing is announced, and each structure keeps asserting a fact about
   characters that are gone.

The invariant is not "destruction must retire" — false for most of them. It is:

> A structure that asserts a fact about content at a position owes an answer for destruction as well as
> motion, and must say which it takes — **retire**, **heal**, or **be positional**. No funnel decides
> it for you.

| Answer | Structure | Why there |
|---|---|---|
| retire | OSC 133 command marks (#750), `term/markers.rs — Term::dispose_markers_on_row` | the mark asserts a command happened here; nothing re-derives `command_lines`, so a wrong answer reproduces on every ask |
| heal | search highlights, `term/search.rs — Term::invalidate_search_highlights` | the consumer re-runs the query after a debounce on output, and an erase is output, so staleness lasts one debounce (the helper's doc owns this decision) |
| heal | settled-cell paint change record, `term/paint.rs — Term::content_changed_at` | a destroyed line is a changed line; it is reported again and the consumer re-matches it |
| be positional | tracked points, selection | the structure never claimed the characters: `anchoredIndex` (`justerm-web/src/search.ts`, `SearchPort`) resolves by nearest position; a selection is a screen region, and showing what is now under it is the semantics |

## Why it is cross-cutting

The structures arrived years and epics apart — markers (#118), search highlights (#108), selection and
tracked points (#691), the paint record (#967) — and share only that they name a position they do not
own. Their fixups do not call each other; nothing brings them into contact, so each destruction answer
was decided (or, for the marker, never decided) in isolation.

The motion half hides it: it is so thoroughly funnelled that "the anchor fixups" reads as the whole
maintenance story; marker's module doc said "the three fixups that keep a mark on its content while
the buffer moves under it" for three years — true in every clause, and why the staleness class had no
home.

Distinct from [row-keyed side maps](row-keyed-side-maps.md), the same shape one layer down that gets
the answer free: those maps are gated by a presence bit inside the cell, so `cell.reset()` retires the
fact regardless. None of these structures rides in a cell.

## Territories it holds in

- [marker](../territory/marker.md) — `dispose_markers_on_row`, the retire answer, added by #750 after a
  `clear` was measured reporting four commands that did not exist.
- [search](../territory/search.md) — the heal answer in `invalidate_search_highlights`'s doc-comment;
  its reference clause was measured half false by #750 and corrected there.
- [selection](../territory/selection.md) — the positional answer, the discriminator that keeps this
  note honest: a selection over rewritten cells showing the new text is correct.
- [tracked points](../territory/tracked-points.md) — positional; `term/tracked.rs —
  Term::tracked_shift_below_margin` / `tracked_rotate_region` / `tracked_evict_oldest` are its motion
  fixups and it has no destruction hook. Added here (new territory; the old note filed tracked points
  under selection).
- [settled-cell paint](../territory/settled-cell-paint.md) — heal; the change record rides damage, so
  a destruction site that bypassed damage would also bypass the record (none known). Added here; this
  territory already linked back.
- [logical lines](../territory/logical-lines.md) — the read surface where a retained-but-false
  assertion becomes a wrong string rather than a wrong colour, which makes the marker case severe.

## What a violation looks like

- **A query answers about content that is gone**, with no error and no event. Marker case (#750):
  `command_lines()` returned `[(0,""),(1,""),(2,""),(3,"")]` after `ESC[H ESC[2J` with zero
  `MarkerDisposed` events and `marker_count` unchanged.
- **A query answers with someone else's content** — the same marks after the shell redrew its prompt
  onto their columns: `(0, "grepo")` for a command nobody ran, announced by a screen reader.
- **Duplicated rather than wrong** — two real commands plus four corpses at the same document lines,
  growing by one dead pair per `clear`.
- **A heal that stopped healing because its consumer changed** — the heal answer is a claim about the
  consumer; remove or over-debounce the re-search and highlights join the first bullet.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| Motion funnel established | selection anchors + marker anchors, side by side | #118 |
| Motion funnel extended, motion only | `invalidate_search_highlights` | #108 / #449 |
| Destruction answered deliberately for one structure, recorded only in a doc-comment | search highlights: heal | #108 |
| Funnel gains tracked points; destruction unasked | tracked points | #691 |
| Destruction found unanswered where retire was needed | OSC 133 marks: phantoms + re-borrowed text | #750 |
| `ED 3` implemented and found to be motion, not destruction | rides `lines_left_the_front(n)` with the scrollback cap | #936 |
| Funnel gains the paint change record | `watches_evict_oldest` | #967 |

The tell is row three: the decision was made correctly, with grounds, inside the one helper only a
search author reads; two files over the marker surface never asked.

## Where it will recur

- **The next structure naming a buffer position it does not own** — a bookmark, a fold, a diagnostic
  range, an OSC 8 link promoted out of the row map, a richer shell-integration span. At design time:
  which of the three answers, and why (the reason is always about its consumer); and is the answer
  written where the next author hits it (the invalidation helper's doc, linked from the territory).
- **Any new destroyer.** `DECALN` is unimplemented and blanks content in place. `Engine::clear`
  (`term.rs — Term::clear`) is both: history and rows above the cursor's logical line leave through the
  eviction funnel, and rows below the cursor are blanked in place, where it answers per structure —
  marks retire (`dispose_markers_on_row`, as `ED 2`), tracked points stay positional, selection and
  highlights are cleared outright.
- **Any new mover** must call every fixup `lines_left_the_front` calls; read that function first.
