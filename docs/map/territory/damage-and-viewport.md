# Territory — damage & viewport

## What it is

What changed since the consumer last acknowledged a frame, expressed as line ranges with column spans
plus a first-class scroll op — and which window of the buffer the consumer is currently looking at.
The two are one territory because **the viewport decides what damage means**: while the user is
scrolled up, screen changes are not visible, so they are not damage.

This is the contract that paces the whole consumer protocol.

## Governing decisions

- [**ADR-0003 — damage model: incremental bounds**](../../adr/0003-damage-model-incremental-bounds.md)
  — line + column spans, ack-gated reset, a *recorded* (not diff-detected) scroll op; and why not
  Mosh's baseline diff or wezterm's per-line seqno
- [**ADR-0013 — expose scroll position in the frame**](../../adr/0013-expose-scroll-position-in-frame.md)
  — wire version 4 → 5
- `docs/architecture.md` §Cadence and §"Damage = line + column span" hold the consumer-facing protocol
  (the ack loop, ≤1 in flight, the pacing/diff split against Mosh · Alacritty · xterm.js)

The most thoroughly recorded territory in the map so far — the contrast case against
[cursor](cursor.md) and [selection](selection.md).

## Design model

- **Damage accumulates from the last ack.** `line_damage: Vec<LineBounds>`, one per row, widened by
  `expand`; `reset_damage()` is the ack and clears them.
- **"Undamaged" is encoded as `left > right`**, so an untouched line can never report as damaged and
  the first `expand` sets a real span. Mirrors Alacritty's `LineDamageBounds`.
- **`Full` is the collapse**, for flood, resize and alt-screen switch.
- **The scroll op is recorded, not detected** — `ScrollOp { top, bottom, count }`, written by the
  engine that performed the scroll, so the renderer moves rows instead of redrawing them.
- **Scrolled up ⇒ no damage, and a scroll ⇒ full damage.** `damage()` returns an *empty* partial while
  `display_offset > 0`, because the viewport is frozen; a user scroll that moves the viewport sets
  `full_damage`. This is the resolution of the viewport-vs-screen mapping question — and it is
  simpler than the mapping that question anticipated.
- **`damage()` is content-only; `frame_damage()` adds the cursor.** A pure cursor move changes no cell
  content, so the flow-control primitive must not see it, but a cell-invert caret must clear its old
  cell and ink the new one. `frame_damage` folds in the last-acked *and* current cursor cells, only
  when the cursor moved — so an idle frame stays empty. Mirrors Alacritty's `last_cursor`.
- **The ack defines "old".** `reset_damage` advances `prev_cursor`, so what counts as the cursor's
  previous cell is a function of the consumer's acknowledgement, not of wall time.

## Code

- `justerm-core/src/damage.rs` — `LineDamage`, `ScrollOp`, `TermDamage`, `LineBounds`
- `justerm-core/src/term.rs` — `Term::damage`, `Term::frame_damage`, `Term::reset_damage`,
  `Term::mark_fully_damaged`, `Term::set_display_offset`, `Term::scroll_up` / `scroll_down` /
  `scroll_to_bottom` / `scroll_delta`

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [cursor](cursor.md) — the old+new cell fold lives here but the rule is about the caret; changing
  either side produces ghosting
- **frame / wire** *(no note yet)* — damage, the scroll op and `display_offset` are all frame fields
  under ADR-0013/0020
- [search & active match](search.md) · [selection](selection.md) — both hold pushed state whose
  visibility is gated by the same `display_offset`
- **renderer** *(no note yet)* — the renderer re-packs every frame and diffs, so it does **not**
  consume incremental damage the way an incremental-repaint renderer would. Damage's efficiency
  argument is aimed at the *wire*, not at the family renderer

## Known holes / open

- ~~`architecture.md` §Cadence declares this an open question and it is not.~~ **Fixed.** The section
  now states the resolution instead of predicting a translation layer that was never built. Kept as a
  line here because the *shape* of that defect is the one to watch on this territory: the paragraph
  did not merely point at a closed issue, it described a **design that never shipped**, so a reader
  planning work from it would have rebuilt a solved problem.
- **`damage()` vs `frame_damage()` is a public/private split with no record.** One is the public
  flow-control primitive, the other is frame-internal, and the reason (content-only must stay
  content-only) survives only in a doc comment.
