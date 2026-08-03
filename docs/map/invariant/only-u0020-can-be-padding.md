# Cross-cutting invariant — only U+0020 can be a row's padding

## The fact

A grid row is always full: the cells an application never wrote hold a **blank**, and
`Cell::default` packs that blank as the codepoint `' '` (U+0020) with default colours and no flags.
So when a reader materialises a row into text and wants to drop the part the application did not
write, **`' '` is the entire set of characters it may remove.**

Anything wider is not a bigger safety margin, it is deletion of content:

1. **The predicate is a codepoint, never a property.** `char::is_whitespace()` / `str::trim_end()`
   test Unicode `White_Space`, which also matches U+00A0, U+2003, U+3000 and a dozen more — every
   one of which an application has to *print* for it to be in a cell at all. A property test cannot
   tell "the app never wrote here" from "the app wrote a space that is not U+0020".
2. **The engine cannot do better than `' '`, and that limit is structural.** A written U+0020 and an
   unwritten cell are the same three packed words; `Cell` has no written-bit and the wire has no
   slot for one. `Cell::is_blank` is the same statement one layer down and is the predicate to reach
   for when trimming *by cell* rather than by character.
3. **The trim belongs at the logical end, not the row's.** Soft-wrapped rows join first, so a space
   at a wrap boundary is interior content. A per-row trim silently deletes it.
4. **A trim decides where the text *ends*, so it moves more than the last character.** Anything
   anchored to the end — a regex `$`, an index map, a highlight span — moves with it.

## Why it is cross-cutting

**Five independent implementations, no shared helper, two crates.** The rule is not enforced
anywhere: each reader materialises its own string and writes its own trailing loop, so the fact is
re-decided at every new read surface rather than inherited. Four of the five are in `justerm-core`
and reach the same cells by different paths (`extract_lines` for linear selection and the a11y
document, a separate per-row loop for block selection, a separate assembly for logical lines, a
separate haystack for search); the fifth is in `justerm-web`, which builds the a11y row tree from
decoded cells and never calls core at all.

That last one is why this is an invariant rather than a note on one territory: **the web mirror had
already reached the correct rule on its own** (with a comment saying NBSP is not U+0020 and therefore
stays) while core had the wrong one — the two layers disagreed about the same character for as long
as both existed, and nothing compared them.

## Territories it holds in

- [selection](../territory/selection.md) — `selection_text`, and it holds **twice**: the linear arm
  through `extract_lines` and the block arm through its own per-row loop. #685's completeness pass
  showed the two returning different text for the same cells when only one was fixed
- [logical lines](../territory/logical-lines.md) — `viewport_logical_lines`, whose text feeds the
  consumer's URL detection and whose `cells` map must stay 1:1 with it through the trim
- [search](../territory/search.md) — the haystack's own trailing loop. The one surface where the
  effect is not "a character is missing from a copy" but "the character does not exist": a written
  NBSP at a row's end was **unfindable**, and a regex `$` anchored one column early
- [accessibility](../territory/accessibility.md) — `accessible_text` (core, via `extract_lines`)
  **and** `justerm-web`'s `cell-mirror` row tree, which is the independent fifth implementation
- [grid & scrollback](../territory/grid-and-scrollback.md) — the source of the constraint:
  `Cell::default` is what makes `' '` the padding codepoint, and reflow's `Cell::is_blank` is the
  by-cell form of the same predicate

## Reference behaviour

In [reference-facts.md § Trimming a line's end](../../agents/reference-facts.md#trimming-a-lines-end-685-verified-2026-08-03)
— linked, never restated. The short of it: **no reference uses a whitespace property on its primary
path**, and two of the three contradict themselves on a secondary one, so a grep that finds a
reference's trim can easily find the arm that is *not* its answer.

## What a violation looks like

All four symptoms are **silent** — nothing throws, and the grid itself is correct in every case,
because the defect is in extraction rather than in storage:

- **The copy is shorter than the highlight.** `selection_range` covers a cell whose character never
  reaches the clipboard. This is the only symptom a user can see directly, and only if they look.
- **The character does not exist.** Search cannot find it; a consumer's regex over logical lines
  cannot match it.
- **An end anchor lands on the wrong column.** A regex `$` matches the second-to-last cell, so a
  match's reported column is off by the deleted run's width.
- **A whole wide glyph disappears.** U+3000 is width 2 *and* `White_Space`, so a property test
  removed a two-column glyph — while word selection, which uses a separate injected set, still
  stopped on it. Two halves of one policy disagreeing about one character is the tell that a
  property is standing in for a list.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| The rule reached independently, and correctly, in the consumer | `justerm-web` a11y row mirror trims `" "` only, with a comment that NBSP is not U+0020 | #153 |
| The divergence became *visible* — a range covering a cell the text lacked | word-boundary set became injected policy, so the two notions of whitespace stopped coinciding | #545 |
| Found and fixed at all four core sites | `extract_lines`, the block arm, `viewport_logical_lines`, `search`'s haystack | #685 |
| The **test harness** had the same defect | `vttest.rs`'s `logical_lines` normalised goldens with `trim_end()`, deleting the character under test — a capture recorded for this would have been unable to fail | #685 |

The shape worth keeping: #545 did not cause this and #153 did not prevent it. The rule was *correct
in one crate and wrong in another* for the whole time both existed, and the only reason it surfaced
is that a third artifact (the highlight) disagreed with a fourth (the copy) loudly enough to file.

## Where it will recur

The next reader that turns cells into a string and wants to drop what the application did not write.
The test is not "does this touch selection" but **"am I about to decide where a materialised row
ends?"** — if so, the predicate is `' '` (or `Cell::is_blank` when working from cells), the trim goes
at the logical end and not the row's, and anything anchored to the end moves with it.

Two nearby places it will look like it does not apply, and does:

- **A test harness or a golden generator.** Normalising a golden is deciding where a line ends, so a
  harness with a wider predicate makes the surface untestable rather than merely untested.
- **A consumer that already trims.** The web mirror is the precedent: it is *not* redundant with the
  core trim, it is a fifth implementation of the same rule over decoded cells, and it has to be
  checked whenever the rule moves.
