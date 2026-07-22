# ADR-0019: The cell composition model — a layered, per-channel, total resolution

Status: accepted (2026-07-21) — **amended 2026-07-22**: the three pins this ADR left for adjudication
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
Four rules; together they answer every cell state.

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
character), `I_line` (underline / strikethrough) and `I_cursor`. **R1:** when the glyph's class is
`BACKGROUND` (`treat_glyph_as_background_color` — Powerline, box / block, and since #507 whatever
`builtin::owns` draws to the cell, asked of the drawer rather than restated), `I_glyph` belongs to the
**background channel** and takes whatever treatment the bg fold applied. R1 reaches `I_glyph` **only**;
`I_line` and `I_cursor` are `TEXT` class always.

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

**What this costs, stated plainly.** The renderer is no longer uniform across routes: the same visual
concept expressed as a decoration erases a tile and expressed as an active match does not. That is a
real seam in the API and it is accepted deliberately — the alternative erases box-drawing and shading
from the screen whenever a user drags across it or steps through search results, which is content loss
in exchange for an internal symmetry no user can observe.

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
  the glyph they are about is gone. **Rule 4 has a limit worth stating**: a cell carries one ink colour,
  so where the glyph is *kept* the line necessarily shares it and rule 4 cannot be honoured there. Both
  references keep a separate channel for this (`RenderableCell.underline`, `textDecorationColor`) — the
  natural home for a future `SGR 58`.
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
