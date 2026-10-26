# Territory — cursor position

## What it is

Where the next glyph lands: `row`, `col`, and the deferred-wrap flag that makes the last column
behave — plus the rules every positioning verb applies to them (clamping, margins, origin mode,
reverse wraparound, save/restore). Purely engine-internal: a consumer never sets it, and what it
*sees* of the cursor is a different territory ([caret report](caret-report.md)).

## Governing decisions

**None** — and for the deferred wrap that is a statement about *records*, not about the rule.

Clamping, margins and the save/restore pairing are governed by no ADR. `docs/architecture.md`
§ Hidden VT state describes the deferred wrap as a hazard to model, which is a warning rather than a
decision. Since #848 the deferred wrap's ownership and lifecycle have a single home: the doc-comment
on `Cursor::pending_wrap`, which names its site-classes. It is deliberately a doc-comment and not a
record — the rule is derived from the flag's one-sentence meaning (*the cursor is logically one past
the column it sits on*), and per-verb clears are measured against the references rather than chosen,
while a record's job is to hold a choice. [ADR-0004](../../adr/0004-spec-faithful-when-alacritty-omits.md)
is the tie-breaker several entries below were derived under, not a record about position.

## Design model

Read out of the source and the issues cited; there is no record to read instead.

- **`pending_wrap` is the deferred last-column wrap** (xterm's *wrapnext*). A print that fills the last
  column leaves the cursor there and defers the wrap to the *next* print; eager wrapping is the
  classic off-by-one that shifts every subsequent line. Its site-classes (the `Cursor::pending_wrap`
  doc-comment is authoritative): **armed** unconditionally by the print path — `Term::write_glyph`,
  `Term::promote_cluster_to_wide`, `Term::relocate_cluster_wide`; **consumed** by `Term::wrapline`
  (performs the wrap) and by `Term::step_back` under `?45` (spends it as the first unit of a move);
  **translated** by `Term::resize`; **cleared** per verb; **restored** by `Term::restore_cursor` and
  `Term::restore_alt_cursor`, each followed by `Term::settle_restored_wrap`; and **read as a `+1`** by
  `Term::add_command_mark` for an exclusive command-text bound.
- **The clear is per-verb and not derivable (#848).** "A verb clears iff it acted" is false: `CUF` at
  the last column moves nothing and all four references clear anyway, while `HT` (and `CHT`, which
  repeats it) with no stop to move to keeps the flag 3-1. `IL`/`DL` clear (3-1, alacritty the
  outlier); `SU`/`SD` deliberately do not, because ghostty saves and restores the flag around them on
  purpose — a per-verb answer, not "row-shift verbs clear". `ICH`, `DCH`, `ECH`, `EL`, `ED` keep the
  park here where xterm clears in all seven of its `ResetWrap` sites; measured, not a defect on any
  measurement so far (`Cursor::pending_wrap` doc-comment). A grep for writes to `cursor.col` /
  `cursor.row` finds 20 functions and is blind to the row-shift and erase verbs. `CBT` clears where no
  reference does (#826) — justerm is the outlier by its own coherence argument: every other
  horizontal-positioning verb clears, so a back-tab that did not would be the sole exception and
  reproduce #826's bug from the other side.
- **Under `?7l` the park is armed anyway and spent in place where it is consumed (#869).**
  `write_glyph` tests DECAWM at the consume site, not the arm site — deleting that guard does not
  regress an edge case, it wraps with autowrap disabled (`autowrap_off_overwrites_the_last_column`
  guards it). All four references print in place there. Until #869 the arm folded the mode in
  (`pending_wrap = self.autowrap`), so the flag could not express a pin under `?7l` and two readers paid:
  the combining-mark attach point (#865) and the OSC 133 command-text bound. #848 had widened a
  pre-existing gap here: until then `put_tab` cleared the flag, so `abc` + `?7l` + `HT` + `X` printed
  in place by accident. DECAWM is also read by `step_back`, `relocate_cluster_wide` and the
  wide-glyph drop at the last column — the consume guard is the only place it decides a *wrap*.
- **A wrap is claimed only if a next row exists** — parked below a DECSTBM region on the last row,
  `wrapline` advances nothing, so a soft-wrap flag set there would be permanently false. The print path
  asks `Term::wrapline_advances` before `begin_wrap`; it was the one caller that did not until #540's
  completeness pass, where a row-shift verb inherited the bogus flag and merged two logical lines.
- **A restored deferred wrap settles where it is not at the last column** (#848). `resize` applies the
  translation to the live cursor; the two saved slots are not reflowed (`decsc` is untouched by resize
  and clamped at DECRC, the alt slot is reflowed by the primary pane only while alt is up), so the
  repair runs at restore — where ghostty puts it for its reflowed saved cursor. Measured before the
  fix: 4 columns, `abcd`, `DECSC`, `resize(8, 3)`, `DECRC` left the flag armed at column 3 of an
  8-column grid and the next print wrapped instead of landing at column 4.
- **Relative vertical motion stops at a scroll margin from its inner side** (#898): `Term::move_up`
  stops at the top margin when the cursor is at or below it, `Term::move_down` at the bottom margin
  when at or above it, each bounded only by the screen edge from the other side. CUU, CUD, VT52
  `ESC A`/`ESC B` and CNL/CPL go through these two. They were screen-bounded until #898 with no record
  choosing that; the rule is **derived** (ADR-0004 applied to a 3-1 tally), and putting it into #898
  rather than a slice of its own was the maintainer's scope call (2026-09-14). **VPR does not go
  through them**: it is a positioning verb sharing `goto`'s row bounds (`Term::addressable_rows`), and
  routing it through `move_down` is what #898 first got wrong.
- **Position is clamped on set** — `Term::goto`, `set_col`, `set_row`, `Cursor::set_point` clamp to
  `rows-1` / `cols-1`, so an out-of-range address yields a degenerate position, never a panic or an
  out-of-bounds write. The input is the parser's: a CSI parameter `vte` saturates at `u16::MAX`.
- **Origin mode (DECOM) makes addressing relative** to the top margin and clamps to the bottom margin
  (`addressable_rows`); setting it homes the cursor, resetting it leaves the cursor put. DECSC saves
  and DECRC restores it (ADR-0004).
- **Three cursor slots exist, not two.** `cursor` (live), `saved_cursor` (the alt-screen save,
  `?1048` and `?1049`) and `decsc` (a `SavedCursor`: position, pen, flag, DECOM, charsets — written by
  DECSC / SCOSC, restored by DECRC / SCORC, reset by DECSTR). Visibility is in neither save. The alt
  pairing is [alt screen](alt-screen.md)'s. A fourth position is not a slot: `primary_cursor_on_alt`
  records where the cursor was when the screen switched, for a resize on alt to keep that line (#1082).
- **Reverse wraparound (`?45`) does two things to a step back** (#80, #873). At column 0 of a
  soft-wrapped row it moves to the end of the previous row (soft wraps only — xterm's
  `!LineTstWrapped(ld)` too), and at a **parked** cursor it spends the deferred wrap as the first unit,
  so the cursor does not move. Both `BS` and `CSI D` take that step through `Term::step_back`, the
  second once per unit of its count — xterm's shape, one `CursorBack` looping per unit — so
  `CSI 3 D` from a park moves two. It was `BS` only until #873, on a comment recording an observation
  about xterm.js rather than a decision; the decision is the maintainer's (2026-09-08), with the tally
  and reach measurement on `justerm-core/tests/reverse_wrap.rs` (`cursor_left_spends_a_park`). Off the
  mode, `CSI D` is one saturating subtraction. **Both halves are gated on `?45` and `?7h`** — xterm's
  `rev` is `(flags & WRAP_MASK) == WRAP_MASK` with `WRAP_MASK = REVERSEWRAP | WRAPAROUND`, the mode name
  hiding an autowrap requirement; ghostty gates plainly. Under `?7l` all three references spend the
  park by *moving*. **The walk leaves the wrap link alone** — it used to clear it by writing the row
  directly, escaping `Term::end_wrap`'s per-verb table and its damage obligation ([soft wrap](soft-wrap.md)).
  Without the spend, a parked and an unparked backspace land in the same place.

## Code

- `justerm-core/src/cursor.rs` — `Cursor` (`row`, `col`, `pending_wrap` and its lifecycle doc-comment),
  `Cursor::point`, `Cursor::set_point`
- `justerm-core/src/term.rs` — `Term::write_glyph` (arms and consumes), `Term::wrapline`,
  `Term::wrapline_advances`, `Term::step_back`, `Term::backspace`, `Term::move_up`,
  `Term::move_down`, `Term::move_forward`, `Term::move_back`, `Term::set_col`, `Term::set_row`,
  `Term::goto`, `Term::addressable_rows`, `Term::vertical_position_relative`, `Term::carriage_return`,
  `Term::linefeed`, `Term::reverse_index`, `Term::put_tab`, `Term::put_back_tab`,
  `Term::insert_lines`, `Term::delete_lines`, `Term::save_cursor`, `Term::restore_cursor`,
  `Term::settle_restored_wrap`, `SavedCursor`; the fields `origin_mode`, `autowrap`,
  `reverse_wraparound`, `decsc`, `saved_cursor`
- `justerm-core/src/term/dispatch.rs` — `Term::set_dec_private_mode` (DECOM, DECAWM, `?45`)

## Reference behaviour

- [Backward tabulation, and who clears the deferred wrap](../../agents/reference-facts.md#backward-tabulation-and-who-clears-the-deferred-wrap-826-verified-2026-09-02)
  — which verbs reset the flag (#826): justerm is the outlier on `CBT`, and xterm does not normally
  clear in the forward tab either. Pinned on justerm's side by
  `back_tab_with_no_stops_lands_at_column_one` and
  `back_tab_on_a_full_row_prints_where_it_landed_not_on_the_next_row`. The outer loop breaks at
  column zero — defensive only, since `vte` saturates the parameter. **If DECSLRM ever lands,
  `put_back_tab` is a site**: xterm and ghostty clamp a back-tab to the left margin under origin mode.
- [Forward tabulation at the right edge](../../agents/reference-facts.md#forward-tabulation-at-the-right-edge-and-the-deferred-wrap-848-verified-2026-09-03)
  — whether `HT` at the last column keeps the flag (3-1 for keeping; all four preserve the character,
  where justerm destroyed it).
- [The deferred wrap across the rest of the verbs](../../agents/reference-facts.md#the-deferred-wrap-across-the-rest-of-the-verbs-848-verified-2026-09-03)
  — IL/DL vs SU/SD, the erase verbs, the arm site (#869: justerm was 3-0 the outlier), and the
  saved cursor across a resize (ghostty's rule, 1-of-1).
- [Where a combining mark attaches](../../agents/reference-facts.md#where-a-combining-mark-attaches-and-the-four-mechanisms-for-locating-it-865-verified-2026-09-07)
  — when the flag is armed under `?7l` (#865). With the arm unconditional the flag now survives
  `?7l`, which reversed #848's local choice on that axis; #848's ground was the site that changed.
- [Relative vertical motion against the margins](../../agents/reference-facts.md#relative-vertical-motion-against-the-margins-and-the-two-verbs-composed-from-it-898-verified-2026-09-14)
  — where CUU / CUD stop inside a region (3-1, alacritty the outlier; justerm was on its side).
- [Reverse wraparound](../../agents/reference-facts.md#reverse-wraparound-and-the-mode-name-that-hides-an-autowrap-requirement-80-873-verified-2026-09-08)
  — the spend, the walk, and the `WRAP_MASK` gate.

`architecture.md` still describes the deferred-wrap model as matching xterm's; the rows above show it
does not on several axes, each a measured divergence.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — the caret is a
  span, and an application can park the cursor on a wide glyph's trailing spacer with an ordinary
  `CUB` / `CHA`, so the position this territory owns can name half a glyph (#454)

## Blast radius

- [soft wrap](soft-wrap.md) — `pending_wrap` is the entry condition for the wrap path; changing when
  it is armed or consumed changes which rows carry a wrap link
- [grapheme clusters](grapheme-clusters.md) — `Term::cursor_cluster_col` reads the flag as "the cell
  the last print landed in"; the three arm sites owe that answer, and a condition re-added to the arm
  breaks the combining attach and the mode-2027 join under `?7l`
- [marker](marker.md) — the OSC 133 command-text bound adds the flag to the column; a change to when
  it survives moves that exclusive bound
- [damage](damage.md) — a pure cursor move changes no cell, so the content-only damage model misses
  it by design and the frame producer folds the old and current cells in
- [caret report](caret-report.md) — reports this position; it assumes the row/col is always inside
  the grid, which the clamps guarantee
- [pen](pen.md) — same struct, same saving verbs; a position change does not change appearance
- [vt interpretation](vt-interpretation.md) — `REP`'s anchor is set by the arm sites' return value, not
  by the flag; a new print path must return where it wrote

## Known holes / open

- **Zero governing records** for rules whose failure mode is a silently shifted screen. The deferred
  wrap has a stated lifecycle (#848); clamping and the save/restore pairing have nothing. #898
  measured the relative-motion half of clamping but wrote no record — it is a derivation.
- **A new cursor-moving verb owes a clear and nothing enforces it.** The doc-comment states the rule
  and the measured per-verb answers; a verb whose author never reads it is unguarded.
- **DECOM's interaction with the clamp is unspecified** in any artifact: origin mode clamps to the
  region, `set_point` clamps to the screen, and no document states which applies when both do. One
  instance measured by #898's refuter pass and left alone: under DECOM, DECRC restores a row clamped
  only to the screen (`Term::restore_cursor`), where xterm routes it through `CursorSet` and caps it at
  the bottom margin (`cursor.c:484-490` @ `6380a3e`); ghostty and alacritty clamp to the screen as
  justerm does. Every relative move afterwards starts from a different row.
