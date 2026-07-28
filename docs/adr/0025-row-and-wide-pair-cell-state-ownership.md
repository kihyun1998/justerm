# ADR-0025: Row-scoped and wide-pair-scoped state has one owner and one lifecycle, not a per-verb rule

Status: **accepted** (2026-07-27, #552) — proposed 2026-07-24, roster refreshed 2026-07-25.

**The adjudication, recorded as an event and not only as reasoning**, because a product judgement and
a derivation have different reversal criteria and a record that does not say which it holds reads as
a derivation. The maintainer was shown: four slices shipped against D1–D4 — #535 (PR #546), #533
(PR #548), #540 (PR #553), #534 (PR #556) — with **no edit to any rule** in the process; each new
combination resolved by pointing at an existing clause rather than by deciding afresh; and D4's one
missing piece turned out to be a *precondition* (#547), not a wrong rule. The alternative was to
leave it proposed and keep deciding the area issue by issue, which is alternative (A) below and is
what failed three times before this record existed. They chose to accept. **Reversing it is theirs**,
not a matter of a better argument about any single clause.

What acceptance changes in practice: a new question in this area arrives as a **conformance item**
against D1–D4 rather than as a fresh "(a) or (b)" decision, and a combination the rules cannot answer
is an **amendment** to this file rather than a new record. The open roster is read that way from
here — and it is **not listed in this file**: it lives in the spine (#552), because a hand-copied one
here went stale in five places within three days of being written. This sentence used to carry
`(#529, #536, #549, #557)` and was wrong again by the time #562 was settled.

What it does **not** cover, so the scope of the judgement is not read wider than it was: the deferred
fifth rule for non-uniform emission (Consequences, below — #549 is filed under the narrow reading),
and the three neighbours deliberately kept out (background-is-not-content, the word-boundary set
#545, consumer-side span projection #454). Those were not adjudicated here.

This record promotes
the model that had been accreting one issue at a time across the soft-wrap / wide-char-spacer
cluster (#521, #528, #530, #532, #533, #534, #535, #538, #540, and the wire-derivation half of #7)
into a single record that *derives* the open questions instead of answering each verb separately.
That list is the *history* the record was promoted out of. The **live roster and its status live in
#552**, not in this file: a hand-copied roster went stale in five places within three days of being
written (2026-07-25 audit), while D1–D4 needed no edit at all. This file holds the rule; the tracker
holds who is open. Scoped to **core VT buffer
state** — how a row/pair property is stored, set, cleared, repaired and read. What such a cell then
*composites to* is ADR-0019 (renderer); this is one layer below it, the same relation ADR-0024 has to
ADR-0019.

## Context

Three facts justerm's `Cell` packs into a 12-byte word are **not** facts about that cell:

1. **The soft-wrap link** (`WRAPLINE`) is a property of the *row* — "this row continues onto the next".
2. **The leading-wide-char-spacer marker** (`C_LEADING_SPACER`) is a property of a *wide-glyph pair* —
   "the last column of this wrapped row is the blank a width-2 glyph vacated when it could not fit, so
   the text extractors must skip it".
3. **The trailing-wide-char-spacer marker** (`C_SPACER`) is the other half of the same pair — "this
   column is the second cell of the width-2 glyph to its left".

`Cell` writes and clears are **whole-word** operations. So every time a byte lands in a cell, or a cell
is freed, it takes any of these along with it — even though the truth being mutated is about a row or a
pair, and the write intends neither. That single mismatch between *storage granularity* (the cell) and
*semantic granularity* (the row / the pair) is what this cluster keeps rediscovering, one VT command at
a time:

| # | Symptom | The row/pair property a cell write silently moved |
|---|---|---|
| #538 | typing in the last column splits the logical line; a plain erase or a wide overwrite too | the **wrap flag** rode the last cell |
| #540 | DL/SU/IL/SD leaves the row *above* the region wrapping into whatever slides in | the wrap flag was left true on a row whose continuation was shifted away |
| #534 | overwrite / ICH / DCH / DL leave a **stale** leading-spacer marker claiming a wrap that is gone | the marker has no clear path; a write past it, or a shift of it, never reaches back |
| #533 | a narrowing resize injects a phantom space and breaks search | reflow *creates* the wide-wrap artefact **without** setting the marker |
| #535 | double-clicking any CJK word selects only its first character | the word walkers read only the *leading* spacer, so the **trailing** spacer ends the word |
| #529 | `relocate_cluster_wide` strands an orphaned wide spacer at the destination | a pair move set one half and not the other |
| #536 | `frame()` panics on a 1-column screen after a wide glyph | a pair-repair computes an out-of-range span on a degenerate width |

Every one was filed with a probe and both-lens attribution exactly as the flow prescribes. The
**discipline is not the problem** — the *sink* is: an issue holds one decision with its rejected
alternatives, and a doc-comment pins a rule to one branch of the code, so neither can hold a rule that
*spans* verbs. The rule is already ~120 lines of prose scattered across `term.rs::end_wrap`'s per-verb
table, `drop_artefact_if_erased`, `free_cell`, and four bullets of `architecture.md §"Hidden VT state"`.
That is the same shape ADR-0019 was written for one layer up (a single `if` reached 80 lines of comment),
and the same promotion trigger: a **consequence chain, not edges** (#528→{#533,#534,#535}, #538→#540); the
**same pair re-decided per verb** (a wide glyph and its spacer; a row and its wrap); an **earlier premise
measured false** before work could start (#528's OR-onto-previous-occupant); and **two references that
already converged** on the fix and were consulted verb by verb instead of once.

`#538` has already implemented the first row of the answer — it moved the *live* wrap flag off the cell
and onto the `Row` (`set_wrapped` / `is_row_wrapped`), leaving `WRAPLINE` as a wire-only bit *derived* at
encode time — and gave the crate `end_wrap` (a per-verb clear) and `drop_artefact_if_erased` (a per-verb
marker clear). This ADR does not invent a model; it **names the one #538 started** and extends it to the
spacer markers, so the still-open issues resolve against it rather than each on its own merits.

## Decision

**D1 — Storage granularity may be the cell; semantic ownership is not.** A property whose truth is about
a *row* or a *wide-glyph pair* has a single **owner**: the `Row` for row properties, and (by convention)
the pair's designated cell for pair properties. Where the wire format forces the bit into a cell
(`WRAPLINE` rides the row's last cell so the frame stays a flat cell grid), that cell bit is **derived at
encode time and is never the authoritative copy** — no live reader consults it. `#538` realised this for
the wrap flag; a future packing pressure on a spacer marker takes the same shape.

A derived bit carries **two** obligations, not one — the encode-side derivation *and* the decode-side
re-arm — and only the first is visible at the site that writes it. `WRAPLINE` satisfies the second for
free, because it is a `CellFlags` bit (`cell.rs:39`) and so travels in the flags byte: `serialize.rs`
never names it, and the derivation in `term.rs::frame` is the whole story. A presence bit that lives
*outside* `CellFlags` has to be reconstructed from whether its wire group carries an entry, and #531 is
that second obligation missing on the ucolor rider (the encoder sets it, the decoder never re-arms it, so
`Cell::is_ucolored()` lies on every decoded cell). Cross-linked, **not folded**: #531 is a codec
conformance bug, not a row/pair ownership question. It is recorded here because it is the general form
of a clause this ADR states for exactly one bit, and because a *future* packing of a spacer marker
(the sentence above) would inherit the same trap.

**D2 — One property, one lifecycle, spelled out per verb — not "remember the rule everywhere".** Each
such property has exactly one SET site-class, one CLEAR/REPAIR discipline, and read sites that gate
**uniformly**. The alternative — a rule a human re-applies at each new write/erase/shift site — is
**rejected on evidence**: it has failed three times in this exact area (#521 extended-attrs, #528 the
wrap artefact, #538 the wrap flag). Which verbs owe a clear or repair is a **named per-verb table**
(the one already in `end_wrap`'s doc-comment), because both references spell it out call-site by
call-site rather than inferring it from the erased range — and justerm carries one *deliberate*
divergence (`EL 2` ends the wrap, xterm does not) that only exists because justerm joins logical lines
for `accessible_text` / `search`. Deriving "does this verb clear?" from the touched range was tried and
is wrong: leftward vs rightward erases differ, and `EL 2` breaks it outright.

**D3 — A pair property is meaningful only at its defining position; a migrated marker describes
nothing.** The leading-spacer marker means "wide-wrap artefact" **only** at the last column of a
soft-wrapped row. A row-shift verb (ICH/DCH) that carries the marker inward has produced a marker that
describes nothing and must be dropped (#528's position rule, generalised). Position is part of the
test, never the marker alone.

**D4 — Both halves of a pair move together, set and clear.** Any path that moves, synthesises or frees
one cell of a width-2 glyph carries the *whole pair* — the lead's extended-attr rider (#521), the
trailing `C_SPACER`, and the reach-**back** repair of the previous row's leading spacer when a wrapped
lead is overwritten (alacritty/ghostty both reach to `row-1, last_column`). "Set one half and not the
other" is the #529 orphan.

**D4's precondition, and what supplies it (#547, 2026-07-24).** D4 is stated unconditionally, and as
first drafted it was **not satisfiable at every size the engine accepted**: at one column there is
physically no room for both halves, while `Term::resize` clamped to `cols.max(1)` and the constructor
did not clamp columns at all. A rule declared over a size the engine also declares supported is a rule
with an unstated precondition, and that gap is not theoretical — it produced the withdrawn half of
#533, where D4 read as universal was implemented as "keep the pair together by dropping the spacer" and
lost data irreversibly (`"ab한cd"` → `resize(1)` → `resize(8)` → the next overwrite yields `"abX d"`,
destroying the `'c'`, because every repair path in the crate keys off `is_wide_spacer()` and a
spacer-less lead has none).

`justerm-core` now publishes **`MIN_COLUMNS = 2`** and clamps `cols` up to it in both the constructor
and `resize` (#547). **That clamp is D4's grounds:** a pair always has room, so D4 holds for every
screen the engine can be in, and no reader needs a width test before trusting it. This is the *accepted*
branch of the choice recorded on #547 — the alternative was to keep one column and give D4 an explicit
exception ("at one column a pair cannot be represented, the buffer is knowingly malformed, readers must
not assume the invariant"), which was rejected on the measured data loss above plus 0-of-3 reference
support: alacritty `MIN_COLUMNS = 2` and xterm.js `MINIMUM_COLS = 2` forbid the width outright with the
wide-char reason stated in both, and ghostty permits it only by destroying the glyph
(*"pretty broken … should be prevented downstream"*). It is a **product/contract judgement by the
maintainer**, not a derivation — `resize(1, r)` silently becoming 2 columns is a contract change, and
reversing it is theirs. It also reverses #536's stated premise (*"1 column is a supported size, not a
rejected one"*); #536 is re-scoped to the defensive `damage_span` clamp, not closed, since that clamp
is still correct for any future caller computing `col + width`.

D4's scope is unchanged in the other direction: the floor guarantees *room* for a pair, not that every
verb carries it. #529 is still an open D4 violation at any width.

**One clause above is now too strong (#529, 2026-07-28): *"no reader needs a width test before
trusting it"*.** The floor guarantees a pair *fits on the screen*; it does not guarantee that a lead
already in the buffer has a column to its right. `Row::resize` truncates cells with no wide repair,
and since #567 the alt screen resizes **without** reflowing — so narrowing a 4-column alt screen
through a pair leaves a `WIDE_CHAR` lead standing in the *last* column. Measured: `?1049h`, `한` at
columns 1-2, `resize(2, 3)` → `cell(1, 1).is_wide()`. A repair site that reads `lead + 1` therefore
still needs its bound, and #529's does: with the bound removed, that state plus a relocation panics
with `index out of bounds: the len is 2 but the index is 2`. The precondition holds for *placing* a
pair, which is what #547 was deciding; it does not extend to *reading* one. The truncation itself is
a separate D4 break, unfiled — it is `Row::resize`'s, not the relocation's, and **ghostty holds the
same position**: its non-reflow column shrink clears only the cells beyond the new width
(`PageList.zig:2362-2374` @ `e6e26e1`, `page.clearCells(row, cols, self.cols)`) with no repair on the
surviving side, and its page-integrity verifier constrains only the spacer side — `.wide => {}` is
empty, while `.spacer_tail` must follow a wide and `.spacer_head` must sit at the end
(`page.zig:510-545`). A lead with no tail is legal there by construction, so this is a shared
position, not an outlier to correct.

### Conformance map (resolved *against* D1–D4)

These stop being independent "(a) or (b)" decisions and become conformance items; the fix site follows
from the rule, not from the issue.

**Status: #552.** The entries below hold each item's *rule* — which decision it conforms to, and where
the fix site follows from. Whether it is still open is the tracker's answer, not this file's.

- **#533** — reflow is a SET site for the artefact (D2). It creates the vacated column, so it owes
  `set_leading_spacer()`, exactly as `write_glyph`'s wrap path does. (The padding cell stays
  `Cell::default()` — that is the separate "background ≠ content" rule below, not this one.)
- **#534** — overwrite / ICH / DCH are CLEAR/REPAIR sites (D2 + D3): a write past the marker, or a
  shift of it off the last column, must drop it, reaching back to the previous row when the overwrite
  lands on a wrapped lead (D4).
  **Amended by the implementation (2026-07-27).** The roster line named three verbs; implementing it
  showed the rule is not about verbs at all, and each correction comes out of D1/D2 rather than out
  of the verb list:
  - **The marker's claim has two clauses, so it has two owners.** It asserts *"this row soft-wraps"*
    **and** *"its continuation still begins with the wide lead that could not fit"*. Splitting it
    that way is what collapses the verb list: `end_wrap` already runs at every site that falsifies
    the first clause (including #540's row-shift seams), so clearing there covers `DL`/`SU`/`IL`/`SD`
    and every wrap-ending erase without naming any of them. Only the second clause needed a new
    primitive.
  - **The record survives exactly one thing: an in-place same-width overwrite.** Anything else
    reaching columns 0/1 of the continuation — a narrow write, an erase, a shift either way — ends
    the pair the record was *about*, and a wide lead arriving afterwards by some other route did
    not wrap from anywhere. That single sentence replaces the verb list, and it is why the check is
    made **before** each mutation rather than after it. Both references gate on the pre-state:
    ghostty on `cell.wide != wide` (`Terminal.zig:1484` @ `e6e26e1`), keeping the marker on a
    wide-over-wide print; alacritty only on the overwritten cell being wide
    (`term/mod.rs:994`, clear at `:1004-1008` @ `852e971`), so it drops a record that is still
    true. **Direction: only alacritty diverges**, so justerm follows ghostty.
    The post-state form — *"is a wide lead standing at column 0 now?"* — was implemented first and
    is **wrong three ways**, each measured: a `DCH` that pulls the *next* wide glyph left satisfies
    it while the pair the record was about is deleted; a VS16 promotion under mode 2027 and IRM's
    insert-then-write are two-step placements that satisfy it only at the end; and worst, running
    the check inside `insert_chars` cleared the marker `vacate_for_wrap` had set microseconds
    earlier, *inside its own set site's critical section*. The last one is the general lesson and it
    is not in D1–D4: **a repair keyed on a state predicate must not run while that state is
    mid-construction.**
  - **Two verbs in the roster line were wrong, both measured.** `ICH` cannot strand a marker: a
    right shift always pushes the last column off the edge. And `DCH` needed an *ordering* fix, not
    a clear — its `end_wrap` ran after the shift, by which time the marker had already moved
    inward. Same shape as #540's `record_scroll` lesson: the clear must happen where the state
    still is.
  - **Two falsifying verbs were missing from the roster** — an *erase* of the wrapped lead (`EL`/
    `ED`/`ECH` covering column 0 of the continuation) and an *intra-row shift* at column 0. Both are
    **ported, not derived**: ghostty's `Screen.splitCellBoundary` (`Screen.zig:1831`, up-a-row
    branch at `:1873`) is called from `deleteChars` (`Terminal.zig:3107-3109`) and `eraseChars`
    (`:3159-3160`). Only justerm's `ICH` site has no counterpart. An earlier draft of this
    amendment claimed 0-of-3 support here on the strength of rows 33/36 of
    `reference-facts.md`; that was wrong, and the row is now marked ⚠ because it is the easy wrong
    conclusion to reach from the print-path citation alone.
  - **The seam-only choice inherited from #540 has a validity condition worth naming.** ghostty
    clears the marker on *every* shifted row; justerm clears at the seams because it rotates whole
    `Row`s. ghostty's own comment gives two reasons for the blanket clear, and the second is
    **left/right margins (DECSLRM)** — a partial-row shift can break an interior pair without
    moving its neighbour. justerm implements no DECSLRM, so seam-only is sound *as long as that
    stays true*; the day margins land, #540's rule and this one break together.
  - **The read side does not change, and that is the point.** The extractors' bare `is_spacer()`
    gate, recorded under #535 above as a live symptom, is now safe by construction rather than by a
    position test. `is_wrap_artefact`'s position clause survives as defence in depth — it is the
    read-side echo of ghostty's write-side page invariant (*"Spacer heads must be at the end"*,
    `page.zig:537`).
- **#535** — the word walkers are READ sites and must gate on **both** spacer kinds (D2, "gate
  uniformly"); every other extractor already does (`is_spacer()`), so the walkers are the outlier
  inside the crate as well as against alacritty.
  **Amended by the implementation (2026-07-24).** "Gate uniformly" means *apply the model
  uniformly*, not *call `is_spacer()`*. Implementing it showed the bare predicate is wrong twice
  over, and both corrections come straight out of D3 and D4:
  - by **D3**, the leading kind is transparent only at the last column of a wrapped row, so the
    walkers use `is_wrap_artefact`, not `is_leading_spacer`. `is_spacer()` has no position test;
    using it would re-open #528.
  - by **D4**, a *trailing* spacer carries no character of its own — it stands for its lead — so
    the walkers resolve it **through the lead**: transparent only where `col > 0`, the previous
    cell `is_wide()`, and that lead is not itself a word boundary. Reading the spacer cell alone
    started a highlight on half of a wide whitespace glyph (U+3000 is wide *and*
    `is_whitespace()`), and let the walk cross #529's lead-less orphan and merge two words in the
    clipboard.

  The correction is the ADR working as intended — D4 answered a combination this list had not
  anticipated — but it is recorded here because the original line, read as a standing instruction,
  says to do the thing that is wrong. Note also what it does **not** fix: the extractors
  (`append_cell`, `viewport_logical_lines`, `search`) still gate `is_spacer()` with no position
  test, so a stranded marker still merges words in the *text*. That is a read-site symptom of
  #534 and is fixed at the write site, not by widening this predicate.

  **One rationale in that amendment is now pinned to #545.** Keeping U+3000 a word boundary (via
  `char::is_whitespace()`) was argued *on the grounds that core has no injection point for the
  boundary set* — which is exactly what #545 is filed to add. #545 is **not** a conformance item of
  this ADR (it is policy routing, ADR-0017 — see "Adjacent" below), but when it lands, this
  rationale must be re-stated as a **default the consumer may override** rather than as a property
  of the predicate. Recorded so the sweep reaches it from here.
- **#540** — the row-shift verbs are CLEAR sites for the **wrap flag** (D2), the analogue of #534's
  marker clear. **Amended by the implementation (2026-07-25).** The roster line read "end the wrap
  on the row above a shifted region"; shipping it found the rule is wider, and each widening comes
  out of D1/D2 rather than out of the verb:
  - **Two seams, not one.** The flag is a claim about *adjacency*, and rotating whole `Row`s keeps
    it true inside the region, so a shift falsifies it exactly where a row's next neighbour
    changed: above the region, *and* at the row that loses its continuation to the blank. The
    second one merges *visible* text across the region boundary and was not in the issue.
  - **The seam is not always a grid row (D2, "read sites gate uniformly" applied to writes).** With
    the region at the grid's top, the row above is the last **scrollback** row, because the readers
    walk `[scrollback ++ grid]` as one buffer. `linefeed` is exempt — it evicts row 0 *into*
    scrollback, so adjacency survives.
  - **Clearing the owner is half the obligation; the derived copy owes damage (D1).** `record_scroll`
    rotates `line_damage` with the content, so a seam clear damaged before it lands on the wrong
    row — the model split the rows and the wire never said so. Ordering now lives inside the one
    primitive rather than at five call sites.
  - **A CLEAR rule needs its SET site to be honest.** The guard that protects a live
    soft-wrap-at-the-last-row was preserving a *false* claim: `write_glyph` set the flag without
    asking `wrapline_advances()`, the predicate both wide-at-boundary paths already ask, so a row
    parked below a DECSTBM region claimed a wrap forever. Fixed at the set site in the same change.
  No reference implements the rule: ghostty clears every row a full-width IL/DL touches *before*
  its row swap, so it splits interior pairs and still never reaches above the shifted range;
  alacritty clears nothing on any scroll path; xterm.js's mirrored polarity moves the exposure to
  the other seam rather than removing it. Derived under ADR-0004 (the spec outranks any
  implementation), which is the first item in this roster with **0-of-3** reference support.
- **#557** — the same CLEAR site, **over-firing**.
  #540's bottom-seam guard exempted only the *screen's* bottom edge, on the recorded ground that
  *"the link is only broken when there is a stationary row below the region"*. That is **necessary
  but not sufficient**: at a DECSTBM region's bottom the same `wrapline`-asked-for scroll happens
  with a stationary row below, and the clear then split the logical line the scroll existed to
  continue (`"\x1b[1;3r\x1b[3;1Habcdz"` → `"\n\nabcd\nz"`, expected `"\n\nabcdz"`).

  The discriminator is not geometry but **why the shift is happening** — a verb *displaces* a
  continuation that already existed, a wrap-serving linefeed *creates* one. `shift_region` cannot
  see that, so it is a parameter, exactly like the `evicts_to_scrollback` that already exempts the
  top seam. **Each seam now has one exemption and both are caller facts**; that symmetry is the
  generalisation, and it is what makes the next shift caller's obligation legible instead of
  remembered. Both exemptions stay one-sided: a wrap-serving scroll still falsifies the top seam.

  **Two claims in the first draft of this entry were wrong, and both are the same failure as #562's.**

  - *"a rule with no reference to check it against"* — inherited from #540's 0-of-3 framing, which is
    right about the seam **clear** and false about this **exemption**. xterm.js carries the identical
    caller fact in the identical place: `BufferService.scroll(eraseAttr, isWrapped)`
    (`common/services/BufferService.ts:68`, stamping at `:77`), with exactly one of four non-test
    callers passing `true` — the auto-wrap branch of `_print` (`InputHandler.ts:588`) and not
    `lineFeed` (`:750`), `index` (`:3366`) or the ED-2 loop (`:1270`). Ported in shape, mirrored in
    polarity: xterm.js stamps the destination row, justerm exempts the source seam's clear. ghostty
    reaches the same outcome structurally (its `index()` path has no `wrap` assignment at all). The
    row now lives in `reference-facts.md` — it was absent, which is why the derivation started from
    a blank slate.
  - *"widening `serves_wrap` to every `shift_region` call turns #540's own suite red"* — true, and
    **not evidence**: that mutation is discriminated by the *down-shift* tests, which `serves_wrap`
    can never reach (it is only ever `true` with `down == false`). The mutation that tests this
    exemption is widening to every **up-shift** (`let orphaned = if !down`), and the whole crate
    stays green under it except one test — `a_verb_that_displaces_a_real_continuation_still_clears`,
    added by this change. So the narrowness rests on a single control, and #540's suite does not pin
    the region-bottom up-shift clear at all. Recorded rather than papered over: a mutation the guard
    cannot reach proves nothing about the guard.
- **#529** — a pair-move D4 violation: carry the trailing half.
- **#536** — a robustness edge of the pair-repair span on a degenerate width; in scope as the same
  code family, though it is a bounds guard, not a state-ownership rule. **Its reproduction is now
  unreachable** through the public API (#547 removed the one-column screen), so what remains is the
  guard itself: `damage_span` still stores an unclamped bound for any future caller computing
  `col + width`. Re-scoped, not closed.
- **#547** — not a conformance item but D4's **precondition**: `MIN_COLUMNS = 2` is what makes "both
  halves move together" satisfiable at every supported size. See the D4 note above.
- **#549** — a **D1 read-side** violation in reflow, and the item that showed this list had a blind
  spot (added 2026-07-25, filed 2026-07-24 out of #533's lens pass and missed by two amendments of
  this record). The re-split loop in `grid.rs` does three things, and this ADR already owned two of
  them: it sets the artefact marker (#533, D2 SET) and it sets the row's wrap flag (D1). The third —
  mapping each tracked point (cursor, selection anchors, OSC-133 columns) to its new position — is
  the one that is wrong, and wrong for a reason this model names: it **re-derives** a fact the loop
  already owns instead of reading the owner. `new_points[pi] = (start + off / new_cols, off %
  new_cols)` (in `grid::reflow`'s re-split loop; the expression is gone as of PR #559, so it is
  cited by shape rather than by a line that now points elsewhere) assumes every emitted row holds
  `new_cols` content cells, while the loop deliberately emits a short row (`take -= 1`) whenever a row would end on a
  `WIDE_CHAR` lead — i.e. **the divergence exists only because of D4**, the pair that must not be
  split. Every wide glyph on a re-split boundary drifts every later anchor by one cell, and the
  errors accumulate until the point crosses into a neighbouring row. The fix follows from the rule:
  record the point *inside* the emit loop, where the segment's true `[i, i + take)` extent is known
  and where `set_wrapped` is already decided — not from arithmetic after the fact.
  **Amended by the implementation (2026-07-27).** Two corrections, one to the shape and one to the
  reference tally:
  - **The loop records the extent; the mapping still runs once per line.** "Record the point inside
    the loop" read as "test every point at every segment", which is `rows × points` — and `points`
    carries **every OSC-133 command mark in the buffer**, not just the cursor and two anchors. The
    loop pushes `(first offset, take, row index)` into a per-line `Vec` and the mapping reads it
    afterwards. The D1 obligation is to read the owner rather than re-derive; it says nothing about
    *when*, and the cost does.
  - **This is 3-of-3, not 2-of-2.** The record's #549 note cited alacritty and xterm.js; the
    closest prior art was missing. ghostty moves a tracked pin **by assignment from the write
    cursor's live position** inside its reflow write loop (`PageList.zig:1650-1659` @ `e6e26e1`) —
    no arithmetic at all. All three decide the position where the real extent is known.

  **What the pass sharpened about D1's boundary, recorded here and not promoted.** The Consequences
  note below says "by construction" covered a *property* but not a row's **extent**. It also does
  not cover a tracked point's **domain**: `points` is typed as a grid coordinate, while reflow needs
  to express `[0, len]` — a point may sit *one past* the last cell (a `pending_wrap` cursor, an
  anchor in the trailing blanks, a mark at end of line). justerm must therefore pick an in-grid
  approximation, and four pre-existing defects live in exactly that gap (one of them a panic).
  **Neither reference has the problem**, and they avoid it structurally rather than by clamping:
  alacritty lifts the cursor *outside the grid* before reflow and restores it
  (`grid/resize.rs:113-116`, `:248-251`, `:173-177`), ghostty **grows the source row to contain the
  pin** (`PageList.zig:1584-1596`, `:1602-1607`) and refuses to absorb a blank row carrying a
  semantic prompt (`:1573`). Recorded as a second instance of the same shape as the extent note —
  still one short of promoting a fifth rule, and alternative (D) is the standing reason this record
  grows per-property rather than by aggregate.

  **Attempted 2026-07-27, five designs built and measured, five rejected — and then settled a day
  later (#562 → PR #565, #567 → PRs #568/#569).** When this paragraph was written no code had
  shipped and it said so; that sentence is retracted here rather than deleted, because the roster
  below is the reason the settlement was possible at all. It is recorded at this length because the
  expensive part was not writing any of the five, it was discovering why each fails, and every one
  of them looks correct until measured.

  | # | design | what it broke, measured |
  |---|---|---|
  | 1 | keep each logical line long enough to hold every tracked point | a **selection** anchored at a line's end moved the app's text down a row (`row2="QQ "` → `row3="QQ "`) |
  | 2 | same, but gated on the cursor: stop the trailing-blank **cell** trim below it | #530 — a BCE tail the cursor is parked in then costs a row, and `CUP` + `EL` under a colour is how a prompt redraws |
  | 3 | let `reflow` return `col == new_cols` and have the cursor read it as `pending_wrap` | **deleted a hard line break**: `"abcdef\r\n…"` + resize + one byte → `"abcdefZ"` where master gives `"abcdef\nZ"`, because `pending_wrap` means *continue this logical line* |
  | 4 | 3 plus alacritty's two guards (`col == width` exactly, last cell not already wrapped) | same deletion at cursor columns 4 and 5 — the guards cannot fire, see the root below |
  | 5 | 4 plus reverting `saved_cursor` to a clamp (alacritty `resize.rs:386`) | lost a glyph instead: `"abcd…"` → `"abc!"` |

  Alongside 1–5, a separate cursor-gated exemption for the trailing-blank **line** destroyed content
  outright on the alt screen (80×24, stock): the exemption emits one more row and the alt caller has
  no scrollback to receive the displaced one, so `"line1"` was gone. ghostty avoids that by
  **deferring** such a row (`PageList.zig:1610-1616` — on `cols_len == 0` it returns early and only
  bumps a counter). *Corrected:* this said ghostty spends **no** destination row, which is true only
  for a blank row with nothing after it — its own comment, *"so that blank rows at the end of the
  page list are never written"*. When a non-blank row follows, `:1634-1637` pays the debt by
  scrolling (`while (self.new_rows > 0) cursorScrollOrNewPage(...)`). The narrow reading is the one
  that applies here, because the join only absorbs *trailing* blank lines — but the general claim
  was wrong, and #567 ① is justerm doing the same thing ghostty does: spending the row when the pane
  can pay for it.

  **The root — as written here, and as it turned out.** This paragraph claimed it was
  `grid::reflow`'s join collapsing a tracked point to `poff.min(line.len())`: a cursor two cells past
  the content and one cell past it become the same offset, and the end of a **hard-ended** line
  becomes indistinguishable from the end of a **full** row. Attempts 3–5 tried to recover that
  distinction *after* the collapse by strengthening a predicate, and it is indeed not recoverable
  there.

  **Measured a day later, that collapse causes none of the four symptoms this was filed for** (#562).
  It is a *no-op* in each: `18.min(18)`, `6.min(6)`, and the trailing-blank-line case never reaches
  the line at all. The three symptoms came from three different clamps — the answer `(row + 1, 0)`
  being the cursor's reading imposed on a mark, the absorbed-blank-line clamp, and #559's defensive
  bound against `reflow`'s own emission. The `min` does lose the *distance* past the content, which
  is a real fifth symptom nobody had listed and which #567 ① then settled at the seam. Left standing
  with its correction rather than rewritten: a stated root that survives four symptoms untouched is
  the more useful thing to have on the record.

  alacritty has no such problem because a cursor column stays a real column through its reflow and is
  lifted out of the grid only when `input_needs_wrap` is already set (`grid/resize.rs:113-116`,
  `:248-251`) — which is also why its guards could never fire here; see the #562 entry.

  **One reusable rule did come out of it**, and it is the criterion that killed design 1: **UI state
  must not move app content; app state may.** A highlight is the user's and transient; the cursor is
  the application's own write position. D1–D4 do not state this and it is not specific to reflow.

  **Two citations in the paragraph above are wrong and are superseded here.** `cols_len =
  @max(cols_len, p.x + 1)` does not grow a row past its width — it raises the floor of a *downward*
  trim (`PageList.zig:1564-1570`) so a cell a pin sits on is not trimmed away, and ghostty never has
  a past-the-end pin at all. And the tally reads the opposite way round from the obvious one: it is
  **ghostty and alacritty** that carry "past" as state, and **xterm.js** that carries it as a column
  value (`x === cols`) — which it then discards on every resize (`Buffer.ts:251` before `:264`).
  That clause was misread three times during this work; the ⚠ rows in `reference-facts.md` carry the
  corrected reading and now also record alacritty `grid/resize.rs:374-384`, the one reference site
  that *sets* a wrap flag from a reflow's output — absent from the file, and therefore unread, for
  all five attempts.
- **#562** — settled, and **the rule of record was D1 twice, in opposite directions.** The five
  rejected designs above all tried to answer "one past the last cell" *inside* `reflow`. They could
  not, because `reflow` does not own what the answer depends on:

  - **The row count is the caller's.** `Grid::set_screen` decides how many rows exist (it pads at the
    bottom); `reflow` was bounding tracked points against `out.len()`, its own emission — a D1
    read-side violation of exactly #549's shape, one level up. It clamped away rows the fit was about
    to create, which is both symptom 2 (the cursor collapsed off a trailing blank line) and symptom 3
    (the cursor folded back onto the last glyph and the next byte destroyed it). The bound is not
    gone — removing it re-opens a real panic (`index out of bounds: the len is 2 but the index is 2`
    in `Grid::row`, reachable from an OSC-133 mark plus a resize, #536's class) — it **moved** to the
    seam, where the final geometry is known.
  - **The meaning of "one past" is the point kind's.** The cursor, a selection anchor and a command
    mark are three different owners with three different answers, and `points: &[(usize, usize)]`
    erases which is which. So `reflow` now returns the honest logical position (`col` may equal
    `new_cols`) and `Term::resize` resolves it per kind. This is the criterion the previous entry
    salvaged — *UI state must not move app content; app state may* — and it turned out to be **in the
    reference verbatim**: ghostty clamps every non-cursor tracked pin before it can widen a row
    (`PageList.zig:1576-1585`) and never clamps the cursor pin (`:1602-1606`). Design 1 was rejected
    for a symptom that clause prevents.

  **A citation in the entry above is corrected here.** It calls alacritty `grid/resize.rs:374-384`
  "the one reference site that sets a wrap flag from a reflow's output" — true — and implies it is
  therefore prior art for a justerm seam that does the same. It is not, and that inference cost
  designs 3, 4 and 5: alacritty lifts its cursor outside the grid **only when `input_needs_wrap` was
  already set**, so its `column == columns` is a flag the cursor arrived with. justerm never hands
  `reflow` a one-past column (`Cursor::point()` returns `col ≤ cols - 1`; `pending_wrap` rides
  beside the reflow), so the same value means a cursor parked past content by CUP. The guards cannot
  fire. `reference-facts.md` carries the correction.

  **Two defects outside the issue's four came out of the same gap**, both fixed here: an OSC-133 mark
  emitted at a filled last column was written one column short **with no resize involved**
  (`add_command_mark` stored `cursor.col` and ignored `pending_wrap`), and `extract_lines` is
  asymmetric — its `to` is exclusive and absorbs a one-past column, its `from` is inclusive and
  cannot. **What this entry left open, and #567 ① then closed**: a point past the content only got
  the row it needs while the pane was shorter than the screen, so on a full one — a prompt at the
  bottom, the ordinary shell shape — the cursor was pulled back onto the last glyph and the next byte
  destroyed a character. Having the row costs a line of history, which is why it was left: spending
  it destroys content on a pane with no scrollback, and that was a constraint only because justerm
  reflowed the alt screen and 0 of 3 references do.

  *(The narrower thing that is still collapsed: `poff.min(line.len())` still loses **how far** past
  the content a point sat. Nothing observable is known to depend on it — the seam clamps such a
  column into the row either way — and it is the fifth symptom noted against the stated root above,
  not a gap left by this entry.)*

  **That premise is now false (#567, 2026-07-28): the alt screen resizes but no longer reflows.** It
  was never a decision — the fix that made both screens take the new dimensions reached for a helper
  that re-splits when the columns change, and #187 later built on the side effect. Turning it off
  reddened exactly one test in the whole workspace, and a real `htop` recording across a live
  `SIGWINCH` showed re-splitting is *harmful* rather than merely useless. Two consequences for this
  record: the alt half of the "measured failures" above is no longer reachable, and the deferred
  half of #562 is unblocked. **Note the exact width of what this record rejects** — the evidence is
  that materialising a row *unconditionally* destroys content, not that materialising is wrong.

  **Amended (#567 ①, 2026-07-28), as that note said it would be: `reflow` does not create rows; the
  seam may, when the pane can pay.** A cursor just after the content needs a row that exists only
  while the pane is shorter than the screen; when the content fills it, the row has to be bought,
  and the price is one line of history — the pane **scrolls**, which is what a terminal does when
  content grows past the bottom. `reflow` could never make that call because the budget is not in
  its scope, which is the actual content of all five rejected designs. The gate is `limit > 0` and
  deliberately **not** "is this the alt screen": since #567 the alt panes pass `limit: 0` because
  that is what an alt screen's history is, so they are excluded by the budget rather than by a
  branch — and needing that branch is what the design carrying this rule was rejected for. Both
  directions are pinned: removing the budget puts the cursor back on top of a glyph, and giving it
  to every pane destroys a line on one with no history.
- **#531** — not a conformance item; the decode-side half of D1's derived-bit clause, on a different
  rider. See the D1 note above.

### Adjacent, deliberately *not* folded in

**"Background is not content" is a different rule and stays separate.** Reflow trims a hard-ended line
by *content* (`is_blank()`), not full-cell equality, so a BCE-coloured tail does not re-split into a
phantom row (shipped in `0cd5216`, the #530 follow-up). That is about *where a line ends*, and it
sits next to D1–D4 in the same file, but it governs the *background*, which these rules explicitly say
nothing about (#530: a freed/erased blank keeps its background; trimming decides length, it does not
blank a cell). Recorded here only so the neighbour is not later mistaken for a fifth rule of this model.

**Which characters separate words is policy, not state (#545).** The word walkers are read sites this
ADR does govern (see #535), but the *boundary set* they consult is a different axis entirely: ADR-0017
routes it to the consumer (mechanism in core, policy injected), and all three references expose it as a
user knob. Folding it in here would put a policy decision inside a state-ownership record and let a
future reader think D1–D4 have an opinion about character classes; they have none. The only tie is the
stale-rationale pin recorded under #535 above.

**Snapping a consumer-side span to a wide pair is projection, not ownership (#454).** That a decoration
or selection span must not bisect a width-2 glyph is D4's *echo* one layer up, but the span lives in the
consumer and is resolved against the frame, so it belongs to ADR-0024 (projection/precedence) with
ADR-0019 underneath. This record stops at the core buffer, exactly as alternative (C) below says.

## Named prior art

Both references already hold this model; justerm consulting them verb-by-verb instead of once is the
history above.

- **ghostty** — the wrap link is a `Row` field (`wrap` / `wrap_continuation`), not a cell flag;
  `cursorResetWrap()` couples the wrap clear and the `spacer_head` clear in one call from `deleteChars`
  / `eraseChars`; `page.zig` enforces *"Spacer heads must be at the end"* as a page-integrity invariant
  (D3); `printCell(.spacer_head)` stamps the artefact from the cursor pen. Its AFL-found test *"print
  over wide char at col 0 corrupts previous row"* pins the reach-back repair of D4.
- **alacritty** — `search.rs` gates on **both** `WIDE_CHAR_SPACER | LEADING_WIDE_CHAR_SPACER` in three
  separate walkers (D2 read-uniformly); `write_at_cursor` reaches back to `grid[line-1][last_column]`
  to clear `LEADING_WIDE_CHAR_SPACER` when a wrapped lead is overwritten (`term/mod.rs:1006-1008`, D4);
  `grid/resize.rs:155,:293` sets the marker at *both* reflow sites (D2 set — this is #533's fix,
  verbatim).
- **xterm.js** — `isWrapped` lives on `BufferLine`, and `replaceCells` takes `clearWrap` as an
  **explicit argument** rather than letting a cell clear decide it (D1 + D2); the wrap artefact is
  pen-written via `setCellFromCodepoint`; and it keeps `getTrimmedLength` (content) separate from
  `getNoBgTrimmedLength` (background-aware) so the reflow caller trims on content only (the adjacent
  rule above).

## Consequences

- The open cluster collapses from six independent decisions to six conformance checks against one
  record; a *new* verb added later (a future scroll/insert primitive) inherits the SET/CLEAR/REPAIR
  obligation by construction, the way #521's `ext_attrs` carry became automatic once it was stated as
  "carry the whole family in one step".
  **Measured boundary of that claim (2026-07-25, #549).** "By construction" holds for a *property* —
  something that is set, cleared, repaired and read. It did **not** cover a row's **extent**: reflow
  inherited the SET obligation (#533) and still shipped with a point mapping that re-derived how many
  cells a row holds, because no D1–D4 clause says a non-uniform emission has an owner to consult. The
  narrow reading is that D1's "no live reader consults the derived copy" extends from *bits* to any
  *re-derivation* of an owned fact, which is how #549 is filed above. The wider reading — a fifth rule
  covering non-uniform emission generally — is **not taken here**: one instance is not a pattern, and
  alternative (D) below is the standing reason this record grows per-property rather than by
  aggregate. Revisit if a second non-uniform emitter appears.
- `end_wrap`'s per-verb table, `drop_artefact_if_erased`, and the four `architecture.md` bullets get one
  home. They stay as *implementation* comments but stop being the *only* statement of a cross-verb rule;
  `architecture.md §"Hidden VT state"` gains a one-line pointer here (Step 6).
- `WRAPLINE` on the wire is now explicitly a *derived mirror*, which makes it an ADR-0020 snapshot
  question too (state, not occurrence; derivable — but derivable by the **encoder**, not the consumer,
  so it stays in the frame). Cross-linked, not folded.
- This does **not** change the theme-agnostic / per-char-width contracts (ADR-0017); it is entirely
  about *layout* state, never colour.

## Alternatives considered

- **(A) Keep the rules as per-verb doc-comments (status quo).** Rejected: it is precisely what failed
  three times (#521/#528/#538), and the flow's promotion bar (≥2 triggers) is met several times over.
  This is #538's own argument, generalised from the wrap flag to the spacer markers.
- **(B) Derive "does this verb clear the wrap/marker?" from the erased range.** Rejected: leftward and
  rightward erases differ, and justerm's deliberate `EL 2` divergence (it joins logical lines) breaks
  any range-derived rule. Both references spell it out per verb; so does `end_wrap`.
- **(C) Fold into ADR-0019.** Rejected: ADR-0019 is renderer cell *composition* (what colour a cell
  paints). This is core buffer *state* (what a cell/row *means* before any renderer sees it) — a
  different layer with a different owner, exactly as ADR-0024 was kept out of ADR-0019 for being
  consumer-side projection. A cell that is a wide spacer is a fact ADR-0019 *consumes*; it is not a
  composition decision.
- **(D) One "wide-char subsystem" object owning all pair state.** Rejected for now as over-reach: the
  cell packing is load-bearing for the wire and the O(1) grid, and #538 already showed the tractable
  move is *per-property ownership* (flag→Row) rather than a new aggregate. Revisit only if a third
  property appears that a per-property owner cannot express.
