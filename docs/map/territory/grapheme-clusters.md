# Territory — grapheme clusters

## What it is

Deciding, for each incoming scalar, whether it **extends** the cluster in the cell the cursor last
printed into or **starts** a new cell — and storing the overflow when a cluster holds more than one
code point. The engine parses one `char` at a time, so this is the **incremental** form of UAX #29
segmentation: decided per scalar, with no lookahead, against a cluster that is still growing. It also
owns the cluster's journey out: the per-span combining group on the wire and the JS-side `sideTable`
the decoder rebuilds from it.

## Governing decisions

**None.**

- [ADR-0005 — binary reference-based serialization](../../adr/0005-binary-reference-based-serialization.md)
  — the wire consequence: a cluster's overflow travels beside the fixed-width cell record. It decides
  the encoding, not the segmentation.
- [ADR-0004](../../adr/0004-spec-faithful-when-alacritty-omits.md) is the tie-breaker the
  variation-selector entry below was kept on (#317 §1); it is a rule about VT semantics, not a record
  about clustering.

## Design model

From reading the source and the issues cited; nothing here is a record.

- **Two halves, and only one is opt-in.** A zero-width scalar always attaches to the cell the cursor
  last printed into (`Term::push_combining`, located by `Term::cursor_cluster_col`), with the mode at
  its default — `e` + U+0301 to a fresh `Engine` gives `combining = {1: ['\u{301}']}` on the span
  (#657, measured with `justerm-wasm-decode/examples/gen_engine_frame.rs`). What DECSET **2027** (`grapheme_clustering`,
  off by default, #295) gates is the *width-changing* half: whether a ZWJ / skin-tone / flag / VS16
  *sequence* collapses into one cell (`Term::try_grapheme_join`). It is opt-in because turning it on
  changes cell widths — a family emoji that took six cells becomes one — and that desynchronises an
  application's own `wcwidth` arithmetic (`grapheme_clustering`'s field doc; #301). DECSTR turns it
  off again (`soft_reset`). Losing this distinction reads as "mode off ⇒ no cluster anywhere".
- **The break decision is delegated to `unicode-segmentation`** (the full UAX #29 rule set) rather than
  reimplemented; what is bespoke is the incremental framing (`grapheme.rs`'s module doc).
- **Nothing on the join path may be O(the cluster's length)** (#867). The engine holds no break state
  between prints — a cursor move would corrupt it. xterm.js persists one and zeroes it at every C0
  execute and dispatch, a granularity that is not behaviour-preserving here:
  `mode_2027_promotion_repairs_an_orphaned_wide_half_at_col_plus_one` feeds CUP and requires the next
  scalar to join the cluster the cursor landed on. So the question is asked against the cell every
  time (ghostty pays O(L) per print for the same choice, rebuilding a `BreakState` over every stored
  codepoint — `src/terminal/Terminal.zig:1150-1168` @ `e6e26e1`), and asking it against the *whole*
  cluster made a growing cluster quadratic: 90 KB of open ZWJ output cost 26 s inside one `feed()`
  (#867). Two rules keep it constant: `joins_cluster` hands the segmenter a bounded **tail** and widens
  it only on `PreContext`; and `UnicodeWidthStr`, still the authority on a cluster's width, is
  consulted only when `width_may_change` says the answer can have moved. **Answering the width
  outright instead of gating an exact oracle is the trap**: a four-line shortcut got eight ordinary
  inputs wrong (`x` + VS16 became a wide cell) and every test still passed — so the gate is swept
  against the oracle in `width_gate_never_skips_a_real_change`.
- **A variation selector on a non-emoji base is kept in the cluster** (#317 §1, decided 2026-08-18).
  `x` + VS16 changes nothing about how `x` looks or how wide it is, but UAX #29 makes VS15/VS16
  `Extend`, so the selector rides the side map; the only place it shows is text extraction. ghostty
  drops it (`src/unicode/grapheme.zig:56` @ `e6e26e16`), and the divergence is narrower than #317
  described: ghostty's own `graphemeWidth('x', 0xFE0F)` returns `len = 2`, so both agree the selector
  is *in* the cluster and differ only on whether the cell **stores** it. A repeated selector never
  moves the width (0 of 196 base/selector combinations at depths 2..8, #867), which is what lets
  `width_may_change` consult the oracle on the first join only.
- **Promotion and demotion move the pair.** A join that grows the cluster to width 2 widens the cell
  in place (`promote_cluster_to_wide`) or, at the last column, relocates it to the next row
  (`relocate_cluster_wide`, #303); VS15 on a default-wide emoji shrinks it (`demote_cluster_to_narrow`).
  The pair obligations those owe are [wide glyph](wide-glyph.md)'s.
- **Storage is the row's combining map, gated by `COMBINED_PRESENT`** (`C_COMBINED` in `cell.rs`). The
  primary code point stays inline in the cell and the overflow sits beside it in `Row`'s column-keyed
  `Combining` map — the cell never grows.
- **A cell holds at most `MAX_CLUSTER_TAIL` (9) code points after its base, and drops the rest
  (#1038).** Unbounded, the tail was the one per-cell payload a stream could multiply: `REP` copies
  the anchor's whole cluster into every cell it repeats into, so `a` + 100 marks + `CSI 65535 b`, 14
  times — 2 926 bytes — retained 445 MiB (measured on 727865d; 80.8 MiB after, bounded by the
  buffer). The value is derived: the longest RGI emoji sequence in `emoji-zwj-sequences.txt` (Emoji
  18.0) is 10 code points, so 9 is the smallest tail that cuts no RGI emoji under mode 2027 — xterm's 2
  (clamped to 5) would cut family and couple sequences, and xterm can afford it only because it has no
  mode 2027. **Overflow is dropped in silence, keeping the first nine**, as xterm's
  `addXtermCombining` does. The maintainer chose the value and the overflow rule on 2026-10-08 from
  three options (9, UAX #15's 30, xterm's 5) shown with their worst-case memory; theirs to reverse.
  Re-derive the number when Unicode publishes a longer RGI sequence. With mode 2027 off this diverges
  from xterm on the object xterm caps (xterm keeps 2, at most 5; justerm 9): one cap serves both
  modes so toggling 2027 never changes how many marks a cell can hold. A scalar the cap drops moves
  nothing: the join path checks for a full tail *before* asking whether the width changes (found by
  the #1038 lens, pinned by `a_scalar_dropped_by_the_cap_does_not_change_the_cell_width`). The mark
  vector grows by doubling clamped at the cap (1, 4, 8, 9, `Row::push_combining`) — an exact
  reservation per mark cost 20 % on cells with three or more marks (#1038).
- **The input is the byte stream**, bounded per cell by `MAX_CLUSTER_TAIL` and per buffer by the
  scrollback cap; the tail the segmenter reads is bounded by the same cap.
- **Width is per character while the mode is off** (see [wide glyph](wide-glyph.md)), which is why VS16
  and keycap sequences arrive as `wide = false` by default and why the renderer classifies emoji by
  structure rather than by width.
- **On the wire the cluster is inlined at its column** in the span's sparse combining group
  (`Span::combining`, v14, #621 — it was a frame-local index into a side table until then, removed
  because nothing ever interned it). Hyperlinks use the same per-span sparse groups.
- **The JS side still presents a side table, and it is not the one that was removed.** The wasm
  decoder rebuilds a per-frame table from the inlined groups, so `DecodedFrame.sideTable` is live at
  v14. It holds **the trailing marks only**, with the base character left in `codepoints` — measured
  end to end (#657): `e` + U+0301 decodes to `codepoints[1] == 'e'` and `sideTable == ["́"]`, never
  `"é"`. The renderer's `DamageFrame::side_table` field doc composes the two; a consumer that renders
  `sideTable[extra - 1]` as the cell's text draws a bare accent, and no type on that seam distinguishes
  the two readings (#646).

## Code

- `justerm-core/src/grapheme.rs` — `joins_cluster` (the tail-bounded break question),
  `width_may_change`
- `justerm-core/src/term.rs` — `Term::place_grapheme`, `Term::try_grapheme_join`,
  `Term::push_combining`, `Term::cursor_cluster_col`, `Term::cluster_text`,
  `Term::promote_cluster_to_wide`, `Term::demote_cluster_to_narrow`, `Term::relocate_cluster_wide`,
  `MAX_CLUSTER_TAIL`, the field `grapheme_clustering`
- `justerm-core/src/cell.rs` — `C_COMBINED` (the bit the docs call COMBINED_PRESENT)
- `justerm-core/src/grid.rs` — `Combining`, `Row::push_combining`, `Row::combining_at`
- `justerm-core/src/serialize.rs` — `Span` (its `combining` group)
- `justerm-wasm-decode/src/lib.rs` — the `sideTable` getter (`side_table`)
- `justerm-renderer/src/frame_grid.rs` — `DamageFrame` (`extra`, `side_table`)

## Reference behaviour

- [Where a combining mark attaches](../../agents/reference-facts.md#where-a-combining-mark-attaches-and-the-four-mechanisms-for-locating-it-865-verified-2026-09-07)
  — where the join point is located, across all four (#865); ghostty runs two paths that disagree
  with each other, and only its 2027 path handles a cursor pinned with autowrap off. It is about
  locating the cell, not about what clustering does once the mode is on.
- [What a mode-2027 join costs](../../agents/reference-facts.md#what-a-mode-2027-join-costs-and-the-model-nobody-else-runs-867-verified-2026-09-08)
  — the per-print cost model each reference runs (#867).
- [Per-cell payload length](../../agents/reference-facts.md#per-cell-payload-length--three-of-four-leave-a-cluster-unbounded-the-binding-one-caps-it-and-a-uri-is-a-different-answer-621-verified-2026-07-29-xterm-row-added-867-2026-09-08)
  — who caps a cluster, and xterm's cap the `MAX_CLUSTER_TAIL` derivation compares against.

The mode-2027 *rationale* — that clustering changes widths and therefore desynchronises `wcwidth` —
is still reasoning recorded in issues and comments, not a comparison against what the references do
with the mode enabled.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the combining map is the first of the
  three and the pattern the others follow: read only through the presence bit, ride with the row,
  clearing the bit retires the fact
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — `sideTable`
  is the worst case: the JS getter rebuilds the whole `string[]` per read, behind a per-cell
  condition, so a mirror that indexed it in the loop rebuilt the table once per cluster cell

## Blast radius

- [wide glyph](wide-glyph.md) — mode 2027 makes a cluster's width a cluster-level fact, so a join can
  create, relocate or destroy a pair; the pair rules assume every width change goes through the three
  promote / demote / relocate sites
- [emoji classification](emoji-classification.md) — classifies structurally *because* it cannot use
  `wide` while the mode is off; a change to when clusters collapse changes what it receives
- [wire format](wire-format.md) — the sparse per-span combining group carries the overflow; its size
  is bounded only by `MAX_CLUSTER_TAIL`
- [logical lines](logical-lines.md) · [selection](selection.md) · [search](search.md) — text
  extraction reads the cluster, not the inline code point, so a change to what a cluster contains
  (the kept VS16, the dropped overflow) changes copied and searched text
- [cursor position](cursor-position.md) — the join point is read off the deferred-wrap flag; the
  attach assumes the flag names the cell the last print landed in
- [vt interpretation](vt-interpretation.md) — `REP` repeats the cluster read off the anchor cell, so
  the cap bounds what a repeat multiplies

## Known holes / open

- **Zero governing records** for an opt-in mode whose activation silently changes how much of the
  screen a string occupies.
- **Almost no reference comparison for mode-2027 behaviour**, in an area where the argument for
  staying off is about how *other* software reacts. Narrowed by #865 (the join point) but the
  width-change question the mode exists for is uncompared.
- **Nothing states what happens to existing cells when the mode is toggled mid-stream.** The
  segmentation changes; the buffer already written does not.
- **The renderer's `DamageFrame::side_table` field doc says core "never unifies ZWJ / skin-tone / flag
  sequences (each of those is its own cell)"**, which is false under mode 2027 — a stale premise in a
  consumer of this territory.
