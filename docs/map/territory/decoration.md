# Territory — decoration

## What it is

What a consumer *paints* at a [marker](marker.md): per-cell colour overrides on the grid and at most
one mark per line on the overview ruler. Entirely consumer-side — the engine anchors and knows
nothing about appearance, because appearance needs a theme and the engine is theme-agnostic by
identity.

The first territory in this map that lives **outside `justerm-core`**.

## Governing decisions

- [**ADR-0024 — decoration projection and precedence**](../../adr/0024-decoration-projection-and-precedence.md)
  — R1 through R6, the whole model. **Check its `Status:` line before relying on it** — that line
  is authoritative and is deliberately not copied here (`CLAUDE.md`: a status copied into another
  document has no gate and goes stale silently)
- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) — decoration colours
  enter the renderer's layer stack under this model; 0024 opens by placing itself on **the axis
  ADR-0019 explicitly put out of its own scope**
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why this is consumer-side at all

## Design model

ADR-0024 is authoritative; this is routing. **If they disagree, the ADR is right.**

- **R1 — a decoration is colours + a mark, not an object.** It projects to per-cell colour overrides
  and at most one ruler mark per covered line. Borders, outlines, per-decoration opacity, classes,
  transitions have **no expression by construction** — the model is not a styling system that happens
  to be small.
- **R2 — cell precedence is registration order, across markers.** Where two decorations set the same
  property on the same cell, the later-registered wins, whichever marker each anchors to.
  Deliberately *not* marker order, which could only express ordering *within* a marker.
- **R3 — ruler order is position class first, registration order second.** A `full`-width mark paints
  above a gutter mark regardless of registration order. **So the ruler order is not the cell order**,
  on purpose.
- **R4 — `anchor` moves the colour span.** `anchor: 'right'` measures `x` from the right edge and
  extends leftward. A declared divergence from the references, and it follows from R1: with no
  element to position, ignoring `anchor` would leave the option affecting nothing — a dead field.
- **R5 — projection is per visible row, not per anchor visibility.** A decoration whose anchor sits
  above the viewport still projects the rows of it that are on screen. **This is why the frame's
  absolute `marker_lines` group exists at all.**
- **R6 — a projection that cannot be computed emits nothing.** A non-finite or out-of-range input
  yields no rect and no mark rather than an invalid one, because the browser silently drops
  `top: NaN%` and stacks marks at the top edge — a wrong answer that looks like a rendering choice.

## Code

- `justerm-web/src/` — the decoration registry and projection (the consumer half)
- `justerm-renderer/src/decoration.rs` — where the projected colours meet the layer stack
- `justerm-core/src/serialize.rs` — `MarkerPosition` / `MarkerLine`, the only inputs it gets from the
  engine

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md` for the decoration model itself. ADR-0024 has
a *"Named prior art — and what upstream actually says"* section, which is a comparison made once
inside a record rather than a pinned row that survives an upstream move.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [marker](marker.md) — every decoration is anchored to one, joined by `MarkerId`; marker lifetime
  and disposal decide what the registry must reconcile
- [frame](frame.md) — consumes two overlay groups, and R5 is the reason the absolute one exists
- [viewport](viewport.md) — the ruler is buffer-relative, dividing by `scrollback_len + rows`
- [cell compositing](cell-compositing.md) — colour overrides enter the ADR-0019 layer stack there, and the
  precedence rules above decide what reaches it

## Known holes / open

- **The governing record may still be a proposal while the model ships.** Read ADR-0024's
  `Status:` line rather than trusting this sentence — but if it is still `proposed`, the
  strongest statement available about this territory is a proposal, and that is worth knowing
  before treating R1–R6 as settled.
- **No pinned reference comparison** (§Reference behaviour) for a model that carries a *declared
  divergence* (R4) — the divergence is argued inside the record and nothing re-checks it.
- **The consumer half is spread across two crates and mapped by neither.** `justerm-web` and
  `justerm-renderer` have no territories yet, so this note names files in areas the map does not
  cover.
