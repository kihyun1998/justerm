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
  *(proposed)* — the axis ADR-0019 deliberately put out of its own scope

## Design model

- **Back-to-front, and decorations sit on *both* sides of the highlight:**
  `base < bottom-decoration < highlight < top-decoration`. A decoration is not simply "above" or
  "below" content — it chooses a side of the selection/search layer, which is what
  `DecorationLayer::{Bottom, Top}` means.
- **Per-channel, not per-layer.** A layer may claim the background and leave the foreground alone.
  This is what makes "which wins" answerable for an active match over a selection without either
  layer having to win outright.
- **Colour references resolve here, against an injected palette.** `Indexed` and `Rgb` become
  concrete through `Palette`; the engine hands over references precisely so this step is the
  consumer's and the theme stays out of core.
- **Colour policies apply before compositing** — inverse, bold→bright, dim, and the minimum contrast
  ratio. They transform the *cell's own* colours; compositing then layers other things over that.
- **Underline and strikethrough fold into the glyph field**, and their ink is a separate channel
  (`line_fg`, #513) rather than the foreground — so a coloured underline is expressible without a
  second draw.
- **The instance is flat and fixed-width**: `col, row, bg(3), fg(3), glyph_field, line_fg,
  bg_default`. One buffer, one instanced draw call — the same fixed-stride reasoning the wire format
  uses, for the same reason.
- **The packer is pure and host-testable.** Glyph-slot resolution and rasterisation are stateful and
  browser-only; this function takes already-resolved slots, which is what lets the hot path be tested
  without a GPU.
- **Every frame re-packs entirely.** There is no incremental repaint here, which is why the
  engine's damage model targets the *wire* rather than this renderer.

## Code

- `justerm-renderer/src/frame.rs` — `pack_instances`, the instance layout
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

**Read ADR-0019's own framing before comparing to xterm.js here:** it is a *design input*, not a
validator. In the four decisions before 0019 it was silent, self-contradictory across its own call
sites, the outlier, or demoted — so a difference from it is not by itself a defect.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [decoration](decoration.md) — its R1–R6 decide what arrives here; the Bottom/Top split is the
  handshake between the two
- [active match](active-match.md) · [selection](selection.md) · [search](search.md) — the highlight
  layer, and where their overlap is finally resolved
- [caret report](caret-report.md) — a cell-invert caret would be a compositing step; this renderer
  draws it as an overlay instead, so the two stay separable
- [wire format](wire-format.md) — consumes decoded cells; a colour-encoding change lands here first
- [cell geometry](cell-geometry.md) — supplies the box each instance is drawn into
- [glyph atlas](glyph-atlas.md) — supplies the resolved slot this packer assumes

## Known holes / open

- **The policy setters have no records.** `set_bg_alpha`, `set_minimum_contrast_ratio`,
  `set_bold_to_bright`, `set_selection_foreground` each change what a cell resolves to, and ADR-0019
  governs the *model* rather than the individual knobs.
- **ADR-0024 is `proposed`** while its rules are what feed the Bottom/Top layers here.
- **The "every frame re-packs" property is load-bearing and unrecorded.** It is why incremental
  repaint work from the previous renderer was deliberately not ported, and it lives in no record.
