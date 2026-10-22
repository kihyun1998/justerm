# Invariant — every writer into an atlas slot and the shader's read agree on one cell origin

## The fact

An atlas slot is the cell plus a guard band (`PADDING`) and a bleed band (`bleed_x`, `bleed_y`) on each
side (#791). The cell's top-left inside the slot — `(PADDING + bleed_x, PADDING + bleed_y)` — must be
the same number for every party that writes a bitmap into the slot and for the shader that reads it
back. There is no single function that answers it; at HEAD 7bb31f7 it is derived three times from the
same inputs:

1. **The shader's read.** `justerm-renderer/src/metrics.rs — cell_uv` computes the origin as
   `(padded - cell) / 2` per axis and hands it to the shader as `u_cell_uv` (`webgl/draw.rs`).
2. **The font writer.** `justerm-renderer/src/rasterizer.rs — Rasterizer::draw` places a glyph at
   `margin_x()` across and `geometry().draw_origin` down, i.e. `metrics.rs — slot_geometry`'s
   `padding + bleed + char_offset`.
3. **The builtin writer.** `rasterizer.rs — Rasterizer::pad` copies a cell-sized builtin bitmap to
   `(margin_x(), PADDING + bleed_y)` with its own arithmetic.

They agree by algebra (`padded = cell + 2·bleed + 2·PADDING`), not by sharing code. A host unit test
pins `slot_geometry`'s `draw_origin` against `cell_uv` (`metrics.rs —
a_slot_carries_the_horizontal_band_on_both_sides_and_the_glyph_starts_past_it`); the two writers, `draw`
and `pad`, are in a browser-only module (`mod rasterizer` is `#[cfg(target_arch = "wasm32")]`), so no
host test reaches their arithmetic and only the pixel proofs hold it.

## Why it is cross-cutting

The three derivations belong to three concerns — the cell geometry the shader samples, the font
rasteriser, the builtin block glyphs — and none calls another: `cell_uv` reads only `slot.padded`,
`pad` calls neither `cell_uv` nor `slot_geometry`. They share the assumption that the slot's layout is
`band | cell | band` with the guard band outermost. Reshaping the slot (a new band, an asymmetric band,
a different guard) is correct only if all three move, and each lives where the others are not visible.

## Territories it holds in

- [cell geometry](../territory/cell-geometry.md) — owns the slot's shape (`slot_geometry`) and the
  shader's origin (`cell_uv`).
- [glyph atlas](../territory/glyph-atlas.md) — the font writer's placement into the slot and the
  `u_cell_uv` inset the shader samples by.
- [builtin block glyphs](../territory/builtin-block-glyphs.md) — `pad()` places a builtin bitmap at the
  origin `cell_uv` owns, with its own arithmetic.

## What a violation looks like

A writer off by the band: its glyphs sit `bleed_y` rows too high or low in every cell, and the shader
samples band rows as cell rows. For builtin block glyphs the symptom is a run of `█` that no longer
meets its neighbour; for font glyphs, clipped or shifted ink. It shows at every density, but only on a
face whose band is non-zero (with `bleed = 0` all three reduce to `PADDING`, which is why the old
layout never exposed it).

## Discovery history

- **#791** (2026-08-21, `1478b7f`) — the bleed band made the slot asymmetric; `pad` kept `PADDING` on
  both axes for one commit and put every builtin glyph `bleed_y` rows too high. Caught by
  `spacing.html` and `cursor.html` at all four densities (`Rasterizer::pad`'s comment).
- **#966** (2026-09-22, `83f9c9e`) — the horizontal band (`bleed_x`) moved the across axis of all three derivations.

## Where it will recur

Any change to the slot's layout — a band, the guard, the wide-glyph source margin
(`bitmap.rs — split_wide_bitmap`, given `margin_x`) — and any new writer into the atlas (a new builtin
family, an image or sixel cell): it must place at `cell_uv`'s origin, and a host test that compares its
origin to `cell_uv`'s is the only thing that can fail early.
