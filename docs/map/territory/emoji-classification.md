# Territory — emoji classification

## What it is

The renderer's decision whether a glyph is drawn in **its own colours** (the atlas texel's RGB) or
as coverage tinted to the cell's foreground. Wrong one way, an emoji renders as a monochrome
silhouette in the text colour; wrong the other, a text glyph ignores the theme. The answer is the OR
of two independent signals — the bitmap the font drew and the text's Unicode emoji presentation —
and the reason it is an OR of two is the content of this territory. The verdict is a flag (bit 15)
on the glyph field, and it also decides which atlas region the glyph's slot comes from.

## Governing decisions

**None.**

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) decides how a
  colour emoji's ink is composited (foreign ink carries "the atlas's own colours"), not how a glyph
  is classified as one.
- [ADR-0033 — subpixel coverage](../../adr/0033-subpixel-coverage-is-one-mask-and-a-measured-curve.md)
  consumes the verdict (a colour emoji keeps its own RGB and gets no LCD mask) without deciding it.

## Design model

Status: from source and module docs only (`emoji.rs`, `bitmap.rs`, `webgl/draw.rs`); no record.

- **Signal one — the bitmap, font ground truth** (#284). `is_color_bitmap` rasterises first and
  looks at the pixels: a colour glyph comes back in the font's palette (COLR/CBDT/SVG), a text glyph
  as white with coverage in alpha. Any sufficiently opaque pixel whose R/G/B spread past a threshold
  is colour. This correctly leaves a text-presentation glyph the font draws monochrome (`✂`) as text.
- **It is blind to exactly one class**: an emoji the font draws in pure grayscale (`⬛ ⬜ ⚫ ⚪`,
  monochrome chess and card emoji) has `R = G = B` everywhere and reads as text, so it would be
  tinted to the cell foreground (pinned by `achromatic_emoji_are_emoji_the_bitmap_check_misses_them`).
- **Signal two — Unicode emoji presentation** (#297), `is_emoji_text`, recovers that class. The two
  are ORed at the one call site (`is_emoji_text(text, wide) || is_color_bitmap(&rgba)` in
  `webgl/draw.rs`), so ground truth decides where it can see and the table where it cannot. When they
  disagree in the other direction — the font colour-draws a text-presentation glyph (`❤ ☺ ♥ ✈`
  without VS16) — the OR makes it colour, and that is intended: the bitmap half alone recovers it
  (`width_1_colour_glyph_without_vs16_is_not_text_emoji_relies_on_bitmap`).
- **What `is_emoji_text` keys on.** A single BMP scalar is decided by the exact
  `is_emoji_presentation` table (60 code points, `U+231A`–`U+2B55`); a single SMP scalar by the
  table *gated on core's `wide`*, because the SMP arm is broad. A multi-codepoint cluster is decided
  by structure — a ZWJ joiner, or an emoji-presentation lead (flags, skin tones) — never by the
  cluster's width: beamterm's `width >= 2` falsely matches a wide text base plus a combining mark
  (`wide_text_base_plus_combining_mark_is_not_emoji`), and this crate takes no width table of its own
  (width is core's; see [glyph atlas](glyph-atlas.md)).
- **VS16 is deliberately not a signal** (#301, #302 removed the renderer workaround). By default core
  computes width per character, so `▶️` / `❤️` / a keycap arrive `wide = false`; reclassifying them
  here would cover for core in the wrong layer. A coloured one is still caught by the bitmap half;
  only an *achromatic* VS16 emoji is left tinted (pinned by
  `text_base_plus_vs16_is_not_reclassified_here_core_width_gap_301`). DECSET 2027 (#295) is the
  opt-in under which core promotes a text base + VS16 to width 2 (`Term::grapheme_clustering`).
- **The Legacy Computing block is text.** `U+1FB00`–`U+1FBFF` is carved out of the SMP arm (#361),
  deliberately wider than `builtin::owns`: the whole block is text presentation whoever draws it
  (`symbols_for_legacy_computing_are_text_not_emoji`; see
  [built-in block glyphs](builtin-block-glyphs.md)).
- **What the verdict does.** In `resolve_frame` the verdict and core's width pick the allocation kind:
  wide + emoji → `GlyphKind::Emoji` (wide region), narrow + emoji → `GlyphKind::EmojiNarrow` (normal
  region, so a width-1 colour glyph never thrashes the wide LRU), and either carries `EMOJI_FLAG`
  (bit 15). The fragment shader reads that bit to take the texel RGB instead of the packed foreground,
  and `Rasterizer::finish` returns a colour glyph unchanged instead of building its LCD mask.
- **Accepted trade-off** (#297, recorded in `emoji.rs`'s module docs): with no colour-emoji font, a
  table emoji the browser draws as a grayscale fallback renders in that fallback's white rather than
  the cell foreground — a correctly achromatic emoji and a no-colour-font fallback are the same
  `R = G = B` bitmap.
- **Both halves are host-testable**: neither needs GL or a rasteriser, which is what makes the class
  decision provable without a browser though one input comes from one.
- **Input bound:** the text is one cell's grapheme as the decoded frame delivers it; its length is
  bounded upstream: core keeps at most `MAX_CLUSTER_TAIL` (9) code points after a cell's base
  (`justerm-core/src/term.rs`; see [grapheme clusters](grapheme-clusters.md)). `is_emoji_text` reads
  two chars to tell a single scalar from a cluster and scans a cluster once for ZWJ.

## Code

- `justerm-renderer/src/emoji.rs` — `is_emoji_text`, `is_emoji_presentation` (the Unicode half)
- `justerm-renderer/src/bitmap.rs` — `is_color_bitmap` (the bitmap half)
- `justerm-renderer/src/webgl/draw.rs` — the OR, inside the `resolve_frame` rasterise closure
- `justerm-renderer/src/glyph_resolve.rs` — `resolve_frame` maps (emoji, wide) to `GlyphKind`
- `justerm-renderer/src/glyph_cache.rs` — `GlyphKind::Emoji`, `GlyphKind::EmojiNarrow`, `EMOJI_FLAG`;
  the cache allocates and does not classify
- `justerm-renderer/src/rasterizer.rs` — `Rasterizer::finish` skips the LCD mask for a colour glyph
- `justerm-renderer/src/shader.rs` — the fragment shader's bit-15 colour sample

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The one comparison is in `emoji.rs`'s module docs:
`is_emoji_presentation` is mirrored verbatim from beamterm `beamterm-unicode::is_emoji_presentation`,
and the multi-codepoint rule diverges from beamterm's `UnicodeWidthStr::width() >= 2` for the reason
under Design model — a comparison with no pinned SHA.

## Cross-cutting invariants

**None.**

## Blast radius

- [glyph atlas](glyph-atlas.md) — the verdict chooses the LRU region and the flag a slot carries; a
  kind added here must keep `EMOJI_FLAG` outside the 13-bit slot address the shader masks.
- [wide glyph](wide-glyph.md) — the SMP gate trusts core's per-cell `wide`; a change to what core
  marks wide (mode 2027, a width-table bump) changes which single scalars classify as emoji.
- [colour policy](colour-policy.md) — a colour glyph must not be tinted, contrast-adjusted or dimmed
  through its foreground; the policy assumes the flag already says which glyphs those are.
- [cell compositing](cell-compositing.md) — foreign ink keeps its owner's emoji bit; a glyph that
  changes class changes what spills onto a neighbour.
- [built-in block glyphs](builtin-block-glyphs.md) — the two disagree on purpose about the Legacy
  Computing range; narrowing either to match the other breaks the one that answers its own question.

## Known holes / open

- **Zero governing records** for a two-signal design whose second signal exists only because the
  first has a named blind spot — the reasoning someone who only sees the bitmap half would simplify
  away. It is held only by `emoji.rs`'s module docs and tests.
- **No pinned comparison** against beamterm's `width >= 2`, the approach it replaced.
- **An achromatic VS16 emoji is tinted when mode 2027 is off** (the default). This is the boundary
  rule working, not a renderer defect; it lasts as long as core's default stays per-character.
- **`emoji.rs` still names #301 as the owner of the VS16 gap**; #301 was closed into #295, which
  shipped mode 2027 — the docs predate it (see report).
