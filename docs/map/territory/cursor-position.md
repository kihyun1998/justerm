# Territory — cursor position

## What it is

Where the next glyph lands: `row`, `col`, and the deferred-wrap state that makes the last column
behave. Purely engine-internal — a consumer never sets it, and what it *sees* of the cursor is a
different territory ([caret report](caret-report.md)).

## Governing decisions

**None** — and for the deferred wrap that is now a statement about *records*, not about the rule.

The position rules — clamping, the deferred wrap, the alt-screen save/restore pairing — are governed
by no ADR. `docs/architecture.md` §"Hidden VT state" describes the deferred wrap as a hazard to
model, which is a warning rather than a decision.

Since #848 the deferred wrap's **ownership + lifecycle** does have a single home: the doc-comment on
`Cursor::pending_wrap`, which names its four site-classes (armed by the print path, *consumed* by the
wrap machinery, cleared by every verb that acts on the position bar one that finds no move to make,
restored by DECRC). It is deliberately a doc-comment and not a record: the rule is **derived** from
the flag's one-sentence meaning — *the cursor is logically one past the column it sits on* — rather
than chosen between alternatives, and a record's job is to hold a choice. Clamping and the
save/restore pairing still have neither.

## Design model

Read out of the source; there is no record to read instead.

- **`pending_wrap` is the deferred last-column wrap** (xterm's *wrapnext*). A print that fills the
  last column leaves the cursor where it is and defers the wrap to the *next* print. **Eager wrapping
  here is the classic off-by-one that shifts every subsequent line**, which is why the flag exists at
  all rather than the cursor simply advancing.
- **Relative vertical motion stops at a scroll margin from its inner side** (#898): `move_up` stops
  at the top margin when the cursor is at or below it, `move_down` at the bottom margin when it is at
  or above it, and each is bounded only by the screen edge from the other side — so a cursor below
  the region moving up crosses the bottom margin and stops at the top one. CUU, CUD, VT52
  `ESC A` / `ESC B`, and CNL / CPL all go through these two. The two movers were screen-bounded
  until #898, with no record choosing that; the rule is **derived** — ADR-0004's spec-over-omission
  tie-break applied to the tally linked under *Reference behaviour* — while putting the change into
  #898 rather than a slice of its own was the **maintainer's scope call** (2026-09-14), made on that
  tally. **VPR does not go through them**: it is a positioning verb sharing `goto`'s row bounds
  (`Term::addressable_rows`), and routing it through `move_down` is what #898 first got wrong.
- **Position is clamped on set**, to `rows-1` / `cols-1` — so an out-of-range addressing sequence
  yields a degenerate position rather than a panic or an out-of-bounds write.
- **Two cursors exist.** `cursor` and `saved_cursor`, the latter written on alt-screen enter
  (DEC 1049) and restored on leave. The pairing is what makes a full-screen application's excursion
  transparent to the shell underneath it.
- **Origin mode (DECOM) makes addressing relative** to the scroll region's top margin, and clamps to
  it — so the same escape sequence means different absolute rows depending on a mode set earlier.
- Reverse wraparound (DEC ?45) does **two** things to a step back, and the second is easy to miss:
  at column 0 of a soft-wrapped row it moves back to the end of the previous row (soft wraps only —
  which is xterm's rule as well as this engine's, `!LineTstWrapped(ld)` at `cursor.c:178`), and at a
  **parked** cursor it spends the deferred wrap as the first unit of the move, so the cursor does not
  move at all (#80). **Both verbs take that step — `BS` and `CSI D`, through `Term::step_back`
  (#873)**, the second applying it once per unit of its count; it was `BS` only until then, on a
  comment that recorded an observation about xterm.js rather than a decision. The decision is the maintainer's (2026-09-08, theirs to reverse); the tally and the reach
  measurement behind it are on `justerm-core/tests/reverse_wrap.rs::cursor_left_spends_a_park`.
  `CSI n D` under `?45` is `n` applications of that step — xterm's shape literally, one
  `CursorBack` looping one unit per step (`cursor.c:160-190`) — so `CSI 3 D` from a park moves two
  and a walk at column 0 costs one of the three. Off the mode it is one saturating subtraction.
  **Both halves are gated on `?45` *and* `?7h`**, which the spend had and the walk did not until
  #873's follow-up: xterm reaches both arms through one `rev`, so `:165` is as dead under `?7l`
  as `:153` is. And **the walk leaves the wrap link alone** — it used to clear it, by writing the
  row directly rather than through `Term::end_wrap`, so it escaped that function's per-verb table
  *and* its damage obligation; the flag is that table's to decide, not this rule's. The second is gated on `?45` **and** `?7h`, and the trap is that
  xterm's spend site does not look like it: `cursor.c:153` reads `(rev || rev2) && screen->do_wrap`,
  but `rev` is `((flags & WRAP_MASK) == WRAP_MASK)` with `WRAP_MASK (REVERSEWRAP | WRAPAROUND)`
  (`:123-127`) — the mode name hides an autowrap requirement. ghostty gates earlier and plainly
  (`Terminal.zig:1756`). Under `?7l` all three references spend the park by *moving*, so the park
  #869 arms there is not this rule's to consume. Without the spend a parked and an unparked backspace
  land in the same place, which is the sharper statement of the defect than "one column off".
- **A restored deferred wrap settles where it is not at the last column** (#848). `resize` applies
  the translation to the live cursor; the two saved slots are not reflowed (`decsc` is untouched by
  resize and clamped at DECRC, the alt slot is copied whole), so the repair runs at the restore —
  where ghostty puts it for its reflowed saved cursor (`terminal/Screen.zig:2094`). Measured before
  the fix: 4 columns, `abcd`, `DECSC`, `resize(8, 3)`, `DECRC` left the flag armed at column 3 of an
  8-column grid, and the next print wrapped instead of landing at column 4.
- **Under `?7l` the print path is the only place DECAWM is tested** (#869). The park is armed
  unconditionally, and `write_glyph`'s consume guard spends it in place instead of wrapping —
  deleting that guard does not regress an edge case, it wraps with autowrap disabled
  (`decawm.rs::autowrap_off_overwrites_the_last_column` guards the guard). All four references print
  in place there; the reference rows are in `docs/agents/reference-facts.md`. #848 widened a
  pre-existing gap here: until then `put_tab` cleared the flag, so `abc` + `?7l` + `HT` + `X` printed
  in place by accident.
- **A wrap is claimed only if a next row exists** — parked below a DECSTBM region on the last row,
  `wrapline` advances nothing, so a soft-wrap flag set there would be permanently false and survive
  into reverse wraparound, reflow and every text reader. The narrow print path was the one caller
  that committed without asking `wrapline_advances` (found by #540's completeness pass, where a
  row-shift verb inherited the bogus flag and merged two logical lines).

## Code

- `justerm-core/src/cursor.rs` — `Cursor` (`row`, `col`, `pending_wrap`), `Cursor::point` /
  `Cursor::set_point`
- `justerm-core/src/term.rs` — `Term::write_glyph` (sets `pending_wrap`), `Term::backspace`,
  the DECOM / DECAWM / reverse-wraparound mode flags, alt-screen enter/leave save-restore

## Reference behaviour

**One axis, measured; the rest still prose.** `architecture.md` describes the deferred-wrap model as
matching xterm's behaviour, and for most of it that is still prose that was never grepped against a
pinned tree — the single most consequential positional rule here.

- [Backward tabulation, and who clears the deferred wrap](../../agents/reference-facts.md#backward-tabulation-and-who-clears-the-deferred-wrap-826-verified-2026-09-02)
  — **which verbs reset the flag**, measured across all four references by #826. The answer is that
  justerm is the outlier: no reference clears it in CBT, and xterm does not normally clear it in the
  *forward* tab either — its one `ResetWrap` there is gated on the `curses` resource, off by default.
  So the "matches xterm" prose is now known to be wrong on at least this axis, in justerm's favour
  by its own coherence argument and not by the reference's. Every horizontal-positioning verb here
  clears the flag, so a back-tab that did not would be the sole exception — and would reproduce the
  bug #826 fixes from the other side. The two tests that pin justerm's side:
  `back_tab_with_no_stops_lands_at_column_one` (the 3/4 clamp) and
  `back_tab_on_a_full_row_prints_where_it_landed_not_on_the_next_row` (the divergence as
  behaviour). The outer loop breaks at column zero, bounding the work by the grid rather than the
  parameter — defensive only, since `vte` saturates a parameter at `u16::MAX`, so no test can redden
  it. **If DECSLRM ever lands, `put_back_tab` is a site**: xterm and ghostty clamp a back-tab to the
  left margin under origin mode, which reduces to column zero only because there is no DECSLRM.

- [Forward tabulation at the right edge, and the deferred wrap](../../agents/reference-facts.md#forward-tabulation-at-the-right-edge-and-the-deferred-wrap-848-verified-2026-09-03)
  — **whether `HT` at the last column keeps the flag**, measured across all four by #848. 3-1 for
  keeping; all four preserve the character, where justerm destroyed it.

- [Where a combining mark attaches, and the four mechanisms for locating it](../../agents/reference-facts.md#where-a-combining-mark-attaches-and-the-four-mechanisms-for-locating-it-865-verified-2026-09-07)
  — **when `write_glyph` arms it**, measured across all four by #865, and the axis this paragraph
  used to name as unpinned. justerm used to fold `DECAWM` into the *arm*
  (`pending_wrap = self.autowrap`) where the other three arm unconditionally and test the mode at
  the *consume* site, and the consequence was that the flag **could not express a pin under `?7l`**
  — a print that filled the last column and a cursor that merely moved onto it were identical in
  every cursor field. **#869 removed that fold**, so the flag now answers the question again and
  this engine matches the other three on the arm. Two readers had already paid for the gap: the
  combining-mark attach point (#865) and the OSC 133 command-text bound (#869).

  A consequence worth carrying, because it moved a decision: with the arm unconditional the flag
  now *survives* `?7l`, which reverses #848's choice on that axis. #848's ground was explicitly
  local — *a flag outliving `?7l` contradicts the site that wrote it* — and that site is what
  changed.

- [Relative vertical motion against the margins, and the two verbs composed from it](../../agents/reference-facts.md#relative-vertical-motion-against-the-margins-and-the-two-verbs-composed-from-it-898-verified-2026-09-14)
  — **where CUU / CUD stop inside a region**, measured across all four by #898. 3-1 for the margin
  clamp, alacritty the outlier, and justerm was on alacritty's side until that change.

How it survives a resize remains unpinned.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — the caret is a span, and an application can park the cursor on a wide glyph's trailing spacer
  with an ordinary `CUB` / `CHA`, so the position this territory owns can name half a glyph (#454)

## Blast radius

- [soft wrap](soft-wrap.md) — `pending_wrap` is the **entry condition** for the wrap path. Changing
  when it is set changes which rows carry a wrap link
- [damage](damage.md) — a pure cursor move changes no cell content, so the content-only damage model
  misses it by design and the frame producer folds the old and current cells in
- [pen](pen.md) — they travel together in the same struct and are written by the same verbs, but the
  coupling is only that: a position change does not change appearance

## Known holes / open

- **Zero governing records**, for rules whose failure mode is a silently shifted screen. Narrowed
  by #848 but not closed: the deferred wrap now has a stated lifecycle, clamping and the
  alt-screen save/restore pairing still have nothing. #898 measured the relative-motion half of
  clamping (the margin stops above) but wrote no record — it is a derivation, not a choice.
- ~~**The deferred-wrap rule survives only as a field comment.**~~ **Closed by #848 — the field
  comment is now the owner rather than a remnant**, and it states the rule the 22 cursor-movers are
  measured against. What the hole predicted had already happened three times over: `put_tab` cleared
  the flag where nothing moved and destroyed a character, `linefeed` and `reverse_index` left it
  armed and advanced a row too far. The residual risk is unchanged in *kind* — a new cursor-moving
  verb that never reads the doc-comment owes a clear and nothing enforces it — which is why the
  comment carries the grep that produced the census.
- **DECOM's interaction with the clamp is unspecified** in any artifact: origin mode clamps to the
  region, `set_point` clamps to the screen, and no document states which applies when both do.
  One instance measured by #898's refuter pass and left alone: under DECOM, DECRC restores a row
  clamped only to the screen (`Term::restore_cursor`), where xterm routes it through `CursorSet` and
  caps it at the bottom margin (`cursor.c:484-490` @ `6380a3e`); ghostty and alacritty clamp to the
  screen as justerm does. Every relative move afterwards then starts from a different row.
