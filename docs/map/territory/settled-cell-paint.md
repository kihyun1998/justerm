# Territory — settled-cell paint

## What it is

Colouring text **after it has been parsed**, by a rule the consumer owns. Two halves:
`changed_logical_lines` reports the soft-wrap-joined lines whose content changed since it last
answered, and `paint_logical_line` writes a colour reference into the cells behind spans of one of
them. The consumer runs its own matcher in between.

The forcing consumer is PenTerm's Output Rules (#967): user-written **JavaScript** regular
expressions, including lookbehind its own whole-word option generates, that must colour the same
lines on a justerm pane as on the xterm.js pane beside it. The `regex` crate has no lookbehind and a
Unicode `\w`, so matching in core would colour different lines. The matcher is policy; the query and
the write are mechanism, because both need the whole buffer (the wrap join, scrollback, absolute
coordinates) that a frame-mode consumer does not hold. PenTerm calls both from its backend (penterm
7dadc3c95 `src-tauri/src/engine/native.rs` — `changed_lines`, `paint_lines`).

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  places it in core and keeps the pattern out; it decides nothing about the shape.
- [ADR-0003 — damage model](../../adr/0003-damage-model-incremental-bounds.md) supplies the ack model
  the change record copies, without deciding this record.
- [ADR-0014](../../adr/0014-carry-interaction-overlays-in-the-frame.md) is why selection and search
  still draw over a paint: they are overlays and the paint is cell content.

## Design model

Status: no record; read from `paint.rs`, `term/paint.rs` and the hooks in `term.rs` at 7bb31f7, with
measurements from #967.

- **The colour lives in the cell, not in an overlay.** So it scrolls into scrollback with the text,
  survives reflow, is overwritten by later output exactly like an SGR colour, and is erased by an
  alt-screen redraw — none of which needed code. Only the channels a `PaintSpan` names change (`fg`,
  `bg`); other attributes are untouched.
- **The change record is a floor per buffer, single-reader, reset on answer.** `ChangeWatch` holds
  `changed_from` — the lowest absolute line changed since the last answer — and the answer walks from
  that line's logical start **to the end of the buffer**. The same ack model as damage (ADR-0003), in
  absolute coordinates instead of screen rows, because damage is reset per frame and withheld while
  scrolled up. Over-reporting is by design: painting what was already painted is idempotent.
