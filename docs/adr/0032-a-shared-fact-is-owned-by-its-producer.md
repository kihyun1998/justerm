# ADR-0032: A fact several sites read is owned by its producer — the site it is first true at

Status: **accepted** (2026-09-04), as a **holding record**. Promoted from the tie-breaker table's row.
The row argued *against* its own promotion — *"an ADR over the four records below would be
archaeology"* — and wrote itself a falsifier that is **still unmet** (see Consequences). It is recorded
anyway because the row named no record of its own, and the maintained copy of that table went with the
generated `thegraph` build when it was retired; what survived was a **frozen** copy in
[`../agents/theflow.md`](../agents/theflow.md), which `CLAUDE.md` designates a cited corpus rather than
a routing target — so a rule living only there is reachable by citation and by nothing else. That copy
now defers to this record. Read this as routing that happens to live in `docs/adr/`, not as a new
derivation.

## Context

**The rule was derived independently four times before anyone wrote it down once.** Each time it
arrived as the *shape* of a decision rather than as the decision:

| Record | The same rule, locally |
|---|---|
| ADR-0025 D1 | owner = the property's scope; the encode-time cell bit is **never** the authoritative copy |
| ADR-0026 D2 | the bound site follows from whether the engine owns a **producer** for the coordinate |
| ADR-0027 D1 | liveness is answered by the source that **owns** it — the listener's flag owns *"have we been told"*, a different fact |
| ADR-0028 D1 | each composition surface has exactly **one** writer |

**No reference can arbitrate, and the reason is structural rather than an omission.** They answer this
inside architectures that never have to ask it. xterm.js's `Marker` is a live object the buffer mutates
in place — `marker.line -= amount` on trim (`src/common/buffer/Buffer.ts:646`, verified at the pinned
SHA on 2026-09-04) — so there are never two copies to reconcile. And every render-free engine hands off
**in-process**: alacritty by borrow, libvterm by C callbacks, ghostty by lock-shared state.

**Frame-mode's stateless consumer is what creates the question** (ADR-0020 R3). A reference's placement
of a fact is therefore *unimportable* here, the same way ADR-0031's defaults are.

## Decision

### D1 — The owner is the site the fact is **first true** at

Never a copy, a derivation, or a report of it — **however locally correct that proxy is**. When several
sites read one fact, exactly one of them produces it, and every other site reads rather than restates.

### D2 — Ask *which site owns this* before *which local rule is better*

This ordering is the whole operational content, and it exists because of how the failure presents:
**each site reaches for whichever nearby signal resembles the answer, and the resemblance holds
everywhere except the window that matters.** So every site is locally right, and review cannot see it —
which is why these arrive as clusters of four rather than as one bug. A debate about the better local
rule is already the wrong debate.

### D3 — Scope: this places a fact, and says nothing about how long it stays true

Once a value leaves its owner, *how long it remains true* is a different axis and is housed one rung
down — [`docs/map/invariant/a-coordinate-carries-the-instant-it-is-true-at.md`](../map/invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
and ADR-0029 for a published coordinate's basis and epoch, spine #630 for derived state's lifetime. Do
not read D1 as settling it; a second home for that axis would split its roster.

## Consequences

- A finding of the form *"site X and site Y disagree about this value"* is answered by naming the
  producer, not by picking the better of the two.
- **The promotion falsifier stands, unmet, and is the condition for this record earning its keep**: if
  D1 ever *derives* a site nobody had to be told about, or settles a question before it is asked, it has
  earned the record on its own merits. Until then it routes, and the four records above remain where the
  reasoning actually happened — this file must not restate their contents.
- **The inverse is also worth acting on**: if a fifth site re-derives D1 from scratch without anyone
  citing this record, the defect is that nothing routes a change here to it, and the repair belongs in
  `docs/map/`, not in more prose here.
