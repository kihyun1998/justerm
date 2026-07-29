# Territory — emoji classification

## What it is

Deciding whether a glyph should be drawn in **its own colours** or tinted to the cell's foreground.
Get it wrong and an emoji renders as a monochrome silhouette in the text colour, or a text glyph
refuses to take the theme.

The answer is a **hybrid of two independent signals**, and the reason it is a hybrid is the whole
content of this territory.

## Governing decisions

**None.**

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) governs how the
  resulting ink is composited, not how the class is decided

## Design model

- **Signal one — the bitmap, which is font ground truth** (#284). Rasterise the glyph and look at
  what came back: a colour glyph arrives in the font's own palette, a text glyph in grayscale white.
  Nothing is more authoritative than what the font actually drew.
- **And it misses exactly one class.** An emoji the font draws in *pure grayscale* — `⬛ ⬜ ⚫ ⚪`,
  monochrome chess and card emoji — has `R = G = B` in every pixel, so the bitmap check reads it as
  text and it renders **tinted to the cell foreground** instead of its own gray.
- **Signal two — Unicode emoji presentation** (#297), a pure text-side check that recovers those.
  The renderer **ORs** the two signals rather than choosing between them: ground truth where it
  works, Unicode where ground truth is blind.
- **Structural signals, not width.** The classification keys off VS16 / ZWJ / emoji-lead rather than
  a width flag — and it *cannot* use core's `wide`, because core computes width **per character**, so
  `FE0F` sequences arrive with `wide = false`. Using width would inherit that blindness.
- **Both halves are host-testable.** The Unicode check needs no GL and no rasteriser, which is what
  makes the class decision provable without a browser even though one of its inputs comes from one.

## Code

- `justerm-renderer/src/emoji.rs` — `is_emoji_text`, the Unicode presentation half
- `justerm-renderer/src/bitmap.rs` — `is_color_bitmap`, the font-ground-truth half
- `justerm-renderer/src/glyph_cache.rs` — takes the resulting `GlyphKind`; the cache allocates and
  does not classify

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. beamterm classified by `width >= 2`, and this design is
explicitly better than that — a comparison stated in commit history and module docs rather than in a
pinned row.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [glyph atlas](glyph-atlas.md) — the class decides which LRU region a slot comes from and how the
  bitmap is interpreted
- [wide glyph](wide-glyph.md) — the reason this cannot key off `wide`: core's per-character width
  makes `FE0F` and keycap sequences arrive as `wide = false`, and DECSET 2027 (#295) is the opt-in
  that changes what arrives
- [colour policy](colour-policy.md) — a colour glyph must **not** be tinted, so the class is an input
  to the tinting rules rather than a consequence of them
- [cell compositing](cell-compositing.md) — where the decision finally shows up as ink

## Known holes / open

- **Zero governing records** for a two-signal design whose second signal exists only because the
  first has a named blind spot — precisely the kind of reasoning that gets simplified away by someone
  who only sees the first.
- **No pinned comparison** against the approach it replaced (`width >= 2`), which is the argument for
  the current design.
- **The union is an OR with no stated precedence.** If the two signals ever disagree in the other
  direction — bitmap says colour, Unicode says text — nothing records what should happen.
