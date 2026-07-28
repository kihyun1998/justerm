# Territory — damage

## What it is

What changed since the consumer last acknowledged a frame, as line ranges each carrying a column
span, plus a first-class scroll op. It is an **efficiency** axis, not a correctness one — the same
pixels either way — and its job is to keep a small update from costing a full frame over the wire.

## Governing decisions

- [**ADR-0003 — damage model: incremental bounds**](../../adr/0003-damage-model-incremental-bounds.md)
  — line + column spans at Alacritty's `LineDamageBounds` grain, **ack-gated** reset, and a
  *recorded* (not diff-detected) scroll op. Also why not Mosh's baseline diff or wezterm's per-line
  seqno
- `docs/architecture.md` §Cadence holds the consumer-facing protocol the ack belongs to

## Design model

- **Accumulates from the last ack.** `line_damage: Vec<LineBounds>`, one entry per row, widened by
  `expand`; `reset_damage()` **is** the ack and clears them. Nothing here is time-based.
- **"Undamaged" is encoded as `left > right`.** An untouched line can therefore never report as
  damaged, and the first `expand` sets a real span without needing a sentinel or an `Option`.
- **`Full` is the collapse** — flood, resize, alt-screen switch. Degrading to all-rows-dirty is a
  deliberate outcome, not a failure.
- **The scroll op is recorded, not detected.** `ScrollOp { top, bottom, count }` is written by the
  engine that performed the scroll, so the renderer moves rows instead of redrawing them. A
  diff-detector would have to *infer* it.
- **`damage()` is content-only; `frame_damage()` adds the caret.** A pure caret move changes no cell
  content, so the flow-control primitive must not see it — but a cell-invert caret has to clear its
  old cell and ink the new one. `frame_damage` folds both cursor cells in, and only when the caret
  moved, so an idle frame stays empty.
- **The ack defines "old".** `reset_damage` advances `prev_cursor`, so what counts as the caret's
  previous cell is a function of the consumer's acknowledgement — not of wall time, and not of the
  previous call.
- **A frozen viewport reports nothing.** While [viewport](viewport.md) is scrolled up, `damage()`
  returns an *empty* `Partial`: changes the user cannot see are not damage. This is the one rule
  these two territories share, and it lives here because it is a statement about what damage *means*.

## Code

- `justerm-core/src/damage.rs` — `LineDamage`, `ScrollOp`, `TermDamage`, `LineBounds` (`expand`,
  `undamaged`, `fully_damaged`, `is_damaged`, `reset`, `span`)
- `justerm-core/src/term.rs` — `Term::damage`, `Term::frame_damage`, `Term::reset_damage`,
  `Term::mark_fully_damaged`, `Term::damage_span`

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row is pinned to a `file:line`
at a recorded SHA; a paraphrase drops the pin).

- [Damage / dirty tracking](../../agents/reference-facts.md#damage--dirty-tracking-536-verified-2026-07-28)
  — the headline is that **justerm's granularity is the outlier and nothing upstream can supply a
  bound for it**: xterm.js is row-granular with no column axis, ghostty is a per-row bool, and
  alacritty has column bounds only because it has no print-site bound at all (it brackets a line via
  the old and new cursor points). It also carries the rule justerm clamps toward — ghostty's *"dirty
  tracking may have false positives but should never have false negatives"*

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [viewport](viewport.md) — its scroll position gates whether damage is reported at all
- [caret report](caret-report.md) — the old+new fold lives in this code but the rule is about the
  caret; changing either side produces ghosting
- [frame](frame.md) — damage and the scroll op are frame fields, so their shape is a wire question
- **renderer** *(no note yet)* — the family renderer re-packs every frame and diffs, so it does
  **not** consume incremental damage the way an incremental-repaint renderer would. Damage's
  efficiency argument is aimed at the *wire*, not at that renderer

## Known holes / open

- **`damage()` vs `frame_damage()` is a public/private split with no record.** One is the public
  flow-control primitive, the other frame-internal, and the reason — content-only must stay
  content-only — survives in a doc comment.
- **No upstream bound to port.** §Reference behaviour records that no reference bounds or asserts its
  damage range, so justerm's clamping has no prior art to check against and only its own tests.
