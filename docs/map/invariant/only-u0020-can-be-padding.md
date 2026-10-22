# Invariant — only U+0020 can be a row's padding

## The fact

A grid row is always full: cells the application never wrote hold a blank, and `Cell::default` packs it
as U+0020 with default colours and no flags. So when a reader materialises a row and drops the part the
application did not write, **`' '` is the entire set it may remove.** Anything wider deletes content:

1. **The predicate is a codepoint, never a property.** `char::is_whitespace()` / `str::trim_end()` test
   Unicode `White_Space`, which matches U+00A0, U+2003, U+3000 and more — each of which an application
   had to print to be in a cell.
2. **The engine cannot do better than `' '`, structurally.** A written U+0020 and an unwritten cell are
   the same packed words; `Cell` has no written-bit and the wire no slot for one.
   `justerm-core/src/cell.rs — Cell::is_blank` is the same statement by cell.
3. **The trim belongs at the logical end, not the row's.** Soft-wrapped rows join first; a space at a
   wrap boundary is interior content.
4. **A trim decides where the text ends, so it moves more than a character.** Anything anchored to the
   end — a regex `$`, an index map, a highlight span — moves with it.

## Why it is cross-cutting

There is no shared helper: each reader materialises its own string and writes its own trailing trim, so
the rule is re-decided at every new read surface. At HEAD 7bb31f7 the core trims are separate
`trim_end_matches(' ')` / pop loops in `term/walk.rs — Term::extract_lines`, the block arm of
`term/selection.rs — Term::selection_text`, `term/logical.rs — Term::viewport_logical_lines`,
`term/search.rs — Term::search_with`'s haystack, and `term/paint.rs — Term::changed_logical_lines` /
`Term::paint_logical_line`; the web mirror (`justerm-web/src/cell-mirror.ts`) trims `" "` over decoded
cells and never calls core; the renderer's suggestion (`justerm-renderer/src/suggestion.rs —
is_blank`) applies the by-cell form. None calls another.

The web mirror had reached the correct rule on its own (with a comment that NBSP is not U+0020) while
core had the wrong one — the layers disagreed about one character for as long as both existed, and
nothing compared them.

## Territories it holds in

- [selection](../territory/selection.md) — `selection_text`, twice: the linear arm through
  `extract_lines`, the block arm through its own loop (#685 showed them differing when only one was
  fixed). `Term::text_end` (#1031) decides where a row's text ends by cell (`!c.is_blank()`); it moves
  a highlight span and a `\n`, point 4, and must agree with the `' '` trim. `Term::select_all`'s own
  closure (`c() != ' ' || is_combined()`) counts a wide spacer as padding and agrees only because the
  pair rule widens its end back over the spacer.
- [logical lines](../territory/logical-lines.md) — `viewport_logical_lines`, whose text feeds the
  consumer's URL detection and whose `cells` map must stay 1:1 through the trim.
- [settled-cell paint](../territory/settled-cell-paint.md) — `changed_logical_lines` and
  `paint_logical_line` (#967) share `walk_logical_line` with `viewport_logical_lines` but each trims on
  its own; the paint compares its trimmed text against the query's, so the two must trim alike or every
  paint is refused.
- [search](../territory/search.md) — the haystack's own pop loop. Here the effect is "the character
  does not exist": a written NBSP at a row's end was unfindable, and a regex `$` anchored a column early.
- [accessibility](../territory/accessibility.md) — `Term::accessible_text` (via `extract_lines`) and
  `justerm-web/src/cell-mirror.ts`'s row tree, the independent implementation.
- [grid & scrollback](../territory/grid-and-scrollback.md) — the source: `Cell::default` makes `' '` the
  padding codepoint; reflow uses `Cell::is_blank`.
- [cell compositing](../territory/cell-compositing.md) — the suggestion's run stops at the first
  non-blank cell (`suggestion.rs — is_blank`: `' '` or empty codepoint, plus the flags that ink an empty
  cell), so an NBSP or U+3000 stops it (#972). Added here; this territory already linked back.

The active-match territory cites this note to say its structural excuse does not
apply there (the haystack pops the padding before matching); it is not a site.

Reference behaviour: [reference-facts § Trimming a line's end](../../agents/reference-facts.md#trimming-a-lines-end-685-verified-2026-08-03).
No reference uses a whitespace property on its primary path, and two of three contradict themselves on
a secondary one, so a grep that finds a reference's trim can find the arm that is not its answer.

## What a violation looks like

Silent; the grid is correct, the defect is in extraction:

- **The copy is shorter than the highlight** — `selection_range` covers a cell whose character never
  reaches the clipboard.
- **The character does not exist** — search cannot find it; a regex over logical lines cannot match it.
- **An end anchor lands on the wrong column** — `$` matches the second-to-last cell.
- **A whole wide glyph disappears** — U+3000 is width 2 and `White_Space`, so a property test removed a
  two-column glyph while word selection (a separate injected set) still stopped on it. Two halves of one
  policy disagreeing about one character means a property is standing in for a list.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| Rule reached independently and correctly in the consumer | `justerm-web` a11y row mirror trims `" "` only, NBSP comment | #153 |
| Divergence became visible — a range covering a cell the text lacked | word-boundary set became injected policy | #545 |
| Found and fixed at all four core sites of the time | `extract_lines`, block arm, `viewport_logical_lines`, search haystack | #685 |
| The test harness had it too | `vttest.rs`'s `logical_lines` normalised goldens with `trim_end()`, deleting the character under test | #685 |
| Two more trims written correctly from the start | `changed_logical_lines`, `paint_logical_line` | #967 |
| A by-cell end measure | `text_end` | #1031 |

#545 did not cause this and #153 did not prevent it: the rule was correct in one crate and wrong in the
other the whole time, and surfaced only when the highlight and the copy disagreed loudly enough to file.

## Where it will recur

Test: am I about to decide where a materialised row ends? If so the predicate is `' '` (or
`Cell::is_blank` from cells), the trim goes at the logical end, and anything anchored to the end moves
with it. Two places it will look inapplicable and is not:

- **A test harness or golden generator** — normalising a golden decides where a line ends; a wider
  predicate makes the surface untestable.
- **A consumer that already trims** — the web mirror is not redundant with core; it is another
  implementation, to be checked whenever the rule moves.
