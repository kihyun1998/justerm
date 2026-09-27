# Territory — settled-cell paint

## What it is

Colouring text **after it has been parsed**, by a rule the consumer owns. Two halves:
`changed_logical_lines` reports the soft-wrap-joined lines whose content changed since it last
answered, and `paint_logical_line` writes a colour reference into the cells behind a span of one of
them. The consumer runs its own matcher in between.

The forcing consumer is PenTerm's Output Rules (#967): user-written **JavaScript** regular
expressions, including lookbehind its own whole-word option generates, that must colour the same
lines on a justerm pane as on the xterm.js pane beside it. The `regex` crate has no lookbehind and a
Unicode `\w`, so matching in core would colour different lines. The matcher is policy; the query
and the write are mechanism, because both need the whole buffer (the wrap join, scrollback,
absolute coordinates) that a frame-mode consumer does not hold.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  places it in core and keeps the pattern out; it decides nothing about the shape

## Design model

- **The colour lives in the cell, not in an overlay.** So it scrolls into scrollback with the text,
  survives reflow, is overwritten by later output exactly like an SGR colour, and is erased by an
  alt-screen redraw — none of which needed code. The selection and the search highlights still draw
  over it because they are overlays (ADR-0014) and paint touches none of them. There is no second
  attribute layer to survive anything.
- **The change record is a floor per buffer, single-reader, reset on answer.** `ChangeWatch`
  holds `changed_from` — the lowest absolute line changed since the last answer — and the answer
  walks from that line's logical start **to the end of the buffer**. The same ack model as damage
  (ADR-0003), in absolute coordinates instead of screen rows, because damage is reset per frame and
  withheld while scrolled up. Over-reporting is by design: an unchanged line below the floor is
  reported too, and that is safe because painting what was already painted is idempotent.
- **A change is anything that alters a line's text, its logical boundaries, or its absolute
  position — colour included.** An application re-printing the same text in its own colour has
  overwritten the paint, so the line must come back for the rule to re-apply. Where the hooks sit,
  and why each one is needed:
  - `damage_span` — every screen write a frame can see; the hook rides the funnel damage already is
  - `mark_fully_damaged` — content changed with no span: resize, alt switch, RIS, `Engine::clear`.
    It also fires for view-only repaints (a user scroll), which only over-reports
  - `shift_region` when it does not feed scrollback — the lines move to other absolute indices. For
    `top > 0` `end_wrap(top - 1)` already damages a row above; the hook is what covers `top == 0`
    on the alt screen (a full-screen scroll) and IL/DL/RI at the top of the primary
  - the scrollback-back unwrap in `shift_region` — the only scrollback mutation with no damage, and
    it splits a logical line
  - the scrollback cap evicting the first row of a wrapped line — the rows left behind are a new
    line with other text
  - `lines_left_the_front` shifts both floors down, and a reflow carries the primary floor through
    `reflow_pane` as extra points, the same idiom as tracked points (#691)
- **Three hooks were written and removed as unreachable, each measured by mutation (all green with
  the hook off):** the rows below a top-anchored sub-region's margin (`end_wrap(bottom - 1)` or the
  glyph a wrap-serving shift prints always damages a row above them; DECSTBM refuses a region under
  two rows), `void_wrap_artefact_above`'s scrollback branch (the write to row 0 that follows walks
  up into the scrollback line), and resetting the alt floor on entry (a stale value is clamped to
  `abs_floor`). **Validity conditions**: the first holds while a region is at least two rows and
  `shift_region`'s orphan clear stays; the second while every artefact void precedes a row-0 write.
