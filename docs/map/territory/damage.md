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
- **A reported scroll never exceeds its region's height.** Repeated scrolls of one region
  accumulate into a single op, and a flood accumulates far past the region — 32 KB of newlines in
  one `feed()`. Past the height the value stops meaning anything (every source row is already
  outside the region) and starts being unrepresentable: `count` is `isize` here and `i16` on the
  wire, and the overflow arrived as a scroll in the *opposite* direction (#661). The cap is applied
  where the op is **read**, so the accumulator stays exact and a region that scrolls far and returns
  still reports its true small net. Both references that state a quantity at their own scroll sites
  clamp to the same bound (alacritty `term/mod.rs:773`, ghostty `Terminal.zig:2703`). The bound's other half is the wire's, not the region's — see
  [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md).
- **`damage_span` clamps both columns to the last column and asserts in debug (#536).** Of its
  fourteen call sites ten derive the bound from a cursor column or `cols`; **four derive it from a
  wide pair's width** — `write_glyph`'s `col + width - 1` (which had no guard), `promote_cluster_to_wide`'s
  `col + 1` (its own `col + 1 >= cols` early return), `demote_cluster_to_narrow`'s
  `(col + 1).min(cols - 1)`, and `relocate_cluster_wide`'s literal `(0, 1)`, valid only because
  `MIN_COLUMNS = 2` (#547). No reference has this shape to port a clamp from: alacritty computes
  damage ranges from a column or `columns()` only (`term/mod.rs:1406`, `:1649` @ `852e971`) and its
  print path records none, bracketing the line by the previous and current cursor points; xterm.js
  tracks whole rows (`markDirty(y)`), ghostty a per-row `dirty: bool`; alacritty's
  `LineDamageBounds::expand`, which `LineBounds::expand` copies, is equally unguarded. The two
  halves do different jobs. The **assert is the detector**: an out-of-range bound is stored
  silently and detonates when `frame()` slices the row, so the trace accuses the reader — the delay
  #536 was filed about; an injected off-by-one moved the panic from `frame()`'s slice to the assert.
  The **clamp is the release backstop**, toward a false positive: a panic crosses into the
  consumer's process, while over-damaging repaints an unchanged cell. `left` is the axis that can
  lose a cell — an `expand` with `left > right` on a clean line leaves it reading undamaged
  (`left = cols, right = 0`; `is_damaged()` is `left <= right`) and drops the span; unreachable
  from the column-derived sites, guarded because it is the failure the rule forbids. `row` is left
  to panic, which is not in tension with `frame_damage` clamping `prev_cursor.0.min(rows - 1)`: a
  stale remembered coordinate clamped repaints the nearest surviving cell (a false positive), while
  a live computed row clamped would damage a different line than the one that changed (a false
  negative).
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

- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — `ScrollOp::count` is `isize` here and `i16` on the wire. The accumulator that produces it lives
  in this territory, so the bound has to as well: `encode` cannot refuse a value, and `decode`
  cannot tell a wrapped one from a real one (#661)
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — the cursor fold expands **one column** at each end, but a caret standing on a wide glyph's
  trailing spacer is drawn from the *lead*, one column further left than the frame ever names. The
  fold is the one place this territory produces a span rather than consuming one, which is why the
  pair rule reaches it at all (#826)

## Blast radius

- [viewport](viewport.md) — its scroll position gates whether damage is reported at all
- [caret report](caret-report.md) — the old+new fold lives in this code but the rule is about the
  caret; changing either side produces ghosting
- [frame](frame.md) — damage and the scroll op are frame fields, so their shape is a wire question
- [GPU upload](gpu-upload.md) — the family renderer re-packs every frame and diffs, so it does
  **not** consume incremental damage the way an incremental-repaint renderer would. Damage's
  efficiency argument is aimed at the *wire*, not at that renderer

## Known holes / open

- **`damage()` vs `frame_damage()` is a public/private split with no record.** One is the public
  flow-control primitive, the other frame-internal, and the reason — content-only must stay
  content-only — survives in a doc comment.
- **No upstream bound to port.** §Reference behaviour records that no reference bounds or asserts its
  damage range, so justerm's clamping has no prior art to check against and only its own tests.
