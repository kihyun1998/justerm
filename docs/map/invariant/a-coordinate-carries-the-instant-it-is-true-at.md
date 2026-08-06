# Invariant — a coordinate leaving core carries the instant it is true at, or the receiver is guessing

## The fact

Every buffer coordinate core hands out — an absolute `[scrollback ++ grid]` line, a document line
over `accessible_text` — is true **at one instant** and false afterwards, because the buffer's origin
moves under it. `evicted_total` is that instant for the absolute space: it counts lines popped since
RIS, and eviction shifts every live marker by the same −1, so one scalar expresses the whole move.

Core hands coordinates out on **three channels**, and each answers the instant question differently:

| channel | how the instant reaches the receiver | example |
|---|---|---|
| the **frame** | by construction — the header carries `evicted_total`, and every scalar in a `Frame` is sampled in one `Term::frame` body, so the snapshot is internally coherent | `display_offset`, `scrollback_len`, `marker_count` |
| an **event** | only if the variant carries it. An occurrence is a *point in time* whose payload outlives the instant that gave it meaning, and the frame's basis does **not** reach it — a `feed` that creates a marker and then evicts closes on a basis the event's line predates | `TermEvent::MarkerCreated { line, evicted_total }` |
| a **query answer** | only if the return type carries it, and today two do not | `Engine::marker_index` does · `Engine::command_marks` and `CommandLine::line` do not |

So the rule is not *"put a basis on the frame"*. It is: **whichever channel a coordinate leaves by, the
instant leaves with it** — and only the frame gets that for free.

**The document space has no scalar at all, and that is a fact about the space rather than a missing
field.** A document line indexes `accessible_text`, where soft-wrapped rows collapse into one logical
line. Its motion under eviction therefore equals the absolute delta **except** when the evicted rows
include a continuation row, and the receiver cannot tell the two cases apart. Measured: over one
eviction of `evicted_total` 0 → 2, the same command's absolute line moved 17 → 15 while its document
line moved 16 → 15. A basis scalar cannot repair this; only an explicit validity window can.

## Why it is cross-cutting

The channels belong to different territories and are decided by different records — the frame by
ADR-0020, the query seams by ADR-0017's mechanism/policy split, the event channel by **nothing at
all** (`events-and-replies.md` records that hole). So no single territory's rules can state this, and
each one that meets it re-derives it: the frame half was settled in #490 by admitting two header
scalars, and the event half was re-decided from scratch in #737 four months later, on the same
mechanism, with the frame's answer already in the same file.

It is also invisible from inside any one territory, which is the usual reason a fact belongs here. A
reader in `marker` sees a basis on the pull and reasonably concludes markers are handled; a reader in
`accessibility` sees a document line and has nothing to compare it against. Only laying the three
channels side by side shows that two of them are silent.

**A wrong answer here is silent by construction.** A stale coordinate is a *plausible* line number:
it decodes, it projects, it paints — on content it no longer names. Nothing errors, and the receiver
has no second source to disagree with.

## Territories it holds in

- [consumer events & query replies](../territory/events-and-replies.md) — the channel with no
  governing record, and the one where the payload outliving its instant is structural rather than
  incidental. `MarkerCreated` is the worked case
- [marker](../territory/marker.md) — where all three channels meet on one primitive: `marker_positions`
  (frame), `MarkerCreated`/`MarkerDisposed` (events), `marker_index` and `command_marks` (queries). The
  first and third of those carry a basis; the second did not until #737, and `command_marks` still
  does not
- [frame](../territory/frame.md) — the channel that gets coherence for free, and therefore the one
  that makes the other two look solved
- [accessibility](../territory/accessibility.md) — `CommandLine::line` is a document line, so it is the
  one coordinate on any channel that **no scalar can rebase**; the consumer holds it across a summon
  and a jump, which are two separate round trips