- **The paint is content-addressed, not versioned.** The guard is the line's current text: a paint
  lands only where the logical line starting at the (eviction-rebased) index still reads exactly
  what the consumer matched. So no epoch is needed — a line that reads the same text is a correct
  target wherever it is. A reference that stops being valid (rewritten, moved by a resize or a
  region scroll) is refused, and the line comes back through the query: every answer since the last
  reflow is re-reported after one, because paints from several answers are in flight at once over
  IPC (a lens pass on #967 caught the first version covering only the latest answer).
- **Which buffer is an identity, not a floor.** `LineRef::alt` names the buffer, because the primary
  and alt grids occupy the same absolute indices — the lesson
  [the alt floor note](../invariant/alt-screen-buffer-floor.md) records from #691. A primary paint
  lands while the alt screen is up; an alt paint after the alt screen closed is refused, as the
  alt-scoped markers and tracked points die on leave.
- **Painting records no change**, or a consumer that paints what it was told about would be told
  about it again forever. It damages through `damage_painted`, not `damage_span`; while scrolled up
  a visible line takes a full repaint, since partial damage is withheld then.
- **Span units are Unicode scalar values of the line's text.** A JavaScript consumer converts its
  UTF-16 match indices.

## Code

- `justerm-core/src/paint.rs` — `LineRef`, `ChangedLine`, `PaintSpan`
- `justerm-core/src/term/paint.rs` — `Term::changed_logical_lines`, `Term::paint_logical_line`,
  `content_changed_at`, `watches_evict_oldest`, `damage_painted`, `row_for_paint`, and
  `ChangeWatch::points` / `ChangeWatch::reflowed`
- `justerm-core/src/term.rs` — `ChangeWatch`, and the hooks in `damage_span`, `mark_fully_damaged`,
  `shift_region`, `linefeed_inner` (the cap eviction), `lines_left_the_front` and `resize`
- `justerm-core/src/term/walk.rs` — `walk_logical_line`, shared with `viewport_logical_lines`
- `justerm-core/src/cell.rs` — `Cell::set_fg`
- `justerm-core/src/lib.rs` — `Engine::changed_logical_lines`, `Engine::paint_logical_line`

## Reference behaviour

**None.**

- iTerm2's triggers are the prior art #967 names (they mutate the grid), but iTerm2 is not in the
  pinned reference set and was not read — unconfirmed, not absent. xterm.js has no cell-paint API;
  its closest analogue is decorations, which are overlays

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the query walks with
  `abs_floor`; the paint floors by the buffer its reference names
- [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md) — both halves trim with
  `' '` only, and the paint compares after the same trim the query produced
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — a painted
  lead takes its spacer only when the spacer is really there; a lead alone in the last column (an
  alt re-fit that cut the pair) is painted alone. The first version indexed past the row
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — the change record rides damage, so a destruction site that bypassed damage would also bypass
  the record; none is known today

## Blast radius

- [damage](damage.md) — `damage_span` and `mark_fully_damaged` now also move the change floor; a new
  write site that damages is covered, one that moves or rewrites text without damaging is not
- [logical lines](logical-lines.md) — the query and `viewport_logical_lines` build text through the
  same `walk_logical_line`
- [reflow](reflow.md) — the primary floor rides `reflow_pane`'s points
- [marker](marker.md) — `LineRef::evicted_total` is the same count `MarkerIndex` reports
- [published surface](published-surface.md) — three new published structs, each with its #844 reason

## Known holes

- **Measured on a release build, 2026-09-28, this workstation** (5 MiB `flood_input`, 64 KiB
  chunks, asking after each): feed alone 45.8 MiB/s, feed plus the query 33.4 MiB/s — the cost is
  building one `String` per reported line. The write-path hook's own cost is inside run-to-run noise
  (98–103 ms with it removed against 94–102 ms with it). Coverage: all 116,509 lines reported whole,
  75 re-reports at chunk boundaries; the positive control (eviction shift removed) reported 106,477
  missing, so the instrument sees a miss.
- **The first resize after a long run re-reports everything answered since the previous resize** —
  up to the whole scrollback once. Correct, and bounded by the buffer the reflow already walks.
- **A lead standing mid-row with no spacer** would be painted alone by the pair predicate, where a
  "next column exists" predicate would paint its neighbour. No producer of that state is known, so
  the two are indistinguishable today (mutation-measured: both green).
- **The frame ordering is the consumer's.** A matched line is shown uncoloured until its paint
  lands; the engine holds no frames, so holding or re-sending one is a cadence decision in the
  consumer that owns the frame loop.
- **No binding.** Neither half is reachable from `justerm-wasm-decode` or `justerm-web`; the one
  consumer calls the engine from its backend.
