# ADR-0029: A coordinate leaving core carries the instant it is true at, or it is answered by a query a re-ask always answers

Status: **accepted** (2026-08-07, #743) — proposed 2026-08-06 (#744). Promotes the model that accreted
across #490, #737, #741 and #742 — four decisions about the same pair of participants (*which channel*
× *which scalar*), each made on its own merits, each producing the next.

*Accepted on the condition this record wrote for itself* (see Consequences): #742 shipped in PR #748,
merged as `8ae6900`, and #743 then discharged the member the record had named as its **hard case** —
where D2's carry discharge would cost a second coordinate space's worth of dating apparatus. D3 granted
re-ask on its own merits rather than by elimination, and no clause needed correcting — the bar
ADR-0028's precedent set. One *ground* was corrected: see alternative (A).

**This is a derivation, not a product judgement.** Every clause below follows from what this engine's
own channels can and cannot deliver, plus ADR-0020's payload rule, so a better derivation retires it.
Nothing here is a taste call. The reference cannot arbitrate the design question at all — no reference
serialises a buffer coordinate across a process boundary, so none has ever needed an instant to travel
with one, and the **Wire / frame / API shape → this repo's own precedent** row of the tie-breaker
therefore governs. What the references *can* settle is a mechanism inside the design, and two of them
did; both are recorded under "Named prior art".

**Scope — what this record does not cover.** It answers *when* a coordinate is true. It does **not**
answer *which buffer* a coordinate names, which is a separate axis: primary and alt occupy the same
absolute indices, so an integer can be undated **and** unscoped independently. That axis lives in
ADR-0026's neighbourhood. Do not read the clauses below as settling it — the neighbour is not decided
just because the same change surfaced it.