- **A change is anything that alters a line's text, its logical boundaries, or its absolute
  position — colour included.** An application re-printing the same text in its own colour has
  overwritten the paint, so the line must come back. The hooks (all `content_changed_at` calls):
  - `damage_span` — every screen write a frame can see; the hook rides the funnel damage already is.
  - `mark_fully_damaged` — content changed with no span: resize, alt switch, RIS, `Engine::clear`.
    It also fires for view-only repaints (a user scroll), which only over-reports.
  - `shift_region` when it does not feed scrollback — lines move to other absolute indices. For
    `top > 0` `end_wrap(top - 1)` already damages a row above; the hook covers `top == 0` on the alt
    screen and IL/DL/RI at the top of the primary.
  - the scrollback-back unwrap in `shift_region` — the only scrollback mutation with no damage, and it
    splits a logical line.
  - the scrollback cap evicting the first row of a wrapped line (`linefeed_inner`) — the rows left
    behind are a new line with other text.
  - `lines_left_the_front` shifts both floors down (`watches_evict_oldest`), and a reflow carries the
    primary floor through `reflow_pane` as extra points (`ChangeWatch::points` / `reflowed`), the same
    idiom as tracked points (#691).
- **Three hooks were written and removed as unreachable, each measured by mutation** (all green with
  the hook off): the rows below a top-anchored sub-region's margin, `void_wrap_artefact_above`'s
  scrollback branch, and resetting the alt floor on entry (a stale value is clamped to `abs_floor`).
  **Validity conditions**: the first holds while a region is at least two rows and `shift_region`'s
  orphan clear stays; the second while every artefact void precedes a row-0 write.
- **The paint is content-addressed, not versioned.** A paint lands only where the logical line
  starting at the (eviction-rebased) index still reads exactly the reported text after the same `' '`
  trim; a reference that starts mid-line (the previous row wraps) is refused. No epoch is needed — a
  line that reads the same text is a correct target wherever it is. A refused reference comes back
  through the query: every answer since the last reflow is re-reported after one, because paints from
  several answers are in flight at once over IPC (a lens pass on #967 caught the first version
  covering only the latest answer).
- **Which buffer is an identity, not a floor.** `LineRef::alt` names the buffer, because primary and
  alt occupy the same absolute indices (#691's lesson). A primary paint lands while the alt screen is
  up; an alt paint after the alt screen closed is refused, as alt-scoped markers and tracked points die
  on leave.
- **Painting records no change**, or a consumer painting what it was told about would be told about
  it again forever. It damages through `damage_painted`, not `damage_span`; while scrolled up a visible
  line takes a full repaint, since partial damage is withheld then. `Engine::paint_logical_line` sets
  `frame_pending` when a paint lands.
- **A wide lead takes its spacer only when the spacer is really there**; a lead alone in the last
  column (an alt re-fit that cut the pair) is painted alone. The first version indexed past the row.
- **Span units are Unicode scalar values of the line's text.** A JavaScript consumer converts its
  UTF-16 match indices. `end` is clipped to the text's length, and an inverted span paints nothing.
- **Outside input.** Spans and their count come from the consumer; each is clipped to the line, so the
  cost is linear in what the consumer hands over. The query's answer is bounded by the buffer
  (scrollback + rows) — it can be the whole scrollback once (Known holes).

## Code

- `justerm-core/src/paint.rs` — `LineRef`, `ChangedLine`, `PaintSpan`
- `justerm-core/src/term/paint.rs` — `Term::changed_logical_lines`, `Term::paint_logical_line`, `content_changed_at`, `watches_evict_oldest`, `watch_mut`, `damage_painted`, `row_for_paint`, `ChangeWatch::points`, `ChangeWatch::reflowed`
- `justerm-core/src/term.rs` — `ChangeWatch`, and the hooks in `Term::damage_span`, `Term::mark_fully_damaged`, `Term::shift_region`, `Term::linefeed_inner`, `Term::lines_left_the_front`, `Term::resize` (via `reflow_pane`)
- `justerm-core/src/term/walk.rs` — `walk_logical_line`, shared with `viewport_logical_lines`
- `justerm-core/src/cell.rs` — `Cell::set_fg`, `Cell::set_bg`
- `justerm-core/src/lib.rs` — `Engine::changed_logical_lines`, `Engine::paint_logical_line`
- `justerm-core/tests/paint.rs`

## Reference behaviour

**None.** iTerm2's triggers are the prior art #967 names (they mutate the grid), but iTerm2 is not in
the pinned reference set and was not read — unconfirmed, not absent. xterm.js has no cell-paint API;
its closest analogue is decorations, which are overlays.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the query walks with
  `abs_floor`; the paint floors by the buffer its reference names.
- [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md) — both halves trim with
  `' '` only, and the paint compares after the same trim the query produced.
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — the lead takes
  its spacer only when the spacer is there (above).
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — the change record rides damage, so a destruction site that bypassed damage would bypass the
  record too; none is known today.
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — `LineRef` carries `evicted_total` and the `alt` identity out; on return the paint rebases by
  eviction and covers the non-uniform moves by refusing unless the line still reads the reported text.

## Blast radius

- [damage](damage.md) — `damage_span` and `mark_fully_damaged` also move the change floor; a new
  write site that damages is covered, one that moves or rewrites text without damaging is not.
- [logical lines](logical-lines.md) — the query and `viewport_logical_lines` build text through the
  same `walk_logical_line`; a change to what it visits changes both, and the guard's comparison.
- [reflow](reflow.md) — the primary floor rides `reflow_pane`'s points; a reflow change that drops
  extra points loses the floor.
- [marker](marker.md) — `LineRef::evicted_total` is the same count `MarkerIndex` reports; a change to
  what `lines_left_the_front` counts moves both.
- [alt screen](alt-screen.md) — `LineRef::alt` assumes alt lines die on leave; a change to alt-buffer
  lifetime changes which paints are refused.
- [published surface](published-surface.md) — three published structs, each with its #844 reason.

## Known holes / open

- **Measured on a release build, 2026-09-28, this workstation** (5 MiB `flood_input`, 64 KiB chunks,
  asking after each): feed alone 45.8 MiB/s, feed plus the query 33.4 MiB/s — the cost is building one
  `String` per reported line. The write-path hook's own cost is inside run-to-run noise (98–103 ms
  without it against 94–102 ms with it). Coverage: all 116,509 lines reported whole, 75 re-reports at
  chunk boundaries; the positive control (eviction shift removed) reported 106,477 missing.
- **The first resize after a long run re-reports everything answered since the previous resize** — up
  to the whole scrollback once. Correct, and bounded by the buffer the reflow already walks.
- **A lead standing mid-row with no spacer** would be painted alone by the pair predicate, where a
  "next column exists" predicate would paint its neighbour. No producer of that state is known, so the
  two are indistinguishable today (mutation-measured: both green).
- **The frame ordering is the consumer's.** A matched line shows uncoloured until its paint lands; the
  engine holds no frames, so holding or re-sending one is a cadence decision in the consumer.
- **No binding.** Neither half is reachable from `justerm-wasm-decode` or `justerm-web`; the one
  consumer calls the engine from its backend.
