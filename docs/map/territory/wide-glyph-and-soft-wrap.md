# Aggregate — wide glyph & soft wrap

**This note owns no detail.** Two concepts sharing one storage problem.

| Concept | Note | ADR-0025 half |
|---|---|---|
| A width-2 glyph occupying two cells | [wide glyph](wide-glyph.md) | **D3, D4** |
| A line continuing onto the next row | [soft wrap](soft-wrap.md) | **D1, D2** |

## Why they sit together

**The scope of each truth is larger than the cell it is stored in.** Soft wrap is a fact about a
*row*; the spacer is a fact about a *pair*; storage in both cases is a per-cell word. So every write
that lands in a cell moves a truth it never intended to touch — and that mismatch, not the glyphs, is
what produced a nine-issue cluster and eventually ADR-0025.

The ADR's own structure follows the split: D1/D2 are about row ownership and lifecycle, D3/D4 about
pair position and pair movement. **The decision line was already drawn**; this map merged what the
record had kept apart.

## Where they actually touch

Two places, and both are causal rather than definitional:

- A **width-2 lead that does not fit at the right margin causes a wrap** and leaves the wide-wrap
  artefact behind — so the wrap rules inherit a pair-shaped mess to clear.
- The **artefact's position test is a wrap fact**: the leading spacer means "artefact" *only* at the
  last column of a soft-wrapped row, which is D3 depending on D1.

Everything else about them is separate: different flags, different verbs, different reference rows,
different failure modes.

## The cluster this area is famous for

`#552`, fifteen issues — one rule rediscovered one VT verb at a time before ADR-0025 named it. The
tell that a change belongs here: **a `Cell` write deciding a fact whose scope is a row or a pair.**
