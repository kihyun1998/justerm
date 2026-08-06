# Invariant — a coordinate leaving core carries the instant it is true at, or the receiver is guessing

## The fact

Every buffer coordinate core hands out — an absolute `[scrollback ++ grid]` line, a document line
over `accessible_text` — is true **at one instant** and false afterwards, because the buffer's origin
moves under it. In the absolute space that instant takes **two** scalars, and reaching for the first
alone is the mistake this note has now watched happen twice (#737, #741): `evicted_total` counts lines
popped since RIS and dates a **uniform** move — eviction shifts every live marker by the same −1, so
one number expresses the whole of it — while `marker_epoch` dates the moves that are *not* uniform
(reflow, a region rotate that moved a survivor, an alt switch, RIS), where nothing smaller than a
re-pull repairs anything. A coordinate carrying only the basis is dated against half its own motion.

Core hands coordinates out on **three channels**, and each answers the instant question differently:

| channel | how the instant reaches the receiver | example |
|---|---|---|
| the **frame** | by construction — the header carries `evicted_total`, and every scalar in a `Frame` is sampled in one `Term::frame` body, so the snapshot is internally coherent | `display_offset`, `scrollback_len`, `marker_count` |
| an **event** | only if the variant carries it. An occurrence is a *point in time* whose payload outlives the instant that gave it meaning, and the frame's basis does **not** reach it — a `feed` that creates a marker and then evicts closes on a basis the event's line predates | `TermEvent::MarkerCreated { line, evicted_total, epoch }` |
| a **query answer** | only if the return type carries it — **or** if the answer is declared instantaneous and a re-ask always works | `Engine::marker_index` carries it · `Engine::command_marks` is declared instantaneous (#742) · `CommandLine::line` is neither |

So the rule is not *"put a basis on the frame"*. It is: **whichever channel a coordinate leaves by, the
instant leaves with it** — and only the frame gets that for free.

**The obligation has two discharges, and which one applies is derived, not chosen (#742).** Carrying
the instant is one way to keep a receiver from guessing; the other is to make sure it never has to
hold the answer in the first place. The second is only available when **a re-ask always answers**,
which needs two things at once:

- the receiver is consumed on a clock that *can* re-ask. The **frame** clock cannot — re-pulling per
  frame is the `O(M)`-per-frame payload ADR-0020 R3 forbids, which is why `marker_index` must be held
  and therefore must carry its instant. A **user-action** clock can, because the ask is the action;
- and the answer's **frame of reference is constant**, so absence has exactly one meaning.
  `command_marks` is primary-scoped whatever screen is up, so a re-ask always answers and an empty
  answer can only mean disposal. `marker_index` reports the *active* buffer, so its silence is
  ambiguous between *"disposed"* and *"you are on the other screen"* — an alt switch is one of the four
  moves its epoch exists to announce. **The sibling's shape was forced by a property `command_marks`
  does not have**, which is why copying it would import the machinery without the problem.

An **event** has neither discharge available: its payload is already detached from its instant by the
queue, so it can only carry.

**The document space has no scalar at all, and that is a fact about the space rather than a missing
field.** A document line indexes `accessible_text`, where soft-wrapped rows collapse into one logical
line. Its motion under eviction therefore equals the absolute delta **except** when the evicted rows
include a continuation row, and the receiver cannot tell the two cases apart. Measured: over one
eviction of `evicted_total` 0 → 2, the same command's absolute line moved 17 → 15 while its document
line moved 16 → 15. A basis scalar cannot repair this; only an explicit validity window can.

## Why it is cross-cutting

The channels belong to different territories and are decided by different records — the frame by
ADR-0020, the query seams by ADR-0017's mechanism/policy split, the event channel by **nothing at
all** (`events-and-replies.md` records that hole). So no single territory's rules could state this, and
each one that met it re-derived the argument: the frame half was settled in #490 by admitting two
header scalars, and the event half was re-decided from scratch in #737 four months later, on the same
mechanism, with the frame's answer already in the same file.

**Since #742 the derivation has a home — [ADR-0029](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)**,
the record that spans the three channels and derives which of the *two* discharges a given surface
owes. That record and this page do different jobs and both stay: the record derives the rule, this page
states the **fact**, lists where it holds, and says what a violation looks like. A new question about
an outbound coordinate's dating is a conformance item against its D1–D6, not a fresh decision.

It is also invisible from inside any one territory, which is the usual reason a fact belongs here. A
reader in `marker` sees a basis on the pull and reasonably concludes markers are handled; a reader in
`accessibility` sees a document line and has nothing to compare it against. Only laying the three
channels side by side shows which of them were silent — and, since #742, that silence on a *query* can
be the correct answer rather than a gap.

**A wrong answer here is silent by construction.** A stale coordinate is a *plausible* line number:
it decodes, it projects, it paints — on content it no longer names. Nothing errors, and the receiver
has no second source to disagree with.

## Territories it holds in

- [consumer events & query replies](../territory/events-and-replies.md) — the channel with no
  governing record, and the one where the payload outliving its instant is structural rather than
  incidental. `MarkerCreated` is the worked case
- [marker](../territory/marker.md) — where all three channels meet on one primitive: `marker_positions`
  (frame), `MarkerCreated`/`MarkerDisposed` (events), `marker_index` and `command_marks` (queries). The
  first and third of those carry the instant; the second carried none until #737 and only half of one
  until #741. `command_marks` carries neither half **and is correct without them** (#742) — it takes
  the other discharge, and the two queries sitting on one primitive with opposite shapes is what makes
  the derivation above visible at all
- [frame](../territory/frame.md) — the channel that gets coherence for free, and therefore the one
  that makes the other two look solved
- [accessibility](../territory/accessibility.md) — `CommandLine::line` is a document line, so it is the
  one coordinate on any channel that **no scalar can rebase**; the consumer holds it across a summon
  and a jump, which are two separate round trips
- [logical lines](../territory/logical-lines.md) — the collapse that makes the document space diverge
  from the absolute one is soft-wrap's, not accessibility's

## What a violation looks like

A coordinate crossing the boundary while the value that dates it stays behind. Concretely:

- a type that pairs lines with nothing **and is meant to be held** — a bare
  `Vec<(MarkerId, usize, MarkerKind)>` where the sibling returns
  `MarkerIndex { markers, evicted_total, epoch }`. The undated type alone is *not* the violation, and
  reading it as one is the mistake #742 corrected: the same signature is correct where a re-ask always
  answers, and the tell is then whether the doc says so. What makes it a violation is an undated
  coordinate on a surface whose receiver has no way back to the engine;
- an event variant gaining a `line`, `row`, `col` or `index` field that carries **fewer scalars than
  its pull-side sibling** — the subset is always the axis whichever bug forced the field, and the
  axis nobody measured is the one left silent (#741 was #737's own missing half);
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
  is what the epoch is for.
- **#741** (2026-08-06) — that boundary closed, and the shape of the miss is the durable part. #737
  fixed the axis its forcing case moved and wrote the obligation as *"carry its own basis"*; the
  event was still undated on the axis nothing in that fixture touched. **A subset chosen by the
  reproducing case reads exactly like the whole rule.** The event now carries the same triple its
  pull-side sibling returns — `MarkerIndex { markers, evicted_total, epoch }` — so the next variant
  is checked against a *sibling*, not against a list of axes someone has to keep complete. Its
  consumer half is one rule at two entry points: an entry is adopted only into the generation it
  names, compared for **equality** because `bump_marker_epoch` wraps. Gating only the queued replay
  would have left the same defect on the path where the event channel simply runs slower than the
  frame channel.
- **#742** (2026-08-06) — the **query** channel, and the first member to discharge the obligation the
  *other* way. It arrived shaped like #737 and #741 — an undated coordinate beside a dated sibling —
  and the shape was a false lead: `command_marks` is consumed when a user acts, and its frame of
  reference never flips, so a re-ask always answers and holding the answer is the caller's mistake
  rather than the API's. Carrying the instant here was measured **sound but expensive**, and the two
  halves want different weight. *Sound*: a differential probe over 400 seeds × 60 random ops — asking
  whether an epoch-stable transition ever moved a surviving command mark by anything other than the
  `evicted_total` delta — found **0 violations** over 20 529 such transitions and 63 367 per-mark
  checks. It has teeth: disabling both `bump_marker_epoch` sites produced 7 317 violations, disabling
  the `evicted_total` increment produced 99, and the baseline re-ran identical after each. *Expensive*:
  an ordinary top-anchored `DECSTBM` footer with a mark inside it bumped the epoch on **200 of 200**
  output lines, which would put a second surface on the consumer-side recovery discipline #746 had to
  invent for the first. So the option was rejected on cost, never on correctness — worth stating
  plainly, because a future reader who needs the consistency can take it and knows what it bills. Two premises in the issue's own body were
  measured false on the way: `command_lines` does **not** call `command_marks` (they are independent
  walks of `normal_markers`), and sampling the basis alongside the marks is *not* a second instant,
  because both are `&self` on one engine and `feed`/`resize` are `&mut self`. And it measured only the
  eviction axis; the epoch axis was found here — #741's lesson recurring one channel over.

**The cluster's anchor was spine #744, which closed on promotion to ADR-0029 (2026-08-06); the roster
is deliberately not here either way.** This page holds the *fact*; which surfaces are on the list and
which are still open lived in that issue, and the derivation now lives in the record. That split is
#552's measured result, not a preference: a hand-copied roster inside ADR-0025 went stale in five
places in three days while the rules beside it needed no edit. The one thing worth reading in the
closed anchor before reaching for this page:
that both shapes are legitimate is **settled** (#742) and *which* one a given surface owes is derived
by ADR-0029's D3 — but whether that derivation reaches every member is open at **#743**, which is now
a conformance item under the record rather than a sibling under an anchor. `CommandLine::line` is the
hard one: no scalar can date a document line, so the carry discharge is structurally unavailable to it
and D3 is its only route.

## Where it will recur

- **Any new `TermEvent` variant carrying a position.** `MarkerCreated.line` is the only one today
  (verified by reading the enum), so the next one has no sibling to copy and will start from the frame's
  model, which is the model that does not apply. The check that does not need a forcing case: name the
  **pull** that answers the same question and carry every scalar it carries. A variant with no such
  pull is the harder case and has not happened yet.
- **`Engine::command_marks`** — **settled (#742), and the settlement is the thing to read before
  re-opening it.** The lines are still undated, deliberately: the contract is *re-ask*, not *rebase*.
  What the next reader needs is that the answer moves on **both** axes, so noticing one of them is not
  grounds to re-derive the fix. Measured: eviction takes `[7, 7, 7, 8]` → `[5, 5, 5, 6]` with the epoch
  still, and a top-anchored `DECSTBM` footer takes `[7]` → `[10]` in one `feed` with `evicted_total`
  still. Both are pinned in `justerm-core/tests/command_marks_instant.rs`, together with the two
  properties the re-ask contract rests on — constant scope, and absence meaning only disposal.
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
  **What the reference *can* settle is a mechanism question inside the design, and #741 asked one.**
  ghostty carries a generation internally (`PageList.zig:372`, `:392` @ `e6e26e1`) and compares it
  with `<` — affordable only because its counter is a `u64` that is stated never to wrap (`:379`),
  where `marker_epoch` is a `u32` moved by `wrapping_add`. So *equality, never order* is **forced**
  here rather than chosen, and even ghostty treats order as a definitely-invalid floor rather than a
  validity answer (`:3623-3625`). Rows in
  [reference-facts.md](../../agents/reference-facts.md#dating-an-anchor-across-a-non-uniform-move--the-one-reference-with-a-generation-and-why-it-may-compare-with--741-verified-2026-08-06).