*Amended 2026-08-07 (#743): the scope stands, and the one member where the two axes met is now
discharged. The surprise worth recording is that D3.2 **constrains** the neighbour without deciding
it — the obvious which-buffer repair for `command_lines` (answer nothing on the alt screen) is closed
by D3.2, because it would make absence ambiguous between* disposed *and* wrong screen *and so demand
the carry discharge, which for this surface costs a second coordinate space's dating apparatus. A rule about the instant turns out to rule some
which-buffer answers out; it still does not rule one in.*

## Context

Core hands buffer coordinates to a consumer that holds **only the current frame** (ADR-0020 R3's
grounds — the stateless consumer is this project's actual novelty). Every such coordinate is true at
exactly one instant, because the buffer's origin moves under it: scrollback eviction shifts every live
anchor, a reflow rewrites them, a region rotate moves a subset.

**A wrong answer here is silent by construction.** A stale line number is a *plausible* line number:
it decodes, it projects, it paints — on content it no longer names. Nothing errors, and the receiver
has no second source to disagree with.

Coordinates leave on three channels, and the channels are governed by three different things — the
frame by ADR-0020, the query seams by ADR-0017's mechanism/policy split, and the event channel by
**nothing at all** (`docs/map/territory/events-and-replies.md` records that hole in its own
`## Known holes`). So no single territory's rules can state the obligation, and each surface that met
it re-derived the whole argument:

| # | surface | decided | and then |
|---|---|---|---|
| #490 | the **frame** | admit `evicted_total` + `marker_epoch` to the header so a pulled index stays usable | the event mirror created *in the same change* was written without either |
| #737 | the **event** | `MarkerCreated` carries `evicted_total` | written as *"carry its own basis"* — the axis its own fixture moved |
| #741 | the **event**, again | it carries the epoch too | the subset had read exactly like the whole rule |
| #742 | a **query** | carries neither, deliberately | the shape that looked like the defect was not one |

**The forcing evidence that this is one root and not four coincidences: #737 re-derived, four months
later, an answer already sitting in the same file** — and the map note beside it asserted the frame's
basis reached the event, which is what the consumer had been written against.

**And the cluster kept measuring its own premises false**, which is the tell that the participants
were being recombined rather than reasoned about. #741 falsified #737's pass ("no non-uniform move
escapes the epoch inside a single `feed`" — `markers_shift_below_margin` does, from the byte stream
alone). #742 falsified two of its own body's premises before work could start: that `command_marks`
is the raw-marks query behind `command_lines` (they are independent walks of `normal_markers`), and
that sampling the basis beside the marks is a second instant (both are `&self` on one engine while
`feed`/`resize` are `&mut self`, so the compiler makes it one).

### The forcing case

#742 arrived shaped exactly like its two predecessors — an undated coordinate beside a dated sibling —
and the proposal that follows from *"whichever channel a coordinate leaves by, the instant leaves with
it"* was to give `Engine::command_marks` the `MarkerIndex` triple.

Measured, that proposal is **sound and wrong**:

- *sound* — a differential probe over 400 seeds × 60 random ops, asserting that an epoch-stable
  transition moves every surviving command mark by exactly `−Δevicted_total`, found **0 violations**
  over 20 529 epoch-stable transitions and 63 367 per-mark checks. It has teeth: disabling both
  `bump_marker_epoch` sites produced 7 317 violations, disabling the `evicted_total` increment
  produced 99, and the baseline re-ran identical after each restore;
- *wrong* — an ordinary top-anchored `DECSTBM` footer with a mark inside it bumped the epoch on
  **200 of 200** output lines. Those are true positives; the mark really moved 7 → 207. Carrying the
  instant would therefore oblige a *second* surface to run the consumer-side recovery discipline
  #738 → #746 had to invent for the first — for a query nothing caches.

What the surfaces actually differ in is not the coordinate space. It is whether the receiver **can
re-ask**, and that turned out to be two measurable properties rather than a preference.

## Decision

**D1 — The obligation is core's, and it is discharged, never waived.** A coordinate crossing the
boundary must leave the receiver able to tell whether the value still names the content it named.
Only core knows when the buffer moved, so "the consumer should be careful" is not a discharge.

**D2 — There are exactly two discharges, and which one a surface owes is *derived from that surface*,
not chosen.**

- **Carry** — the value travels with every scalar that dates it.
- **Re-ask** — the answer is declared instantaneous, and the receiver asks again instead of rebasing.

**D3 — Re-ask is available only when *both* of these hold. If either fails, the surface must carry.**

1. **The consumption clock can re-ask.** The **frame** clock cannot: re-answering per frame is the
   `O(M)`-per-frame payload ADR-0020 R3 exists to forbid, so a frame-clock consumer is *obliged* to
   cache and therefore obliged to be given the instant. A **user-action** clock can, because the ask
   is the action.
2. **The answer's frame of reference is constant**, so absence has exactly one meaning. A surface
   whose scope follows the active buffer cannot promise this: an id missing from its answer may be
   disposed *or* may simply belong to the other screen, and the caller cannot tell.

Worked, on the two queries that sit on one primitive and come out opposite:

| | `Engine::marker_index` | `Engine::command_marks` |
|---|---|---|
| D3.1 clock | **frame** — feeds an overview ruler that must be current every frame | **user action** — prompt-to-prompt navigation |
| D3.2 scope | **active** buffer; an alt switch is one of the four moves its epoch announces | **primary**, whatever screen is up |
| discharge | **carry** (`markers`, `evicted_total`, `epoch`) | **re-ask** (declared instantaneous, #742) |

**The corollary is the useful half: a sibling's shape is not evidence.** `marker_index`'s triple was
forced by properties `command_marks` does not have, so copying it would import the machinery without
the problem. Ask D3 of the surface in front of you, never *"what does the one next to it return"*.

**D4 — An event has no choice: it can only carry.** An occurrence's payload is detached from its
instant by the queue before any consumer sees it, so neither D3 condition is even askable. And what
it carries is checked **against its pull-side sibling** — every scalar the pull answering the same
question carries — never against a list of axes someone has to keep complete. #741 is why: #737 wrote
the obligation as the axis its fixture happened to move, and the subset read exactly like the rule.

**D5 — In the absolute space "the instant" is two scalars, and they date different kinds of motion.**
`evicted_total` dates a **uniform** move (eviction shifts every anchor by the same amount, so one
number expresses all of it). `marker_epoch` dates the **non-uniform** ones — reflow, a region rotate
that moved a survivor, an alt switch, RIS — where nothing smaller than a re-pull repairs anything.
Compare generations for **equality, never order**: `bump_marker_epoch` wraps.

**D6 — A re-ask discharge is *stated where the caller reads it* and *pinned by a test*.** The
declaration is the whole of the contract, so it belongs on the public doc surface (docs.rs), not in a
body comment — and prose alone is not sufficient here, because prose is exactly what failed in #737,
where a doc asserted a rebasing rule that did not hold and the consumer was written against it. The
test pins the properties D3 rests on for that surface, so a later change to scope or dating reds
rather than silently invalidating the contract.

## Named prior art

**On the design question: none, and that is a finding rather than a gap.** Neither xterm.js, ghostty
nor alacritty serialises a buffer coordinate across a boundary, so none has ever needed an instant to
travel with one. xterm.js keeps a marker valid by mutating a **live object** and firing events
(`src/common/buffer/Buffer.ts:646`, `:654`, `:665`; `Marker.ts:23`, `:32` @ `699f553`); ghostty marks
a pin `garbage` rather than renumbering it (`terminal/PageList.zig:1039`, `:3593` @ `e6e26e1`). Both
are also on this project's **deliberate-divergence list** — a row-attached mark cannot hold ADR-0015's
identity/kind/exit/column, and a retained live handle is what ADR-0020 R3's stateless consumer gives
up — so neither is importable, and a finding shaped *"the reference does it another way"* is
`DELIBERATE` here, not a defect (#490's war story is the cost of forgetting that).

**On mechanisms inside the design, two references do arbitrate, and both agree with a clause above.**

- **D5's equality comparison is forced, not chosen.** ghostty carries a generation internally and
  compares it with `<` (`PageList.zig:372`, `:392` @ `e6e26e1`) — affordable only because its counter
  is a `u64` stated never to wrap (`:379`). `marker_epoch` is a `u32` moved by `wrapping_add`, so
  order is unavailable to us; even ghostty treats order as a definitely-invalid floor rather than a
  validity answer (`:3623-3625`). Recorded from #741.
- **D2's re-ask discharge is the shape both references reach for when a query returns coordinates.**
  ghostty's OSC-133 query is a lazy iterator over live pins, walked from the cursor's current pin per
  action and never materialised (`PageList.zig:5473`, `:5622`; its only caller `Surface.zig:4196`).
  The one place xterm.js *does* materialise a batch of buffer coordinates — the search addon's
  results — recomputes it fully on any buffer motion and never rebases, and exposes no coordinate on
  its public boundary at all (`addons/addon-search/src/SearchAddon.ts:65-66`;
  `typings/addon-search.d.ts:81-89`). *Reference-free restatement:* a query that materialises
  coordinates into a caller-owned batch is the only shape that can go stale.

## Consequences

- **A new `TermEvent` variant carrying a position** is answered by D4 with no forcing case needed:
  name the pull that answers the same question and carry every scalar it carries. `MarkerCreated.line`
  is the only coordinate on the whole enum today, so the next one has no sibling to copy and would
  otherwise start from the frame's model — the model that does not apply.
- **A new query returning coordinates** runs D3 rather than copying its neighbour. Two answers are
  legitimate and the record says which is which, so this stops being a fresh decision.
- **#743 (`CommandLine::line`) was the conformance item that tested the derivation**, and it is the
  interesting one: a *document* coordinate over `accessible_text`, where soft-wrapped rows collapse,
  so no *published* scalar dates it and giving it one would take two — D2's carry discharge is the
  expensive one here, and D3 was expected to be the only route. **Resolved 2026-08-07: D3 passed on both conditions without being stretched** (clock = a user
  action; frame of reference = `[scrollback ++ primary]`, whatever screen is up), so the surface takes
  re-ask because it *qualifies*, not merely because nothing else was left. D6 discharged as a doc
  statement on `Engine::command_lines` plus `justerm-core/tests/command_lines_document.rs`.
  Two things it taught this record. **The derivation reaches its own hard case** — which was the open
  question, since a rule that only covers the members it was derived from is a filing cabinet. And
  **the answer's fixture conditions are load-bearing in a way the rule does not mention**: a pin for a
  document coordinate is vacuous unless the fixture actually contains a collapse, and a pin for the
  alt-screen half is vacuous unless `scrollback.len()` is non-zero so `abs_floor()` does something.
  Both were measured, not reasoned — two drafts of those tests passed against a deliberately broken
  engine before the fixtures were rebuilt.
  It also carried the **scope** defect this record puts out of scope: on the alt screen its line
  indexes a document `accessible_text` does not return. Discharged the same way and in the same
  breath, which is the part that was not predicted — see the amendment note under Scope.
- **The rule and the roster stay apart.** `docs/map/invariant/a-coordinate-carries-the-instant-it-is-true-at.md`
  keeps the fact descriptively and this record keeps the derivation; the roster lived in spine #744,
  which closes pointing here. That split is #552's measured result — a hand-copied roster inside
  ADR-0025 went stale in five places in three days while the rules beside it needed no edit.
- **ADR-0020's two amendments are subsumed on this axis, not retired.** Its clause *"a group admitted
  to the frame gets its basis from the frame; an occurrence routed to the event channel must carry its
  own"* is D4's special case; ADR-0020 still owns frame *membership*, which this record does not touch.
- ~~**One member has shipped against it and it is not merged yet** (#742, PR #748), so the status
  stays `proposed`.~~ **Discharged 2026-08-07 (#743):** PR #748 merged as `8ae6900`, and a *second*
  member then shipped — the hard case, which is the stronger evidence, because a record that only
  covers the members it was derived from is a filing cabinet. ADR-0028's precedent was the bar (accept
  once a member has shipped, not when it was written) and it is met. Where that precedent had three of
  five clauses corrected *by implementing them*, this one had none, so what accepts it is reach rather
  than repair — a weaker signal per clause and the reason the hard case had to be the member that
  carried it.

## Alternatives considered

**(A) One rule: always carry the instant.** The obvious reading of the invariant note before #742, and
what a fourth issue in this shape invites. Rejected on two independent grounds, both measured: the
cost on a re-askable surface is real (200/200 stale signals above, plus a second consumer needing
#746's recovery discipline), and `CommandLine::line` **complies only at a disproportionate price** — a document line's
motion under eviction equals the absolute delta *except* when the evicted rows include a continuation
row, and no scalar distinguishes the two cases. A rule one member is structurally unable to satisfy is
not a rule.

**(B) One rule: always declare answers instantaneous.** Rejected: `marker_index` cannot: re-pulling
per frame is precisely ADR-0020 R3's prohibition, which is what #490 exists to remove. And an event
cannot even be asked the question (D4).

**(C) Keep deciding per surface, as before.** Rejected because it is the *observed* failure, not a
hypothetical one: four decisions about the same two participants, one of which re-derived an answer
already in the same file, one of which shipped a subset of its own rule, and one whose body carried
two premises that measurement broke. An issue holds one decision with its rejected alternatives and a
doc-comment pins a rule to one branch of the code; neither can hold a rule that *spans* decisions.

**(D) Give the document space its own monotonic logical-line counter.** Not rejected — **deferred, and
#743 re-confirmed the deferral rather than lifting it** (2026-08-07). #490 weighed and withdrew a
monotonic coordinate for the *absolute* space on the grounds that eviction is uniform there; that
argument does not transfer, because the document space has no uniform delta. What #743 established is
that the deferral does not rest on that alone: it is the *carry* discharge, and the surface that would
need it **passes D3**, so nothing demands it. The trigger to revisit is therefore no longer "someone
finds the document coordinate stale" — that is answered — but a consumer that has to hold a document
line across a buffer it *cannot* re-sample, which is a D3.1 failure and does not exist today.

**(E) Make the transience compiler-enforced** — return a type bound to the `&self` borrow so a caller
cannot stash the answer. Rejected as over-engineering at this cost: `.collect()` is one keystroke
away, so it is a speed bump rather than a guarantee, and it changes a published signature to buy what
D6's doc-plus-test already buys. Recorded because it is the natural objection to D6 — that "declared
instantaneous" is a claim the compiler does not check — and the honest answer is that it is checked by
a test instead.
