# ADR-0019: The cell composition model — a layered, per-channel, total resolution

Status: accepted (2026-07-21) — **amended 2026-07-22**: the three pins this ADR left for adjudication
were adjudicated *against* the pins. R1 now states its precedence over rule 2, and the Consequences
record the outcome. Scoped to `justerm-renderer`'s **cell composition**; it does not change the core
boundary (ADR-0017) and does not govern span projection (see *Out of scope*).

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
`BACKGROUND` (`treat_glyph_as_background_color` — Powerline / box / block), `I_glyph` belongs to the
**background channel** and takes whatever treatment the bg fold applied. R1 reaches `I_glyph` **only**;
`I_line` and `I_cursor` are `TEXT` class always.

**R1 outranks rule 2** (amended 2026-07-22). Rule 2 says an absent `ink` declaration passes the layer
beneath through — that is how a selection's ink treatments survive under a bg-only `ActiveMatch` (#430).
But `BACKGROUND`-class ink is *not on the ink channel at all*, so a layer that declares only `bg` still
owns it: a bg-only layer above the selection **replaces** the tile, and rule 2's pass-through governs
only `TEXT`-class ink. Without this ordering the model gave two answers for one cell — the state that
sent the three pins below to adjudication. It applies to **every** bg-only layer above the selection,
whichever route it arrives by: a consumer-pushed top decoration (#494) and justerm's own `ActiveMatch`
overlay kind resolve identically. A layer declaring `fg` as well as `bg` is the escape hatch, unchanged.

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

- **The open questions stop being decisions.** #496 (transparent is fg-only) and #508 (underline /
  strikethrough / blink vanish on a tile that follows a top decoration) are **model-conformance defects**
  — rule 4 and the coherence clause answer both. #507 (two disagreeing notions of tiling ink) reduces to
  an implementation choice between extending the classifier and inverting the dependency, since the model
  requires the class predicate to agree with what `builtin.rs` actually draws as tiling ink. #398 is a
  **won't-fix with a stated reason**: a background-class glyph's ink colour is `L0`'s resolved ink, which
  includes bold→bright.
- **Existing behaviour is validated, not reversed.** All 100 pins in `frame.rs` / `overlay.rs` /
  `decoration.rs` / `glyph_class.rs` were checked against the model. It reproduces them, and it *derives*
  two decisions that were taken as standalone judgements: #430 (an `ActiveMatch` declares no ink, so the
  selection's ink treatments pass through — rule 2) and #494 (a top decoration replaces, so it replaces
  background-class ink too — rules 3 and 4).
- **Three pins contradicted the model; adjudicated 2026-07-22 against the pins.** Two hold the
  *selection* colour on a tile whose bg an `ActiveMatch` owns
  (`an_inverse_default_bg_tile_on_an_active_matched_selected_cell_...`,
  `an_active_match_over_a_decorated_transparent_tile_...`); the third keeps the cell's swapped-in colour
  on the bg channel while the ink goes flat (`an_inverse_default_bg_tile_under_selection_is_transparent`,
  the bg assertion — #496). Each was justified in its own comment solely as xterm parity, the footing this
  ADR demotes, and each leaves a cell whose two channels disagree about one surface. Three separate rules
  converge on flipping them: the `BACKGROUND` classification is already load-bearing in three other places
  (#226 contrast exclusion, #239 re-tint, #494 occlusion) and these are its only exception; #241's own
  premise is that a transparent tile *dissolves into the band painted over it*, and under an `ActiveMatch`
  that band is the active colour; and #400 requires a match to read **crisp rather than as a muddy tint** —
  which the current behaviour satisfies on the bg channel while reproducing exactly that muddiness on the
  ink channel. All three are **conformance defects**, fixed as one batch (same cell class; fixing them
  separately would be the pairwise pattern this ADR exists to end).
- **The route difference recorded at `frame.rs` as intended is retracted.** That comment states a tile
  under justerm's own `ActiveMatch` keeps the raw selection colour while the same visual concept pushed as
  a bg-only top decoration goes solid, and calls both intended — "a consumer choosing between the two
  routes is choosing between those two looks". Under the R1 ordering above they are one layer shape with
  one answer, so the choice was never a feature: a consumer asking for the same highlight got a different
  cell depending on which API it reached for.
- **#506 was closed on a premise this amendment falsifies.** It described a bg-only top decoration making
  a tile glyph blink out as the user cycles search results, and was closed as *not currently real* because
  it needed a consumer porting xterm's decoration-based search model. justerm's own active match now
  behaves the same way, so the scenario is native. It is the accepted cost of a solid match (#400), not a
  reason to revisit — but the closure's stated reason no longer holds and must not be read as evidence
  that the behaviour cannot occur here.
- **#496's cost estimate is falsified.** Its body priced option (a) as touching *"the bg of every inverse
  Default-bg cell"*. Under rule 4 transparency is a property of the **background-class ink resolution**,
  not of `L0`, so it reaches only inverse + Default-bg cells carrying a tile glyph;
  `an_inverse_default_cell_blends_over_its_swapped_in_background` (a text cell) is untouched. This is the
  model's one novel, falsifiable prediction — if that pin breaks when #496 is fixed, rule 4 is wrong.
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
