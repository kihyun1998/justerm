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
  becomes one — and that desynchronises an application's own `wcwidth` arithmetic.
- **What the mode gates is the *width-changing* half, not the combining map.** The distinction is
  easy to lose, and losing it reads as "mode off ⇒ no cluster anywhere". Measured with the mode at
  its default (#657, `examples/gen_engine_frame.rs`): feeding `e` + U+0301 to a fresh `Engine`
  produces `combining = {1: ['\u{301}']}` on the span. A zero-width mark does not change the cell's
  width, so it attaches to the base cell either way; what 2027 decides is whether a ZWJ / skin-tone /
  flag / VS16 *sequence* collapses into one cell (`term.rs:91-94`).
- **The break decision is delegated to `unicode-segmentation`**, the full UAX #29 rule set, rather
  than reimplemented. What is bespoke is the *incremental* framing around it.
- **Storage is the row's combining map, gated by `COMBINED_PRESENT`.** The primary code point stays
  inline in the cell and the overflow sits beside it — the cell never grows.
- **Width is still per character** (see [wide glyph](wide-glyph.md)), which is why VS16 and keycap
  sequences arrive as `wide = false` and why the renderer classifies emoji by structure rather than by
  width. Mode 2027 is what would change that, and it is off.
- **On the wire the cluster is inlined at its column** in the span's sparse combining group (v14,
  #621 — it was a frame-local index into a side table until then, and the table is gone because
  nothing ever interned it). The same
  arrangement hyperlinks use, for the same fixed-stride reason.
- **The JS side still presents a side table, and it is not the one that was removed.** The decoder
  rebuilds a per-frame `sideTable` out of the inlined groups, so `DecodedFrame.sideTable` is alive
  and populated at v14 — a reader who takes "the table is gone" as the whole story will look for a
  dead getter and find a live one. What it holds is the part that decides whether a consumer draws
  the right thing: **the trailing marks only**, with the base character left in `codepoints`.
  Measured end to end (#657) — `e` + U+0301 decodes to `codepoints[1] == 'e'` and
  `sideTable == ["́"]`, never `"é"`. `justerm-renderer/src/frame_grid.rs:41` composes the two
  and says so; a consumer that renders `sideTable[extra - 1]` as the cell's text would draw a bare
  accent, and no type on that seam distinguishes the two readings (#646).

## Code

- `justerm-core/src/grapheme.rs` — the incremental segmentation
- `justerm-core/src/cell.rs` — `COMBINED_PRESENT`, and the inline primary code point
- `justerm-core/src/grid.rs` — `Combining`, the row's column-keyed cluster map
- `justerm-core/src/term.rs` — `Term::grapheme_clustering` (the mode), and the print path that
  extends or breaks
- `justerm-core/src/serialize.rs` — `Span`'s `combining`

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The mode-2027 rationale — that clustering changes
widths and therefore desynchronises `wcwidth` — is reasoning recorded in issues and comments rather
than a comparison against what the references do when the mode is enabled.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the combining map is the first of the
  three, and the pattern the other two follow: read only through the presence bit, ride with the row,
  and clearing the bit is what retires the fact
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — `sideTable`
  is the worst case of it: the JS getter rebuilds the whole `string[]` per read, and it is read
  behind a per-cell condition, so a mirror that indexed it in the loop rebuilt the table once per
  cluster cell

## Blast radius

- [wide glyph](wide-glyph.md) — the width question is decided per character today; mode 2027 is the
  switch that would make a cluster's width a cluster-level fact
- [emoji classification](emoji-classification.md) — classifies structurally *because* it cannot use
  `wide`, which is a direct consequence of the above
- [wire format](wire-format.md) — the sparse per-span groups that carry rare per-cell payloads
- [logical lines](logical-lines.md) · [selection](selection.md) — text extraction reads the cluster,
  not the inline code point, so a change to what a cluster contains changes copied text

## Known holes / open

- **Zero governing records** for an opt-in mode whose activation silently changes how much of the
  screen a string occupies.
- **No reference comparison** for mode-2027 behaviour, in an area where the argument for staying off
  is entirely about how *other* software will react.
- **Nothing states what happens to existing cells when the mode is toggled mid-stream.** The
  segmentation changes; the buffer already written does not.
