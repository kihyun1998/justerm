# Territory — cell compositing

## What it is

Turning one decoded cell into the numbers a GPU draws: resolve its colour *references* against a
palette, apply the colour policies, composite every layer that claims the same pixel, fold the
underline into the glyph field, and pack it into a flat per-cell instance buffer for **one** instanced
draw call.

This is the renderer's hot path, and it is the first place in the family where a colour reference
becomes an actual colour — the engine never does that by identity.

## Governing decisions

- [**ADR-0019 — the cell composition model**](../../adr/0019-cell-composition-model.md) — a layered,
  per-channel, **total** resolution. The model answers a combination *by construction* rather than
  case by case, and a combination it cannot answer is an **amendment**, not a new decision
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — this is the "A-ii"
  hot-path-in-wasm decision the whole crate exists to execute
- [ADR-0024 — decoration projection and precedence](../../adr/0024-decoration-projection-and-precedence.md)
  — the axis ADR-0019 deliberately put out of its own scope (check its `Status:` line; it is not
  copied here)

## Design model

- **An IME preedit is not a layer in this stack — it is a *pass* that removes cells from it**
  (ADR-0019's 2026-08-03 amendment, #249/ADR-0028). Nothing here can *supply* a glyph: every layer
  recolours a channel or blanks a slot, and rule 5's authorship axis has no value for content the
  browser owns and the application never declared. So the composed cells leave the stack at resolve
  time and come back with bg, fg and glyph together, and `pack_instances` stands every stage below
  glyph resolution down inside the run. Replacing only the resolver's *inputs* is not enough and was
  measured not to be: a selection covering the run still tinted it.
  **Every per-cell column owes an answer for a composed cell; which half gives it is free**
  (ADR-0028 D2, #711). Five columns are re-supplied by the patch and `underline_colors` is stood down
  in the packer, and that split is a gate artifact rather than a rule — `0` already means *follow the
  fg the pass supplied*, so either half writes the same value. What bites is a column answered by
  **neither**: `SGR 58` was, for one published release, so a composition drew its underline in the
  colour of the text it had erased. The pass shipped *after* that column existed, so the obligation
  is on whoever writes a pass, not only on whoever adds a column.
  **The mirror of that hazard is a *cell* answered by the wrong half** (#715, same release). The pass
  also writes the pair-repair beside its run — a cell it does not take, only un-pairs — and the patch
  re-supplied that one's colours too, so the application's background vanished for as long as the
  composition stayed open. `preedit::Span` had drawn the line correctly the whole time; the patch had
  not, and the two halves of one pass are what disagreed. The rule both now share: a repair blanks the
  glyph, never the pen.
- **Back-to-front, and decorations sit on *both* sides of the highlight:**
  `base < bottom-decoration < highlight < top-decoration`. A decoration is not simply "above" or
  "below" content — it chooses a side of the selection/search layer, which is what
  `DecorationLayer::{Bottom, Top}` means.
- **Alpha is a channel of the composite, not a postscript to it** (#317 §2). The shader emits one
  straight-alpha RGBA per cell — this renderer enables no GL blending, so the fragment *is* the
  composite. The ink accumulates premultiplied from nothing, the background's surviving weight is
  the product of every ink source's complement, and the two are recombined against `u_bg_alpha`:
  `a = 1 - w_bg(1 - A)`, `rgb = (ink + bg·A·w_bg) / a`. The failure this replaced is the one to
  recognise if it reappears anywhere: the colour was composed against a **fully present**
  background while the alpha declared that background mostly absent, so the two channels described
  different cells. That is ADR-0019's Coherence clause, one axis over from where it is usually read.
  Invisible at `A = 1`, where the two expressions agree exactly.
- **Per-channel, not per-layer.** A layer may claim the background and leave the foreground alone.
  This is what makes "which wins" answerable for an active match over a selection without either
  layer having to win outright.
- **Colour references resolve here, against an injected palette.** `Indexed` and `Rgb` become
  concrete through `Palette`; the engine hands over references precisely so this step is the
  consumer's and the theme stays out of core.
- **Colour policies apply before compositing** — inverse, bold→bright, dim, and the minimum contrast
  ratio. They transform the *cell's own* colours; compositing then layers other things over that.
- **Underline and strikethrough fold into the glyph field**, and their ink is a separate channel
  from the foreground (#513) rather than the foreground itself — so a coloured underline is
  expressible without a second draw.
- **Which ink source ends up on top is a question about their CLASS, not about draw order** (#712,
  ADR-0019 rule 6) — with one source whose position is *not* class-derived: `I_neighbour` sits above
  the receiver's own tile and below everything else the receiver owns, whatever class its owner had,
  because the thing being prevented is one row's letter amputating the next row's. Background-channel ink cannot occlude a `TEXT`-class source, so an underline draws
  *over* a tile and *under* a letter — and the glyph field carries the class (bit 16) because the
  shader sees an atlas slot, never a codepoint. Two things here are easy to miss: this is the one place
  a background-class glyph's ink is *ordered* rather than merely recoloured, and the whole question is
  **invisible unless something declared a colour** — with the inks equal both orders composite to the
  same bytes, which is why it survived undetected until `SGR 58`.
- **The two marks are one ink source split by authorship of the colour** (#525, ADR-0019 rule 4).
  They share the follow-fg pipeline and separate only where something *declared* a colour: `SGR 58`
  declares the underline's and there is no SGR for a strikethrough's. A cell with no `SGR 58` has
  both inks equal, which is what keeps the split from inventing a divergence of its own.
- **The instance is flat and fixed-width**: `col, row, bg(3), fg(3), glyph_field, underline_fg,
  strike_fg, bg_default`, and since #791 also `neighbour_up, neighbour_dn, neighbour_up_fg,
  neighbour_dn_fg` — 16 floats. One buffer, one instanced draw call — the same fixed-stride
  reasoning the wire format uses, for the same reason. **The offsets are named** (`frame.rs`), not
  arithmetic: `INSTANCE_FLOATS - 1` for "the last field" and a literal stride both read the *wrong*
  float the moment a field is appended, and appending four is how that was found.
- **A cell's ink can come from the cell above or below it** (#791, ADR-0019 **R1.2**). A glyph whose
  ink exceeds its cell used to have the excess destroyed — at bake, by a slot the size of the cell,
  and again at sample, by a texcoord inset that never reached past it. The slot now carries a
  **bleed band** above and below, derived per font configuration from what that face overshoots its
  own `█`, and the receiving cell's fragment reads the adjacent slots and folds their coverage into
  the same rule-6 chain. Reader-side, deliberately: the quad stays exactly one cell, so nothing
  overlaps, the composite stays one evaluation per pixel, and no GL blending is involved — the
  writer-side shape every reference uses would have forced a premultiplied buffer and put
  foreign-vs-own occlusion back under instance order.
- **A colour emoji's overflow keeps the ATLAS's colours, and since #794 something proves it.** The
  shader mixes `tex.rgb` under the *owner's* emoji bit, because R1.2 says foreign ink keeps its
  owner's ink and an emoji's ink lives in the texture. The guard could not be built on
  `neighbour-ink.html`'s instrument, which picks its probe from a 2D-canvas `fillText` — that is not
  the path a wide glyph takes. Taking the precondition from the renderer's own bake instead shows
  why: a colour emoji on the **wide** path deposits nothing on its neighbour, while the same
  codepoint on the **narrow** (width-1) path spills 6 device px at dpr 1 and 10 at dpr 2. So
  `demo/emoji-neighbour-ink.html` is built on the narrow path, and states the claim as two
  hypotheses — the overflow's mean colour is nearer the owner's own ink than the foreground every
  cell wears — because the defect it guards produced that foreground exactly.
- **Foreign ink is withdrawn where the two cells' backgrounds differ**, and *resolved* backgrounds:
  two cells can both hold `Default` and still differ once a selection covers one. Two of the three
  producers of that difference are invisible to the packer, so the rule is enforced in two places —
  the **block cursor** is a per-fragment background and is withdrawn in the shader, and a **wide
  pair** is one glyph across two cells whose receivers must reach the same verdict.
- **The packer is pure and host-testable.** Glyph-slot resolution and rasterisation are stateful and
  browser-only; this function takes already-resolved slots, which is what lets the hot path be tested
  without a GPU.
- **Every frame re-packs entirely.** There is no incremental repaint here, which is why the
  engine's damage model targets the *wire* rather than this renderer.

## Code

- `justerm-renderer/src/frame.rs` — `pack_instances`, the instance layout
- `justerm-renderer/src/preedit.rs` — `patch`, `WriteKind`, `Span`: the composition's copy-on-write,
  applied before anything here resolves, and the one place that decides which cells are the pass's
- `justerm-renderer/src/render_policy.rs` — `ColorPolicy`, `resolve_cell`, `dim_foreground`
- `justerm-renderer/src/overlay.rs` — `HighlightKind`, `composite_bg`, `blend_over`,
  `should_blend_kind`
- `justerm-renderer/src/decoration.rs` — `DecorationLayer`, `DecorationRect`,
  `decoration_override_at`
- `justerm-renderer/src/contrast.rs` — `ensure_contrast_ratio`
- `justerm-renderer/src/palette.rs` · `attrs.rs` · `color.rs` — the reference→colour step and the
  attribute decode
- `justerm-renderer/src/webgl.rs` — `packs`, and the policy setters that feed it

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Renderer ink channels](../../agents/reference-facts.md#renderer-ink-channels)
- [How a translucent background composites](../../agents/reference-facts.md#how-a-translucent-background-composites-317-2-verified-2026-08-18)

**Read ADR-0019's own framing before comparing to xterm.js here:** it is a *design input*, not a
validator. In the four decisions before 0019 it was silent, self-contradictory across its own call
sites, the outlier, or demoted — so a difference from it is not by itself a defect.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — this is where every span in the family finally meets the flags: the three overlay lookups, both
  decoration layers and the caret all resolve their pair through one helper (#454)
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — this crate is outside the root workspace, so no `--workspace` or `--all` command reaches it;
  every gate it has is named for it by `--manifest-path`

## Blast radius

- [decoration](decoration.md) — its R1–R6 decide what arrives here; the Bottom/Top split is the
  handshake between the two
- [active match](active-match.md) · [selection](selection.md) · [search](search.md) — the highlight
  layer, and where their overlap is finally resolved
- [caret report](caret-report.md) — a cell-invert caret would be a compositing step; this renderer
  draws it as an overlay instead, so the two stay separable
- [wire format](wire-format.md) — consumes decoded cells; a colour-encoding change lands here first
- [cell geometry](cell-geometry.md) — supplies the box each instance is drawn into
- [glyph atlas](glyph-atlas.md) — supplies the resolved slot this packer assumes; since #791 a slot
  is taller than its cell, and this packer hands each cell its neighbours' slots as well as its own

## Known holes / open

- **The policy setters have no records.** `set_bg_alpha`, `set_minimum_contrast_ratio`,
  `set_bold_to_bright`, `set_selection_foreground` each change what a cell resolves to, and ADR-0019
  governs the *model* rather than the individual knobs.
- ~~**The record feeding the Bottom/Top layers may not be accepted yet**~~ — **accepted 2026-08-18
  (#502)**. ADR-0024's `Status:` line remains the place to check and is still not restated here;
  what changes is that the layers' feed is no longer governed by a proposal.
- **A mark's z-order is only observable where a *declared* colour splits it from the glyph's ink.**
  A strikethrough has no declared-colour regime at all (#525), so its position relative to the glyph
  cannot be asserted by a proof on an ordinary cell — rule 6 states it, and only a cell where a
  glyph-only treatment moves `fg` away from the line inks (a selected tile, #513's own case) could
  see it. Recorded because the natural next test to write here is one that cannot fail.
- **The "every frame re-packs" property is load-bearing and unrecorded.** It is why incremental
  repaint work from the previous renderer was deliberately not ported, and it lives in no record.
