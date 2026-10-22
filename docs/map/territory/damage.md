# Territory — damage

## What it is

What changed since the consumer last acknowledged a frame — line ranges each carrying a column span,
plus a first-class scroll op — and the wider signal that says whether a frame is owed at all
(`Engine::frame_pending`). It is an **efficiency** axis, not a correctness one: the same pixels either
way. Its job is to keep a small update from costing a full frame over the wire, and the ack that
resets it is the consumer's cadence.

## Governing decisions

- [ADR-0003 — damage model: incremental bounds](../../adr/0003-damage-model-incremental-bounds.md) —
  line + column spans at alacritty's `LineDamageBounds` grain, **ack-gated** reset, and a *recorded*
  (not diff-detected) scroll op; also why not Mosh's baseline diff or wezterm's per-line seqno.
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  — why an overlay change is never damage, which is what makes `frame_pending` wider than `damage()`.
- `docs/architecture.md` §Damage and §Cadence hold the consumer-facing protocol the ack belongs to.

## Design model

From ADR-0003, `docs/architecture.md`, and the source.

- **Accumulates from the last ack.** `line_damage: Vec<LineBounds>`, one per row, widened by
  `LineBounds::expand`; `Term::reset_damage` **is** the ack and clears it, the scroll op and
  `full_damage`. Nothing is time-based (`cadence.rs` — `damage_accumulates_until_ack`,
  `intermediate_writes_collapse_into_one_diff`).
- **"Undamaged" is `left > right`** (`LineBounds::undamaged` sets `left = cols, right = 0`), so an
  untouched line never reports and the first `expand` sets a real span without a sentinel.
- **`Full` is the collapse** — resize, alt-screen switch, `clear`, a user scroll, a cap eviction while
  scrolled up, and two scrolls of different regions inside one ack window (`Term::record_scroll`
  cannot express both as one op, so it drops the op and goes full rather than lose one).
  Degrading to all-rows-dirty is a deliberate outcome.
- **The scroll op is recorded, not detected.** `ScrollOp { top, bottom, count }` is written by the
  code that performed the scroll — `Term::shift_region` for region shifts, and `Term::linefeed_inner`
  directly for the full-screen accrual, the one row shift not routed through `shift_region`.
  `record_scroll` rotates `line_damage` with the content and marks the exposed line fully damaged, so
  it runs **before** any seam clear in `shift_region`, or damage recorded first lands on the wrong row
  (ADR-0025).
