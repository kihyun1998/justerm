# ADR-0019: The cell composition model — a layered, per-channel, total resolution

Status: accepted (2026-07-21) — **amended 2026-08-20** (#791): rules 1–6 resolve the sources belonging
to **one** cell, and a glyph whose ink exceeds its cell produces a source whose owner is a *different*
cell. The model had no term for it, so the renderer resolved it by destruction — at bake and again at
sample — and the loss is total: measured on our own renderer, the ink drawn equals the ink inside the
cell box exactly (`ᾷ` 96 = 96, `ǰ` 70 = 70, against `g` 65 = 65 lossless), and on DejaVu Sans Mono 439
of 1579 sampled codepoints lose ink, `À Á Â Ã Ä Å` among them. Rule 4's enumeration gains
**`I_neighbour`** and rule 6's order gains one position for it; rule 5 says whose colour it keeps and
when it is withdrawn. Same class of gap as #712's, one axis over — an accident of the fold deciding
what Totality reserves for the model. — **amended 2026-08-18** (#317 §2): **R1.1** answers whether a
background-class glyph goes translucent with the background it joins. It does not, and the reason is
that `u_bg_alpha` is not a layer — translucency is gated on *no* layer having touched the bg, so at
that moment R1 has no treatment to transfer. Recorded because until now the answer lived only in a
shader comment, while R1's own wording read the other way. Same amendment carries the Coherence
clause's reach: a cell's colour and its **alpha** describe one surface, and the shader had them
disagreeing on every antialiased edge of a translucent cell.
 — **amended 2026-08-04** (#712): rule 4 enumerated the ink sources and
gave each a class, but never said what happens where **two of them claim the same pixel**. Occlusion was
decided by the order the shader happened to composite in — an accident of the fold, which is exactly what
Totality forbids — and it stayed invisible until `SGR 58` (#520) let two inks differ. New **rule 6** puts
a total order on rule 4's own enumeration, so a pair is answered by construction rather than pairwise.
 — **amended 2026-08-04** (#525): rule 4's `I_line` was one ink source
named "underline / strikethrough", so the #520 amendment below made a declared `SGR 58` colour
authoritative over *both* marks. Read literally the model therefore endorsed a defect: SGR 58 is the
underline's colour and there is no SGR for a strikethrough's. The correction is to rule 4's
**enumeration** — the two marks were always two ink sources that coincide by default — and the
declared-colour regime then attaches to the one the application actually spoke about. — **amended 2026-08-03** (#249/#640, ADR-0028): Totality's first gap that
no layer can close — an IME preedit supplies a *glyph*, and nothing in this stack can. Resolved as a
**pass**, not a layer; see the Consequences. — **amended 2026-07-22**: the three pins this ADR left for adjudication
were adjudicated *for* the pins. R1 is scoped by who declared the layer (rule 5 below), the pins stand,
and nothing in the renderer changes. An earlier amendment the same day said the opposite and is retracted
in place — see rule 5 and the Consequences for what was tried and why it failed. Scoped to
`justerm-renderer`'s **cell composition**; it does not change the core boundary (ADR-0017) and does not
govern span projection (see *Out of scope*).

## Context

`justerm-renderer` resolves every viewport cell to a background colour, a foreground colour, and a glyph
field, each frame (ADR-0018, `frame.rs::pack_instances`). What that resolution *is* has never been stated
as a whole. It exists as an imperative sequence of conditional overrides whose individual steps are each
well-argued in a doc-comment naming the issue that added them — and the set of those issues has been
growing by consequence, not by feature.

### The forcing case: the cluster grew by self-reference

Of the 20 issues in the 2026-07 cell-composition cluster, **17 were surfaced by another issue in the same
set** — almost all by its adversarial two-lens pass (theflow Step 5). The roots are #398 and #400; the rest
are consequence edges, notably `#453 → {#494, #495, #496}` and `#494 → {#506, #507, #508}`.

That is not a defect rate. It is enumeration. The state a cell can be in is combinatorial —
`{tile, text} × {inverse+Default bg, other} × {Selection, Match, ActiveMatch, none} × {bottom deco, top
deco, none} × {bg-only, fg-only, both} × {dim, bold, underline, blink}` — and the two-lens pass is doing
exactly its job by walking it. Without a rule that answers a combination *by construction*, every new
combination is a new decision, and the walk does not terminate.

The cost shows up three ways:

- **Re-litigation.** "A tile glyph's ink versus a background-ish layer" was decided eight separate times
  (#241, #398, #430 AC-③, #453, #494, #496, #507, #508). "Active match versus selection" was re-opened
  every time a new channel was touched (#400 ②, #424, #427, #430, #453, #494).
- **Stale premises.** Eleven decisions contradicted or narrowed an earlier one. #241's "transparent"
  premise was falsified by #444 and re-narrowed by #496; #444's guarantee was found "half-true" by #453;
  #427's pin was flipped by #430; #453 measured **both** of its own body's premises false before
  implementing. With no model to check a premise against, each issue re-derives one by guess.
- **Serial reinterpretation.** The 2026-07-21 backlog sweep saw this coming and said so, on #494/#496:
  *"No recorded rule orders them … it has to be **stated**, not inferred"*, recommending #494/#495/#496 be
  decided **in one pass** because *"deciding them serially means each later decision reinterprets the
  earlier one."* They were decided serially; #494 duly extended the rule #495 had just stated, and #506,
  #507 and #508 followed.

### The reference has stopped being able to answer

Each of these decisions consulted xterm.js as the oracle. In the four most recent it could not serve:

- **Silent** (#494) — `typings/xterm.d.ts:687-691` defines a decoration's `layer` *only* relative to the
  selection, never against glyphs. Neither reference nor justerm had stated the answer.
- **Self-contradictory** (#495) — `CellColorResolver.ts:133` classifies a combined cell's **last** UTF-16
  unit while `TextureAtlas.ts:538` classifies the **first**, with inconsistent guards inside one file.
- **The outlier** (#459) — we concluded justerm's behaviour is the doc-conformant one and *"upstream's
  colour path is the outlier"*.
- **Demoted** (#458) — decided on ADR-0017's boundary rule (precedence is consumer policy); xterm's
  documented contract then *agreed*, which was recorded as convergence rather than as the reason.

ADR-0018 declared the renderer first-party. Practice stayed a reimplementation: the reference was still
being consulted for questions it never posed, and each answer it could not give became a fresh
"deliberate divergence" needing its own justification (#444, #459, #494, #495).

## Decision

The renderer composes a cell by a **total function** over a fixed layer stack, resolved **per channel**.
Six rules; together they answer every cell state. (This read *"Four rules"* until 2026-08-04, through
the amendment that added rule 5 — a count is a status claim with nothing gating it.)

**1 — The layer stack.** Bottom to top: `L0` the cell (after inverse and bold→bright, `resolve_cell`) <
`L1` bottom decoration < `L2` highlight < `L3` top decoration. Within `L2`, `ActiveMatch > Selection >
Match`.

**2 — Layers declare channels independently.** Each layer declares a `bg` and an `ink` **separately**;
an absent declaration is transparent on that channel and the layer beneath shows through. This
generalises #452's per-property decoration merge to the whole stack.

**3 — Each layer has a paint mode.** A decoration **replaces**. A `Match` / `ActiveMatch` **replaces**
(solid — a match's job is to be found, #400). A `Selection` **blends** at `HIGHLIGHT_BLEND_ALPHA` over
anything with a real colour beneath it, and replaces only over a bare default background.

**4 — Ink sources are distinct, and one of them is background.** A cell's ink is `I_glyph` (the
character), `I_underline`, `I_strike`, `I_cursor` and `I_neighbour` (added 2026-08-20, #791 — the one
source the cell does not author; see **R1.2**) (amended 2026-08-04, #525 — this list read
`I_line` *"(underline / strikethrough)"*, and naming two marks as one source is what let a colour
declared for one of them paint the other). The two line bands are **separate sources that coincide by
default**, not one source that splits: they run the same treatments off the same follow-fg base, and
the only thing that can pull them apart is a *declared* colour — which only the underline can have,
since `SGR 58` is its and no escape sets a strikethrough's. **R1:** when the glyph's class is
`BACKGROUND` (`treat_glyph_as_background_color` — Powerline, box / block, and since #507 whatever
`builtin::owns` draws to the cell, asked of the drawer rather than restated), `I_glyph` belongs to the
**background channel** and takes whatever treatment the bg fold applied. R1 reaches `I_glyph` **only**;
`I_line` and `I_cursor` are `TEXT` class always.

**R1.2 — `I_neighbour`: the ink of a glyph that belongs to an ADJACENT cell** (added 2026-08-20, #791).
Every other source in rule 4's list is authored by the cell being resolved. This one is not: a glyph
larger than the cell it occupies deposits ink outside it, and that ink is a source for whichever cell
it lands in. Three properties, all of which follow from its owner rather than from its receiver:

- **It carries its owner's resolved ink** — including, for a colour emoji, the atlas's own colours
  rather than either cell's foreground, since that is where an emoji's ink lives. R1 is asked of the
  *owning* cell's glyph, so a background-class tile overflowing its cell is background-class where it
  lands too. **That classification does not move it in rule 6**, which places `I_neighbour` by whose
  content it is rather than by what it looks like; it is recorded because R1 is stated per ink source
  and this source would otherwise read as exempt.
- **It is not the receiver's content.** The receiver's own layers do not recolour it — see rule 5.
- **Its reach is bounded, and it bounds only the axis it is a band on.** How far ink may travel is
  the bake's bleed, a per-configuration quantity, not something this model fixes. The model says
  such ink *has* a home; it does not claim every glyph fits in one.
  **Corrected 2026-08-21 (#792): the residue this bullet recorded was horizontal, and it is no
  longer a residue of this model — it is answered before the model sees it.** The sentence here read
  *"a glyph the font draws two cells wide still loses what falls outside that budget"*, which is
  false since the bake condenses such a glyph onto the horizontal axis (`metrics::horizontal_fit`).
  Nothing in rules 1–6 changed: the ink is still `I_glyph`, still its owner's, still in the owner's
  slot — there is simply less of it to place, and no `I_neighbour` contribution is produced on an
  axis that never had a band. What survives of the original sentence is the vertical claim: ink
  beyond the vertical band is still destroyed, because the band is what bounds that axis and a
  face can overshoot even its own declared line box.

The renderer produces this **reader-side**: the receiving cell's fragment samples the adjacent slots
and folds their coverage into its own chain. That is a mechanism note rather than part of the model,
and it is here because it is what keeps the model intact — the quad stays exactly one cell, so the
composite remains one evaluation per pixel and rule 6 keeps deciding occlusion. See *Alternatives*
for the writer-side shape and why it was refused.

**R1.1 — "whatever treatment the bg fold applied" means the LAYERS' treatments, not the surface's
opacity** (added 2026-08-18, #317 §2). A background-class glyph stays **opaque** on a translucent
terminal. The question looks open — R1 says the ink follows the background, and a translucent
background is one — but it is closed by *when* translucency applies: the shader gates it on
`v_bg_default`, the provenance flag meaning **no layer touched the bg at all** (#455). So at the
moment a cell is translucent the fold has applied *nothing*, and there is no treatment for R1 to
transfer. Translucency is a property of the surface the resolved stack is drawn onto, reached after
rule 1's layers have all declined; `u_bg_alpha` is not a layer and never enters the stack.

The answer rule 5 would give independently is the same, which is why this is a clarification rather
than a new decision: background-class ink is *"still the only thing drawing a table border or a
progress bar"*, and attenuating it by `u_bg_alpha` deletes every box-drawing character, block element
and Powerline separator at `bg_alpha = 0` — the content loss rule 5 exists to refuse, arrived at from
the other direction. #398 had already narrowed the same sentence once (R1 does not transfer the
background's *identity*: background-class ink keeps `L0`'s resolved ink, bold→bright included).

Measured on this renderer at `bg_alpha = 0`, dpr 1 — the shades keep their own coverage as alpha,
which is what a shade means: `░` → `rgba(255,255,255,64)`, `▒` → `…,128`, `▓` → `…,192`, `█` →
`…,255`. Before #317 §2 the same `░` read `rgba(100,136,184,64)`, three quarters of a background the
alpha channel said was absent — the incoherence that fix removed.

**5 — An interaction highlight does not remove content; a declared decoration may** (amended
2026-07-22). Rules 2 and 4 collide on one cell: a bg-only layer above a `BACKGROUND`-class glyph. Rule 2
passes the layer beneath through on the ink channel, so the tile keeps its own colour; R1 says that ink
belongs to the background channel, so the layer owns it and the tile vanishes. Both are available; the
model must say which, and it says: **by who declared the layer.**

- **A consumer-pushed `decoration` replaces.** The application said "this cell is now this colour". It
  knows what it covered and chose to. A `BACKGROUND`-class glyph goes with the background (#494).
- **An interaction highlight does not.** Selection, search matches and the active match are the *user*
  passing over content, not the application replacing it. They wash across a cell and leave what is in
  it — including background-class ink, which is still the only thing drawing a table border or a
  progress bar. Rule 2's pass-through governs here, and the three pins below are its statement.
- **`HIDDEN` (`ESC[8m`) is not an exception to this, it is the other side of it** — the application
  asked for invisibility explicitly, so it gets it. The rule is about who asked.

The line is *authorship*, not paint mode: a decoration and an active match can both `REPLACE` on the bg
channel (rule 3) and still differ here, because rule 3 says how a layer paints its own background and
rule 5 says whether it may take the cell's ink with it.

**`I_neighbour` is authored by neither party, and that answers two questions at once** (added
2026-08-20, #791). An adjacent cell's ink is not the application declaring *this* cell's colour, nor
the user passing over *this* cell. So:

- **This cell's layers do not recolour it.** It keeps its owner's resolved ink — a selection washing
  over me does not repaint the descender that fell in from the row above, because that descender is
  not my content. Rule 2's pass-through, reached by the same authorship test.
- **It is withdrawn where the two cells' backgrounds differ.** Ink from one cell sitting on a
  differently-coloured neighbour reads as a rendering fault rather than as a tall letter, and the
  boundary it crosses — a selection edge, a search highlight, a coloured prompt segment — is exactly
  where a user is looking. Withdrawal is **symmetric on both edges ink can cross**, which today is
  the vertical pair: the horizontal axis grants nothing, so there is nothing there to withdraw. Worth
  naming even so, because xterm.js guards only the *left* (`GlyphRenderer.ts:263`, SHA `699f553`) and
  its own reason is that it walks cells left to right and therefore knows only the *previous*
  background — a property of its loop, not a rule about rendering. Resolving reader-side, the receiver
  holds both backgrounds already, so symmetry costs nothing and asymmetry would have to be argued for.
  Should the horizontal axis ever grant, the rule extends by construction rather than by amendment.
  **It does not, and #792 is why (added 2026-08-21):** a horizontal band has no derivable depth —
  the Canvas API exposes no face-level counterpart to `fontBoundingBox{Ascent,Descent}` — so that
  axis was closed by condensing the glyph at bake instead, and there is nothing there to grant. A
  reader reaching for the band on this axis should read #792's rejected alternatives first: the
  withdrawal *rule* would extend, but `block_cursor_at` is a different predicate on a horizontal
  neighbour, and a wide pair's horizontal neighbour is its own spacer.

  **Two of the three producers of a background difference never reach the packer, so the rule is
  enforced in two places.** A *block cursor* is a background applied per fragment (`base_bg`), so no
  packed value differs and the shader completes the withdrawal; a *wide pair* is one glyph across two
  cells whose two receivers can sit under different backgrounds, so the packer reconciles them — the
  cross-cutting invariant *a span covers a wide pair whole* reaches a withdrawal **gate** and not only
  the four ranges it enumerates. Both were found by an adversarial pass after this amendment was
  first written, which is why they are stated here rather than left to the code.

**What this costs, stated plainly.** The renderer is no longer uniform across routes: the same visual
concept expressed as a decoration erases a tile and expressed as an active match does not. That is a
real seam in the API and it is accepted deliberately — the alternative erases box-drawing and shading
from the screen whenever a user drags across it or steps through search results, which is content loss
in exchange for an internal symmetry no user can observe.

**6 — Where two ink sources claim the same pixel, the later one in this order wins** (amended
2026-08-04, #712):

```
background channel  <  I_glyph (BACKGROUND class, by R1)  <  I_neighbour  <  I_underline  <  I_glyph (TEXT class)  <  I_strike  <  I_cursor
```

`I_neighbour`'s position (added 2026-08-20, #791) is the whole of what the model buys here, and it is
one clause: **above the receiver's own tile, below everything else the receiver owns.** Above the
tile, because foreign ink is ink and a descender crossing a box-drawing border must be visible, which
is R1's own argument read from the other side. Below the receiver's underline, glyph, strike and
caret, because the alternative is that a letter from the row above can amputate this row's letter —
and *nothing about who typed first should decide that*. This is the clause the writer-side shape
cannot state at all: there, two overlapping quads are ordered by instance order, i.e. by column, which
is the accident this rule exists to abolish.

Rules 1–5 resolve a cell's channels; **none of them says what happens when two of rule 4's ink sources
land on the same pixel**. That was left to whatever order the shader composited in, which is precisely
the accident Totality forbids — and it was unobservable for as long as every ink was the same colour.
`SGR 58` (#520) made two of them differ and the gap became a defect: measured on our own renderer at
44 px, a red underline destroyed **100 %** of the descender ink of `g j p q y` crossing it (0 of 94
glyph pixels survived inside the band; with the order corrected the surviving ink is *identical* to the
same text drawn with no underline at all).

The order is not a table of answers; it is a **total order over rule 4's enumeration**, so every pair is
resolved by construction and a new ink source is placed once rather than compared against each existing
one. Two things determine a source's position:

- **Class first.** R1 puts a `BACKGROUND`-class glyph's ink on the **background** channel, and rule 4
  makes `I_underline`, `I_strike` and `I_cursor` `TEXT` class always. Background-channel ink therefore
  cannot occlude a `TEXT`-class source: an underline draws **over** a tile, exactly as it draws over the
  cell's background. This is the clause with a real cost — see the trade below.
- **Then, within `TEXT` class, by what each mark is for.** An underline sits **below** the glyph so a
  coloured one does not amputate descenders; a strikethrough sits **above** it, because crossing the
  character out *is* the mark; the cursor is last, because it recolours the whole cell.

**Coherence with rule 5.** This is an ordering *within* one cell's ink resolution, not a layer. Rule 5
still decides whether a layer may take the cell's ink at all; rule 6 only says who is on top once the
surviving sources are known.

**What is deliberately unprovable here, and why that is not a gap.** Only a *declared* colour can pull
two ink sources apart (rule 4), and no escape declares a strikethrough's — so `I_strike`'s position
relative to `I_glyph` is invisible in every ordinary cell: a band whose colour equals the ink beneath it
composites the same either way. The order still has to be stated, because a cell where a glyph-only
treatment moves `fg` away from the line inks (a selected tile, #513's own case) *can* see it. What the
GL proof asserts is what is observable; the rest is recorded here rather than pinned by a test that
could only confirm its own premise.

**The trade, and why the references cannot arbitrate it.** Both references that draw the underline first
do it **unconditionally**: ghostty appends the underline before the glyph and states the descender
reason (`renderer/generic.zig:2932` @ `e6e26e16`), and xterm.js puts its glyph `fillText` between the
underline block and the strikethrough (`addons/addon-webgl/src/TextureAtlas.ts:735` @ `699f5537`) —
xterm even computes `treatGlyphAsBackgroundColor`, but feeds it only to `_getForegroundColor` (`:538`),
never to draw order. Taken blanket, that trades this defect for its mirror, and the cost was measured
here, not inferred: a red underline over `█ ▄ ▓ ░` drops from 66 surviving band pixels per cell to
**0**. alacritty has neither behaviour for a structural reason rather than a decided one — its
`draw_rects` pass (`alacritty/src/display/mod.rs:990` @ `852e971c`) runs after `draw_cells` (`:878`) and
carries the visual bell and the message bar with it, so bands-over-glyphs is what batching those
together produces, and it carries no comment defending it. **No reference has a background ink class
driving occlusion, so none of them ever faced this choice**; the class clause above is this model's,
taken on ADR-0018's first-party footing and on the measurement.

**Coherence.** Where a channel's resolution and R1 describe the *same surface*, they must agree. A cell
whose bg says one colour and whose background-class ink says another is not a trade-off; it is an
unresolved state.

**Totality.** Every cell state has an answer by construction. A combination with no answer is a **gap in
this model**, closed by amending this ADR — not by a new pairwise decision.

### xterm.js is a design input, not a validator

For cell composition, xterm is consulted for *what problems exist* and *how they have been solved*, and
its solutions are adopted when they fit this model. It is **not** the tie-breaker, and a difference from
it is not by itself a defect or a thing requiring justification. Divergences are recorded as
**documentation for consumers porting from xterm**, not as exceptions to a parity contract. This makes
ADR-0018's first-party declaration behavioural; it is the compositing-layer counterpart of ADR-0004
(spec-faithful where alacritty omits) one layer up.

### Named prior art

The per-channel split is xterm's, taken deliberately: `CellColorResolver` resolves `$hasBg` and `$hasFg`
independently, which is why #430's fg/bg independence is a *consequence* of rule 2 rather than a separate
adoption. The solid-match paint mode is the one place the references converge — xterm drops the match
decoration's alpha and alacritty's `compute_cell_rgb` forces `bg_alpha = 1.0` (#400). The layered fold
itself is Porter-Duff: *what is underneath shows through*, the first principle #444 invoked to reject both
of its parent's framings. xterm's DOM renderer resolves the same question a third way
(`DomRendererRowFactory.ts:399-408`, decorations before selection, selection suppressed under a top
decoration); it is rejected here because it drops a highlight the user explicitly made.

### Out of scope

- **Span projection** — clamping a decoration to the viewport, anchor placement, precedence across
  markers (#457, #458, #459, #461). These are consumer policy under ADR-0017 and were only clustered with
  compositing by proximity.
- **The rest of the renderer** — glyph rasterisation, palette resolution, cursor metrics, contrast maths.
  Their parity-derived behaviour is unchanged; extending this stance to them is not evidenced yet.

## Consequences

- **The open questions stop being decisions.** #508 (underline / strikethrough vanish on a tile a top
  decoration took) is a **model-conformance defect** — rule 4 answers it: `I_line` is TEXT class, so the
  decoration takes the *glyph* and must leave the ink channel alone. Fixed by dropping the glyph's slot
  instead of recolouring its ink. (#496 was listed here too before rule 5; under it that behaviour is
  correct and the issue is closed won't-do — see the pins bullet below.) #507 (two disagreeing notions
  of tiling ink) reduced to
  an implementation choice, since the model requires the class predicate to agree with what this crate
  actually draws as tiling ink; **it shipped as the dependency inversion** (`651a503`) —
  `treat_glyph_as_background_color` now asks `builtin::owns` rather than restating its ranges, so the two
  can no longer disagree by construction. The *geometric* premise underneath R1 — a tiling glyph is drawn
  to the **cell**, not to its ink box, so a run of them meets — is ADR-0022, which records the same chain
  from the other end. #398 is a **won't-fix with a stated reason**: a
  background-class glyph's ink colour is `L0`'s resolved ink, which includes bold→bright.
- **Existing behaviour is validated, not reversed.** All 100 pins in `frame.rs` / `overlay.rs` /
  `decoration.rs` / `glyph_class.rs` were checked against the model. It reproduces them, and it *derives*
  two decisions that were taken as standalone judgements: #430 (an `ActiveMatch` declares no ink, so the
  selection's ink treatments pass through — rule 2) and #494 (a top decoration replaces, so it replaces
  background-class ink too — rules 3 and 4).
- **The three pins stand; the renderer does not change.** They hold the *selection* colour on a tile
  whose bg an `ActiveMatch` owns (`an_inverse_default_bg_tile_on_an_active_matched_selected_cell_...`,
  `an_active_match_over_a_decorated_transparent_tile_...`) and keep the cell's swapped-in colour on the bg
  channel while the ink goes flat (`an_inverse_default_bg_tile_under_selection_is_transparent`, the bg
  assertion — #496). All three are rule 5: an interaction highlight leaves the content. What reads as an
  intra-cell "seam" in #496's title is the glyph being legible, which is what a glyph is for. **#496 and
  #511 are closed as won't-do**, and #508 keeps its original scope — the decoration route only, where it
  is now fixed: the glyph is dropped by slot so the underline keeps the cell's ink, and rule 4's two
  glyph-only treatments (the #239 re-tint, #226's contrast exclusion) stand down on that cell because
  the glyph they are about is gone. **Rule 4's former limit is lifted (#513).** It read: a cell carries
  one ink colour, so where the glyph is *kept* the line necessarily shares it and rule 4 cannot be
  honoured there. The instance record now carries a second ink, so rule 4 holds on every route — the
  glyph-only rules (the #239 re-tint) stop at the glyph, and the cell-wide ones (a decoration's fg,
  `selectionForeground`, DIM, minimum contrast) reach both — for a *follow-fg* line; an explicitly
  declared `SGR 58` colour is immune to them (the amendment below). Both references arrived at a separate
  channel for the same reason (`RenderableCell.underline`, `textDecorationColor`), which is also where
  a future `SGR 58` lands.
  **`SGR 58` has now landed (#520), and it gives `I_line` two regimes by authorship of the line colour.**
  A *follow-fg* line (no `SGR 58`) rides the cell-wide treatments above, as #513 said — a decoration's
  fg, `selectionForeground`, DIM and minimum contrast all reach it, because its colour is the glyph's.
  An *explicitly declared* underline colour is **authoritative**: drawn raw, immune to every one of those
  — a decoration fg (top or bottom), `selectionForeground`, DIM, and minimum contrast all leave it alone.
  The two-lens (#520 slice 5) established this as the only coherent stance: adjusting an explicit colour
  by some of those rules but not others — which the first implementation did, letting a top decoration and
  dim/contrast rewrite it while selection and a bottom decoration could not — is an invented asymmetry
  with no basis in the layer stack. xterm draws the explicit underline `strokeStyle` raw with its
  threshold-clear disabled, for the same reason. So the axis is **authorship**, the same one rule 5 turns
  on: a colour the application declared for the ~~line~~ **underline** is the application's, and the glyph's
  treatments do not get to rewrite it. (Struck in place, 2026-08-04 / #525: "the line" is the elision the
  next bullet is about — the application declared it for one of two marks, and writing the broader word
  handed the colour to both.)
  One rule is deliberately shared rather than duplicated: **#226's contrast exclusion gates both inks
  together.** It exists because `ensure_contrast_ratio` reads `eff_bg`, so per-cell correction over a
  varying background breaks a run — and an underline is exactly as continuous across cells as a tile
  is. Splitting that gate re-created #513's own symptom through the contrast path; it was measured and
  reverted before merge.
  **The authorship axis, once stated, immediately found the paragraph above under-scoped (#525).**
  *"A declared colour is authoritative over `I_line`"* was written while `I_line` was one ink, so it
  handed the **strikethrough** a colour nobody declared for it: `SGR 58` is the underline's, and no
  SGR sets a strikethrough's. Rule 4 now carries two bands and the regime attaches to the one the
  application actually spoke about. Two things about how this arrived are worth keeping, because
  neither is visible from the fix itself:
  - **The model did not fail to answer — it answered, and the answer was wrong.** Totality's clause
    is written for a combination with *no* answer; this one had a derivable answer that a stale
    premise had quietly made false. That premise was #513's single hardware channel, which was
    perfectly true when rule 4 was written and stopped being true when #513 shipped the channel.
    A rule that inherits a limit as a *definition* goes on returning answers after the limit is
    lifted, and nothing in the model flags it. Amending on the premise rather than on the symptom is
    what stops the same sentence from re-deciding the next band.
  - **The cheap fix was the wrong one, and this model is what says so.** #525's own acceptance text
    proposed drawing the strike *"in the fg"* — free, since `v_fg` is already in the instance record.
    Rule 4 forbids it: `I_line` is `TEXT` class **always**, while `fg` carries glyph-only treatments
    (the #239 re-tint moves it on a selected tile, which is the cell #513 exists for). Taking the
    free route would have re-entered #513's symptom through the new band — the second time this area
    has been bitten by *"reuse the channel that happens to be nearby"*, the first being the contrast
    gate directly above.
- **Totality reaches the alpha property too (#455).** The model names three channels — bg, fg, glyph — but
  a cell also resolves to an *alpha*: whether its background is the see-through default backdrop (#298).
  The shader decided this by arithmetic — `base_bg == u_default_bg` — so a content cell whose composite
  *coincidentally* landed on the default RGB (an `SGR 48` set to the theme bg, an `Indexed` slot resolving
  to it, a decoration painting it) went translucent inside opaque content. That is the exact failure
  Totality forbids: resolution determined by an accident of the fold rather than by the cell's state. The
  fix keys translucency on **provenance** — did any layer in the stack (`L0` inverse, a decoration bg, a
  highlight) write the background? — the same stack rules 2/3 already resolve, so the packer emits the
  answer as a per-cell flag and the shader stops inferring it. A conformance fix, not a new rule: the bg
  channel's provenance was always in the model; only the alpha derived from it was being recomputed by
  colour equality. (The `u_default_bg` uniform the old test needed is gone with it.)
- **This was decided on the visual, twice, and the second one governs.** A record of the *event*, because
  the reasoning alone reads as re-derivable and was in fact re-derived to the wrong answer for most of a
  day. First pass: the maintainer was shown one cell, resolved two ways, and chose the dissolving look —
  which was recorded as three converging rules and generalised into "a bg-only layer replaces the tile
  whichever route it arrives by". Second pass: shown the same rule applied across all four highlight
  states *with neighbouring cells in frame* — a reverse-video status line of box-drawing and shading — the
  same maintainer chose the opposite. Nothing was contradicted; the first artifact could not show what the
  rule cost, because a single cell has no neighbours and no structure to lose. **A prototype scoped to the
  argument rather than to the decision produces a decision about the prototype.**
- **Two named references agree, and that is recorded rather than dismissed.** xterm keeps the tile visible
  by setting `$fg` flat over a blended `$bg` (`CellColorResolver.ts:133-139`); alacritty guards the state
  outright — `content.rs:254-264`, *"Reveal inversed text when fg/bg is the same"*, gated on `!HIDDEN`.
  They disagree with each other about almost everything else in this area, so convergence here is signal.
  The tie-breaker still holds — our model governs cell composition, and a reference is not authority — but
  a model that produces content loss where two independent implementations deliberately prevent it was
  reporting a defect in itself, not a divergence.
- **`frame.rs`'s "Both are intended" comment stands, and its reasoning is now rule 5.** A tile under
  justerm's own `ActiveMatch` keeps the raw selection colour while the same visual pushed as a bg-only top
  decoration goes solid. That is not a consumer choosing between two looks by accident of API; it is
  authorship — the application declaring a cell's colour versus the user passing over it. The comment
  should cite this rule so the difference reads as designed rather than as drift.
- **#506's closure holds.** It described a bg-only top decoration making a tile blink out as the user
  cycles search results, closed as *not currently real* because it needs a consumer porting xterm's
  decoration-based search model. justerm's own active match does **not** behave that way under rule 5, so
  the scenario stays non-native and the stated reason is intact.
- **#496's cost estimate was never tested and is void.** Its body priced option (a) as touching *"the bg of
  every inverse Default-bg cell"*, and an earlier version of these Consequences argued the true reach was
  narrower. Neither matters now: the option is not being taken.
- **A new combination is a lookup, not an issue.** Two-lens output in this area is phrased as "does the
  model answer this?" — a combination it answers needs no issue even when the answer is surprising, and
  one it cannot answer is an ADR amendment. This is the cost the case-by-case default was charging.
- **Totality's first unclosable gap: a preedit supplies a glyph, and no layer can (#249, ADR-0028).** Every
  layer here recolours a channel or blanks a slot; `I_glyph`'s identity belongs to `L0` alone, and rule 5's
  authorship axis — a declared decoration may take the cell's ink, a user's interaction highlight may not —
  has no value for an IME preedit, which is neither. The application did not declare it and the user did not
  pass over it; the *browser* owns it, and the engine never sees it at all. So this is not a combination the
  stack answers badly, it is one it cannot express, which is what Totality says to amend rather than decide
  pairwise. **Resolution: a preedit is a pass, not a layer.** It removes its cells from the stack and
  re-supplies bg, fg and glyph together; nothing underneath shows through, which is also the only way a
  selection tint under a preedit stops reading as *selected text*. Both grid-drawing references reached the
  same structure independently — ghostty excludes the range during row rebuild and emits `addPreeditCell`
  after (`renderer/generic.zig` @ `e6e26e1`), alacritty draws the run with its own `draw_string` pass after
  the grid (`display/mod.rs` @ `852e971`) — so the shape is prior-art-convergent rather than invented here.
  The rules are unchanged for every cell the pass does not cover, and this ADR keeps governing those.
- **Rule 6 cost the packer one bit and the pins caught the one place it was wrong (#712).** The shader
  sees an atlas slot, never a codepoint, so only `pack_instances` can know R1's answer — it was already
  computing it for the #226 contrast exclusion and the #239 re-tint and throwing it away. It now rides
  bit 16 of the glyph field, *above* the `u16` the rest of it fits in: the attribute is read as
  `uint(a_glyph)` and an `f32` carries every integer below 2²⁴ exactly, so the bit was free where a
  thirteenth instance float would not have been. The u16 looked full because it was — the **transport**
  was not. The one judgement this needed came from the existing suite: publishing the class on a glyph a
  top decoration had *taken* (#508) turned five pins red at once. They were right, and for the reason
  this ADR already gives — the glyph-only treatments stand down there *because the glyph they are about
  is gone*, and a class is a fact about a glyph. Visually the bit would have been inert (a blank slot has
  zero coverage); it would still have asserted something false. This is what *"the pins are the
  conformance suite for such a change"* looks like when it fires.
- **The implementation does not yet have the model's shape.** `pack_instances` computes the ink channel as
  a seven-step conditional overwrite chain that *satisfies* the model without *being* it, which is why new
  combinations read as open questions in the code. Restructuring it to resolve ink over the same stack as
  bg is a follow-up; the model holds either way, and the pins are the conformance suite for such a change.

## Alternatives considered

- **(A) Document the decisions as a precedence table.** Rejected. It records the seventeen answers already
  given and says nothing about the eighteenth combination, so the generator keeps running. A filing
  cabinet is not a skeleton.
- **(B) Keep xterm as the parity oracle and maintain a divergence register.** Rejected. It presumes the
  oracle can answer, and in the four most recent decisions it was silent, self-contradictory, judged the
  outlier, or explicitly demoted. Consulting a reference for questions it never posed manufactures work
  and produces "deliberate divergences" that are really just our own unstated model leaking out.
- **(C) Keep deciding each combination as it surfaces.** Rejected — this is the status quo the Context
  measures: 17 of 20 issues by consequence edge, eight re-litigations of one pair, eleven stale premises,
  and a sweep note predicting the serial reinterpretation that then happened.
- **(D) Extend the first-party stance to the whole renderer at once.** Deferred, not rejected. The
  evidence is concentrated in cell composition; rasterisation and colour resolution have not shown the
  same pattern. Revisit if they do.

### For `I_neighbour` (2026-08-20, #791)

WebGL2 has no framebuffer fetch, so ink can reach a neighbouring pixel exactly two ways — the owner
writes into it, or the receiver reads the owner's. There is no third; depth and stencil do not order
colour, and a ping-pong FBO is multi-pass under another name. Stating that is what makes this a
two-way decision rather than an open field.

- **(E) Writer-side: the quad becomes the glyph's own bounding box.** What all three references do
  (xterm.js `GlyphRenderer.ts:53`, alacritty `renderer/text/glsl3.rs:357`, ghostty
  `renderer/generic.zig:3202`, SHAs in `reference-facts.md`). **Rejected, and not for being theirs** —
  the tie-breaker for this layer (**ADR-0031**, which is where that table's row went when the generated
  `thegraph` build was retired on 2026-09-04) says a difference from them is not by itself a defect, and
  the converse holds too: agreement among them is not by itself an argument. Rejected because it costs
  three things this model already owns. ① A quad overlapping its neighbour cannot know what is under
  it, so it needs hardware blending, which needs a **premultiplied** drawing buffer — and straight
  source-over onto a destination of alpha `A` computes `ink·c + dst·(1−c)` where the answer is
  `(ink·c + dst·A·(1−c))/a`, which at `A = 0, c = 0.5` is verbatim the failure #317 §2 measured and
  removed. ② A cell's background and its glyph are **one fragment** here, so ordering foreign ink
  against them means splitting the draw; honouring rule 6 across the split needs **three** passes, not
  two. ③ Even then, two overlapping glyph quads are ordered by *instance order* — by column — so
  foreign-vs-own occlusion returns to being an accident of the fold, which is the defect class #712
  closed. The references pay ①–③ willingly because their model never contained rules 4 and 6; ours
  does.
- **(F) Grow the cell instead — size it from the font's declared line box.** Rejected here, and it is
  **not this ADR's to decide**: it is ADR-0022's alternative (A), still open. It also does not answer
  the question — measured 2026-08-20, under the line-box metric Cascadia Mono still loses 168 glyphs
  and Lucida Console 263, because the cell being *bigger* does not stop it being a scissor. Folding
  the two would move every grid's size in the same change that fixes clipping, and neither decision
  would then be reviewable on its own.

**What (E)'s rejection preserves, stated so it is not re-derived:** the composite stays **one
evaluation per pixel with no GL blending** — #317 §2's `a = 1 − w_bg(1−A)` and
`rgb = (ink + bg·A·w_bg)/a` unchanged, `premultipliedAlpha: false` unchanged — because `I_neighbour`
enters the same `w_bg` product as every other source. Verified by spike before this amendment was
written: with the mechanism switched on, a full-block glyph deposited exactly `cell_w × bleed` pixels
into the adjacent cell, on the correct edge, with the owning cell untouched, and the whole existing
proof corpus stayed green with the spike's uniform at zero. *"Switched off"* means that throwaway
uniform, not a mode the renderer has — `vertical_bleed` floors at its headroom, so every shipped
configuration carries a band. The corpus is 146 cases across four densities as of this amendment,
142 of which predate it. The numbers are on #791.
