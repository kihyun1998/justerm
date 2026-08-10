# ADR-0028: What a composition puts on screen is browser-owned, and each surface it touches has exactly one writer

Status: **accepted** (2026-08-04, #640) — proposed 2026-08-03, accepted once #249 shipped against it
(#703 renderer, #704 web) and the rules below had been implemented rather than only written. Promotes
the model that accreted across #592, #631, #637, #649 and #249 — five decisions about what an IME
composition does to the screen, made one site at a time, none of them written down as a rule.

**Four of its clauses were corrected by implementing them**, and the corrections are in the text
below rather than appended: D2's "removes its cells from the stack" turned out to be a claim about the
whole pipeline and not about the resolver; D2's pair-repair sentence said "blanks the cell" where the
debt it derives from is owed to the *pair*, so the repair was taking the application's colours with it
(#715); D4's bypass is legitimate only because the *origin* is latched while the *extent* stays live;
D5 needed an explicit answer at the right edge, where "one past the end" has no referent. Each was
found by measurement after the first implementation passed review — which is the argument for
accepting a record only after a member has shipped against it. The fourth is the one that arrived
*after* acceptance, and it did not weaken the case for the rule: the clause was over-broad by one
word, and the correction is derived from the same ADR-0025 debt the clause already cited.

**Mixed type, and the split matters for what may retire it.** D1–D4 are derivations: they follow from
what the browser guarantees, what this family's layers are, and numbers measured here — a better
derivation retires any of them. **D5 carries a product judgement made by the maintainer on #592** and
is not this record's to overturn; it is written down because a derivation-shaped rule sitting next to
it would otherwise be read as reopening it.

## Context

**`justerm-core` learns about a composition only when it ends.** The committed text arrives as a raw
`text` intent (#116); the in-progress preedit — its content, its length, its own caret — never reaches
the engine. There is no VT sequence for a preedit and no frame field carrying one, and `CLAUDE.md`'s
boundary puts the browser on the consumer's side by construction. The fact is recorded as a
cross-cutting invariant: [an IME composition is browser-owned state the engine never
sees](../map/invariant/composition-is-browser-owned-state.md).

So every question of the form *"what happens on screen while composing?"* has been answered wherever
one happened to be needed, with nothing behind the answers:

| Question | Answered at | Outcome |
|---|---|---|
| Does the caret keep blinking? | #592 | No. ghostty's stronger form **rejected** by the maintainer |
| Is the anchor right when the composition *starts*? | #631 | Re-read at the point of use, ordered before the controller is told |
| May a **frame** move the anchor mid-composition? | #637 | No — measured against a real IME |
| May a **forced** caller move it? | #649 | No either; the guard was on one writer, not on the state |
| Is the preedit **drawn**? | #249 | This record |

#637's guard and #649's correction are the shape of the missing rule: the first was placed on the
frame writer, and the second exists only because a *second* writer reached the same harm through
another entrance. A guard placed on one writer is not a rule about the state.

### Four things measured for this record, none of them obtainable from source

1. **A DOM layer cannot stay aligned with this grid.** Comparing the renderer's own CSS cell width
   against the browser's text advance for the same font, 16px at dpr 1.5, in Chromium: `monospace`
   agrees exactly (8 vs 8), and **every named monospace font disagrees on wide characters** —
   Consolas 9.333 vs 14.72 for `가` (−3.95 px per syllable), Cascadia Mono the same, Courier New and
   Lucida Console −5.28. Ten Korean syllables walk 40–53 CSS px, four to five cells, off the grid.
   The cause is structural rather than a metric accident: a wide character occupies **two cells by
   definition** (`wcwidth`) while the browser advances it by whatever the *fallback* font says.
   The generic `monospace` alias agreeing is a trap, not a reassurance — it is what the demo uses, so
   a DOM implementation would look perfect there and break in every real consumer.
2. **`compositionupdate.data` leads `textarea.value` by exactly one event.** Measured with the
   Windows Korean IME, typing 한글: `data="ㅎ" value=""` → `data="하" value="ㅎ"` → `data="한"
   value="하"`, then one settling update where the two agree. Drawing from `value` shows the preedit
   one keystroke behind.
3. **Continuous CJK is the ordinary case, not an edge case.** `compositionend` for one syllable and
   `compositionstart` for the next land in the **same millisecond**, and the deferred commit read
   fires *after* the new composition has already started — observed at every syllable boundary in
   both samples. This is the window `composition.ts`'s `finalizeDeferred` handles and the reason the
   anchor guard is keyed on `composing`, not `active` (#649).
4. **ghostty draws no terminal caret during a preedit.** `cursor.zig:47` does return `.block` for
   preedit, but `rebuildCells` discards the cursor before it can be used — `// If we have preedit
   text, we don't setup a cursor` / `if (preedit != null) break :cursor;`, after
   `setCursor(null, null)` (`src/renderer/generic.zig:2453-2454` @ `e6e26e1`). It underlines every
   preedit cell instead. `docs/agents/reference-facts.md` recorded the opposite and is corrected in
   the same change as this record: a correct citation with a wrong conclusion drawn from it.

Fact 4 matters beyond the correction. #592 rejected *"a preedit outranks DECTCEM"* believing it was
the reference's behaviour; it is not, at this pin. **The rejection therefore costs nothing against the
reference**, and the reference's actual answer — suppress the caret entirely — was never on the table.

## Decision

**D1 — A preedit is drawn into the grid by the renderer, never by a DOM layer over the canvas.**
The alternative is xterm.js's `_compositionView`, and it works there for a reason that does not
transfer: xterm's cell width *is* a browser advance (`CharSizeService` measures `'W'.repeat(32)`), so
its composition view cannot lose an alignment it never had to maintain. justerm's cell is the ink box
of the font's `█` (ADR-0022), and measurement 1 above is what that costs a DOM layer. Two independent
seconds: since #273 `justerm-web` has no compositor of its own, and `TerminalOptions.element` is
documented as possibly being the canvas itself — whose child nodes are fallback content and are never
painted.

**D2 — The preedit is a *pass*, not a layer. It removes its cells from the composition stack and
re-supplies background, foreground and glyph.** ADR-0019's stack has no layer that can *supply* a
glyph — every layer recolours a channel or blanks a slot — and its rule 5 axis (authorship: a declared
decoration may take content, an interaction highlight may not) has no value for a preedit, which is
neither. By that ADR's own Totality clause this is a **gap in the model**, closed by amending it
rather than by a new pairwise decision. The amendment's shape is validated twice over: both
grid-drawing references solve it outside their per-cell model, as a pass that excludes the covered
cells and then emits them (ghostty `preedit_range` → skip → `addPreeditCell`; alacritty a separate
`draw_string` after the grid).

Consequences that follow rather than being chosen: nothing from the stack shows through — a selection
tint under a preedit would otherwise read as *selected text* — and the preedit carries a single
underline, on both halves of a wide pair.

**"Removes its cells from the stack" is a claim about the whole pipeline, not about the resolver.**
Replacing the resolver's *inputs* is not enough: every stage after glyph resolution — the selection /
match / active-match composite, `selectionForeground`, both decoration layers — is keyed on
`(row, col)` and re-enters the cells one channel at a time. The first implementation did exactly
that, and it was measured: a selection covering the run raised every composed cell's mean channel
value by ~90 while an in-run control confirmed the tint was real. The span therefore travels *with*
the frame to the packer, which stands each stage down inside it.

**Every per-cell column must be answered for a composed cell; *which* half answers it is free, and
#711 is the column that was answered by neither.** The obligation is the rule — a column the patch
does not re-supply and the span does not stand down reaches the packer describing the cell the pass
erased. What is *not* a rule is a taxonomy of which columns belong to which half: for a column whose
`Default` encoding already means *what the pass supplied*, the two are the same declaration written
in two places. `SGR 58`'s colour is exactly that — `0` means "follow the fg", and the fg is the one
the pass declared — so zeroing it in the patch and standing it down in the packer pack byte-identically.
The gate went into the packer because `webgl.rs` is wasm32-only and 0-compiles on host, which is a
fact about this repo's gates and not about this model.

The reference is what settles that the *value* is right, and it also refutes the tidier-sounding
argument that a preedit "declares a mark, not a colour for one": both grid-drawing references declare
the colour outright, and both make it the run's own fg — alacritty as a field beside the glyph's
(`renderer/mod.rs:225`, `underline: fg`), ghostty by passing one `screen_fg` into the glyph and into
both `addUnderline` calls (`generic.zig:3299`, `:3335`).

Note the direction of the failure, because the obvious generalisation runs backwards. The column did
not arrive after the pass and slip past it — #520 shipped twelve days *earlier* (`7735f93`,
2026-07-23; this pass `764b316`, 2026-08-04, published as `renderer-v0.10.0` with the defect). The
pass enumerated the columns that already existed and missed one, so the guard belongs on whoever
writes a *pass*, not only on whoever adds a column.

**And a pass that writes half a wide pair owes the other half** (ADR-0025: one pair, one owner, one
lifecycle). A run landing on an existing spacer leaves its lead drawing *"its left half only"* — the
resolver's own words for a state core's resize can legitimately produce, but which a preedit has no
business creating. The run blanks the **glyph** of the cell it orphans, on either side — the
codepoint, the cluster override and the two `WIDE` bits, and nothing else.

**This sentence read "blanks the cell" for one published release, and the code implemented it
faithfully** (#715, `renderer-v0.10.0`, the same release that shipped #711). So the composed cell's
background rule was applied to a cell the composition never took: an orphaned lead lost the
background, foreground and SGR the *application* had painted, for as long as the composition stayed
open. The repair cell is not a boundary case of D2's re-supply — it is outside it, and this clause is
the one place a reader could think otherwise.

**Three grounds settle it, and none of them is a reference.** The first two are this record's own,
and the third is the one that makes the rule falsifiable rather than merely coherent:

1. **The pass already declares which cells are its own, exactly once.** `preedit::Span` — the span
   the packer stands every later stage down inside — is built by asking `writes` for the run with
   *no* grid flags, precisely so the repairs are excluded: *"the span is the RUN, never the repair
   cells beside it"*, in its own words. A patch that treats a repair as composed does not add a
   second rule, it contradicts the only one there is.
2. **A cell the pass does not take stays in the stack** (the ADR-0019 amendment above). The pass
   removes *its* cells and re-supplies them whole; everything else resolves through the ordinary
   pipeline. So a repair cell's colours are the application's by construction, and re-supplying them
   would leave that cell with the pass's colours under the application's treatments — a column
   answered by the wrong half, which is the same defect as #711 one column over.
3. **Measured, and it is the reason the rule is not a matter of taste.** Under a selection, a
   re-supplied repair cell carries the pass's `Default` background into `should_blend_kind`, which
   reads a `Default` backdrop as pristine and therefore paints the highlight **solid** where every
   neighbour blends: the repaired cell packs `[0.188, 0.376, 0.753]` inside a band whose other cells
   pack `[0.157, 0.314, 0.565]` — a bright notch in the middle of the user's own selection. Blanking
   only the glyph makes it pack byte-identically to its untouched neighbours. Pinned by
   `a_repaired_cell_packs_identically_to_its_untouched_neighbours_under_a_selection` (`frame.rs`),
   which packs both rules side by side.

**Neither reference can arbitrate this**, and recording that is what stops the next reader spending
an afternoon on it: **neither does pair repair at all**. The repair is justerm's own ADR-0025
obligation, reached from the consumer's side of the boundary. Transposing each reference's *own*
repair into this pass's terms is actively misleading and was tried: alacritty's `clear_wide` keeps the
cell's own attributes (`term/cell.rs:171` @ `852e971`) and lands on this rule, while ghostty's
`blankCell` fill (`Screen.zig:1770`, `:1929`) and xterm.js's whole-pen stamp (`InputHandler.ts:538`,
`:669`) both land on **the code #715 removed** — so the count runs 2–1 *against* the rule this record
states, and it is not evidence either way. The tie-breaker for renderer cell composition is justerm's
own model; a count here is the shape of argument the #490 war story exists to warn about. What the
references *do* settle is the narrower fact that the exclusion is run-scoped: ghostty's `continue` for
an in-range cell precedes its own per-cell background write (`generic.zig:2677` then `:2899`, no other
cell loop between), so a cell outside the range keeps everything it had, background included.

**Rejected alternative: widen `Span` to swallow the repair.** The two halves of the pass can also be
reconciled the other way — declare the whole orphaned pair the composition's for the duration, keep
the old patch, and the disagreement disappears just as completely. It is coherent and it is
expressible (`Span` is one contiguous inclusive range, and the repairs are adjacent by construction),
which is why it belongs here rather than being rediscovered. It is **worse than the defect it would
replace**: with the repair inside the span every later stage stands down over it, so under a
selection the cell packs the terminal's default background — a black hole in the selected row, where
the re-supplied version was merely a bright notch. Recorded so *"cancel the pair and nothing more"*
is not read as the only available reconciliation. It was the better of two.

**Where core answers the same question differently, and why that is not a divergence to close.**
`justerm-core`'s `free_cell` repairs the identical structural event — a write that lands on half a
pair — and answers it with *the pen's background, everything else default*, a choice the maintainer
made on 2026-07-24 (#530) after rejecting exactly the shape this clause adopts. Its grounds are what
separate the two layers, and they are worth stating because the surface similarity is strong enough
to invite a "fix" in either direction. Core rejects the cell's own attributes because a destroyed
glyph's **hyperlink** would stay alive and clickable (#529) and the whole pen because **DECSCA**
protection would land on a cell the application never wrote — this crate has neither; its per-cell
word is SGR presentation plus the two `WIDE` bits. And core's repair is a durable buffer mutation the
wire then carries, while this one is recomputed every frame and stored nowhere, so there is no value
for the two rules to disagree about. What *does* transfer is core's third, affirmative ground — *"a
blank cell carries the current background… a bare default would punch an uncoloured notch into a
coloured run"* — and that is the half both layers agree on, and the half #715 was.

The corollary generalises past this pair, and it is the mirror of the column rule two paragraphs up:
**a pass owes an answer for every column of a cell it takes, and owes nothing for a cell it merely
repairs.** "Which cells are the pass's" is therefore a question with one answer, not one per consumer
of the write list.

**D3 — The drawn preedit reads `compositionupdate.data`; the committed text reads `textarea.value`.
The two sources are not interchangeable and neither is a fallback for the other.** By measurement 2,
`value` is one event stale, so it cannot drive a view. By #116's own finding, `data` misdescribes the
committed character for Korean (a 종성 migrating into the next syllable), so it cannot drive a commit.
The apparent conflict dissolves once `data` is read as what it is: **the OS's own preedit**. Whatever
it says is what the IME is showing the user, so drawing it is correct *by definition*, and its
disagreement with the eventual commit is the IME's behaviour rather than our defect.

**D4 — Each surface a composition touches has exactly one writer, and a writer that knows where the
composition is does not pass through the guard that exists for writers that do not.**
The IME anchor's guard (`textareaMove`, `composing` outranking `force` since #649) exists because the
frame stream and the focus path describe *where the output cursor went*, which during a composition is
not where the user is typing. A preedit writer does not have that defect: it knows the run's extent,
so it re-aims rather than being frozen. It therefore bypasses `textareaMove` entirely instead of
gaining a third discriminator there — which is exactly xterm's structure (`updateCompositionElements`
never goes through `_syncTextArea`) and keeps the existing guard a rule about the **involuntary**
writer, which is what #637 and #649 actually measured.

**Knowing where the composition is means knowing both halves, and only one of them is live.** The
**extent** comes from the run and changes on every keystroke; the **origin** is latched at
`compositionstart` and does not move for the composition's life. This clause is not a refinement of
the rule, it is the condition that makes the bypass legitimate — a writer that re-reads the origin
from the frame stream is asking the one source #637 established cannot answer it, and inherits the
same defect the guard exists for. Measured on the first implementation, which did re-read it: with
unsolicited output running, the anchor correctly held at row 5 while the composition was open and
then **jumped to row 9 on the next keystroke**, taking the drawn run with it.

Corollary, from measurement 3: the preedit view's lifetime is owned by the composition events alone
and may not assume the commit has reached the grid. At every syllable boundary it has not.

**D5 — Browser ownership governs position and extent. It does not govern visibility.**
A composition may decide *where* things are and *how far they reach*; it may not reveal something the
application asked to hide. Concretely: the caret's position rides the end of the preedit run (that is
where the insertion point is, and it keeps a block caret off a wide lead it would bisect), while
`DECTCEM` continues to decide whether the caret is drawn at all. Position is re-asserted on **every
frame**, not only when the composition changes: frames keep arriving and each one carries the
engine's cursor, which cannot know about a preedit, so a caret set once snaps back under the composed
text on the next output frame.

**At the right edge there is no cell past the run, and "one past the end" has no referent.** Clamping
to the last column drops the caret onto the run's own final cell — the *spacer* of a wide tail, which
a block caret then covers alone, inverting half the glyph being composed (measured: a 106-column grid
returned caret 105 for anchors 104 and 105 alike, the spacer of the run's last pair). The caret takes
the last glyph's **lead** instead, so it covers a whole glyph. That is inside the run, which this
clause's own phrasing does not describe — recorded as the exception rather than smoothed over, because
the alternative is the one thing the rule exists to prevent.

**This clause is the maintainer's product decision from #592, not a derivation.** It was made when
ghostty appeared to hold the opposite rule; measurement 4 shows ghostty does not hold it either, so
nothing about the reference reopens it. What the decision was made *on* — the caret alone — is now
narrower than the surface it governs, since a drawn preedit is a second visible thing. That widening
is recorded here as the thing to re-ask if a case ever appears where a preedit is invisible *because*
the caret is hidden; no such case is known, and it is not a reason to act.

## Named prior art

- **xterm.js** (`699f5537`) — DOM `_compositionView`, positioned per render, textarea sized to the
  view's bounds. Its own stylesheet carries `TODO: Composition position got messed up somewhere`, and
  the view is hardcoded `background:#000; color:#FFF` with no theming and no underline. Read as a
  design input (ADR-0019's posture), not a validator: D1 explains why its mechanism is not portable.
- **alacritty** (`852e971`) — preedit drawn into the grid (`draw_string`), underlined, shortened to
  `num_cols`, with its **own** IME caret (Beam, or HollowBlock for a multi-character range) and the
  terminal caret's blink suppressed.
- **ghostty** (`e6e26e1`) — `Preedit{codepoint, wide}` with `width()` and a `range()` that shifts the
  run left at the right edge (two host tests); preedit cells excluded from row rebuild and emitted
  separately in the default foreground with an underline; **no terminal caret at all** while
  composing; the preedit's width folded into the IME rect pushed on every key.

Convergence: both grid-drawing references keep every codepoint (shift or shorten, never wrap), colour
from the default foreground, and underline. justerm follows them, and follows ghostty on the right
edge because keeping the tail visible beats clipping the text the user is currently typing.

## Consequences

- `justerm-renderer` gains a retained preedit state and a pack-time pass. It is per-grid under
  ADR-0021's tier rule (consumer-settable per terminal), and it needs no delta bookkeeping because the
  renderer re-packs fully every frame.
- The renderer takes `unicode-width`, pinned to the same version `justerm-core` uses. Per-char width
  needs no whole-buffer knowledge, so ADR-0017 does not route it to core; but the two copies answer
  the same question and must not drift, which is why the version is stated rather than floated.
  **The known per-char defect is inherited deliberately**: VS16/ZWJ sequences measure width-1
  (#295/#297/#300/#303/#304), and both references have the same defect per codepoint or per char. A
  preedit is re-rendered on every keystroke and dies at commit, so a one-cell error self-corrects;
  inventing a third answer here would be the divergence.
- `justerm-web` drives the preedit from composition events, adds nothing to the accessibility tree
  (the preedit is pixels), and creates no new `reactivate()` reset obligation.
- A preedit whose row's cursor is not in the viewport is not drawn (ghostty's rule).
- **#454 becomes reachable through a new path**: a preedit wide pair under a decoration covering only
  one of its columns. The span-snapping gap is unchanged in kind, but this record's pass is a second
  producer of wide pairs the consumer did not author.
- Spine **#640 closes** with a pointer here. Its falsifier — *"if #249 needs no rule to land beside
  it, the anchor retires"* — resolved against retirement: #249 could not land without D2 and D4, and
  D2 forced an amendment to another record.

## Alternatives considered

- **(A) A DOM composition view over the canvas** (xterm's shape). Rejected on measurement 1, and
  independently on `element` possibly being a canvas. It was the cheaper option by a wide margin —
  one package instead of two, no ADR-0019 amendment, and a trivially reversible one, since it adds no
  published renderer surface. Recorded because the reverse migration does not exist: a shipped
  `setPreedit` cannot be withdrawn without a renderer major.
- **(B) Route per-char width through `justerm-wasm-decode`** so core stays the family's only width
  authority. Rejected on cost, not on principle: it makes the slice a three-package release for a
  value that needs no buffer, no VT state, and no engine.
- **(C) Give the preedit its own caret** (alacritty's Beam / HollowBlock). Deferred, not rejected —
  it is the natural home for the IME's own cursor offsets, which we do not currently read. D5's
  position rule covers the common case without it.
- **(D) Suppress the terminal caret entirely while composing** (ghostty's actual behaviour, per
  measurement 4). Not taken: it is a *visibility* change, which D5 places outside browser ownership,
  and #592 has already been decided in that space. It is the option to revisit first if D5's boundary
  is ever reopened.