- **A reported scroll never exceeds its region's height** (#661). Repeated scrolls of one region
  accumulate, and a flood accumulates far past the region (32 KB of newlines in one `feed`). Past the
  height the value means nothing, and past `i16` the wire wrapped it into a scroll in the *opposite*
  direction. `cap_scroll` bounds it at the region height and `MAX_SCROLL_COUNT` where the op is
  **read** (`Term::scroll_delta`), so the accumulator stays exact and a region that scrolls far and
  returns reports its true small net. alacritty and ghostty clamp to the same bound at their scroll
  sites (`term/mod.rs:773`, `Terminal.zig:2703`, recorded on #661).
- **`damage()` is content-only; `frame_damage()` adds the caret** (#38). A pure caret move changes no
  cell, so the public flow-control answer must not see it, but a cell-invert caret has to clear its
  old cell and ink the new one. `Term::frame_damage` folds both cursor cells in only when the caret
  moved, so an idle frame stays empty (alacritty's `last_cursor`). `reset_damage` advances
  `prev_cursor`, so "the caret's old cell" is a function of the ack, not of wall time.
- **`damage_span` clamps both columns to the last column and asserts in debug (#536).** Of its
  **fifteen** call sites, four derive the bound from a wide pair's width — `write_glyph`'s
  `col + width - 1`, `demote_cluster_to_narrow`'s `(col + 1).min(cols - 1)`,
  `promote_cluster_to_wide`'s `col + 1` (behind its own `col + 1 >= cols` return) and
  `relocate_cluster_wide`'s literal `(0, 1)`, valid only because `MIN_COLUMNS = 2` (#547); the rest
  derive it from a cursor column or `cols`. The **assert is the detector**: an out-of-range bound
  stored silently detonates later when `frame()` slices the row, so the trace accused the reader —
  the delay #536 was filed about. The **clamp is the release backstop**, toward a false positive: a
  panic crosses into the consumer's process, while over-damaging repaints an unchanged cell. `left`
  is the axis that could lose a cell (an `expand` with `left > right` on a clean line reads
  undamaged). `row` is left to panic: a live computed row clamped would damage a different line (a
  false negative), whereas `frame_damage` clamping the *remembered* `prev_cursor` only repaints the
  nearest surviving cell.
- **A frozen viewport reports nothing.** While [viewport](viewport.md) is scrolled up, `damage()` and
  `frame_damage()` return an empty `Partial` and `scroll_delta` is `None`; the bounds keep
  accumulating underneath. A scroll that moves the view sets `full_damage` (`cadence.rs` —
  `content_scroll_while_scrolled_up_is_invisible`, `user_scroll_marks_full_damage`).
- **Damage is not the answer to "is there a frame to send"** (#1043). That is
  `Engine::frame_pending`, raised by every content or geometry mutator, every viewport scroll, and
  every overlay change — selection calls, the two settings a selection resolves with, search
  highlights, the active match, markers (#1044) — and cleared by `reset_damage`. A fresh engine starts
  raised. `Session::pump` reads it, never `damage()`; the predicate swap is what
  `session.rs` — `output_while_scrolled_up_sends_a_frame_though_it_leaves_no_damage` reddens on. This
  is what retires a consumer's own dirty flag ([session](session.md)).
- **Damage also feeds the change record.** `damage_span` and `mark_fully_damaged` call
  `content_changed_at`, so a write that damages is reported to [settled-cell paint](settled-cell-paint.md)
  and one that changes text without damaging is not.

**Bounds.** `line_damage` is one `LineBounds` per screen row (`MAX_ROWS`); the op is one value. Nothing
grows with input volume.

## Code

- `justerm-core/src/damage.rs` — `LineDamage`, `ScrollOp`, `TermDamage`, `LineBounds`
  (`LineBounds::expand`, `LineBounds::undamaged`, `LineBounds::fully_damaged`,
  `LineBounds::is_damaged`, `LineBounds::reset`, `LineBounds::span`)
- `justerm-core/src/term.rs` — `Term::damage`, `Term::frame_damage`, `Term::reset_damage`,
  `Term::mark_fully_damaged`, `Term::damage_span`, `Term::record_scroll`, `Term::scroll_delta`,
  `cap_scroll`, `bounds_to_lines`, `Term::shift_region`
- `justerm-core/src/serialize.rs` — `MAX_SCROLL_COUNT`
- `justerm-core/src/lib.rs` — `Engine::damage`, `Engine::reset_damage`, `Engine::scroll_delta`,
  `Engine::mark_fully_damaged`, `Engine::frame_pending`
- `justerm-core/src/session.rs` — `Session::pump` (the reader of `frame_pending`)
- Tests: `justerm-core/tests/damage.rs`, `cadence.rs`, `wrap_flag_damage.rs`, `session.rs`

## Reference behaviour

- [Damage / dirty tracking](../../agents/reference-facts.md#damage--dirty-tracking-536-verified-2026-07-28)
  — justerm's column granularity is the outlier and nothing upstream supplies a bound for it: xterm.js
  is row-granular, ghostty a per-row bool, alacritty has column bounds only because its print path
  records none. It carries the rule justerm clamps toward — ghostty's *"dirty tracking may have false
  positives but should never have false negatives"*.
- [Scrolling a region by more than its own height](../../agents/reference-facts.md#scrolling-a-region-by-more-than-its-own-height-661-verified-2026-07-31)
  — the scroll-count cap.

## Cross-cutting invariants

- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — `ScrollOp::count` is `isize` here and `i16` on the wire; the accumulator lives here, so the bound
  has to as well — `encode` cannot refuse a value and `decode` cannot tell a wrapped one (#661).
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — the cursor fold
  expands one column at each end, but a caret on a wide glyph's spacer is drawn from the lead, one
  column further left than the frame names; the fold is the one place this territory produces a span
  (#826).

## Blast radius

- [viewport](viewport.md) — its scroll position decides whether damage is reported at all, and a
  move of the window is itself full damage.
- [settled-cell paint](settled-cell-paint.md) — its change record rides `damage_span` and
  `mark_fully_damaged`, so it assumes every text-changing write damages.
- [caret report](caret-report.md) — the old+new fold lives here but the rule is about the caret;
  changing either side ghosts a cell-invert caret.
- [frame](frame.md) · [wire format](wire-format.md) — damage kind, spans and the scroll op are frame
  fields, and the count must fit the wire's `i16`.
- [session](session.md) — `pump` sends a frame only when `frame_pending` is raised, so a mutator that
  forgets to raise it leaves the consumer stale.
- [soft wrap](soft-wrap.md) — seam clears in `shift_region` must run after `record_scroll`, or their
  damage lands on the wrong row.
- [GPU upload](gpu-upload.md) — the family renderer re-packs every frame and diffs, so it does not
  consume incremental damage; damage's efficiency argument is aimed at the wire, not that renderer.

## Known holes / open

- **`damage()` vs `frame_damage()` is a public/private split with no record.** The reason —
  content-only must stay content-only — survives in a doc-comment.
- **No upstream bound to port.** No reference bounds or asserts its damage range, so the clamping is
  checked only by justerm's own tests.
- **`frame_pending` is raised by hand at the end of each `Engine` mutator** (some two dozen sites in
  `lib.rs`); a new mutator that forgets it is caught by no compiler check.
