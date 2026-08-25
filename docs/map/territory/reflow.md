# Territory — reflow

## What it is

Re-splitting soft-wrapped logical lines when the width changes, and carrying every tracked point
through the move. A resize does not merely crop — on the primary screen it re-lays the whole buffer,
scrollback included, so a long line keeps its tail instead of losing it at the old margin.

**This territory was invisible to the first pass of this map.** It has no dedicated module, its code
sits inside `term.rs` and `grid.rs`, and no public method is named for it — you reach it only through
`resize`.

## Governing decisions

**None.**

- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  bounds it from one side: D4 governs the verbs that *edit*, and reflow is a **reallocation**, so a
  lead left without its spacer here is a legal state rather than a violation
- `docs/architecture.md` §"Hidden VT state" carries two entries that are this territory's contract —
  *"the alt screen resizes but does not reflow"* and *"soft-wrap vs a hard line-end must be
  distinguished for reflow"*

## Design model

- **Primary reflows; the alt screen does not** (#567). The alt pane is re-fit only — rows dropped or
  added to reach the new size, nothing re-wrapped — because a full-screen application places its own
  lines and re-wrapping them changes what it drew.
- **Reflow is not gated on DECAWM**, deliberately, and ghostty gates its equivalent. Three grounds
  are recorded at the call site: the wrap flag is not a lie after a re-split, the mode is global and
  momentary while the buffer is neither, and not reflowing truncates each row so the tail of a long
  line leaves the grid.
- **Tracked points travel with the content.** `reflow` takes a `points` slice — the cursor, selection
  anchors, markers — and returns where each landed. Anything anchored to a line has to be in that
  slice or it is silently wrong afterwards.
- **Scrollback and screen reflow as one stream**, which is what makes the concatenated coordinate
  space coherent across a resize rather than only within the visible grid.
- **Query-derived state is invalidated, user-authored state is re-anchored.** Search highlights are
  dropped and the consumer re-searches; the selection is carried. The engine can recompute neither,
  but only one is reproducible by the consumer.
- **A point one past the last cell cannot be expressed** (#562). Five designs were built, measured
  and rejected — that issue is the content, not a pointer to it.

## Code

- `justerm-core/src/grid.rs` — `reflow`, which takes and returns the tracked `points`
- `justerm-core/src/term.rs` — `Term::resize`, `ReflowDims`, `PaneReflow`, `reflow_pane`
- `justerm-core/src/lib.rs` — `Engine::resize`, whose doc comment is the consumer-facing contract

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
- [Relocating a cluster that grew to width 2](../../agents/reference-facts.md#relocating-a-cluster-that-grew-to-width-2-529-verified-2026-07-28)

The DECAWM divergence is argued at the call site against ghostty's `Terminal.zig`, with the
counter-evidence measured — a full row still re-splits with DECAWM off, and ghostty truncates instead.
That reasoning is in a code comment rather than in a record.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — this
  territory is where that fact stops being cosmetic and becomes **irreversible**, which is what
  settled its repair in #810. A hidden element measures `0x0`, the fit paths used to floor that to a
  `2x1` grid, and a resize is where a proposed grid becomes a change to the buffer: on the primary
  screen the re-split preserves logical lines, so re-widening restores the content; on the **alt
  screen** a resize is a re-fit — rows dropped, nothing re-wrapped (see the second bullet under
  *Design model*) — so there is nothing to restore from. The invariant is about absence producing a
  *plausible* answer; here the plausible answer also cannot be taken back

## Blast radius

Everything anchored to a line, because reflow is the one operation that moves content **between**
rows rather than within one.

- [selection](selection.md) — anchors are tracked points, and reflow is one of the four things that
  move an absolute coordinate (that note carries the set; a three-item count hides one of them)
- [marker](marker.md) — same, and alt markers additionally re-anchor on a base that shifts when the
  primary scrollback rewraps beneath them
- [search](search.md) — highlights are invalidated rather than moved, which is the asymmetry above
- [soft wrap](soft-wrap.md) — the wrap links are the input; a re-split writes new ones
- [wide glyph](wide-glyph.md) — `Row::resize` cuts through pairs and D4 stops at this boundary
- [cursor position](cursor-position.md) — the cursor is a tracked point
- [viewport](viewport.md) · [damage](damage.md) — a resize marks the whole screen damaged

## Known holes / open

- **Zero governing records** for the operation with the widest blast radius in the engine.
- **The DECAWM divergence is deliberate and unrecorded.** It diverges from a named reference with
  three stated reasons, in a comment — the exact shape ADR promotion exists for.
- **#562 — a point one past the last cell has no representation.** Five rejected designs; read the
  issue before touching relocation.
- **Nothing states which point sets must be passed.** The `points` slice is a convention: forget to
  include an anchor set and it is silently misplaced, with no compiler or test naming the omission.