- [logical lines](../territory/logical-lines.md) — the collapse that makes the document space diverge
  from the absolute one is soft-wrap's, not accessibility's

## What a violation looks like

A coordinate crossing the boundary while the value that dates it stays behind. Concretely:

- a type that pairs lines with nothing — `Vec<(MarkerId, usize, MarkerKind)>` where the sibling
  returns `MarkerIndex { markers, evicted_total, epoch }`;
- an event variant gaining a `line`, `row`, `col` or `index` field with no companion scalar;
- a doc comment that tells the reader to rebase one surface's answer by *another* surface's delta;
- a consumer storing an answer past the frame it was asked in, with no recorded validity window.

The tell in review is a sentence of the form *"it is absolute at the moment of X"* with no field
named X. `events-and-replies.md` carried exactly that sentence — *"`line` is absolute on the same
basis the frame header reports"* — and the consumer was written against it.

## Discovery history

- **#490** (2026-08-04, wire v15/v16) — settled it for the **frame**, by admitting `evicted_total` and
  `marker_epoch` to the header so a pulled marker index stays usable. The split it derived is the
  general one: a uniform move is one scalar, a non-uniform move is an epoch, and nothing smaller than
  a re-pull repairs the second.
- **#737** (2026-08-06) — the **event** channel, re-decided from scratch. Two single-`feed` batches
  differing only in whether an OSC-133 mark preceded or followed three evictions agreed on the event
  line, on both frame bases, on the epoch and on `marker_count`, and their true lines were three
  apart. Its Step 5 pass then found the **query** channel holding the same shape twice, unfixed.
- The same pass established the boundary of the fix: the carried basis makes the drain orders
  equivalent on the **eviction** axis only. A reflow between a birth and its drain moves markers
  non-uniformly — measured, a mark at absolute 3 reflowed to 5 is answered as 3, permanently — which
  is what the epoch is for and what the event does not yet carry.

**The cluster's anchor is spine #744, and the roster is deliberately not here.** This page holds the
*rule*; which surfaces are on the list, which are still open, and what is not yet decided all live in
that issue, where they can be edited without touching the rule. That split is #552's measured result,
not a preference: a hand-copied roster inside ADR-0025 went stale in five places in three days while
the rules beside it needed no edit. The one thing worth reading there before reaching for this page:
whether the rule is *"carry the instant"* or *"declare the answer instantaneous"* is **not settled** —
`tracked_point` already does the second (ADR-0026 D2/D3), and it may be the right shape for a query
nobody caches.

## Where it will recur

- **Any new `TermEvent` variant carrying a position.** `MarkerCreated.line` is the only one today
  (verified by reading the enum), so the next one has no sibling to copy and will start from the frame's
  model, which is the model that does not apply.
- **`Engine::command_marks`** — absolute lines, no basis, no epoch. Measured: over `evicted_total`
  14 → 16 its lines went `[6, 6, 7, 8]` → `[4, 4, 5, 6]`, a uniform −2 the caller has no way to apply,
  because the tuple has nowhere to put it.
- **`CommandLine::line`** — the document coordinate above, which no scalar can rebase.
- **Any consumer cache added beside `MarkerIndexCache`.** It is the only class in `justerm-web/src`
  holding a buffer coordinate across frames, so the second one will be written without its
  rebase-at-read-time discipline unless someone says this out loud.
- **The reference cannot help here and that is settled, not open.** xterm.js keeps a marker valid by
  mutating a live object and firing events, and states the uniformity assumption in prose
  (`DecorationService.ts:24-26`, *"they should all change by the same amount"*, @ `699f553`); ghostty
  marks a pin `garbage` rather than renumbering it (`PageList.zig:1039`, `:3593`, @ `e6e26e1`).
  Neither serializes a coordinate across a boundary, so neither has ever needed an instant to travel
  with one. Every claim on this page is first-party, which is what the
  **Wire / frame / API shape → this repo's own precedent** tie-breaker requires.
