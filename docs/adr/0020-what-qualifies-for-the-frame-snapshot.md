# ADR-0020: What qualifies for the per-frame snapshot

Status: **accepted** (2026-07-22; proposed 2026-07-21) — R2 sharpened on acceptance (derivability, not
provenance; an answer and its viewport projection are different groups). Accepted because the rule was
already deciding: #440's routing to the search port and #490's promotion from deferred optimisation to
*the fix for this ADR's one violation* both rest on it. Scoped to *what a frame carries*; the encoding is ADR-0005,
the decode boundary ADR-0008, and where a mechanism lives at all is ADR-0017.

**Amended 2026-08-04 (#721): the evidence table below graded one row wrong, and it is the row that
decides R3.** `markers` (v7/v10) was recorded as `O(viewport)` because `marker_positions` filters to
visible rows. Measured: an 80x24 grid holds **70 000** records in *that* group, because several marks
legitimately share one line and nothing dedups them. So **R3 has two violations, not one** — the
Decision's *"one stated violation"* wording is corrected in place below. The rule itself is unmoved,
and so is #490's ownership of the fix: bounding the population (#721) makes the group's `u16` count
safe without making it viewport-bounded, which is a wire-capacity answer and not an R3 one.

**Amended 2026-08-04 (#490, wire v15): two header scalars admitted, and the check recorded rather
than assumed.** `evicted_total` (u64) and `marker_epoch` (u32) ride the header so a consumer can pull
the marker set **once** and keep it valid. Against the three rules: **R1** — both are current state,
not occurrences (delivering either twice is harmless; missing a frame loses nothing, because each is
an absolute value rather than a delta). The *occurrences* in this design go to the event channel where
R1 puts them — `MarkerDisposed` (#160) and its new mirror `MarkerCreated`, since an appearance is no
more a field of a thing than a disappearance is. **R2** — a consumer holding only this frame cannot
compute either: `scrollback_len` saturates at the cap while eviction continues, and nothing observable
distinguishes a reflow from an ordinary scroll. **R3** — both are `O(1)`.

Note what this admission is *for*, because it is the first of its shape here: these two exist to let
the two marker groups **leave** in v16. A group is being removed by admitting the smallest thing that
lets the consumer reconstruct it — which is the same trade R3's remedy always described ("a slice of
that statelessness, deliberately, in one place"), now with a measured price on the other side: the
marker groups were 37–70% of an 80×24 frame at ordinary OSC-133 densities.

## Context

The wire has grown from v3 to **v12**. Four ADRs cover part of it, and they were all accepted on the
**same day**:

| ADR | wire | admitted |
|---|---|---|
| 0013 | 4 → 5 | scroll position (`display_offset` + `scrollback_len`), #112 |
| 0014 | 5 → 6 | interaction overlays — selection + search-match spans, #108 |
| 0015 | 6 → 7 | decoration marker primitive + overlay marker group, #118 |
| 0016 | 7 → 8 | mouse tracking mode as a wanted-events mask, #129 |

Four decisions, one axis, four separate arguments, **no rule**. And the axis kept moving without ADRs at
all: `serialize.rs` records v9 (alt-screen flag, #149), v10 (marker kind + exit, #159), v11 (every live
marker's absolute line, #120 S3) and v12 (a fifth overlay group — the active search match, #428). Four
more admissions, zero ADRs.

Each addition was individually well-argued. That is the point: a sound case for one group says nothing
about the next, and there has never been a statement of what a frame is *for* that a proposal could be
checked against.

Two costs have since surfaced from the same gap:

- **#482 / #490** — `markerLines` carries **every live marker** in every frame, so the consumer's join is
  `O(M)` per frame for `D ≪ M` decorations. `docs/research/terminal-engine-renderer-architectures.md`
  §6.2 found **no prior art** for that cost: references either have no line-mark concept
  (alacritty/libvterm) or keep marks as engine-side objects an in-process consumer resolves `O(1)`
  (ghostty tracked refs, xterm's event-maintained line cache). It is structural, not incidental.
- **#440** — deciding whether search-match ruler lines should ride the frame had **nothing to appeal
  to**. Its body had routed them into the wire by analogy with `markerLines`; the analogy failed on
  inspection (a marker is engine-owned state that moves with the buffer, a match is query-derived state
  the backend already holds), and the decision had to be derived from scratch.

### The split that already exists in practice

Read as data rather than as design, the three-way split is already there:

| carried in the frame | bound |
|---|---|
| cursor row/col/visibility (v3), shape + blink (v4) | `O(1)` |
| scroll position (v5), mouse wanted-events mask (v8), alt-screen flag (v9) | `O(1)` |
| cell columns + span directory | `O(viewport)` |
| selection / search-match / active-match spans (v6, v12) | `O(viewport)` |
| marker **positions**, kind, exit (v7, v10) | **`O(M)`, unbounded** (recorded as `O(viewport)` until #721 measured 70 000 in this group on 80x24 — row-filtered, but several marks share a row) |
| marker **lines** — every live marker, on-screen or not (v11) | **`O(M)`, unbounded** |

| carried **out-of-band** | why it cannot be state |
|---|---|
| `TermEvent::Title` / `Bell` / `Cwd` / `ColumnMode` / `ColorSchemeQuery` / `SetPaletteColor` … | an occurrence: as per-frame state a bell rings every frame, or is lost between two |
| `TermEvent::MarkerDisposed` (#160) | the disappearance of a thing cannot be a field of the thing |

| never on the wire at all | why |
|---|---|
| decorations | the consumer *supplied* them |
| search matches (`Vec<Match>`) | the backend ran the query and holds the result; only their viewport spans cross |
| query answers (text extraction, `command_lines`) | the consumer asked, and gets an answer, not a snapshot |

Two rows break the pattern — `markerLines`, and (since #721 measured it) the viewport `markers` group
above it. `markerLines` is the one #482 and #490 measure.

## Decision

A group qualifies for the per-frame snapshot only if it passes **all three**:

**R1 — State, not occurrence.** A frame is the terminal's *current state*. Anything that *happens* —
a bell, a title change, a disposal — is an occurrence and rides the out-of-band event channel. The test:
if delivering it twice is wrong, or if missing one frame loses it, it is not state.

**R2 — Not derivable by the consumer.** A frame carries only what the consumer cannot compute from what
it already holds or supplied. The test is **derivability, not provenance**: *can the consumer compute
this from what it holds?* — not *who asked first*. State the consumer pushed (decorations) or obtained by
asking (a query's answer, the set of matching lines) does not ride the snapshot, however frame-shaped it
looks.

**An answer and its viewport projection are different groups.** The consumer that ran a search holds the
match set, so the *set* fails R2. Folding a buffer coordinate onto a viewport row needs scrollback,
soft-wrap and wide-char state the consumer does not hold — ADR-0017's whole argument — so the
*projection* passes. `matchSpans` (v6, ADR-0014) and `activeMatchSpans` (v12, #428) are therefore
admitted, and stay admitted: both are `O(viewport)` projections of state only the engine can compute,
regardless of who first asked for the match. Read "search matches" here as the set, never as the spans.

**R3 — Viewport-bounded.** A group must be `O(1)` or `O(viewport)`. Buffer-wide, unbounded groups do not
qualify: a stateless consumer pays the full payload every frame, so an unbounded group makes per-frame
cost scale with a quantity unrelated to what changed.

**Why R3 is load-bearing, and not merely an optimisation.** Research §6.1 sharpening #2: *"the true
novelty is the STATELESS consumer. Every reference consumer — even Mosh's receiver — retains terminal
state … justerm's frame-mode consumer retains only the current frame's snapshot."* Every other engine can
afford unbounded engine-side structures because its consumer resolves them by handle. justerm's cannot,
so for justerm the bound *is* the contract.

**The stated violations — one of them resolved 2026-08-05 (#490, v16).** `markerLines` (v11) failed R3
knowingly; **it has now left the frame**, which is what #490 held the fix for and what this ADR
predicted would happen ("#490 acquires a home … it stops being a deferred optimisation"). A consumer
pulls the index once (`Engine::marker_index`, v15) and keeps it current from the header basis plus the
marker events. What replaced an unbounded group is four bytes: `marker_count` (v16).

**`marker_count` is admitted as a stated exception to R2, not as a pass.** A consumer that holds the
pulled index *and* consumes `MarkerCreated`/`MarkerDisposed` can derive the count exactly, so R2's own
test — *can the consumer compute this from what it holds?* — says no. It is admitted anyway because it
is a **diagnostic for the consumer that did not wire the events**, whose index then drifts with nothing
to notice it, and this family's recorded posture is that a silently wrong answer costs more than a
loudly absent one. Recording it as an exception rather than arguing it into a pass is the point of
having the rule; the earlier draft of this amendment claimed all three rules and was wrong.

**`markers` (v7/v10) still fails R3, and it is accepted rather than resolved.** Unrecorded until #721,
because this ADR had graded it `O(viewport)`. The honest statement of why it stays:

- its ordinary-session size is small — but note the measurement's limit, because the first draft of this
  paragraph leaned on it: the 0–36 figure comes from a probe that spread marks one command per N
  *lines*, so a row-filtered group is bounded **by construction** in that workload. It could not have
  come out large. The number that shows the other end is this repo's own: **70 000 records** in this
  same group on an 80×24 grid (#721), which is what the sentence above must be read against;
- `MAX_MARKERS` keeps its `u16` count sound — but that is a **wire-capacity answer, not an R3 one**,
  exactly as this file's 2026-08-04 amendment says. Using it as an R3 defence is alternative **(B)**
  ("cap each group's size instead"), which this ADR rejected on acceptance, re-entering through the
  back door;
- at that bound the group is ~0.5–0.8 MB per frame against 26 880 B of cells, and `Term::frame` emits
  it on `Partial` frames too — so R3's stated harm is fully present in the worst case.

So: it stays because nothing has yet needed it to go, not because it is defensible under R3. The
justification the first draft gave — *"it is what the a11y command announce consumes"* — is a
non-sequitur and is withdrawn: that consumer reads `id`, `kind` and `exit` and never the `row`
(`justerm-web/src/command-announce.ts`), all three of which `MarkerCreated` already carries. #490's
successor owns the question.

The original wording, kept because the reasoning still applies to what remains: It was admitted before this rule
existed, for a real need — the overview ruler must place marks the viewport cannot show. It is recorded
here as a violation rather than grandfathered into the rule: #482 mitigated the join, #490 holds the fix
(marker positions move to an out-of-band, incrementally-maintained index) and its trigger condition. A
rule with a documented exception is still a rule; a rule bent to fit its exception is not.

## Consequences

- **#440 is a derivation, not a judgement.** Search-match ruler lines fail **R2** — the backend holds
  `Vec<Match>` and the consumer learned it by asking — so they ride the existing `SearchPort` hand-over.
  That decision (2026-07-21) was reached by the argument this ADR now states; it stops being one issue's
  reasoning and becomes the rule's first application.
- **v6 and v12 pass.** Both overlay match groups are viewport spans of engine-projected state the consumer
  cannot compute — `O(viewport)`, state, not derivable. Admitting them was right. They are named in R2
  itself because the rule's first draft cited "search matches" without separating the *set* from its
  *projection*, which read as condemning two shipped groups; the tests always cleared them.
- **#490 acquires a home.** It stops being a deferred optimisation with no stated principle behind it and
  becomes *the fix for this ADR's one violation*. Its "not warranted until measured" trigger is unchanged
  — the rule says what is wrong, not when to pay to fix it.
- **A new group is a lookup, not a debate.** Three questions, in order: is it state (R1)? does the
  consumer already hold it (R2)? is it bounded (R3)? A "no" routes it — to the event channel, to the
  consumer's own seam, or to a persistent out-of-band index. A group that passes but *feels* wrong, or
  fails but seems necessary, is a gap in this ADR, closed by amending it.
- **Wire VERSION bumps get cheaper to judge.** The recurring question at every bump has been "should this
  be here?", re-derived each time. It is now a check.

## Alternatives considered

- **(A) Leave it implicit and keep arguing each group.** Rejected — that is the status quo the Context
  measures: eight admissions, four ADRs on one axis in one day, four more with none, one unbounded group
  that produced #482 and #490, and a routing question (#440) with nothing to appeal to.
- **(B) Cap each group's size instead.** Rejected. A cap bounds *cost* but says nothing about *kind*: a
  bell does not belong in a snapshot at any size, and a capped `markerLines` would still be a buffer-wide
  index re-sent every frame. Caps are an implementation answer to R3, not a substitute for R1/R2.
- **(C) Event-source everything; let the consumer maintain the state.** Rejected as the default. It
  requires a *stateful* consumer, which is precisely the property §6.1 identifies as justerm's unusual
  one and ADR-0011 has been narrowing (the web mirror is text-only since #504). R3's remedy trades a
  slice of that statelessness deliberately and in one place, which is why it is a tracked exception
  rather than the model.
- **(D) Defer until #490 is actually implemented.** Rejected. The rule's value is in what it *prevents*,
  and admissions are still happening (v12 landed this month); a rule written after the next four would
  again be describing history rather than governing it.

## Out of scope

- **The encoding** — layout, strides, zero-copy views (ADR-0005), and the decode boundary (ADR-0008).
- **Where a mechanism lives** — mechanism/core vs policy/consumer is ADR-0017; this ADR assumes that
  answer and only decides *how* core-owned state reaches the consumer.
- **The cadence** — when a frame is produced, and damage granularity (ADR-0003, `docs/architecture.md`).
