# Territory — built-in block glyphs

## What it is

`U+2580`–`U+259F` — the block and quadrant elements — drawn by the renderer itself rather than by the
font, **to the cell instead of to the ink box**. They are the characters TUIs use to tile a region,
and tiling is exactly what breaks when a glyph is drawn to its own bounds.

It is the **second-largest module in the family and the largest with no governing record at all**
— `find justerm-renderer/src -name '*.rs' | xargs wc -l | sort -rn` for the current ranking, since
a line count written here is a stored answer to a question a command answers better.

## Governing decisions

**None.**

- [ADR-0022 — cell geometry from an ink scan](../../adr/0022-cell-geometry-from-an-ink-scan.md) is
  the *cause* — it establishes the cell/glyph box split this module exists to escape — but it decides
  nothing about intercepting a character range
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — the crate's scope,
  not this

The entire rationale lives in `builtin.rs`'s module doc (#359).

## Design model

- **The problem, stated as the module states it.** `U+2580`–`U+259F` are meant to tile: a region of
  `█` is one solid fill, `▀▄▌▐` halve the cell exactly, `▖▗▘▝` quarter it. The browser draws them as
  glyphs and the renderer masks every glyph to its **ink box** — so as soon as `letterSpacing` or
  `lineHeight` moves the cell away from the ink box, the fills stop meeting.
- **And it is worse than a gap.** The renderer *measures* its cell by ink-scanning `█`. At
  `lineHeight = 1.5` the very glyph that defines the cell no longer fills it — the measurement and
  the drawing disagree about the same character.
- **Both references do the same thing, and that is unusually strong agreement**: xterm.js intercepts
  the range ahead of the font with `CustomGlyphRasterizer` at `deviceCellWidth × deviceCellHeight`;
  alacritty's `builtin_font::builtin_glyph` draws at `average_advance + offset.x` by
  `line_height + offset.y`. Both draw at **cell** size.
- **So the range is intercepted before the font is consulted** and drawn geometrically to the cell.
  The font's own version of these characters is never used.

## Code

- `justerm-renderer/src/builtin.rs` — the whole territory, and its module doc is the only record of
  why any of it exists
- `justerm-renderer/src/glyph_resolve.rs` — where the interception happens on the resolution path

## Reference behaviour

**None** in `docs/agents/reference-facts.md` — although `builtin.rs`'s module doc quotes both
references with their symbol names. That is the shape this map treats as most fragile: a comparison
made once, in prose, in a place nothing re-checks, describing upstream code that can move.

Promoting those two citations into pinned rows would be a cheap, high-value addition — the agreement
is unusually clean and it is currently unverifiable.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — this crate is outside the root workspace, so no `--workspace` or `--all` command reaches it;
  every gate it has is named for it by `--manifest-path`

## Blast radius

- [cell geometry](cell-geometry.md) — the cell/glyph box split is the *cause* of this territory. If
  the nesting rule changes, this module's reason to exist changes with it
- [glyph atlas](glyph-atlas.md) — these glyphs occupy atlas slots like any other, but are produced
  rather than rasterised from a font
- [cell compositing](cell-compositing.md) — a built-in glyph is still a glyph field in the instance

## Known holes / open

- **Thousands of lines with zero governing records and zero public surface.** Nothing about it is
  addressable from outside, and nothing decides it — the largest ungoverned mass measured in this
  family.
- **The reference agreement is quoted, not pinned.** Two named symbols in two upstream projects,
  cited in a module comment, with no SHA behind either.
- **Nothing states which characters are intercepted as a contract.** The range is a constant in the
  code, so a font that draws these characters well is overridden anyway, and a user cannot know that
  from any document.
