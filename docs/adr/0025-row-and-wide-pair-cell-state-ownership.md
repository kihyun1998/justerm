# ADR-0025: Row-scoped and wide-pair-scoped state has one owner and one lifecycle, not a per-verb rule

Status: **proposed** (2026-07-24; roster refreshed 2026-07-25). DRAFT for adjudication — promotes
the model that has been accreting one issue at a time across the soft-wrap / wide-char-spacer
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
- **#540** — the row-shift verbs are CLEAR sites for the **wrap flag** (D2): end the wrap on the row
  above a shifted region, using `end_wrap`, the wrap-flag analogue of #534's marker clear.
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
  new_cols)` (`grid.rs:508`) assumes every emitted row holds `new_cols` content cells, while the
  loop deliberately emits a short row (`take -= 1`, `grid.rs:457`) whenever a row would end on a
  `WIDE_CHAR` lead — i.e. **the divergence exists only because of D4**, the pair that must not be
  split. Every wide glyph on a re-split boundary drifts every later anchor by one cell, and the
  errors accumulate until the point crosses into a neighbouring row. The fix follows from the rule:
  record the point *inside* the emit loop, where the segment's true `[i, i + take)` extent is known
  and where `set_wrapped` is already decided — not from arithmetic after the fact.
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
