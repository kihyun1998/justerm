# Territory — grapheme clusters

## What it is

Deciding, for each incoming scalar, whether it **extends** the previous cell's cluster or **breaks**
into a new cell — and holding the overflow when a cluster needs more than one code point.

The engine parses one `char` at a time, so this is the **incremental** form of UAX #29 segmentation:
the decision has to be made per scalar, with no lookahead, against a cluster that is still growing.

## Governing decisions

**None.**

- [ADR-0005 — binary reference-based serialization](../../adr/0005-binary-reference-based-serialization.md)
  — the wire consequence: a cluster's overflow goes to a frame-local side table so the cell record
  stays fixed-width. It decides the encoding, not the segmentation

## Design model

- **Opt-in, and the opt-in is the interesting part.** Clustering is DECSET mode **2027**, off by
  default, because turning it on *changes cell widths* — a family emoji that occupied six cells
  becomes one — and that desynchronises an application's own `wcwidth` arithmetic. With the mode off,
  input is stored per character, verbatim.
- **The break decision is delegated to `unicode-segmentation`**, the full UAX #29 rule set, rather
  than reimplemented. What is bespoke is the *incremental* framing around it.
- **Storage is the row's combining map, gated by `COMBINED_PRESENT`.** The primary code point stays
  inline in the cell and the overflow sits beside it — the cell never grows.
- **Width is still per character** (see [wide glyph](wide-glyph.md)), which is why VS16 and keycap
  sequences arrive as `wide = false` and why the renderer classifies emoji by structure rather than by
  width. Mode 2027 is what would change that, and it is off.
- **On the wire the overflow becomes a frame-local index** into the grapheme side table — the same
  arrangement hyperlinks use, for the same fixed-stride reason.

## Code

- `justerm-core/src/grapheme.rs` — the incremental segmentation
- `justerm-core/src/cell.rs` — `COMBINED_PRESENT`, and the inline primary code point
- `justerm-core/src/grid.rs` — `Combining`, the row's column-keyed cluster map
- `justerm-core/src/term.rs` — `Term::grapheme_clustering` (the mode), and the print path that
  extends or breaks
- `justerm-core/src/serialize.rs` — `Frame`'s `side_table`

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The mode-2027 rationale — that clustering changes
widths and therefore desynchronises `wcwidth` — is reasoning recorded in issues and comments rather
than a comparison against what the references do when the mode is enabled.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the combining map is the first of the
  three, and the pattern the other two follow: read only through the presence bit, ride with the row,
  and clearing the bit is what retires the fact

## Blast radius

- [wide glyph](wide-glyph.md) — the width question is decided per character today; mode 2027 is the
  switch that would make a cluster's width a cluster-level fact
- [emoji classification](emoji-classification.md) — classifies structurally *because* it cannot use
  `wide`, which is a direct consequence of the above
- [wire format](wire-format.md) — the side table and its frame-local indices
- [logical lines](logical-lines.md) · [selection](selection.md) — text extraction reads the cluster,
  not the inline code point, so a change to what a cluster contains changes copied text

## Known holes / open

- **Zero governing records** for an opt-in mode whose activation silently changes how much of the
  screen a string occupies.
- **No reference comparison** for mode-2027 behaviour, in an area where the argument for staying off
  is entirely about how *other* software will react.
- **Nothing states what happens to existing cells when the mode is toggled mid-stream.** The
  segmentation changes; the buffer already written does not.
