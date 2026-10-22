# Aggregate — wide glyph & soft wrap

**This note owns no detail.** Two concepts sharing one storage problem and one decision record.

| Concept | Note | ADR-0025 half |
|---|---|---|
| A width-2 glyph occupying two cells, and the artefact it leaves when it wraps | [wide glyph](wide-glyph.md) | D3, D4 |
| A line continuing onto the next row, owned by the row | [soft wrap](soft-wrap.md) | D1, D2 |

## Why they sit together

**The scope of each truth is larger than the cell it used to be stored in.** Soft wrap is a fact
about a *row*; the spacer is a fact about a *pair*; storage in both cases was a per-cell word. So every
write that landed in a cell moved a truth it never intended to touch — and that mismatch, not the
glyphs, produced the cluster that became [ADR-0025](../../adr/0025-row-and-wide-pair-cell-state-ownership.md).
The ADR's structure follows the split: D1/D2 are row ownership and lifecycle, D3/D4 pair position and
pair movement. The decision line was already drawn; a map that merged them would erase it.

## Where they actually touch

Two places, both causal rather than definitional, and both meet in `Term::vacate_for_wrap` and
`Term::end_wrap`:

- A **width-2 lead that does not fit at the right margin causes a wrap** and leaves the wide-wrap
  artefact behind (`vacate_for_wrap` blanks the column *and* calls `begin_wrap`), so the wrap rules
  inherit a pair-shaped cell to clear.
- The **artefact's position test is a wrap fact**: the leading spacer means "artefact" only at the last
  column of a soft-wrapped row (D3 depending on D1), which is why `end_wrap` clears the marker with the
  flag and `shift_region`'s scrollback seam clears both inline.

Everything else about them is separate: different flags, different verbs, different reference rows,
different failure modes. The tell that a change belongs to both: **a `Cell` write deciding a fact whose
scope is a row or a pair.** Spine #552 (closed) is fifteen issues of that tell — one rule rediscovered
one VT verb at a time before ADR-0025 named it.
