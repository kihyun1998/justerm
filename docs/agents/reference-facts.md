# Reference facts — what alacritty / ghostty / xterm.js actually do

A Step 5 lens ② pass kept re-deriving the same handful of facts from scratch. They are
**stable** — upstream rarely changes what a terminal does when a wide glyph is
overwritten — so re-fetching them every pass was pure latency. This file is the
accumulated map, so a pass starts from what is already known and spends its budget on
new ground.

**Trees and pins:** `../.refs/`, SHAs in `theflow.md` § "Step 1 — reference routing
table". Every line number here is **at those SHAs**; a pin refresh invalidates the
column and must re-verify the rows it moves.

## The rules for a row — these are what make the file trustworthy

1. **`file:line` or it does not go in.** A prose row ("ghostty defaults here") is
   unverifiable, and an unverifiable row is how a wrong fact gets eternal life —
   #530's body scored the references 2:1 on exactly such a claim and was wrong; the
   real tally was 3:0. A row a reader can `rg` in five seconds cannot do that.
2. **Verify before recording, not before quoting — and do not type the line number.**
   Every row below was grepped against the pinned tree on the day it was added. Copying a
   citation out of an issue body or a lens report *is not verification* — on 2026-07-29
   five rows went in wrong in two days that way (#610), and review caught all five while
   tooling caught none. So the number comes out of the tree, not out of your hands:

   ```sh
   node .github/scripts/cite.mjs xterm.js common/buffer/BufferLine.ts --find 'startCol +='
   node .github/scripts/cite.mjs xterm.js common/buffer/BufferLine.ts:569
   ```

   The first locates the line so nobody counts by eye; the second prints it with two lines
   of context and hands back the `Site` cell to paste. `--pins` checks the local trees
   against the SHAs this file's line numbers are valid at.
3. **The tool does not read the row for you — that half is still yours.** Of those five
   errors it puts the evidence in front of you for three (a wrong file, two off-by-ones);
   the other two were a *correct* citation with a wrong conclusion drawn from it — a `u2`
   field called too narrow to hold a 3 when it holds 0–3, and a mask called a runtime check
   when it admits the value it was said to reject. Nothing mechanical reaches that class,
   and it was the more dangerous of the two: an accurate `file:line` is exactly what makes
   a wrong reading of it credible. Read the lines the tool prints.
4. **Record the mechanism when the site alone misleads.** Two rows below carry a
   "read this too" note because the obvious grep hit gives the opposite answer.
5. **This file does not decide anything.** It records what a reference *does*.
   Whether justerm should match it is ADR/issue territory — and per ADR-0004,
   spec-faithful beats reference-faithful where they disagree.

## Wide glyphs, spacers, and the wrap artefact

| Fact | Reference | Site |
|---|---|---|
| A freed wide half keeps its colours: `clear_wide` removes `WIDE_CHAR` + zerowidth and sets `c = ' '`, leaving `fg`/`bg`/`extra` intact | alacritty | `alacritty_terminal/src/term/cell.rs:171-177` |
| Overwriting near column 0 reaches **back to the previous row** to clear its `LEADING_WIDE_CHAR_SPACER` — gated on `point.column <= 1 && point.line != topmost_line()` | alacritty | `term/mod.rs:1004-1008`, inside `write_at_cursor` (`:984`) |
| Word / inline search skips **both** spacer kinds (`WIDE_CHAR_SPACER \| LEADING_WIDE_CHAR_SPACER`), at three separate sites | alacritty | `term/search.rs:521` `semantic_search_left`, `:548` `inline_search_left`, `:569` `inline_search_right` |
| Reflow **marks** the column it vacates: `T::default()` + `LEADING_WIDE_CHAR_SPACER`, in both directions | alacritty | `grid/resize.rs:155-156` (grow), `:293-294` (shrink) |
| Printing over a wrapped wide glyph clears the previous row's spacer head — **only if it is one**: `if (head_cell.wide == .spacer_head)`, which an AFL++-found regression pins | ghostty | `terminal/Terminal.zig:1501-1506` |
| A spacer head anywhere but the end is a page-integrity violation | ghostty | `terminal/page.zig:537` |
| Row-shift verbs reset the wrap rather than leaving a mid-row marker | ghostty | `Terminal.zig:3133` `deleteChars`, `:3163` `eraseChars`, `:3208` `eraseLine`, all → `cursorResetWrap()` |
| A dedicated hook exists for orphaned spacer heads when a row is shifted | ghostty | `Terminal.zig:2579` `rowWillBeShifted` |

### Relocating a cluster that grew to width 2 (#529, verified 2026-07-28)

justerm's `relocate_cluster_wide` — a narrow base at the last column that a joining scalar (VS16,
a second RI) promotes to width 2, so the whole cluster moves to the next row — is **not** novel, and
the 2:1 split below is the useful part. The concept exists in two references and is *structurally
absent* from the third, while the mechanism it needs (a wide write's far-half repair) is in all three.

| Fact | Reference | Site |
|---|---|---|
| A direct counterpart, named in a comment: *"Combining character widens 1 column to 2. Move old character to next line."* — `copyCellsFrom(oldRow, oldCol, 0, oldWidth, false)`, then the source columns are cleared | xterm.js | `common/InputHandler.ts:583-611`, the move at `:605-607`, the source clear at `:608-610` |
| …and its orphan repair is **not** incidental here: the relocation leaves `x == 2`, so the once-per-print-run right-edge repair tests exactly the column the relocated pair half-destroyed | xterm.js | `common/InputHandler.ts:668-669` |
| A direct counterpart, reached through two `printCell` calls — the second (`.spacer_tail`, at `x == 1`) runs the same `cell.wide != wide` repair switch, so the far half is freed with no rule of its own | ghostty | `Terminal.zig:1188-1252`, spacer-head set at `:1200`, lead at `:1205`/`:1240`, spacer at `:1251-1252` |
| **No counterpart at all** — a width-0 codepoint returns early through `push_zerowidth`, so a cluster never changes width and nothing is ever relocated. A negative result, not an unread file: alacritty is the mechanism reference here but not the concept reference | alacritty | `alacritty_terminal/src/term/mod.rs:1069-1085` |
| ⚠ **ghostty's reach-back fires at its own relocation and clears the marker it just set.** The `.spacer_tail` write lands at `cursor.x == 1`; if the destination cell is `.wide`, the `.wide` arm clears the tail **and then** clears the previous row's `.spacer_head` under `cursor.y > 0 and cursor.x <= 1` — the head this relocation set seven statements earlier. justerm deliberately does not port this: it is #534's mid-construction rule (a repair keyed on a state predicate must not run while that state is being built). **Derived from source, not executed** — the composition is read off the two sites, no ghostty binary was run | ghostty | `Terminal.zig:1200` (the set), `:1251-1252` (the spacer write), `:1484` (the gate), `:1504-1506` (the reach-back) |

### The marker's clear discipline (#534, verified 2026-07-27)

Row 33 and row 36 above name the same repair in the two references, and read together they look like
convergence. They are not the same rule — the *gate* differs, and the difference decides a case that
comes up whenever a wrapped CJK glyph is redrawn in place. Recorded because the two-reference tally
here is 1:1, not 2:0.

| Fact | Reference | Site |
|---|---|---|
| ⚠ **Row 36 above is the *inner* test only.** The reach-back is first gated on the **width changing** — the whole wide-repair `switch` sits under `if (cell.wide != wide)` — so overwriting a wide glyph with **another wide glyph** skips it entirely and the previous row keeps its spacer head. Re-deriving the negative case from row 36 alone gives alacritty's answer, not ghostty's | ghostty | `terminal/Terminal.zig:1484` (the `!=` gate), `:1501-1506` (`.wide` arm), `:1529-1532` (`.spacer_tail` arm) |
| ⚠ The reach-back is gated only on the **overwritten cell** being wide (`cursor_cell.flags.intersects(WIDE_CHAR \| WIDE_CHAR_SPACER)`), then clears unconditionally — so a wide glyph replaced by another wide glyph **loses** a marker that is still true. Alacritty is the outlier of the two; justerm follows ghostty | alacritty | `alacritty_terminal/src/term/mod.rs:994` (the block's gate), `:1004-1008` (the clear) |
| The wrap clear and the spacer-head clear are **coupled in one function**, which is what justerm's `end_wrap` mirrors. Note rows 38-39 point at *callers*: the site is `Screen.zig`, and it early-returns on `if (!page_row.wrap)` where justerm clears unconditionally | ghostty | `terminal/Screen.zig:1524` `cursorResetWrap`, spacer-head clear at `:1539-1545`; callers `Terminal.zig:3133` `deleteChars`, `:3163` `eraseChars`, `:3208` `eraseLine` |
| ⚠ **ghostty *does* reach back to the previous row from an erase and from `DCH`** — not print-path-only, which is the easy wrong conclusion from rows 33/36. `Screen.splitCellBoundary(x)` handles the boundary *before* a move or clear: at `x == cols` it clears a spacer head at the end of a wrapped row, and at `x == 0 or x == 1` — gated on the row being a `wrap_continuation` and `cells[0].wide == .wide` — it reaches **up one row** and clears that row's spacer head | ghostty | `terminal/Screen.zig:1831` `splitCellBoundary`, the `x == cols` branch at `:1849`, the up-a-row branch at `:1873`; callers `Terminal.zig:3107-3109` (`deleteChars`, all three x values) and `:3159-3160` (`eraseChars`) |
| **Negative result, narrowed to what it actually is:** the only justerm call site with no counterpart is **ICH** — ghostty's `insertBlanks` (`Terminal.zig:2988`) calls `splitCellBoundary` nowhere, and alacritty touches the marker on no path but `write_at_cursor` and reflow (all 21 `LEADING_WIDE_CHAR_SPACER` sites checked). xterm.js has no marker to clear. So justerm's erase and `DCH` sites are **ported**, not derived | all three | — |
| ⚠ `splitCellBoundary` gates on the state **before** the mutation, and that is load-bearing rather than incidental: a post-mutation "is a wide lead standing at column 0" test also accepts a `DCH` that pulled the *next* wide glyph left, and a two-step placement (IRM's insert-then-write, a VS16 promotion) satisfies it only at the end. Both were measured diverging from ghostty before justerm's call sites moved to the pre-mutation form | ghostty | `terminal/Screen.zig:1831` (doc comment: *"call this function with `x = a` and `x = b + 1`"*) |
| `rowWillBeShifted` clears the end cell's spacer head on **every** shifted row, gated on `scrolling_region.right == cols - 1 or scrolling_region.left < 2`. ghostty needs the blanket clear for two reasons its own comment names: it shifts cells within pages (splitting interior pairs) **and** it supports left/right margins (DECSLRM), so a partial-row shift can break an interior pair without moving its neighbour. justerm rotates whole `Row`s and implements no DECSLRM, so seam-only is sound — **conditional on justerm never gaining left/right margins**, the day which #540's seam rule and this marker rule break together | ghostty | `terminal/Terminal.zig:2589-2601`, the two-reason comment at `:2586-2594` |

## A span, a highlight and a caret over a wide pair (#454, verified 2026-08-10)

**Read the whole section before quoting one row.** The obvious grep gives the wrong answer twice
here: ghostty's `terminal/Selection.zig` has no `wide`/`spacer` hit at all — its rule is in the
*renderer* — and alacritty's `Selection` does snap, in a method whose name does not say so. A tally
taken from either file alone comes out 1-of-3, and the real answer is that all three have a rule.

| Fact | Reference | Site |
|---|---|---|
| Selection normalises at the **model**: a start on a wide char's second half moves right (`selectionStart[0]++`) | xterm.js | `src/browser/services/SelectionService.ts:573-577` |
| ...and so does an end (`selectionEnd[0]++`), so neither endpoint rests inside a pair | xterm.js | `src/browser/services/SelectionService.ts:667-678` |
| Neither fixup is inside the `_activeSelectionMode !== SelectionMode.COLUMN` guard above them, so a **rectangular** selection gets the rule too | xterm.js | `src/browser/services/SelectionService.ts:659` (the guard), `:667` (the ungated fixup) |
| Selection normalises at the **read** site, per cell: a `WIDE_CHAR` lead counts as selected when its spacer's column is contained | alacritty | `alacritty_terminal/src/selection.rs:85-87`, reached from `alacritty/src/display/content.rs:222` |
| **Read this too**: the `contains(indexed.point)` early return two lines above means it is one-directional — a lead-only range still leaves the spacer unpainted | alacritty | `alacritty_terminal/src/selection.rs:80-82` |
| The wide arm sits **after** the block-cursor early return and carries no `is_block` guard, so a rectangular selection gets it too | alacritty | `alacritty_terminal/src/selection.rs:64-84` |
| The **copy** path has its own half of the rule, spacer-direction only: `cols.start -= 1` | alacritty | `alacritty_terminal/src/term/mod.rs:583-585` |
| Selection normalises in the **renderer**, per cell: a `.spacer_tail` asks the selection about its lead's column (`x -\| 1`) | ghostty | `src/renderer/generic.zig:2747-2753` |
| The same `x_compare` feeds the **search-highlight** test, so one helper covers both | ghostty | `src/renderer/generic.zig:2760-2761` |
| A **caret** on a spacer tail moves back one cell *and* sets `cursor_wide` | ghostty | `src/renderer/generic.zig:2510-2523` |
| A caret's width is `WIDE_CHAR`-only, so on a spacer it paints the glyph's right half alone | alacritty | `alacritty/src/display/content.rs:138-142` |
| A caret takes `cell.getWidth()` — `0` on a spacer — leaving `x >= cursorX && x <= cursorX - 1`, an empty range: **no caret is drawn at all** | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:538-549` (the model at `:541`, the range at `:547`) |

**What the rows add up to.** On a *selection* all three refuse to paint exactly one half, by three
different mechanisms and at three different layers — and the two read-site ones are opposite halves
of the same union (ghostty closes spacer→lead, alacritty closes lead→spacer). On a *caret* they
split three ways with no majority: whole glyph, right half, nothing. So the caret question has no
reference answer to adopt, which is recorded here rather than re-derived.

## Minimum screen size (#547)

Added 2026-07-24. Every row grepped at the pinned SHAs that day.

| Fact | Reference | Site |
|---|---|---|
| Column floor is **2** — *"A minimum of 2 is necessary to hold fullwidth unicode characters"* — but enforced in the **app**, not the engine crate: `Term::new` / `Term::resize` pass `Dimensions` through unclamped | alacritty | const `alacritty_terminal/src/term/mod.rs:35-36`; enforced `alacritty/src/display/mod.rs:249`, `:1627`; unclamped engine entry points `term/mod.rs:410`, `:655` |
| A **row** floor exists alongside it: `MIN_SCREEN_LINES = 1` | alacritty | `term/mod.rs:39`; applied `display/mod.rs:246`, `:265`, `:1628` |
| The **clamped** size is what reaches the PTY — `WindowSize::from(SizeInfo)` reads the clamped fields, so buffer and `TIOCSWINSZ` cannot disagree | alacritty | `alacritty/src/display/mod.rs:186-193`, pushed at `:718` |
| `MINIMUM_COLS = 2, // Less than 2 can mess with wide chars` **and** `MINIMUM_ROWS = 1`, clamped in *both* the constructor and the public resize; `onResize` then fires the **clamped** pair | xterm.js | `common/services/BufferService.ts:13-14`, ctor `:41-42`, event `:55`; `common/CoreTerminal.ts:192-199` |
| ⚠ DECCOLM **bypasses** that public clamp — `?3h`/`?3l` call `_bufferService.resize` directly; harmless only because 80/132 ≥ 2. (justerm has no such bypass: DECCOLM is a `TermEvent::ColumnMode`, so the consumer re-enters the clamped `resize`) | xterm.js | `common/InputHandler.ts:1947`, `:2206` |
| *Why* the boundary is exactly 2: reflow emits line lengths of only `newCols` or `newCols - 1` (the latter when a line ends in a wide char), so at `newCols == 1` the length is 0 and the loop never advances — *"Calling this with a `newCols` value of `1` will lock up."* | xterm.js | `common/buffer/BufferReflow.ts:173`, mechanism `:167-171`, `:202-206` |
| Zero dimensions are **rejected**, not clamped: `ResizeError.InvalidValue`, with a test asserting no mutation on rejection | ghostty | `terminal/Terminal.zig:3679`, `:3721`; test `:3885` |
| ⚠ ghostty says the 1-wide case *"should be prevented downstream"*, but its downstream floor is **1**, not 2 (`@max(1, calc_cols)`) — so it ships the path it documents as *"pretty broken"*. This is the argument **for** flooring in the engine rather than delegating | ghostty | `terminal/Terminal.zig:1422-1426` vs `renderer/size.zig:260-261`; the destroy-the-glyph path `PageList.zig:1783-1788` |

## Logical-line assembly (#601)

Added 2026-07-29. Grepped at the pinned SHAs that day, and the occasion is worth recording: the
[logical lines](../map/territory/logical-lines.md) territory had been carrying *"`text` matches
xterm's `translateToString(true)`"* as its central design claim, flagged in its own `## Reference
behaviour` as **an unpinned paraphrase that had never been grepped**. It was grepped, and it is
partly wrong in **two** places, not one — which is what the flag was for.

| Fact | Reference | Site |
|---|---|---|
| `translateToString(trimRight)` is a **`BufferLine`** method — it spans **one row**, not a wrapped run. So the headline equivalence is false in its load-bearing half: justerm's `viewport_logical_lines` joins soft-wrapped rows, and this does not | xterm.js | `common/buffer/BufferLine.ts:536` |
| Reaching a wrapped run needs a **second** call — the range comes from `Buffer.getWrappedRangeForLine`, which selection drives before translating each line | xterm.js | `common/buffer/Buffer.ts:562` |
| The spacer skip **does** match: xterm advances by the cell's own width, `startCol += (content >> WIDTH_SHIFT) \|\| 1` | xterm.js | `common/buffer/BufferLine.ts:569` |
| ⚠ The right-trim **does not quite** — `trimRight` clips to `getTrimmedLength()`, which scans back for `HAS_CONTENT_MASK`, so a *printed* space (`cp == 32`) is content and survives. justerm's `text.trim_end()` cannot tell a printed space from a blank cell and drops both, so the two disagree on a line ending in typed spaces | xterm.js | clip `common/buffer/BufferLine.ts:553`; the scan `:484-490` |

**Direction: justerm is a superset, and nothing moves toward the reference.** Joining the wrapped run
inside the engine is the whole point of ADR-0017 for this surface — a frame-mode consumer cannot do it
— so the divergence is the feature. Only the *claim* was wrong.

## Maximum glyph width (#595)

Added 2026-07-29. Every row grepped at the pinned SHAs that day. The mirror of the section above:
that one floors the **screen** so a pair always has room, this one caps the **glyph** so a pair is
always enough. Both are preconditions for ADR-0025's D1–D4, which are stated only over *a pair*.

The occasion: `unicode-width` 0.2 reports **3** for `U+17D8` KHMER SIGN BEYYAL, and justerm passed it
through. **3 of 3 references bound it and none permits a triple** — so this was justerm drifting
alone, not a family divergence.

| Fact | Reference | Site |
|---|---|---|
| Clamps at the **source**, in the *comment on the field* rather than in its type — *"We clamp to [0, 2] since Ghostty handles control characters and we max out at 2 for wide characters (i.e. 3-em dash becomes a 2-em dash)"*. ⚠ The field is `u2`, which holds **0–3**, so the type does *not* forbid a triple; the clamp does | ghostty | `src/unicode/props.zig:10-14` |
| **And** asserts at the **write site** — *"it is possible to have a width of `3` and a width of `-1` from uucode.x's wcwidth … `assert(width <= 2)`"*. So ghostty does source **and** site, and its own comment says why the second is reachable: the upstream table is what produces the out-of-range value | ghostty | `src/terminal/Terminal.zig:1310-1315` |
| The width is a **type**, `0 \| 1 \| 2` — and that is the *whole* bound. ⚠ It is **not** enforced at runtime: `extractWidth` returns `((value >> 1) & 0x3) as UnicodeCharWidth`, a checker-only cast, and `createPropertyValue` packs `((width & 3) << 1)`, whose mask admits 3. A provider emitting 3 would carry it through unchanged; none does | xterm.js | type `common/services/Services.ts:342`; packing/extraction `common/services/UnicodeService.ts:22`, `:28` |
| Coerces at the **write site**: `if width == 1 { … } else { WIDE_CHAR + exactly one spacer, cursor += 2 }`, so any width above 1 becomes a pair | alacritty | `alacritty_terminal/src/term/mod.rs:1104-1137` |
| ⚠ alacritty's coercion is **incomplete**: the IRM shift above it runs with the *unclamped* width, so insert mode opens a 3-column gap and writes 2. This is the argument for bounding at the source rather than only coercing at the write branch — justerm clamps in `print`, upstream of its own `insert_chars` | alacritty | `term/mod.rs:1093-1101` (the shift) vs `:1104` (the coercion) |

## Word selection started *on* a separator — the references disagree, so justerm is not an outlier

Added 2026-07-24, clearing a candidate defect raised by #547's Lens ①: justerm's word walkers break
only on the **neighbour** cell's class, never on the start cell's own, so word-selecting the interior
space of `"ab cd"` returns `"ab cd"` — both words joined. That looked like a bug and is not.

| Fact | Reference | Site |
|---|---|---|
| Whitespace **is** a semantic escape char (`",│\|:\"' ()[]{}<>\t"`, note the space) — but both semantic walkers **exclude the starting cell**, so starting *on* a space never terminates there and the selection joins the words on either side. `iter_from(point)` never yields `point`: `next()` advances *before* returning, `prev()` decrements before returning | alacritty | const `term/mod.rs:45`; `term/search.rs:541` `inline_search_left` (`iter.prev()` first), `:564` `inline_search_right`; iterator semantics `grid/mod.rs:412` `iter_from`, `:595-609` `next` |
| The **opposite** design: an explicit branch on the start cell's own class — `if (line.charAt(startIndex) === ' ')` expands over whitespace *only*, `else` expands until whitespace. Double-clicking a space selects the whitespace run and never crosses into a word | xterm.js | `browser/services/SelectionService.ts:858-865`, inside `_getWordAt` (`:833`); the caller carries an explicit `allowWhitespaceOnlySelection` flag (`:344`, `:988`) |

| The **same** design as xterm.js, reached independently: `selectWord` derives `expect_boundary` from the **start cell's own** class and then walks while `this_boundary == expect_boundary` (`:3240` forward, `:3277` backward). So double-clicking a space selects the whitespace run and never crosses into a word | ghostty | `src/terminal/Screen.zig:3217` |

**Verdict recorded so it is not re-litigated: justerm converges with alacritty, and the references
split.** So "the walk crosses a separator" is not by itself evidence of a defect here — by the
two-lens divergence-direction rule, a split reference means this is a *product* choice, not a
correctness fix.

⚠ **The split is 2:1, not 1:1 — corrected 2026-08-03 (#545).** This section shipped tabling only
alacritty and xterm.js and concluding "the references are 1:1"; ghostty is a third data point and
it sides with xterm.js (row 3, added above). The verdict is unaffected — 2:1 is still a split, so
this is still a product choice — but the arithmetic was wrong, and it was wrong in the direction
that flatters justerm's position. Recorded rather than silently fixed, because "the references are
evenly split" and "justerm is in the minority of three" are different things to hand the next
person deciding here.

Valid as long as justerm's walk keeps *never testing the start cell's own class* — which is a
property of `word_start` / `word_end`'s loop shape (each steps to a neighbour before applying the
predicate), **not** of the boundary set's contents. That distinction was not visible when this was
written, because the set was hardcoded and the two moved together; #545 made the set consumer
policy, so it is worth being explicit: no choice of separator set can break this clearance, and
only adopting the `expect_boundary` design would.

## What a blanked / freed cell is made of

| Fact | Reference | Site |
|---|---|---|
| A freed cell takes the **cursor style's background**: `printCell` → `Screen.clearCells`, which fills `blankCell()` | ghostty | `terminal/Screen.zig:1667` `clearCells`, `:1929` `blankCell` |
| ⚠ **Not** `page.zig`'s `clearCells`, which memsets to zero — that one is for inter-page row copies only. Grepping `fn clearCells` finds both; taking the first hit is how #530's body reached "ghostty is the outlier" | ghostty | `terminal/page.zig:1215` |
| The erase fill is default-everything **plus the pen's background**: `DEFAULT_ATTR_DATA.clone()` with `bg \|= curAttr.bg & ~0xFC000000` — i.e. `reset(); set_bg(pen.bg)` | xterm.js | `common/InputHandler.ts:3436-3440` `_eraseAttrData()`, base at `:111` |
| Reflow padding is a **default** cell, not a pen-derived one — `nullCell` throughout | xterm.js | `common/buffer/BufferReflow.ts:83`, `:89` |

## Mapping a tracked point through reflow (#549, verified 2026-07-27)

justerm hands `reflow` a list of `points` — the cursor, selection anchors and every OSC-133 command
mark — and gets their new positions back. The question these rows answer is *where* each reference
decides that position, because deciding it **after** the re-split, by dividing the offset by the new
width, is only right if every emitted row is full.

| Fact | Reference | Site |
|---|---|---|
| The cursor is re-anchored **inside** the resize loop, on the iteration that processes its own line: `if i == cursor_buffer_line && reflow`, moving it by `num_wrapped` — the count of cells actually wrapped — rather than by any arithmetic on the new width | alacritty | `alacritty_terminal/src/grid/resize.rs:167` (`cursor_buffer_line`), `:169-188` (the branch) |
| ⚠ It also handles the point that sits **past** the content, and does it by clamping *back onto* the row: *"Clamp to the last column, if no content was reflown with the cursor"* → sets `input_needs_wrap` and subtracts one more. That is a different answer from justerm's (which moves such a point to the start of the next row when the last row is full) — but it is not comparable as-is, because alacritty first moves the cursor *outside the grid* when `input_needs_wrap` was already set (`:113-116`) and restores it after | alacritty | `alacritty_terminal/src/grid/resize.rs:173-177`, and the pre-pass at `:113-116` |
| The larger-reflow path **refuses to touch any wrapped run containing the cursor** — *"If these lines contain the cursor don't touch them, the program will handle fixing up wrapped lines with the cursor"* — gated on the `reflowCursorLine` parameter | xterm.js | `common/buffer/BufferReflow.ts:23` (the `@param`), `:45-51` (the skip) |
| Tracked pins are moved **by assignment from the write cursor's live position** inside the reflow write loop — `p.node = self.node; p.x = self.x; p.y = self.y;` — with a second remap for the cell the spacer `.skip_next` path jumps over. This is the closest prior art to justerm's `points`, and it is arithmetic-free | ghostty | `terminal/PageList.zig:1650-1659`, second remap at `:1665-1676` |
| **No reference computes a post-reflow position by dividing an offset by the new width — 3 of 3.** All decide it where the real extent is known. Recorded as a negative result because that arithmetic is the obvious implementation and was justerm's until #549 | all three | — |

**And the harder half, which justerm's model cannot express (found by the #549 pass, not yet fixed).**
A tracked point may sit *one past* the last cell — a `pending_wrap` cursor, an anchor in the trailing
blanks, an OSC-133 mark at end of line. justerm returns a `(row, col)` grid coordinate, so it has to
pick an in-grid approximation; **neither reference ever has that problem**, and they avoid it in two
different ways:

| Fact | Reference | Site |
|---|---|---|
| The cursor is lifted **outside the grid** before reflow when `input_needs_wrap` (`point.column += 1`) and restored afterwards by clamping to the last column and re-arming the flag. Two pre-passes, one per direction | alacritty | `alacritty_terminal/src/grid/resize.rs:113-116` (grow), `:248-251` (shrink), restore at `:173-177` |
| ⚠⚠ **The shrink path's cursor reconciliation — the ONLY reference site that *sets* a wrap flag from a reflow's output.** Two guards, and both are load-bearing there: the column must equal the new width **exactly**, and the reflowed last cell must **not already be `WRAPLINE`**. Arming on a property of the *content* instead (justerm's first attempt used "the point's offset reached the line's end") is far wider and deletes hard line breaks: measured on `"abcd\r\nWXYZ"` resized 6→2, alacritty and that rule agree at cursor column 2 and 3 and disagree at 4 and 5. **This row exists because the site was not in this file and so was not read** | alacritty | `alacritty_terminal/src/grid/resize.rs:374-384` |
| ⚠⚠ **CORRECTION (#562): the row above is *not* prior art for a justerm seam that arms `pending_wrap` from a reflow result — the transfer is invalid, and this file said the opposite.** It previously called that site "the true prior art for anything like `Cursor::set_reflowed_point`". The two `column == columns` do not mean the same thing: alacritty lifts the cursor outside the grid **only when `input_needs_wrap` was already set** (`:113-116`, `:248-251`), so its guard is re-arming a flag the cursor *arrived with*. justerm never hands `reflow` a one-past column at all — `Cursor::point()` returns `col ≤ cols - 1` and `pending_wrap` is carried beside the reflow — so every `col == new_cols` coming back out is a cursor **parked past content by CUP**, never a deferred wrap. The guards therefore cannot fire, which is why porting them still deleted hard breaks; measured, and on the alt screen it destroyed a line one byte *after* the resize | alacritty vs justerm | `alacritty_terminal/src/grid/resize.rs:113-116`, `:248-251`, `:374-384` vs `justerm-core/src/cursor.rs` `point()` |
| **The cursor pin and every other tracked pin are treated differently, in the same function.** A non-cursor pin past the content is **clamped first** — `if (p.x >= cols_len) p.x = @min(p.x, self.page.size.cols - 1 - self.x)` — and only then widens the row; the cursor pin gets the widening with no clamp. So a selection anchor or a mark can never move content to make room for itself, and the cursor can. This is the source-level form of "UI state must not move app content; app state may", and it is the split justerm's seam ports (#562) | ghostty | `terminal/PageList.zig:1576-1585` (other pins), `:1602-1606` (cursor pin) |
| **0 of 3 references reflow the alt screen** — one flag on the same resize function, no special path. ghostty: *"Alternate screen, if it exists, doesn't reflow"* then `alt.resize(.{ .reflow = false })`; alacritty passes `reflow` as `!is_alt` / `is_alt` so whichever grid is the alt one gets `false`; xterm.js gates reflow on `_hasScrollback` and builds the alt buffer with `hasScrollback = false`. **justerm diverged until #567 and has now converged** (2026-07-28). It was never a decision: `8f09d58` needed both screens to take the new *dimensions* and reached for a helper that re-splits when the columns change. Two things measured while settling it are worth keeping — turning it off reddened exactly **one** test in the workspace (#187's consequence pin, and #187's own acceptance criteria mention neither resize nor reflow), and re-splitting is **harmful, not merely useless**: on a real `htop` recording taken across a live `SIGWINCH` it leaves debris in the cells htop does not overwrite, because htop repaints without erasing. `vim` clears first and cannot tell the difference — which is why one application was not enough to answer the question | all three | ghostty `terminal/Terminal.zig` `resize` (the comment above the alt branch), alacritty `alacritty_terminal/src/term/mod.rs:677-678`, xterm.js `common/buffer/Buffer.ts:315-320` + `common/buffer/BufferSet.ts:47` |
| The **saved** cursor is *clamped*, never re-armed — the line immediately after the block above | alacritty | `alacritty_terminal/src/grid/resize.rs:386` |
| ⚠ A blank row is **deferred**, not free. On `cols_len == 0` the reflow returns early and only bumps a counter (`if (!src_row.wrap_continuation) self.new_rows += 1; return;`) — and `:1634-1637` pays the debt the moment a non-blank row follows: `while (self.new_rows > 0) try self.cursorScrollOrNewPage(list, cap);`. What is actually free is a blank row with **nothing after it**, which is what its own comment says (*"so that blank rows at the end of the page list are never written"*). **This row previously claimed "costs no destination row" as a general rule**, which justerm's join happens to satisfy — it absorbs only *trailing* blanks — but the claim was wrong and the correct reading is the one #567 ① ported: ghostty **spends** the row, by scrolling, when there is something to scroll into | ghostty | `terminal/PageList.zig:1610-1616` (the defer), `:1634-1637` (the payment) |
| ⚠⚠ **`cols_len = @max(cols_len, p.x + 1)` does NOT grow the row past its width — it stops the *trim* from cutting below a pin the row already contains.** `cols_len` starts at `size.cols` and is walked *down* over trailing empty cells (`:1564-1570`); the pin clause raises that floor, nothing more. `p.x` is a column in `0..cols-1` by construction. **Misread twice while designing #549's follow-up** — once as "grow the row so a point past the end fits" (it does not; it cannot) and once as "irrelevant to a point past the end" (it is the reason ghostty never has one). Read it as: *a cell a pin sits on is content, so do not trim it away.* Comment on the cursor clause: *"If the cursor is after blanks on the right, those cells are still before the next write and must reflow with it"* | ghostty | `terminal/PageList.zig:1564-1570` (the downward trim), `:1584-1596` (tracked pins), `:1602-1606` (cursor pin) |
| ⚠ **ghostty has no past-the-end pin at all**, which is why the clause above is sufficient *there* and not here: `pending_wrap` keeps its cursor at `cols - 1`, so a pin's column is always a real cell. A port of the clause alone does not give justerm the same guarantee — justerm's tracked points arrive as offsets into a joined logical line, where `off == len` is reachable | ghostty | `terminal/Screen.zig:2094-2097` (the `pending_wrap` repair) and `:2106-2108` (the un-mappable fallback, which zeroes the position *and* clears the flag — unrecorded until now), `page.zig:2210` `isEmpty` |
| ⚠ ghostty's trim is **background-agnostic** for a textual cell — `isEmpty` is `!hasText() and self.wide == .narrow`. So a BCE-coloured blank *is* trimmed, and a cursor parked in a BCE tail then raises `cols_len` back over it: in ghostty that tail costs a row. justerm deliberately does **not** follow this (#530: a BCE tail must not re-split into an extra row, and a prompt redraw — `CUP` then `EL` under a colour — parks the cursor in exactly such a tail). Direction: **justerm and its own record agree against ghostty**; carried as a family decision, not a defect | ghostty | `terminal/page.zig:2210` `isEmpty`, vs `justerm-core/tests/reflow_trim.rs` |
| A blank row is **not absorbed** if it carries a semantic prompt: `if (cols_len == 0 and src_row.semantic_prompt != .none) cols_len = 1;` — the direct analogue of justerm losing an OSC-133 mark on a trailing blank line | ghostty | `terminal/PageList.zig:1573` |
| `pending_wrap` survives a column resize — `cursorReload` never touches it, and the saved cursor's copy is repaired explicitly (*"If we had pending wrap set and we're no longer at the end of the line, we unset the pending wrap and move the cursor"*) | ghostty | `terminal/Screen.zig:2092-2098` |
| The state is encoded as `x === cols` (a column one past the end is representable by construction), and the reflow skips the cursor's wrapped run rather than mapping it | xterm.js | `common/buffer/BufferReflow.ts:45-51` |

## Soft wrap is a row property

| Fact | Reference | Site |
|---|---|---|
| `wrap` and `wrap_continuation` are fields on the row | ghostty | `terminal/page.zig:1938`, `:1942` |
| `isWrapped` is a field on the line | xterm.js | `common/buffer/BufferLine.ts:87` |
| ⚠ The explicit `clearWrap` argument is on the **erase helper**, not on `replaceCells`: `_eraseInBufferLine(y, start, end, clearWrap, respectProtect)`. `replaceCells(start, end, fillCellData, respectProtect)` has no such parameter | xterm.js | `common/InputHandler.ts:1175`; `BufferLine.ts:342` |
| `clearWrap` is passed `true` only when the erase reaches the whole line — `x === 0` at `:1236` (ED-from-cursor) and `:1323` (`EL 0`), `true` at `:1246` (ED-to-cursor) and `:1329` (`EL 2`), `false` at `:1326` (`EL 1`) | xterm.js | `common/InputHandler.ts:1236, 1246, 1323, 1326, 1329` |

| Which verbs *end* a wrap is a **per-verb** rule, not derivable from the erased range. `EL 0`, `ECH` and `DCH` end it at **any** column; `EL 1` and `ICH` never do | ghostty | `terminal/Terminal.zig:3208` (`eraseLine(.right)`), `:3163` (`eraseChars`), `:3133` (`deleteChars`, comment *"Our row's soft-wrap is always reset"*) |
| ⚠ `EL 2` does **not** end the wrap in either C xterm or ghostty — xterm's `ClearLine` has no `LineClrWrapped`, and ghostty copies that deliberately: *"it seems like complete should reset the soft-wrap state of the line but in xterm it does not"*. This is the one place justerm diverges (see #538) | ghostty | `terminal/Terminal.zig:3226` and the comment above it |
| C xterm ends `ClearRight` with `LineClrWrapped(ld)` **unconditionally**, comment *"with the right part cleared, we can't be wrapping"* — reached by `EL 0` and by `ECH`. Note this contradicts the xterm.js row above (`clearWrap` only when the erase covers the whole line): the two references genuinely differ, and xterm.js is the outlier | xterm (C) | `util.c:1871`, callers `:1961` (ECH) and `:1979` (EL 0) — **not** in `../.refs/`, fetched from `ThomasDickey/xterm-snapshots` |

### Row-shift verbs and the wrap link (#540, verified 2026-07-25)

No reference repairs the wrap link a row shift falsifies. Recorded in full because the #540 issue
body claimed the opposite for two of the three, and because a future pass will otherwise re-derive
a 0-of-3 answer from scratch.

| Fact | Reference | Site |
|---|---|---|
| A full-width `IL`/`DL` clears `wrap` **and** `wrap_continuation` on **both** rows of every shifted pair, gated on the region being full width (`if (!left_right)`) | ghostty | `terminal/Terminal.zig:2746-2752` (insertLines), `:2906-2912` (deleteLines) |
| ⚠ That clear runs **before** the row swap (`dst_row.* = src_row.*`), so both ends stay `false` — ghostty *destroys* interior wrap links rather than carrying them. The naive reading ("clear then the copy restores it") is wrong | ghostty | swap at `terminal/Terminal.zig:2936-2939` |
| ⚠ Neither verb touches the row **above** the shifted range: the loop starts at `cursor.y`, and `rowWillBeShifted` only clears spacer heads and split wide chars. ghostty's own text join reads the upper row's `wrap` (`formatter.zig:1109`), so it carries the same top-seam defect | ghostty | `terminal/Terminal.zig:2837-2860` (loop), `:2579-2620` (`rowWillBeShifted`) |
| There is **no** `WRAPLINE` clear on any scroll path — the only insert is at the cursor cell in `wrapline` | alacritty | `alacritty_terminal/src/term/mod.rs:968`; `grid/mod.rs:191` (`scroll_down`), `:252` (`scroll_up`) |
| `insertLines`/`deleteLines` splice whole line objects and never touch `isWrapped`; the inserted line is `getBlankLine(...)`, whose `isWrapped` defaults to `false` | xterm.js | `common/InputHandler.ts:1345-1402`; `common/buffer/Buffer.ts:102-103` |
| ⚠ The mirrored polarity ("I continue the *previous* row") is **not** immunity — it moves the exposure to the other seam, where a spliced-in line keeps a continuation claim about a predecessor it never met. The join walks `lines.get(y).isWrapped` upward/downward | xterm.js | `common/buffer/Buffer.ts:566-570` |
| `printWrap` guards marking the row wrapped on `cursor.x == cols - 1` — a **right-margin** condition, so it does not answer "will the linefeed actually advance?" | ghostty | `terminal/Terminal.zig:1611-1617` |

**And the exemption the seam rule needs (#557, verified 2026-07-27).** The rows above are about the
*clear*; this is about when it must **not** fire. `wrapline` scrolls the region so a line that ran
past the last column has somewhere to continue, and the seam rule then ends the wrap it was serving.
The 0-of-3 tally above is right for the clear and **wrong for this**: one reference carries the same
caller fact in the same place, and none of the three splits the line.

| Fact | Reference | Site |
|---|---|---|
| ⚠⚠ **The scroll primitive takes "this scroll was asked for by an auto-wrap" as a parameter** — `public scroll(eraseAttr: IAttributeData, isWrapped: boolean = false)` — and stamps the new line with it (`newLine.isWrapped = isWrapped`). **Exactly one of four non-test callers passes `true`: the auto-wrap branch of `_print`.** `lineFeed()`, `index()` and the ED-2 `scrollOnEraseInDisplay` loop all take the default. justerm's `serves_wrap` is the same fact at the same seam with mirrored polarity — xterm.js stamps the *destination*, justerm exempts the *source's* clear | xterm.js | `common/services/BufferService.ts:68` (signature), `:74`/`:77` (the stamp); callers `common/InputHandler.ts:588` (`true`) vs `:750`, `:1270`, `:3366` |
| ghostty expresses the same caller fact as *ordering* rather than a parameter: `row.wrap = true` before `index()`, `wrap_continuation = true` on the destination after it — and its wrap clears live only in `insertLines`/`deleteLines`, which `index()` reaches only under DECSLRM where the clear is gated off by `if (!left_right)`. So its wrap-serving scroll is structurally exempt | ghostty | `terminal/Terminal.zig:1617`, `:1640-1644`; `index` dispatch `:2219-2258`; the gated clear `:2906-2912` |
| **Negative result:** none of the three produces a split logical line for `"[1;3r[3;1Habcdz"`. Pre-fix justerm diverged from all three; post-fix it converges with all three | all three | — |

**Correction, recorded because it propagated.** Several justerm artefacts state that
xterm.js "makes `replaceCells` take `clearWrap` as an explicit argument" — #538's body,
two merged commit messages, and doc prose in `term.rs` / `architecture.md`. The
*argument* those passages make survives (a row property should be an explicit
parameter, not a side effect of clearing a cell); the function named is wrong. The
`clearWrap` half of #538's acceptance cites `_eraseInBufferLine` correctly, so the same
change carries both the right and the wrong name.

## Damage / dirty tracking (#536, verified 2026-07-28)

This file had **no damage section at all** before #536, which is why justerm's `damage_span` shape
was never checked against anything. The headline is that justerm's granularity is the outlier and
nothing upstream can supply a bound for it.

| Fact | Reference | Site |
|---|---|---|
| Column bounds exist and `expand` is `min`/`max` with **no clamp** — justerm's `LineBounds::expand` is a copy of it | alacritty | `alacritty_terminal/src/term/mod.rs:165-168`, `damage_line` at `:257-259` |
| ~12 damage sites compute column ranges (backspace `(column - 1, column)`, `EL` `(left, right - 1)`, CR, tabs), but **every bound derives from a column or `columns()`** — never from a glyph width | alacritty | `term/mod.rs:1199, :1238, :1250, :1406, :1416, :1530, :1551, :1588, :1615, :1649`; points at `:476, :1025` |
| ⚠ **The print path records no damage at all.** `Term::input` writes a wide glyph *and* its spacer without damaging; coverage comes from damaging the previous and current cursor **points**, which — because `expand` is min/max — *bracket* everything printed on that line: *"Add information about old cursor position and new one if they are not the same, so we cover everything that was produced by `Term::input`"*. So alacritty has no width-derived bound because it has no print-site bound; structural, not stylistic. justerm cannot copy this — ADR-0003 records damage at the mutation site | alacritty | `term/mod.rs:1062-1137` (`input`), the bracket at `:472-478` |
| A resize rebuilds the bounds **and resets `last_cursor`** — *"Reset point, so old cursor won't end up outside of the viewport"* — i.e. the stale cursor is neutralised at **write** time. justerm rebuilds + `mark_fully_damaged` and clamps the stale cursor at **read** time instead (`frame_damage`); equivalent safety, opposite placement | alacritty | `term/mod.rs:236-247` |
| Dirty tracking is **row-granular only** — `markDirty(y)` / `markRangeDirty`, no column axis exists to get wrong | xterm.js | `common/InputHandler.ts:3651-3685` |
| Dirty is a **per-row `bool`**, set through a resolved `Pin`, so an out-of-range mark is not representable | ghostty | `page.zig:1985-1996`, `PageList.zig:6265-6267` |
| ⚠ **The asymmetry stated as a rule, and the reason justerm clamps toward over-damage**: *"Dirty tracking may have false positives but should never have false negatives. A false negative would result in a visual artifact on the screen."* | ghostty | `page.zig:1993-1995` |
| **No reference bounds or asserts its damage range anywhere** — so there is no upstream clamp to port, and the only reference with a column axis is a library carrying justerm's own pre-#536 shape | all three | as above |

## Cursor blink — who decides (#575, verified 2026-07-28)

This file had **no cursor section at all** before #575, which is why justerm-web blinked
unconditionally for its whole life without anyone comparing that to a reference. Both references
resolve blinking from the **same two inputs** — the application's mode and the user's setting — and
the one that expressed an explicit intent wins. They differ only in *which side* carries the
three-state, and justerm follows alacritty's placement because it is the one ADR-0017 already
implies (core reports the mechanism, the consumer holds the policy) and it needs no wire change.

| Fact | Reference | Site |
|---|---|---|
| **The resolution, verbatim: the application's mode wins, the user option is the fallback.** `decPrivateModes.cursorBlink` is `boolean \| undefined`, so `undefined` means "the app has not spoken" | xterm.js | `browser/renderer/dom/DomRenderer.ts:531` (`?? rawOptions.cursorBlink`), same shape for the shape at `:532` |
| **The mirrored resolution: a user *force* wins, otherwise the application decides.** `Always`/`Never` return `Some`, `On`/`Off` return `None` — so the three-state sits on the *consumer* side here | alacritty | `alacritty/src/event.rs:1631`, `alacritty/src/config/cursor.rs:125-131` |
| DECSCUSR `0` **resets to "the app has not spoken"**, it does not mean "steady block" — both fields go back to `undefined` | xterm.js | `common/InputHandler.ts:2855-2856`, the blink write at `:2873` |
| ⚠ **`CSI ?12 h/l` is ignored unless a quirk is enabled**, because it writes the *user's* option rather than the app channel | xterm.js | `common/InputHandler.ts:1958-1960` (set), `:2217-2219` (reset), the DECRQM report at `:2371` |
| `?12` writes the terminal's cursor style and fires a UI event; DECRQM reports it back | alacritty | `alacritty_terminal/src/term/mod.rs:1987-1990`, `:2036-2039`, report at `:2053-2055` |
| The default is **not blinking**, on both | xterm.js / alacritty | `common/services/OptionsService.ts:16` (`cursorBlink: false`); `alacritty/src/config/cursor.rs:107` (`Shape(shape) => blinking: false`) |
| Focus gates blinking — a blurred terminal is solid. (Valid as long as alacritty keeps `is_focused` in this expression; justerm-web already did this and it is now confirmed rather than assumed) | alacritty | `alacritty/src/event.rs:1643` |
| Also gated: an IME preedit suppresses the blink, and there is a **blink timeout** (default 5s) that stops it entirely. **justerm-web has neither** — collected, not filed | alacritty | `alacritty/src/event.rs:1633` (preedit), `:1645` (`schedule_blinking_timeout`), `config/cursor.rs:34, 63-70` |
| ⚠ **Negative result: `prefers-reduced-motion` has no prior art.** Zero hits across xterm.js's entire `src`; alacritty is native, so the question cannot arise there. justerm-web's #119 behaviour is original, and its precedence over an application request is **derived, not ported** — reduced motion only ever *subtracts* motion, so it is safe in one direction only | xterm.js | `src/**` — grepped, 0 matches |
| **The whole decision is one ordered chain, and ghostty writes its own down as such** — *"the order of conditionals below is important. It represents a priority system of how we determine what state overrides cursor visibility and style."* Order: `viewport → preedit → password_input → visible (DECTCEM) → focused → blinking`. Note it resolves **shape and visibility together** (returns `?Style`), where justerm-web's chain resolves *blink only* | ghostty | `src/renderer/cursor.zig:35-67` |
| ⚠ **CORRECTED 2026-08-03 (#249) — the citation was right and the conclusion was dead.** This row read *"a preedit outranks DECTCEM there — an explicitly hidden cursor is **shown** as a solid block during composition"*. `cursor.zig:47` does return `.block` for preedit, but the renderer **throws the whole cursor away** before it can be used: `rebuildCells` runs `setCursor(null, null)` and `cursor_pos = maxInt`, then *"If we have preedit text, we don't setup a cursor"* → `break :cursor`. `cursorStyle` has one production caller, so ghostty's live behaviour during a composition is **no terminal caret at all** — plus one underline under every preedit cell. **What this costs #592: nothing.** Its rejection of *"a preedit outranks DECTCEM"* was a maintainer product call against a rule the reference does not actually hold, so the reference reopens nothing — but the option it *does* hold (suppress the caret entirely) has never been on the table, and is recorded as ADR-0028's alternative (D) | ghostty | style `src/renderer/cursor.zig:47`; **discarded at** `src/renderer/generic.zig:2453` |
| alacritty suppresses the blink during a preedit too, as a term in the same expression | alacritty | `alacritty/src/event.rs:1633` |
| **Negative result: xterm.js has no preedit rule for the caret.** Its only `isComposing` guard near the cursor is `_syncTextArea`, which stops *moving the hidden textarea* mid-composition — an IME-disturbance guard, unrelated to the caret | xterm.js | `browser/CoreBrowserTerminal.ts:337-339` |
| ⚠ **Measured, not read (#592, real browser, 2026-07-28)**: composition driven through the hidden textarea, cursor cell and a content cell sampled 5x over 1.4s. With the application silent — the default since #575 — the caret shows **one** distinct colour (already solid) and no content cell changes; with the application asking to blink, the caret shows **two**. So justerm-web adopted alacritty's suppression as a **no-op in the common case**, biting only where an application explicitly asked to blink. ghostty's stronger form was **rejected**: revealing a DECTCEM-hidden caret would invert `cursorCommand`'s contract for a rare case | justerm-web | `justerm-web/src/cursor.ts` `setComposing`, decision recorded on #592 |

### A device-pixel-ratio change — who notices, and what they do about it (#325, verified 2026-08-10)

Only xterm.js has this problem: alacritty and ghostty own their OS window and are told about a scale
change by the windowing system, so there is no media query and no consumer half. xterm.js is
therefore the whole reference here, and it is close enough to port.

| Fact | Reference | Site |
|---|---|---|
| The **library** owns the listener, not the consumer: `matchMedia("screen and (resolution: ${devicePixelRatio}dppx)")`, built inside the browser service | xterm.js | `src/browser/services/CoreBrowserService.ts:127` |
| **It re-arms.** On every change it removes the listener from the old query, re-reads `devicePixelRatio`, builds a *new* query at that ratio and attaches — because a resolution query is bound to the ratio it was created with, so the obvious version works exactly once | xterm.js | `src/browser/services/CoreBrowserService.ts:118-137`, teardown at `:131-137` |
| ⚠ **It does NOT re-fit the grid.** `handleDevicePixelRatioChange` re-measures the char size (*"DomMeasureStrategy(getBoundingClientRect) is not stable when devicePixelRatio changes"*), tells the renderer, and repaints — there is no `resize(cols, rows)` on the path, and `FitAddon` stays manual. This is what justerm's "no re-fit" follows | xterm.js | `src/browser/services/RenderService.ts:279-290`, subscription at `:84` |
| It uses the **deprecated** `addListener`/`removeListener` pair rather than `addEventListener`; justerm uses the modern one. The only deliberate difference in the port | xterm.js | `src/browser/services/CoreBrowserService.ts:123, 129` |

**Negative result, and it decides how this can be tested (measured 2026-08-10, headless Chromium).**
CDP's `Emulation.setDeviceMetricsOverride` moves `window.devicePixelRatio` and **re-evaluates** the
queries — `screen and (resolution: 1dppx)` flips `matches` to `false` and `(min-resolution: 1.5dppx)`
to `true` — but dispatches **no `change` event**, to a retained `MediaQueryList` or otherwise. Three
variants tried (an exact-ratio query, a broader one, and the widget's own watcher); all silent. So the
listener half cannot be proven end-to-end in Playwright and is unit-tested instead, while the
*adoption* half is driven through a test hook, the way `justerm-renderer/demo/dpr-change.html`
already does. Holds as long as Chromium's override keeps that split.

**⚠ The negative result has a validity condition, and it took a wrong conclusion to find it (#808,
measured 2026-08-24, same headless Chromium).** It holds for a density move *alone*. Change the
**viewport size in the same `setDeviceMetricsOverride` call** and the queries are re-evaluated *and*
`change` **is** dispatched — measured on `demo/shared-surface.html` at a fixed `deviceScaleFactor: 2`,
three variants:

| `width`/`height` | viewport moves? | `change` fires? | cell after 300ms |
|---|---|---|---|
| `0` / `0` (size override disabled) | no | no | `11x23` — unchanged |
| the page's own `1280x720` | no | no | `11x23` — unchanged |
| `1280x900` | **yes** | **yes** (exact `Xdppx` *and* `(min-resolution: 1.5dppx)`) | `22x47` — adopted |

Two things follow. First, a harness that wants the watcher **blind** — which is the only way to reach
a restore that adopts an unannounced density — must hold the size fixed; `demo.spec.ts`'s #325 restore
test passes `1280x720`, which equals that suite's viewport, so it has been size-preserving by
coincidence rather than by intent. Second, and the reason this row is worth its space: the first
reading of the `1280x900` run was *"the #325 negative result is now false"*, filed against a run that
had moved **two** variables. The result stands; the conclusion drawn from an uncontrolled variable did
not.

### Cursor policy knobs — where each reference puts them (#580, verified 2026-08-10)

Pins the two constants `justerm-renderer` borrowed for its cursor policy, plus where each reference
*exposes* them. The first was a recorded hole:
[caret drawing](../map/territory/caret-drawing.md) named the alacritty thickness default as "a
borrowed constant with a path and no SHA", which is exactly the class this file exists to close.

The placement rows are here because they are what a consumer-facing API question is decided
*against* — and in this case decided **away from**: justerm carries both contrast ratios on `Theme`
where neither reference puts contrast in its colour scheme. That is a deliberate divergence on the
"consumer-facing API shape" tie-breaker row (our own API's coherence governs), recorded in
`theflow.md`'s divergence table; these rows are its evidence, not a defect report.

| Fact | Reference | Site |
|---|---|---|
| Cursor thickness is a **fraction of the cell width**, defaulting to `0.15`, and lives under the `cursor` config section — not under `colors` | alacritty | `alacritty/src/config/cursor.rs:31` |
| ⚠ The cursor contrast guard is a **compile-time constant, not a setting**: `1.5`, with no config path at all. justerm's `setCursorContrast` therefore exposes configurability the reference does not have — the *number* is borrowed, the *knob* is not | alacritty | `alacritty/src/display/content.rs:22` |
| xterm.js's cursor width is `cursorWidth`, an **option in CSS px** — a length, not a fraction, which is why #270 took alacritty's rule instead (a fixed length gives a 32px font the same hairline caret as a 12px one) | xterm.js | `src/common/services/OptionsService.ts:19` |
| `minimumContrastRatio` is likewise an **option**, defaulting to `1` (off) — and xterm.js has no cursor-specific contrast guard of any kind | xterm.js | `src/common/services/OptionsService.ts:43` |
| **`ITheme` is colours only.** Every one of its ~30 members is a colour string; no policy scalar appears on it. So "contrast belongs outside the theme" is xterm.js's position by construction, not by omission | xterm.js | `typings/xterm.d.ts:372` |

### Drawing the preedit: two draw into the grid, one draws a DOM box (#249, verified 2026-08-03)

Read this before touching `preedit.rs` or `Terminal.showPreedit`. The split is 2–1 and the odd one
out is the only reference with our architecture, which is the trap: xterm's mechanism is not
portable here, for a reason that has nothing to do with the feature.

| Fact | Reference | Site |
|---|---|---|
| The preedit is drawn **into the grid**, as a pass that *excludes* the covered cells from the normal row rebuild — *"we don't want to render anything here because we will render the preedit separately"* — and emits them afterwards in the terminal's **default foreground**, with an underline under both halves of a wide pair | ghostty | skip `src/renderer/generic.zig:2368`; underline of the pair's second cell `:3337` |
| ⚠ **Its right-edge loop does not do what its own comment says, and justerm deliberately diverges.** The comment states the intent (*"adjust our codepoint start to a point where our width would be less than the number of cells we have"*), but the loop breaks with the index of the codepoint that **overflowed**, so the kept slice still includes it and the run still exceeds the room; the shift then saturates at column 0. Both of its own tests pass either way — neither exercises an over-long run. `preedit.rs` keeps the largest suffix that *fits* and pins the difference with a third test. **Derived by walking the source, not executed** | ghostty | `src/renderer/State.zig:179` (the break), tests `:201`, `:225` |
| Also drawn into the grid, as a `draw_string` pass after the grid, **underlined**, shortened to `num_cols` with an ellipsis rather than shifted, and with its **own** IME caret (Beam, or HollowBlock when the IME reports a multi-character range) | alacritty | `alacritty/src/display/mod.rs:1190` (the underline), the pass around it |
| **Both grid-drawing references take the preedit underline's ink from the preedit's *own* fg, and neither reads the cell underneath (#711).** ghostty passes **one** `screen_fg` into the glyph and into both `addUnderline` calls (the wide pair's second cell included), and its call site hands that parameter `state.colors.foreground`. alacritty builds the underline from the same `fg` its `draw_string` just used, which the caller sourced from `foreground_color` (or the footer-bar fg while searching). So a covered cell's `SGR 58` reaches the composition in neither — which is what makes justerm's reading of it a defect rather than a divergence, and it is the same value in both, not merely a similar one | ghostty · alacritty | ghostty `src/renderer/generic.zig:3299` (the parameter), `:3335` (the underline takes it), call site `:2574`; alacritty `alacritty/src/display/mod.rs:1190`, `fg` from the call at `:960` |
| **Negative result: neither grid-drawing reference repairs a wide pair the run cuts in half — neither does pair repair at all (#715).** ghostty's preedit block only *adds* cells (`addPreeditCell` per codepoint, `x += if (cp.wide) 2 else 1`) and touches no neighbour; alacritty draws its preedit as one `draw_string` over the finished grid. So the repair is justerm's own ADR-0025 obligation and the reference cannot arbitrate what it may touch — only what the *exclusion* covers, which is the row below | ghostty · alacritty | ghostty `src/renderer/generic.zig:2566-2584`; alacritty `alacritty/src/display/mod.rs:1190` |
| ⚠ **Do not transpose a reference's own pair repair into the preedit pass — it lands 2–1 on the shape #715 REMOVED, and it is not evidence either way (#715).** Each reference repairs a pair it breaks while holding a pen, and the pass's counterpart of a pen is the declaration it supplies its own cells (`cp=' '`, `UNDERLINE`, bg/fg `Default`). Transposed: alacritty's `clear_wide` keeps the cell's own attributes → the rule justerm now has; ghostty's `blankCell` fill and xterm.js's whole-`curAttr` stamp both → the pre-#715 code. A count here is exactly the argument shape the #490 war story warns about — the tie-breaker gives the reference no authority on renderer cell composition. Recorded because the tempting count runs the *other* way if you stop before transposing | alacritty · ghostty · xterm.js | alacritty `alacritty_terminal/src/term/cell.rs:171`, call site `alacritty_terminal/src/term/mod.rs:1001`; ghostty `src/terminal/Screen.zig:1770` (the `@memset`), `:1929` (`blankCell`); xterm.js `src/common/InputHandler.ts:538`, `:669` |
| **The exclusion is run-scoped, and a cell outside it keeps its background.** ghostty's per-cell `continue` for an in-range cell (`:2677`) precedes that same loop's background write (`:2899`) with no other cell loop between them — so *everything* an outside cell had survives, background included. This is what makes justerm blanking a repair cell's background a defect rather than a divergence: the reference has no repair to compare, but it is unambiguous that a cell the pass did not take is left alone | ghostty | `src/renderer/generic.zig:2677` (the skip), `:2899` (the bg write it precedes) |
| The preedit is a **DOM element** over the terminal, `‎`-wrapped for RTL overflow, positioned per render, with the textarea then sized to its bounding box. ⚠ **No underline and no theming** — its stylesheet hardcodes `background:#000; color:#FFF` and carries `TODO: Composition position got messed up somewhere`. (The `‎` is a direction mark, not decoration; #249's body called this an "inline underline" and that was wrong.) | xterm.js | `src/browser/input/CompositionHelper.ts:94`; CSS `css/xterm.css:79-91` — **outside the sparse checkout**, read via `gh api` at the pin |
| ⚠ **Why xterm's mechanism does not transfer, measured rather than argued (2026-08-03, Chromium, 16px, dpr 1.5).** xterm's cell width *is* a browser advance (`CharSizeService` measures `'W'.repeat(32)`), so its DOM view cannot lose an alignment it never maintains. justerm's cell is the ink box of `█` (ADR-0022). Renderer cell width vs the browser's advance for `가`: `monospace` 8 vs 16 (exact, 2 cells), **Consolas 9.333 vs 14.72, Cascadia Mono 9.333 vs 14.72, Courier New 10 vs 14.72, Lucida Console 10 vs 14.72** — a drift of **−3.95 to −5.28 CSS px per syllable**, four to five cells over ten. The cause is structural: a wide char is two cells *by definition* while the browser advances it by whatever the fallback font says. The generic `monospace` alias agreeing exactly is a trap — it is what the demo uses | justerm-web vs the browser | measured through `renderer.cellSize()` against a DOM span; recorded on #249 |

### The IME anchor: nobody caches it, and xterm shares our staleness (#631, verified 2026-07-30; #637 adjudicated 2026-07-30; #649 measured 2026-07-31)

Read this before touching `Terminal.positionTextarea` or adding another reader of the cell. The
headline is a **negative** result about the reference, which is why it is worth a section: the
obvious assumption — "xterm must invalidate this properly, go copy that" — is false, and copying the
*visible* half of what xterm does (no cache, re-read per frame) imports a cost xterm does not pay.

| Fact | Reference | Site |
|---|---|---|
| ⚠ **xterm.js shares justerm-web's staleness.** An option change (`fontSize` / `lineHeight` / `letterSpacing`) reaches `RenderService.handleResize`, which forwards to the **renderer** and full-refreshes — it never touches `BufferService`, so `onResize` never fires and the `_syncTextArea` registered on it is never called | xterm.js | `browser/services/RenderService.ts:98-112` (the option list), `:291-301` (`handleResize`); the registration it fails to reach is `browser/CoreBrowserTerminal.ts:579` |
| …and the consumer's re-fit reaches it only when the **grid** changed. `FitAddon.fit()` has no dedupe, but `Terminal.resize` early-returns on `x === this.cols && y === this.rows`, so `BufferService.resize` — which otherwise fires `_onResize` *unconditionally* — never runs | xterm.js | `addons/addon-fit/src/FitAddon.ts:47-54`; `browser/CoreBrowserTerminal.ts:1055-1062`; `common/services/BufferService.ts:49-56` |
| **Its whole mitigation is a point-of-use re-sync at `compositionstart`, and the comment names this exact symptom** — *"…would cause the IME to appear in the wrong position. The theory is that when the IME is triggered during a partial render the textarea position becomes locked and will not move until it is hidden and a custom move occurs."* The order is `_syncTextArea()` **then** `compositionHelper.compositionstart()`, deliberately, so the `isComposing` bail cannot block it | xterm.js | `browser/CoreBrowserTerminal.ts:417-425` (comment `:418-422`, the sync `:423`, the handoff `:424`); the bail is `:338` |
| `_syncTextArea` holds **no cache** — the cell is read live from `dimensions.css.cell` on every call. Its only triggers are `onCursorMove`, `onResize`, `compositionstart` | xterm.js | `browser/CoreBrowserTerminal.ts:337-360` (reads at `:348-352`); `rg '_syncTextArea' src/` → 1 decl + 3 calls |
| **…and it can afford that because `dimensions` is a pushed field, not a layout read.** `DomRenderer.dimensions` is assigned by `_updateDimensions()` at discrete points. justerm-web's cell arrives through a consumer `getGeometry` callback that does `getBoundingClientRect()`, so "the reference has no cache" does **not** transfer | xterm.js | `browser/renderer/dom/DomRenderer.ts:58` (field), `:89-90` (init), `:136` (`_updateDimensions`), re-called at `:331, :355, :360, :495` |
| **Pushes the IME rect on every key event** — the *input* moment, not the output moment — reading the cell live. Comment: *"This can trigger an input method so we need to notify the im context where the cursor is so it can render the dropdowns in the correct place."* | ghostty | `src/apprt/gtk/class/surface.zig:1263` (inside `keyEvent`, `:1241`; comment `:1258-1260`); `src/Surface.zig:2105` (`imePoint`, live `size.cell` at `:2119`, `:2132`) |
| **Pushes the IME rect on every draw**, no dedupe, live `size_info`. Cheap because it owns its window — a compositor rect, not a forced layout | alacritty | `alacritty/src/display/window.rs:450`; called from `display/mod.rs:1141`, `:1215`, reached from `draw()` at `:960` |
| ⚠ **Negative result: `RenderService`'s canvas-box dedupe is dead code — do not cite it as prior art for a box-keyed dedupe (#632).** The *key* is the canvas CSS box, but `_canvasWidth`/`_canvasHeight` are declared and **never assigned anywhere in `src/`**, so the comparison is always against `0` and `onDimensionsChange` fires on every option change and every `RenderService.resize` | xterm.js | decls `browser/services/RenderService.ts:38-39`, sole use `:237`; `rg '_canvasWidth\|_canvasHeight' src/` → 3 hits, all three above |
| The **executed** dedupe prior art is against *authoritative live state*, never a shadow copy: `Terminal.resize` compares to `this.cols` / `this.rows` | xterm.js | `browser/CoreBrowserTerminal.ts:1055-1062` |
| ⚠ **ADJUDICATED — yes, the OS re-reads the anchor mid-composition (#637, real IME, 2026-07-30).** Measured with the Windows Korean IME in Chrome: with an output frame moving the cursor while a composition was open, the Hanja candidate window **followed the anchor down the screen**, away from the text being composed. So the frame stream must not move the anchor while composing. Not obtainable from source — a dispatched `CompositionEvent` exercises our listeners but not the OS's query path, which is why this sat unadjudicated | justerm-web | guard in `justerm-web/src/terminal.ts` `textareaMove` (`composing` — every path since #649 closed the `force` exemption); observation harness = the demo's `Cursor drift` + `IME HUD` buttons |
| **…and that dissolves the apparent xterm.js contradiction: both sites are load-bearing.** Once the OS is known to re-read, whoever writes the position owns the candidate window — so xterm suppresses the **involuntary** writer (`_syncTextArea` off the frame/cursor path bails while composing, gating **all three** of its callers; the `compositionstart` one works by running *before* `isComposing` is set, not by bypassing) and keeps the **voluntary** one (`updateCompositionElements`, deliberately tracking its own preedit view every render). Not rival rules; different intents. **xterm has no focus-driven sync at all** — `onFocus` only calls `handleFocus()` | xterm.js | bail `browser/CoreBrowserTerminal.ts:338`; callers `:423` (compositionstart), `:575` (`onCursorMove`), `:579` (`onResize`); `onFocus` `:582`; rewrite `browser/input/CompositionHelper.ts:245`, textarea writes `:273-274`, driven by `:430` |
| ⚠ **Do not generalise xterm's suppression into "freeze the anchor during composition" — 2 of 3 references do the opposite.** The convergent rule is *the anchor tracks where the user's composition is, never where the output cursor went*, and ghostty/alacritty satisfy it by **actively re-aiming mid-composition** with no gate whatsoever. ghostty folds the preedit's width into the rect it pushes on every key event; alacritty picks the point **from the preedit** when one exists and from the cursor when it does not. justerm-web freezes only because the preedit never reaches it (#249 absent), so *"where the composition is"* collapses to *"where it started"*. **#249 landing makes freezing wrong**, not merely conservative | ghostty · alacritty | ghostty `src/Surface.zig:2108` (`preedit_width`), used `:2151`, pushed from `src/apprt/gtk/class/surface.zig:1262`; alacritty `alacritty/src/display/mod.rs:1136-1142` (`draw_ime_preview`, no-preedit branch), `:1215` (`ime_popup_point`) |
| ⚠ **Method note — this observation cannot be screenshotted.** Taking a screen capture moves focus, which commits the composition and clears the textarea, so the artifact under observation is destroyed by the act of capturing it. It has to be watched live and reported. Cost a false conclusion once: a capture showing `text "한" ← IME COMMIT` and an empty textarea was read as *"Hanja conversion happens after the composition ends"*, which the maintainer corrected | justerm-web | demo `IME HUD` readout; corrected on #637 |
| ⚠ **xterm suppresses the focus scroll with `preventScroll: true`; justerm-web does not.** So the two do not merely differ on *whether* focus re-syncs the anchor (xterm has no focus-driven sync at all, row above) — xterm has also removed the reason one would exist. Only this layer diverges: ghostty and alacritty own their window and have no hidden textarea to focus, so they cannot arbitrate. **Not adopted on #649**, because dropping our re-sync in favour of it is a *product* change (`Terminal.focus()` would stop scrolling the terminal into view) that also bets on the unmeasured half of spine #640's Q4 | xterm.js | `src/browser/CoreBrowserTerminal.ts:288` |
| **Pointer-down is `preventDefault()` then focus, in the reference too.** justerm-web's `onDown` focuses the hidden textarea and relies on the *consumer* cancelling the default action; xterm does both itself in one handler. Family + reference agree on the pairing, so this is not a divergence — but justerm-web's half of it was undocumented until #649, and `TerminalOptions.element` claimed the opposite (*"the widget makes it focusable"*, with no `tabIndex` anywhere) | xterm.js | `src/browser/services/MouseService.ts:226` (after `preventDefault()` at `:225`) |
| ⚠ **MEASURED (#649, real Chrome, 2026-07-31) — the browser's focus steps are a second reader of the anchor, and they track it 1:1.** Focusing the hidden textarea from `scrollY: 2000` scrolled the page back to it (control: `preventScroll: true` moved 0px, plain focus again moved 2000px). The destination follows the anchor exactly — anchor `top` 80px → 1100px moved the landing point 2483 → 3503, a 1020px anchor delta producing a 1020px destination delta. Staleness is row-proportional: `lineHeight` 1 → 1.6 moved row 5's anchor by 47px (~9px per row), so a stale anchor scrolls the page proportionally wrong rather than negligibly wrong. **This is why #631's focus re-sync is load-bearing rather than vestigial** — and why the third reader (AT / magnifier following focus) is still the open question | justerm-web | probe run against the demo; `justerm-web/src/terminal.ts` `Terminal.focus` → `syncTextareaAnchor` |

### Idle timeout — both stop blinking, 60x apart (#593, verified 2026-07-28)

⚠ **They are the same rule, not two models.** An earlier reading of this recorded alacritty's clock as
measuring *time spent blinking* and xterm.js's as measuring *idleness*. That is wrong:
`on_typing_start` reschedules the blink on every keystroke and, when the timeout has already fired,
calls `update_cursor_blinking()` — which clears the latch. **Both are "stop after N with no user
input".** The corrected difference is the threshold and the reset set.

| Fact | Reference | Site |
|---|---|---|
| Stops after **5 seconds**; `0` disables; floored at `blink_interval * 2 * MIN_BLINK_CYCLES_BEFORE_PAUSE` | alacritty | `alacritty/src/config/cursor.rs:34`, `:63-70`; armed at `event.rs:1645`, latch `window_context.rs:54`, cleared `event.rs:1641` |
| Typing resets it — including reviving an already-timed-out blink | alacritty | `alacritty/src/event.rs:1201-1213` (`on_typing_start`) |
| Stops after **5 minutes**; reset by keystroke / mousedown (`restartBlinkAnimation`) and by focus `resume()`, paused on blur | xterm.js | `browser/renderer/shared/Constants.ts:12`, manager at `browser/renderer/dom/DomRenderer.ts:663-717` |
| It stops the **animation**, not the state machine — a CSS class on the row container | xterm.js | `DomRenderer.ts:713` `_stopBlinkingDueToIdle` |
| **Negative result: ghostty has no timeout at all.** `style()` takes `blink_visible` as an input; no timer, idle state or latch anywhere in the blink path | ghostty | `src/renderer/cursor.zig` (whole file) |

**Measured, not read (real PTY, RHEL 9.2, `TERM=xterm-256color`, 24×80, 2026-07-28).** What real
programs actually emit, because the reference sources above do not say which channel gets used:

| Program | DECSCUSR `CSI Ps SP q` | att610 `CSI ?12 h/l` |
|---|---|---|
| bash (login), less | none | none |
| vim (both normal and `startinsert`) | **none** | `?12h` ×1, `?12l` ×1 |
| htop, top | **none** | `?12l` ×1 |

⚠ **The `?12` is not an application preference — it is terminfo's cursor-visibility string carrying
one.** `xterm-256color` defines `cnorm=\E[?12l\E[?25h` and `cvvis=\E[?12;25h`, so an ncurses
`curs_set()` turns the blink off as a *side effect*. And this is not a terminfo gap: the same entry
does advertise DECSCUSR (`Ss=\E[%p1%d q`, `Se=\E[2 q`) and the programs still did not use it. The
consequence for a consumer is concrete — merely quitting vim pins the cursor steady for the rest of
the session — which is why justerm-web carries an override, and is the same hazard xterm.js answers
by quirk-gating `?12`. Corpus limits: six programs, bash only, no vi-mode shell prompt (starship /
zsh), which is where DECSCUSR emitters are most likely to live — so "nothing uses DECSCUSR" is
**not** established, only "nothing in this corpus did".

## Text blink — SGR 5 (#576, verified 2026-07-29)

⚠ **The headline is a negative result: only one of the three references animates blinking text at
all, and it ships the feature off.** That is why this section exists as evidence rather than as a
port — there is **no inheritable cadence**, so justerm-web exposes an *interval* (`0` = off, the
default) instead of a boolean with a number baked in. A design that had assumed "pick the reference's
rate" would have been picking xterm.js's `0`.

Note what the tally is *not*: it is not "nobody wants this". All three parse SGR 5 and carry the
attribute; the split is entirely about whether the renderer acts on it.

| Fact | Reference | Site |
|---|---|---|
| **Implements text blink, as an opt-in consumer interval.** `TextBlinkStateManager` runs a `setInterval` and flips `isBlinkOn`; the duration is the `blinkIntervalDuration` option | xterm.js | `src/browser/renderer/shared/TextBlinkStateManager.ts` (whole file), `:66-88` (`_updateIntervalState`) |
| ⚠ **The default is `0`, i.e. disabled** — the option is validated `>= 0` and throws below it | xterm.js | `src/common/services/OptionsService.ts:17`, validation at `:173-178` |
| The interval runs only when there is something to blink **and the viewport is visible**: `duration > 0 && needsBlinkInViewport && isViewportVisible` | xterm.js | `TextBlinkStateManager.ts:67` |
| ⚠ **"Viewport visible" means an `IntersectionObserver`, not document visibility** — a terminal scrolled out of view inside a visible page stops blinking. Do not read this as something rAF gives you for free | xterm.js | `src/browser/services/RenderService.ts:126-137` (observer), `:140-142` (dispatch), consumed at `addons/addon-webgl/src/WebglRenderer.ts:251-253` |
| **Starting the blink forces the phase ON** before arming the interval, so enabling it never conceals the text at that instant | xterm.js | `TextBlinkStateManager.ts:72-73` |
| `needsBlinkInViewport` comes from a **per-row scan of the viewport** for `cell.isBlink()`, kept as a row flag array plus a count | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:529-531`, `:608`; the DOM twin at `browser/renderer/dom/DomRenderer.ts:650-659` |
| ⚠ **Stopping forces the phase back ON and re-renders** — a stopped blink must never leave text hidden | xterm.js | `TextBlinkStateManager.ts:83-87` |
| The concealed phase is expressed as an ink flag, not as an erase: `fg \|= FgFlags.INVISIBLE` (webgl) / a `xterm-blink-hidden` class (DOM) | xterm.js | `WebglRenderer.ts:562`, `dom/DomRendererRowFactory.ts:166-168` |
| **Negative result: no text blink anywhere in the tree.** Every `blink` hit under `alacritty_terminal/src` is the *cursor*; SGR 5 sets no cell flag | alacritty | `alacritty_terminal/src/**` — grepped, cursor-only (`event.rs:45`, `term/mod.rs:1987`, `:2036`) |
| **Negative result: parses and stores it, never draws it.** `sgr.zig` maps SGR 5 (and 6) to `.blink`, `style.zig` carries `flags.blink` and round-trips it in DECRQSS output — and the renderer reads `cursor_blink_visible` only | ghostty | `src/terminal/sgr.zig:291`, `:293`; `src/terminal/style.zig:33`, `:343`; `src/renderer/generic.zig:1130`, `:1378` (no cell-blink read anywhere) |

**Direction, since a divergence is not a direction.** justerm's renderer already conceals a blinking
cell on the off phase (`justerm-renderer/src/attrs.rs` `is_concealed`, #282) and takes the phase in
the damage header — i.e. the *engine side* is xterm-shaped and the two silent references are the
outliers. What was missing was only the consumer's clock, so this is "this layer alone drifted → move
toward the reference", not a family decision.

**What justerm-web deliberately does not port, and why** (recorded so the next reader does not read
these as oversights):

- **`needsBlinkInViewport` is ported, in its conservative form** — and the reasoning that first said
  it needn't be is recorded here because it was wrong in an instructive way. *"The feature is off
  unless a consumer opts in, so the default costs nothing"* is true and answers a **different
  question**: xterm's gate protects the *enabled* case, which is the only one that costs anything.
  And "a frame-mode consumer cannot answer it" is too strong — the gate only has to be
  **conservative** (a false positive costs one redundant re-pack; only a false negative would freeze
  blinking text), and a Full frame carries every row, so a replace-on-Full / or-on-Partial latch over
  the frame's flag column is sound. Without it, an opted-in consumer pays `resolve_and_pack` over
  every cell plus a full `plan_upload` diff twice a second on grids where no cell has ever carried
  SGR 5, producing a byte-identical buffer.
- **`setViewportVisible` is NOT covered by rAF, and the first version of this note said it was.**
  xterm's input is an `IntersectionObserver({threshold: 0})` on the screen element
  (`src/browser/services/RenderService.ts:126-142` → `addons/addon-webgl/src/WebglRenderer.ts:251-253`),
  not document visibility. rAF keeps firing at full rate for a terminal scrolled out of view inside a
  visible document, so that case is still ungated here. It is a **family-level** gap rather than this
  slice's: justerm-web's cursor loop has no visibility input either, and the fix belongs to both.
- **Throwing on a negative interval** — this widget's other policy setters adopt-and-report rather
  than panic across the seam (`CursorBlink.setIdleTimeout`), and internal coherence is the recorded
  tie-breaker for consumer-facing API shape (ADR-0023's posture).

## Widget teardown — who ends a handed-over component (#606, verified 2026-07-29)

The question justerm-web could not answer from its own code: the consumer **constructs** the renderer
and hands it to `Terminal`, so may the widget end it? Only one reference has the shape (alacritty and
ghostty are native applications with no injected-component lifecycle), and it answers clearly — with a
**condition** that turns out to matter more than the answer.

| Fact | Reference | Site |
|---|---|---|
| **The widget disposes what it was handed.** `AddonManager.dispose()` walks its addons in **reverse** registration order calling `instance.dispose()` on each — and the addons are consumer-constructed (`new FitAddon()` → `terminal.loadAddon(addon)`) | xterm.js | `src/common/public/AddonManager.ts:17-21`; the hand-over at `src/browser/public/Terminal.ts:244-246` |
| The manager is registered in the terminal's own disposable store, so the cascade is `Terminal.dispose()` → `super.dispose()` → store → each addon | xterm.js | `src/browser/public/Terminal.ts:36`, `:200-202` |
| **Idempotence is bought, not assumed.** `loadAddon` **replaces** the addon's own `dispose` with a wrapper (`instance.dispose = () => this._wrappedAddonDispose(loadedAddon)`) that early-returns when already disposed — so either party may call it, twice is a no-op | xterm.js | `src/common/public/AddonManager.ts:26`, `:30`, `:34-37` |
| ⚠ **The condition the rule rests on: dispose is END OF LIFE.** `DisposableStore` latches `_isDisposed` permanently, and anything `add`ed afterwards is disposed **immediately** rather than stored (`if (this._isDisposed) { o.dispose(); }`). There is no re-open path | xterm.js | `src/common/Lifecycle.ts:46-52` (add), `:54-62` (dispose) |
| **Negative result: no counterpart in the other two.** alacritty and ghostty own their renderer outright — no injected component, no teardown protocol to compare. So this rule has **one** witness, not three | alacritty / ghostty | structural; nothing to cite |

**Direction, and why the condition is the load-bearing half.** justerm-web alone drifted — its
`FrameSource` port already handed the widget a teardown means (`Unsubscribe`) while the `Renderer`
port handed it none, so the two injected ports disagreed with each other before either disagreed with
xterm. But copying the rule required copying its premise: xterm's dispose is safe to cascade *because*
nothing is expected to come back. #606 therefore had to **declare** `Terminal.dispose()` end-of-life
(and make `mount()` throw afterwards) rather than inherit the behaviour and hope. A reference read
without its enabling condition is how a correct rule lands in a codebase that cannot support it.
## Per-cell payload length — nobody caps a cluster, the one that can run out *grows*, and a URI is a different answer (#621, verified 2026-07-29)

Added 2026-07-29 while filing #621; **the conclusion corrected the same day** — see the note under the
table, which is the more useful half of this section. Every row grepped at the pinned SHAs. The
occasion: justerm's wire writes the grapheme cluster and the OSC 8 URI behind `u16` length prefixes
(`serialize.rs`), and nothing on the producing side bounds either — so `feed()`ing 70000 combining
marks produces a frame whose own `decode` answers `Err(BadTag)`.

**The question is not "should a terminal cap cluster length".** No reference does, so capping in the
engine would be justerm drifting alone *and* would discard Unicode material this engine exists to
carry. The useful split is what happens when the storage cannot take it: two references cannot run
out, and the one that can **grows until it fits**.

**A URI is not the same question and does not share the answer — rows 5-6.** The cluster rows below
say nothing about OSC 8, and the first version of this section let its title imply they did.

| Fact | Reference | Site |
|---|---|---|
| Appends to a JS string with no cap and no failure mode — *"we already have a combined string, simply add"*. Growth is bounded only by the engine's own memory | xterm.js | `common/buffer/BufferLine.ts:263-265` |
| `push_zerowidth` pushes onto an unbounded `Vec<char>` behind the cell's `extra`; no length check at the write site | alacritty | `term/cell.rs:164-166` |
| The grapheme arena can run out, and the allocator reports it as a *named* error — the comment on the branch reads *"The grapheme alloc capacity needs to be increased"* | ghostty | `terminal/page.zig:1520-1523` |
| ⚠ **…and that error is a growth signal, not an answer.** The caller catches it in a `while (true)` loop — *"Grow our capacity until we can fit the extra bytes"* — reallocating the page until the payload fits, so the error never reaches a user. Ghostty's answer to "the storage cannot take it" is **make the storage bigger** | ghostty | `terminal/PageList.zig:1871-1886` |
| ⚠ **A URI *is* capped, by the reference the cluster rows read as uncapped.** The OSC parser builds its payload into a `LimitedStringBuilder(PAYLOAD_LIMIT)`, `PAYLOAD_LIMIT = 10000000` | xterm.js | `common/parser/OscParser.ts:196`, `common/parser/Constants.ts:67` |
| ⚠ **…and its failure mode is silent whole-sequence discard**, not truncation and not an error: on overflow the builder *clears itself* and reports it, `put` short-circuits, and `end` returns without ever calling the handler — so the hyperlink simply never happens | xterm.js | `common/StringBuilder.ts:52-53`, `common/parser/OscParser.ts:209-221` |

The direction that falls out, **for clusters**: 3 of 3 leave the *input* unbounded, and the only one
with a hard storage limit **grows past it**. justerm's pre-#621 behaviour — encode silently emits a
length its own decoder rejects — matched none of them, and is the one shape all three avoid.

**For URIs the tally is 2:1, not 3:0**, and it does not change #621's direction: 10 000 000 is two
orders of magnitude past the `u16::MAX` that was actually in question, so "widen the field" is still
the answer either way. What it changes is what may be *cited*. This section may not be used to say
"no reference caps a URI".

**Correction, same day, and it is why row 4 exists.** This section first carried only row 3 and
concluded that ghostty *"points at making the failure visible rather than at shrinking the input"* —
i.e. that it argued for a fallible writer. That reading survives exactly as long as you stop at the
`return`. One caller up, the error is caught and answered by growing the arena, so the reference
argues for **widening the field**, which is close to the opposite recommendation. The citation was
correct and the conclusion was not: precisely the failure mode Rule 2 above records from #610 (*"two
quoted the right line and drew a wrong conclusion from it"*), reproduced here by the person who wrote
that rule down. **A `return` is not a stance until you have read its caller.**

**Second correction, 2026-07-29, and it is the same failure at a different scale.** Rows 5-6 were
added after a refuting lens found that this section — four grapheme-side rows — was being cited for
a claim about **URIs**, in `docs/map/territory/wire-format.md` and in #621's own decision comment.
Nobody mis-read a citation this time; the extension happened in the *gap between* the rows and the
heading, which said "nobody caps it" without naming what "it" was. The lesson is narrower than the
first correction's and worth keeping separate: **a section's title is cited as if it were a row.**
Scope the heading to what the rows actually establish.

## Who re-fits after a spacing change (#578, verified 2026-07-29)

The load-bearing facts behind #578's central design call — the runtime spacing setters forward only,
and the **consumer** re-derives the grid — were recorded in no artifact before this. ADR-0023 pins the
*unit* question and stops there.

| Fact | Reference | Site |
|---|---|---|
| A `letterSpacing` / `lineHeight` option change re-lays out **at the current grid** — the handler is `clear(); handleResize(bufferService.cols, bufferService.rows); _fullRefresh()`, reading the grid from the buffer service rather than re-deriving it from the pixel box | xterm.js | `browser/services/RenderService.ts:100-112` |
| ⚠ **And the pixel-box half stays manual**: `FitAddon` registers **no listeners at all** — no `ResizeObserver`, no window handler. `fit()` is something the consumer calls. So xterm splits this exactly where justerm does | xterm.js | `addons/addon-fit/src/FitAddon.ts` (grep for `addEventListener|onResize|register(` returns nothing) |
| alacritty **does** auto-re-fit: a font/offset change recomputes the cell, rebuilds `SizeInfo` from the same window box, and resizes the PTY + terminal when the column/line count moved | alacritty | `display/mod.rs:420` (`compute_cell_size`), `:714-722` (the PTY + terminal resize) |
| xterm **throws** for `lineHeight < 1` where justerm clamps — already recorded at `webgl.rs` `set_line_height`, repeated here because it is the same call site | xterm.js | `common/services/OptionsService.ts:182-186` |

**Direction — not a drift, and the two references are not in conflict.** justerm's renderer already
performs xterm's half automatically: `adopt_spacing` ends with `self.resize(cols, rows)` at the stored
grid, which *is* `handleResize(bufferService.cols, bufferService.rows)`. What is left to the consumer is
precisely what xterm leaves to `FitAddon`.

> ⚠ **The first sentence stopped being true on 2026-08-19 (#773, renderer 0.15.0).** `adopt_selectors`
> no longer resizes anything: a spacing or font change moves *that grid* to another configuration and
> stops. It cannot do xterm's half any more, and not by choice — the drawing buffer is a **surface**
> holding N grids, so "re-lay out at the current grid" has no single grid to mean. **So justerm now
> leaves the consumer both halves where xterm leaves it one**, and that is a real divergence rather
> than a drift: it follows from a structure xterm does not have (one canvas, many terminals), and
> xterm's own `RenderService` is per-terminal precisely because its canvas is. The rows above are
> unchanged and still describe xterm correctly; what changed is which of them justerm can match.
> Recorded here rather than only in the setter doc-comments because this paragraph is where a later
> reader comes to ask whether we drifted. alacritty differs because it **owns its OS window** and can
rebuild `SizeInfo` from a box it controls; an embeddable widget does not own its box — justerm's adapter
pins the canvas to a grid-exact size, so the consumer measures the *viewport* instead. So the applicable
analogue is xterm, and #578's own Sketch (*"drive a re-fit through the existing FitController path"*) was
reasoning from the alacritty shape.

**A second, repo-local reason the Sketch is wrong**, recorded because it is not derivable from either
reference: `FitController` dedupes on `cols`/`rows`, and a spacing change can move the cell while
leaving the grid identical (guaranteed once the `MINIMUM_COLS`/`MINIMUM_ROWS` floors bind). A
grid-keyed dedupe cannot express *"the cell moved but the grid did not"*, so routing the re-fit through
it would drop exactly the flush that resizes the canvas box.

> ⚠ **Corrected 2026-07-30 (#632).** The paragraph above is kept as the record of what was believed,
> but its mechanism is no longer live: the dedupe key now carries the cell, so the flush is not
> dropped. The Sketch is still wrong, on a reason that was there all along and is **not** a dedupe
> question — `ResizePort.resize(cols, rows)` carries a **grid**, while the canvas display box is set
> only inside `JustermRenderer.resize(cssWidth, cssHeight)`, from a **box**. No chain from
> `FitController` to that method exists in this repo. Do not cite the dedupe half as live prior art.

## When is a resize redundant — box, grid, or cell (#632, verified 2026-07-30)

Scoped to *when a resize may be skipped*, which is a different question from the section above (*who*
re-fits). The headline is that **the three references do not agree on one shape**, so "the reference
does X" cannot settle it — each row says which constraint makes its shape available.

| Fact | Reference | Site |
|---|---|---|
| **Prior art for keying a resize dedupe on the CELL exists.** `handle_update` coalesces both input families, then runs **two** dedupes over the candidate: a grid-keyed one (`screen_lines`/`columns`) that resizes the PTY, the terminal and the damage tracker, and a **whole-`SizeInfo`** one that queues the renderer update. The memory is then assigned from the candidate — a live-updated snapshot, not state read back from a sink | alacritty | `alacritty/src/display/mod.rs:713-715` (grid), `:728` (full `new_size != self.size_info`), `:736` (the store) |
| …and the cell is in that second key **by construction**: `SizeInfo` derives `PartialEq` and carries `cell_width` / `cell_height` among its fields | alacritty | `alacritty/src/display/mod.rs:144` (derive), `:153`, `:156` |
| ⚠ **ghostty splits the two obligations instead of widening one key** — so its box-keyed dedupe is valid only *because* the cell has its own undeduped entry point. Do not read its box dedupe as permission to dedupe a cell change | ghostty | `src/Surface.zig:2495` (box dedupe, *"if the screen size didn't change, then our grid size could not have changed"*), `:2411` (`setCellSize`, no dedupe) |
| **xterm.js keeps no fit-side memory at all** — its executed dedupe is at the sink (`Terminal.resize` vs `this.cols`/`this.rows`), which is available to it because the dedupe lives *on* the thing being resized. justerm's `ResizePort` is published and **write-only**, so that shape is unavailable — the reason the widened key is a proxy rather than the real thing | xterm.js | `addons/addon-fit/src/FitAddon.ts:47-54`; `browser/CoreBrowserTerminal.ts:1055-1062` |
| **Direction: justerm-web converges with alacritty**, not with xterm. The `(cols, rows, cellWidth, cellHeight)` key is a live-updated snapshot widened to every input whose change can require an emit — alacritty's shape one layer up. This layer does **not** diverge alone, and there is no family parity fix to track | justerm-web | `justerm-web/src/fit.ts` `FitController.last` |
| ⚠ **In-repo, the shape xterm uses does exist** — `JustermRenderer`'s `lastFrameGrid` compares against live `backend.cols()`/`rows()`. Cite *that* as the local example of "dedupe against authoritative live state", not `FitController` | justerm-web | `justerm-web/src/justerm-renderer.ts` `lastFrameGrid` |

## Resizing while the GL context is lost — the reference never asks the question (#639, verified 2026-08-03)

A **negative result**, and the reason it is worth a section rather than a shrug: "xterm's `handleResize`
has no context-loss guard" is true and reads as permission to have none. It is not — xterm has nothing
to guard, because it never performs the read that makes a lost context dangerous. The distinction is
the whole finding, and it is invisible unless the absence is recorded with the thing it is absent from.

**The title is narrower than the table, deliberately** (2026-08-04): the heading is an anchor other notes
link to, so it stays as written while the last four rows now cover context loss *beyond* resizing —
including a correction to a row this section originally got wrong. Which is itself the lesson the
section was already about, turned on its author: the negative result *"xterm never asks"* was true and
the row beside it generalised from one silence to three implementations. **An absence is evidence about
the question you asked, never about the neighbouring one you did not.**

| Fact | Reference | Site |
|---|---|---|
| **`handleResize` runs unguarded during a context loss** — no `isContextLost` check, no deferral; it recomputes dimensions and assigns the canvas straight through. The context-loss listeners sit in the constructor and touch only the restore timeout and the atlas rebuild | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:192` (the handler), `:125-146` (the two listeners, which it never consults) |
| …because **the canvas is sized from the grid, never from what the driver granted**: `_canvas.width = dimensions.device.canvas.width`, a `cols * cell` product. There is no read-back to return 0 | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:205` |
| ⚠ **`drawingBufferWidth` / `drawingBufferHeight` appear nowhere in the project** — `rg drawingBuffer src addons` returns **0 hits** at this pin. So the failure class justerm's adopt-what-fits creates (a lost context reports a 0x0 buffer, the loop floors the grid to 1x1) is structurally unreachable there, and xterm cannot arbitrate the guard | xterm.js | absence, reproducible: `rg -n drawingBuffer ../.refs/xterm.js/{src,addons}` |
| ⚠ **`isContextLost` appears nowhere either** — `rg isContextLost src addons` is also **0 hits**. So xterm has no position on the event-vs-state race that this territory turns on (a browser kills a context synchronously and only *queues* `webglcontextlost`); it consults neither the query nor its own listeners outside the constructor. The "cannot arbitrate" conclusion therefore reaches further than the buffer read-back — it covers the predicate too | xterm.js | absence, reproducible: `rg -n isContextLost ../.refs/xterm.js/{src,addons}` |
| ~~**The comparison set does not extend.** alacritty and ghostty … have no context-loss concept at all~~ — **WRONG for alacritty, corrected 2026-08-04 (#579).** It has both a concept and a recovery path: `make_current` asks `was_context_reset()`, or catches glutin's `ErrorKind::ContextLost` when re-making current, and then rebuilds the context *and* the renderer in place. The row was written while checking whether the reference guards a **resize**, and generalised from that silence to the whole territory | alacritty | `alacritty/src/display/mod.rs:561` (the ask), `:564` (the glutin arm), `:576-595` (recreate both) |
| **What it asks is the useful half, and it agrees with ADR-0027 D1/D2.** `was_context_reset` calls `glGetGraphicsResetStatus` under `GL_KHR_robustness`, returning `false` when the extension is absent — i.e. it asks **the driver, at the point of use**, never a flag some earlier event set. That is D1's rule reached independently, in a codebase with no queued-event race to have taught it | alacritty | `alacritty/src/renderer/mod.rs:281` (`was_context_reset`), `:304` (`supports_robustness`) |
| **It still cannot arbitrate a *consumer* surface, which is the distinction that matters for #579.** alacritty is an application: it recovers synchronously at the point of use, with no deadline, no notification and nobody to tell. So "no reference to lose to" holds for the question of what to publish to a consumer, and fails for the question of which source a guard asks | alacritty | absence of a notify path; the recovery above is the whole of it |
| **ghostty confirmed genuinely n/a**, re-checked rather than inherited: no context/device/surface-loss concept, no graphics-reset query | ghostty | absence, reproducible: `rg -n 'context_lost\|ContextLost\|GetGraphicsResetStatus\|device_lost' ../.refs/ghostty/src` |
| **Direction: justerm decides alone.** The read-back is this repo's own #339 design — `resize`'s comment states *"No reference does this"* and names what the three references do instead — so the guard's shape came from the four sibling entry points in the same crate, not from prior art | justerm-renderer | `justerm-renderer/src/webgl.rs` `resize`, `adopt_spacing`, `set_device_pixel_ratio` |

## Reading a GL parameter that a lost context answers with `null` (#688, verified 2026-08-03)

The sibling of the section above, and the useful contrast: there the reference had **nothing** to
guard, here it does the **same thing we do** and gets away with it. So this is not a divergence to
correct — the shape is shared and the difference lives entirely in the binding. Recorded because
"xterm reads it unguarded too" is the sentence that would otherwise close the question, and it is
only half of what the reference says.

| Fact | Reference | Site |
|---|---|---|
| **`MAX_TEXTURE_SIZE` is read in the constructor, unguarded**, exactly as justerm's is — no `isContextLost`, no null check | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:123` |
| …and the **context-loss listeners are attached two lines *after* it** (`:125`, `:137`). So "attach before any GL work" is a promise justerm made to itself, not a reference practice | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:123` then `:125` |
| ⚠ **But its other two parameter reads *are* falsy-guarded** — `throwIfFalsy(gl.getParameter(...))`, which turns a `null` into a throw rather than a value carried on. So the reference is not indifferent to the read; it is inconsistent across its own call sites, and the guarded form is the one it wrote where the value is load-bearing | xterm.js | `addons/addon-webgl/src/GlyphRenderer.ts:128`, `:130` |
| **Why it survives there and not here — the binding, not the design.** JS receives `null` and carries on; `glow` unwraps. `Context::from_webgl2_context` does `get_supported_extensions().unwrap()` and panics *before* the parameter read is reached, and `get_parameter_i32` itself answers `0` for a `null` (`.as_f64().map(…).unwrap_or(0)`) rather than failing | glow 0.18.0 | `glow-0.18.0/src/web_sys.rs:237-239` (the panic), `:3590` (the harmless read) |
| **Every other glow call this crate makes fails cleanly on a lost context**, so the panic surface is one call, not a class: `create_*` → `Result`, `get_uniform_location` → `Option`, `get_*_status` → `.as_bool().unwrap_or(false)`, `get_*_info_log` → `unwrap_or_else(String::new)` | glow 0.18.0 | `web_sys.rs:1730/1757/1890/2446/2588`, `:3823`, `:1846`, `:1997`, `:2036`, `:1856` |

### A second loss *during* the restore — the reference is strictly weaker, so it grants nothing (#688 lens, verified 2026-08-03)

Recorded because the reference's shape here is the one a reader is most likely to cite as
permission, and it is the outlier. justerm's `restore` builds every replacement into locals and
commits only after the re-bake succeeds; its in-repo siblings (`bake_config`,
`rebuild_all_configs`, `adopt_selectors`' rollback) do the same. (Those three were named
`rebake_atlas` / `rebake_for_cell` / `adopt_spacing`'s rollback until #772 replaced the
single-configuration bake path with a keyed one — the family position is unchanged.) **Direction: this layer *and* its siblings agree against
the reference — a family position, held, and xterm must not be read as licence to half-commit.**

| Fact | Reference | Site |
|---|---|---|
| The `webglcontextrestored` handler calls `_initializeWebGLState()` **unguarded and without `try`/`catch`**, straight from the event listener | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:137-146` |
| That method assigns the rectangle renderer, **then** the glyph renderer — two sequential commits with no rollback between them | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:279-287` |
| …and `GlyphRenderer`'s constructor throws on a lost context (`throwIfFalsy(gl.getParameter(...))`). So a second loss mid-restore leaves it **half-committed** — new rectangle renderer, stale/disposed glyph renderer — with no retry latch, and the exception escapes into the listener | xterm.js | `addons/addon-webgl/src/GlyphRenderer.ts:128`, `:130` |
| justerm cannot reach that state by construction: `restore` deletes the half-built replacements and returns `Err` on a re-bake failure, leaving the live objects in place, and the state machine keeps `pending_rebuild` set so the next frame retries | justerm-renderer | `justerm-renderer/src/webgl.rs` `restore` (the `rebake` error path), `context_loss.rs` `a_failed_rebuild_is_retried_on_the_next_frame` |

## Surviving a throw from inside a rAF loop — the reference clears its handle first (#696, verified 2026-08-03)

The useful kind of reference reading: the question was framed as *"catch it, or let it die?"* and
xterm answers a **third** thing — it never catches, and never needs to, because it orders two
assignments the other way round. Recorded because the framing is the trap, not the answer.

| Fact | Reference | Site |
|---|---|---|
| `_innerRefresh()` sets `this._animationFrame = undefined` as its **first statement**, before it computes anything or invokes the render callback | xterm.js | `src/browser/RenderDebouncer.ts:54-55` |
| So a throw from the callback leaves no handle behind, and the next `refresh()` schedules a fresh frame — its guard is the same `if (this._animationFrame !== undefined)` shape ours had | xterm.js | `src/browser/RenderDebouncer.ts:47-51` |
| ⚠ **There is no `try`/`catch` anywhere in the debouncer**, so an error from rendering propagates to the browser exactly as it would from any other rAF callback. The reference does not choose between swallowing and re-raising; it removes the need to choose | xterm.js | absence, reproducible: `rg -n 'try \{' ../.refs/xterm.js/src/browser/RenderDebouncer.ts` |
| **The shapes differ in one way that matters when copying**: xterm's loop is a *debouncer* (re-armed by whoever calls `refresh`), justerm's is *self-perpetuating* (the body re-arms itself). Clearing first therefore makes xterm's loop continue and justerm's **stop-but-restartable** — acceptable here only because `updateCursor` calls `start()` on every decoded frame, which is a justerm fact the reference cannot supply | justerm-web | `justerm-web/src/justerm-renderer.ts` `updateCursor`, `applyFrame` |

## Background transparency — the shape of the knob (#577, verified 2026-07-29)

The file had **zero** rows on transparency before this, though `set_bg_alpha` has existed since #298
and `justerm-renderer/README` and `colour-policy.md` both name it. The two references answer the
design question in genuinely different shapes, so "what does the reference do" needs the shape spelled
out before any value is copied.

| Fact | Reference | Site |
|---|---|---|
| The knob is a **scalar** `opacity: Percentage` (0.0–1.0), and it lives under **`window`**, not under `colors` | alacritty | `config/window.rs:46` |
| The knob is a **boolean** `allowTransparency`, default **`false`**, and it is an *option* — the theme is colours only | xterm.js | `common/services/OptionsService.ts:47` |
| ⚠ **The boolean is not the alpha.** With it off, xterm *discards* the alpha channel of the theme colour it was given — so the amount of transparency is carried by the colour, and the option only says whether to honour it. A scalar has no place to live in that model | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:339` |
| The option feeds the **GL context creation flag** (`alpha: this._config.allowTransparency`), so changing it rebuilds the atlas rather than re-drawing. justerm creates its context `alpha: true` unconditionally (#298), which is why justerm can toggle at runtime and xterm cannot | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:101` |
| Only the **default-background** cell is transparent: `compute_bg_alpha` returns `0.` for `Color::Named(NamedColor::Background)` and `1.` for everything else — an explicitly coloured cell stays opaque | alacritty | `display/content.rs:388-396` |
| …and the clear carries the opacity, which is what the `0.` above reveals: the transparent cell draws nothing and the cleared buffer shows through | alacritty | `display/mod.rs:470` |
| Extending alpha to explicitly coloured cells is a separate **opt-in**, `colors.transparent_background_colors` — not the default. justerm has no equivalent | alacritty | `display/content.rs:275-278` |
| The same opt-in exists under another name, also defaulting off — so **two of three references converge** on "explicitly coloured cells stay opaque unless you ask otherwise", and justerm has neither the mechanism nor the knob | ghostty | `config/Config.zig:1018` (`@"background-opacity-cells": bool = false`) |
| **Minimum contrast ignores the background's alpha**: luminance is taken from `bgRgba >> 8`, which shifts the alpha byte off before the ratio is computed | xterm.js | `common/Color.ts:297` |
| …and ghostty agrees by a different route — it composites the background *first* and still reads only `bg.rgb`, so the opacity never enters the ratio | ghostty | `renderer/shaders/glsl/common.glsl:97-110` |
| ⚠ **alacritty also makes the surface behind itself transparent** — `window.set_transparent(config.window_opacity() < 1.)`. It can, because it owns the OS window. A browser widget cannot: the page behind the canvas is the consumer's, which is why `bgAlpha`'s doc says making it transparent is consumer CSS and is the first thing to check when the option appears to do nothing | alacritty | `display/window.rs:197` |

**Direction.** Not a divergence to fix. On the *shape* of the knob the references split 1–1 (alacritty
scalar, xterm boolean-plus-colour-alpha) and justerm took alacritty's in #298, so there is no majority
to be an outlier against. On *which cells* go transparent, justerm's shader
(`webgl.rs`, the `(!block && v_bg_default > 0.5) ? u_bg_alpha : 1.0` gate — the expression quoted here
until 2026-08-18 was `bg_a = … mix(u_bg_alpha, 1.0, cov) …`, which #317 §2 replaced; the *convergence*
is unaffected, only the code it was read off) converges with
alacritty's **default** exactly — default-bg only, glyph coverage pulled back to opaque, cursor cell
forced opaque. The one thing justerm has no expression for is the *opt-in* both alacritty and ghostty
carry for extending alpha to explicitly coloured cells — an absent **feature**, not a wrong
behaviour, and the renderer's to add (#298's layer) rather than the widget's.

**On minimum contrast, the answer is "nobody does it" and that is a real answer.** justerm corrects
against the nominal opaque background and ignores `bgAlpha`; so do both references that have the
feature, by two different routes. None of the three can know what is behind the window, so there is
nothing to converge *on* — which is why this is recorded here rather than filed as a defect.

## OSC 8 hyperlinks — where the URI lives, and what frees it (#628/#635, verified 2026-07-30)

The area had **no rows at all** until now, which `docs/map/territory/hyperlinks.md` recorded as a
known hole while `LINK_PRESENT` carried an unpinned claim about xterm's bit layout. The occasion:
#628 found justerm's `hyperlink_pool` was append-only — every OSC 8 open cost a `String` for the life
of the `Term` — and the options turned on what the references do instead. Every row grepped at the
pinned SHAs.

Two questions, and they have **different answers**, which is the point of splitting them: *where does
the URI live* and *what frees it*.

| Fact | Reference | Site |
|---|---|---|
| The URI hangs off the cell's side structure as `Arc<HyperlinkInner>` — **no registry, no pool, no ids**. It dies with the last cell holding it, so there are no release paths because there is nothing to release *from* | alacritty | `alacritty_terminal/src/term/cell.rs:45` |
| A cell→id map plus a ref-counted set, freed at refcount 0 — *"ref-counted so that a set of cells can share the same hyperlink **without duplicating the data**"* | ghostty | `src/terminal/hyperlink.zig:209` |
| …and the id is deliberately **not** on the cell: *"its a waste to store the hyperlink ID in the cell itself"* — justerm already satisfies this half via the row map | ghostty | `src/terminal/hyperlink.zig:20-23` |
| A global registry keyed by a minted id (`_nextId++`), which is the shape justerm's pool copied | xterm.js | `src/common/services/OscLinkService.ts:24` |
| ⚠ **…and the registry is the thing that reclaims.** Each entry holds the line markers referencing it; when the last is disposed the entry is deleted from **both** maps. Copying `_nextId++` without this is what left justerm's pool immortal | xterm.js | `src/common/services/OscLinkService.ts:100` |
| ⚠ **Dedup happens only when the sequence declares `id=`.** A link with no id *"will only ever be registered a single time"* and gets a fresh id per open; one with an id is looked up by an `id`-plus-`uri` key and reuses the entry. So "never merge on URI alone, always merge on a declared id" is one rule, not two — justerm shipped the first half only until #635 closed the second | xterm.js | `src/common/services/OscLinkService.ts:51` |

**Direction: 3:0 for reclamation, 1:2 for `id=` grouping.** All three free the storage; justerm was
the only one that never did. Only xterm.js groups by `id=`, so that gap is a conformance item against
the single reference that has the feature, not a divergence from a consensus.

### How xterm.js parses `id=`, and the three places a reasonable reading goes wrong (#635, verified 2026-07-30)

Added while implementing the grouping half. Rows split this finely because each was checked against a
*wrong* implementation that still passes a single-parameter test — the mutation that reddens each is
noted, since a row nobody can fail is a row nobody can trust.

| Fact | Reference | Site |
|---|---|---|
| `params` is a **`:`-separated** key=value list — `id=xyz123:foo=bar:baz=quux` — and the split is on colons, not on the OSC `;` | xterm.js | `src/common/InputHandler.ts:3100` |
| ⚠ **`id` may sit anywhere in that list**: the scan is `findIndex(e => e.startsWith('id='))`, so matching only a *leading* `id=` is the wrong parse and is green on every single-parameter test | xterm.js | `src/common/InputHandler.ts:3129` |
| ⚠ **An empty value is not an id** — `slice(3) \|\| undefined`, and the `\|\|` is the entire behaviour. Reading `slice(3)` alone gives the opposite answer, and the resulting empty key groups *every* `id=`-with-no-value link in a session across unrelated URIs: a wrong answer that grows with uptime | xterm.js | `src/common/InputHandler.ts:3130` |
| Only the **first** `id=` is consulted (`findIndex`), so an empty first one yields no id rather than searching on for a non-empty sibling | xterm.js | `src/common/InputHandler.ts:3129-3131` |
| The group key is `` `${id};;${uri}` `` — **id and URI together**, so a reused id aimed at a new target is not the same link | xterm.js | `src/common/services/OscLinkService.ts:87` |
| ⚠ **The id-lookup map is a second map, and it is reclaimed, not weak.** `_entriesWithId` is deleted alongside `_dataByLinkId` when the entry's last line marker is disposed — so grouping does **not** cost xterm a permanent registry either. justerm has no disposal hook by design and expresses the same lifetime with `Weak` | xterm.js | `src/common/services/OscLinkService.ts:98-100` |
| Its own doc states the user-visible contract: *"Cells that share the same ID and URI share hover feedback"* — the symptom, not the mechanism | xterm.js | `src/common/InputHandler.ts:3101-3102` |

**Read here, measured, and then ported (#650).** xterm.js splits OSC 8's own arguments on the **first
`;` only** (`setHyperlink`, `InputHandler.ts:3106-3112`) — `data.indexOf(';')`, then `slice(idx + 1)`
takes *all* the rest as the URI — explicitly *"to support unencoded semi-colons in the URIs"*.
justerm read `params[2]` out of vte's `;`-split and kept only the first segment. Throwaway probes,
2026-07-30, deleted after reading; the numbers are why this became a fix rather than a note:

| Fed | `Engine::link_at`, before #650 |
|---|---|
| `OSC 8 ; ; https://example.com/a;b=c BEL` | `https://example.com/a` — truncated at the `;`, silently |
| the same with `id=q` in `params` | `https://example.com/a` — the `id=` path was no different |
| control: `https://example.com/a%3Bb=c` | `https://example.com/a%3Bb=c` — intact |

Two things that table cannot show, and both were needed to act on it:

- **The control pins the cause** to the *raw* `;` rather than to anything else in the parse. Without
  it, "truncated" is a guess about which step dropped the tail.
- **Nothing is lost at the parser**, which is what made this a rejoin instead of an impossibility —
  vte hands the handler `["8", "", "https://example.com/a", "b=c"]`, and
  `["8", "id=q", "https://x/p?a=1", "b=2", "c=3"]` for a longer one. Only the handler discarded it.

The fix is `params[2..].join(';')`. The close survives it (`]8;;` is `["8", "", ""]`, whose rejoin is
empty), and a `%3B` is still never decoded — `Hyperlink::uri` hands the target over exactly as
declared, and openability is consumer policy (ADR-0017).

**The trap this section exists to stop.** Reading only the first row of each reference gives *"they
all keep a registry"* — which is how #46 arrived at a global pool and stopped. The reclamation is the
half that does not show up at the site where the id is minted, and in xterm.js it is eighty lines
further down in the same file.

## A glyph the font draws wider than its cell (#792, verified 2026-08-21)

Read for #792, where the question was what to do with ink that leaves the cell **sideways**. The
short version: all three let it overflow, because their glyph quad *is* the glyph's bounding box —
which is the capability justerm gave up in ADR-0019 to keep one evaluation per pixel. So their
defaults rest on something this renderer does not have, and neither of the two mechanisms below was
imported as an authority (the tie-breaker table has no row for glyph bake geometry).

| Fact | Reference | Site |
|---|---|---|
| Rescaling exists, and it is **off by default** — an option the user turns on | xterm.js | `src/common/services/OptionsService.ts:51` (`rescaleOverlappingGlyphs: false`) |
| What it rescales: single-width only, ink over **1.5 cells**, never ASCII (`codepoint > 0xFF`), never emoji, powerline or nerd-font ranges | xterm.js | `src/browser/renderer/shared/RendererUtils.ts:47` |
| ⚠ **The mechanism is not a re-rasterisation.** It shrinks the *quad's width* to `cell.width - 1` and leaves the texture alone — an anisotropic squeeze of an already-baked bitmap, i.e. a resample. justerm condenses at bake instead, so the browser rasterises a condensed outline; same direction, different artefact | xterm.js | `addons/addon-webgl/src/GlyphRenderer.ts:301` |
| ghostty has a general constraint system — `fit` / `cover` / `fit_cover1` / `stretch`, plus per-axis alignment, padding and a `max_xy_ratio` | ghostty | `src/font/Glyph.zig:135` |
| ⚠ **But ordinary text glyphs get `.none`.** The renderer applies a Nerd-Font table entry if there is one, else `.fit` **only when the codepoint is a symbol**, else nothing — so a Latin digraph like `Ǆ` is left to overflow, exactly as alacritty leaves it | ghostty | `src/renderer/generic.zig:3175` |
| A constraint may span two cells when the next cell is blank (`constraint_width`, `max_constraint_width: u2 = 2`) — the width-2 axis justerm's #792 states as uncounted rather than covered | ghostty | `src/font/Glyph.zig:115` |

## Renderer ink channels

| Fact | Reference | Site |
|---|---|---|
| The strikethrough draws in the **glyph foreground**, never in the SGR 58 underline colour — confirming #525's premise | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:758-762` |
| ⚠ **The mechanism is a `save`/`restore` bracket, and the obvious grep hit says the opposite.** The underline block opens with `save()` (`:565`), sets `strokeStyle` from `getUnderlineColor()` (`:576-583`), then assigns `fillStyle = strokeStyle` (`:585`) — read alone, that says the SGR 58 colour becomes the fill for everything after it. `restore()` at `:688` undoes it, so the glyph `fillText` (`:735`) and the strikethrough's `strokeStyle = fillStyle` (`:762`) both get the foreground back | xterm.js | `TextureAtlas.ts:565`, `:585`, `:688`, `:735`, `:762` |
| ⚠ Path note: `TextureAtlas.ts` lives under `addons/addon-webgl/src/`, **not** `src/browser/renderer/shared/` as #525 cites | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts` |
| **The clearest statement of #525's rule in any reference — one ternary picking the ink by which mark it is:** `let color = if flag.contains(Flags::STRIKEOUT) { cell.fg } else { cell.underline };`. The SGR 58 colour is reachable *only* on the underline branch | alacritty | `alacritty/src/renderer/rects.rs:198` |
| The strikethrough is a separate emit from the underline, so the underline colour has no path to it | ghostty | `src/renderer/generic.zig:3029` (`addStrikethrough`) |
| **Draw order: the underline goes UNDER the glyph and the strikethrough OVER it — and the stated reason is exactly the coloured-underline case.** *"We draw underlines first so that they layer underneath text. This improves readability when a colored underline is used which intersects parts of the text (descenders)."* **Note what it does NOT condition on:** ghostty appends the underline before the glyph unconditionally, with no ink-class test — so a `█` swallows a coloured underline there. justerm settled this differently in #712 (ADR-0019 rule 6: occlusion follows the ink class), which is a divergence *taken*, not one outstanding | ghostty | `src/renderer/generic.zig:2932` |
| Same order in xterm: the glyph `fillText` at `:735` sits **between** the underline block (`:565`–`:688`) and the strikethrough (`:762`). So "xterm draws the strike after the underline" is true but incomplete — cite it for band order, not for band-vs-glyph order. ⚠ **xterm computes the ink class and never uses it for order**: `treatGlyphAsBackgroundColor` feeds `_getForegroundColor` at `:538` only, so the reordering here is blanket, exactly as ghostty's is | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:735`, `:538` |
| **alacritty's band-over-glyph is a batching artefact, not a decision** — `draw_rects` runs after `draw_cells` and carries the visual bell and message bar in the same pass, which must be on top; nothing in the file defends the underline's place, and its `descent` references are all band *position* | alacritty | `alacritty/src/display/mod.rs:878`, `:990`, `renderer/rects.rs:81-116` |
| No reference has any strikethrough-**colour** concept: SGR 58/59 are underline-colour set/reset only, and `strike*_color` has zero hits across all three trees. Making the strike permanently follow-fg forecloses nothing that exists | ghostty | `src/terminal/sgr.zig:388` |

### How a translucent background composites (#317 §2, verified 2026-08-18)

**Read the tie-breaker before using these.** Renderer cell composition answers to justerm's own model
(ADR-0019); these rows are mechanism, not authority. They exist because #317 §2 deferred a fix for
three years' worth of readers on the premise that a correct composite *"would belong in the shared
shader"* and needed *"premultiplied-AA / two-pass"* — a premise inherited from beamterm and never
re-derived. What the references actually establish is narrower: **they get there with a different
architecture**, not that this architecture cannot.

| Fact | Reference | Site |
|---|---|---|
| Background and text are **separate passes with hardware blending**, so no fragment ever has to compose the two itself: `BlendFuncSeparate(SRC_ALPHA, ONE_MINUS_SRC_ALPHA, SRC_ALPHA, ONE)` | alacritty | `alacritty/src/renderer/mod.rs:252` |
| Text then switches to dual-source subpixel blending — a second, different blend func on the same frame | alacritty | `alacritty/src/renderer/mod.rs:260` |
| Translucency is a whole configurable blending *mode*, with an sRGB framebuffer for linear blending | ghostty | `src/renderer/OpenGL.zig:157`, `generic.zig` (`use_linear_blending`, `use_linear_correction`) |
| ⚠ **Neither is precedent for justerm's shader, and the difference is structural.** justerm enables no GL blending at all and emits one fragment carrying the whole composite (`premultipliedAlpha: false`). Straight-alpha source-over is closed-form there — `a = 1 - w(1-A)`, `rgb = (ink + bg·A·w)/a` — so the arithmetic is available in one pass and was simply not being done. A finding of the form *"the references use two passes"* is a design proposal on a layer where they have no vote, not a defect | — | this repo, `justerm-renderer/src/webgl.rs` |

## Validating a decoded payload against its own declared geometry (#582, verified 2026-07-31)

The occasion: justerm's `decode` accepted a frame whose parts did not fit the `cols`/`rows` the same
frame declared. The question routes oddly — `docs/map/territory/wire-format.md` records that **no
reference serializes terminal state this way**, so there is no format to compare against. What *can*
be compared is the posture: when a coordinate cannot exist in the buffer it names, does the
implementation reject, clamp, drop, or index it anyway? Every row grepped at the pinned SHAs.

| Fact | Reference | Site |
|---|---|---|
| A coordinate computed **from the VT stream** is clamped, never refused — `grid_clamp` with an explicit `Boundary`, and `goto` clamps the line into `[0, bottommost]` | alacritty | `alacritty_terminal/src/index.rs:92`, `term/mod.rs:1168` |
| Same posture at the same layer: the cursor setter writes, then `_restrictCursor()` pulls it back inside | xterm.js | `common/InputHandler.ts:900-910` |
| A point that **cannot exist** is answered with absence, never a manufactured in-range one — *"Never manufacture an out-of-bounds pin for that page"*, and the doc leaves clamping to the caller | ghostty | `terminal/PageList.zig:4930-4943` |
| A **structured payload declaring its own dimensions** is cross-checked against them and rejected whole: `DimensionsRequired`, `DimensionsTooLarge`, and `expected_len != actual_len → InvalidData` | ghostty (kitty graphics) | `terminal/kitty/graphics_image.zig:419-431` |
| ⚠ **The closest analogue of all, and it lands on the other side of the reject/assert line.** `verifyIntegrity` checks exactly justerm's class — a sparse side-map entry must pair with a cell carrying the bit (`UnmarkedGraphemeCell`, `MissingHyperlinkData`) — and its doc names the use case: *"useful for assertions, deserialization, etc."* | ghostty | `terminal/page.zig:377`, `:334`, `:450` |
| ⚠ **…but every caller is debug-gated**, `if (comptime build_options.slow_runtime_safety …)`, so none of it runs in a shipped build | ghostty | `terminal/page.zig:365` |
| ⚠ **alacritty *does* have a decode boundary and validates nothing at it.** `Grid`, `Row` and `Storage` all derive `Deserialize` under the `serde` feature; `Storage`'s private `len`/`zero` are taken verbatim and indexing is unguarded | alacritty | `grid/mod.rs:109`, `grid/row.rs:16`, `grid/storage.rs:32` |
| Interior access is unguarded **as policy**, not by omission — *"for performance reasons there is no bounds checking here"* | xterm.js | `common/CircularList.ts:105-108` |
| **A grid has a floor and no ceiling.** `MINIMUM_COLS = 2` — *"Less than 2 can mess with wide chars"* — and `MINIMUM_ROWS = 1`, applied with `Math.max` on every resize. There is no maximum anywhere | xterm.js | `common/services/BufferService.ts:13`, `common/CoreTerminal.ts:192` |
| ⚠ **…and the one layer that could pick a ceiling refuses to.** justerm's own renderer does not choose a number: it asks the GL implementation and adopts what it gets, documenting *"Do not try to predict the limit"* — Chromium clamps each axis to `min(max_texture_size, max_renderbuffer_size, max_viewport_dims)` and *then* applies a hard-coded `5760×5760` area budget derivable from no `getParameter` | justerm-renderer (sibling, not a reference) | `justerm-renderer/src/webgl.rs` `resize` |

**The direction, and it is not the simple one.** The convergent rule is *validate where the
coordinate is computed, index freely inside* — which is the rule justerm's own `Term::damage_span`
(clamp + `debug_assert`) and `justerm-renderer`'s `FrameGrid::validate` (reject at the boundary)
already follow, and which `decode` alone did not. That much supports #582.

**What it does not support is reading the last four rows as a mandate for a hard production reject.**
The one reference that validates the *same* structure justerm does treats it as a debug assertion,
and its production restore path validates nothing. The distinction that survives is **who produced
the payload**: ghostty deserializes its own snapshot in its own process, so an integrity failure is a
ghostty bug and belongs in an assert. justerm's `decode` reads bytes a consumer hands back over its
own transport (ADR-0008, `tests/robustness.rs` names them attacker-influenced), so the same failure
is *input*, and input is rejected rather than asserted. **The validity condition to re-check if it
ever changes: the day justerm decodes something it produced in-process, ghostty's answer is the
better one.** That split is why #582 rejects on the decode side and only `debug_assert!`s on the
encode side — encode's input is a `Span` justerm built, which is exactly ghostty's case.

**The geometry rows resolve a second question and they resolve it asymmetrically.** A frame's own
`cols`/`rows` cannot be validated against anything inside the frame, and the temptation is to pick a
"sane" ceiling in the consumer, since `justerm-web`'s a11y mirror allocates `cols × rows` objects
straight from the header and its row tree makes one DOM element per row. **Do not**: nobody in the
family or the references picks such a number, and the only layer that knows the real limit — the
renderer — deliberately asks the device instead of predicting it. A ceiling in the widget would be
the single arbitrary constant in the whole stack. The **floor** is the opposite: 2 columns is
non-arbitrary, agreed by xterm.js and by justerm's own `MIN_COLUMNS` (#547), and enforced at every
engine entry point — while `decode` accepts `cols: 0` and `cols: 1`. That is the half worth acting
on, and it is why the "measure what a huge geometry actually does to the tab" experiment was
**deliberately not run**: no outcome of it changes the recommendation, because the only fix it could
motivate is the arbitrary number this paragraph rules out.

**Acted on as #663, and the reject-vs-clamp row is the one it adds — including a derivation that
looked right and is not.** `decode` rejects `cols < MIN_COLUMNS` and `rows == 0` as `BadGeometry`; no
ceiling was added, and the recommendation above is why. On the floor's *value* there is no divergence
at all — justerm and xterm.js agree exactly. On its *form* the references **split, 2–1**, and the
split does not fall where the obvious rule predicts:

| Fact | Reference | Site |
|---|---|---|
| Clamps a sub-floor geometry at its own resize — `Math.max(MINIMUM_COLS, …)` | xterm.js | `common/services/BufferService.ts:13`, `common/CoreTerminal.ts:192` |
| Clamps likewise; publishes `MIN_COLUMNS` / `MIN_SCREEN_LINES` because its app reads them | alacritty | `alacritty_terminal/src/term/mod.rs:36`, `:39` |
| ⚠ **Rejects, at a site it owns** — *"Screen and scrolling-region invariants require non-zero dimensions. Validate before changing any terminal state."* `if (opts.cols == 0 or opts.rows == 0) return error.InvalidValue` | ghostty | `src/terminal/Terminal.zig:3719-3721` |

**So "clamp if you own the number, reject if you are reading it" is not the rule, and #663's first
draft said it was** (in four places, until the completeness pass found the third reference — one this
repo already cited, at `justerm-core/tests/min_rows.rs:19`). Ghostty owns its resize and refuses
anyway. Two facts stand once that leg is removed. First, **this boundary cannot repair**: the payload
behind the header was laid out for the width it declares, so widening `cols` re-indexes a frame rather
than fixing one, leaving reject and hand-back-wrong-cells as the only total answers. Second, **no
reference arbitrates the site**, because none of them decodes a serialized grid — the tie-breaker
table's *"wire / frame / API shape → this repo's own precedent"* row, not a divergence. What ghostty
does contribute is that refusing an impossible geometry outright is ordinary, not an invention.

## A selection when the screen changes under it (#660, verified 2026-07-31)

The occasion: on justerm's alt screen a selection outlived a resize, and `selection_range` then
indexed past the shrunk grid and panicked. The question is not "should a selection survive a resize"
but **what stops one from pointing at content that is no longer there**. All three references answer,
and they answer differently — which is useful, because the three answers are the three available
designs.

| Fact | Reference | Site |
|---|---|---|
| Clear on a **width** change, rotate by the line delta otherwise — `if old_cols != num_cols { self.selection = None }`, else `selection.rotate(self, &range, -delta)` | alacritty | `alacritty_terminal/src/term/mod.rs:680-682`, `:686-689` |
| Clear on a **height** change, and the comment names the bug class rather than a design: *"Clear selection when resizing vertically. This experience could be improved, this is the simple option to fix the buggy behavior"*, citing xterm.js issue 5300 | xterm.js | `src/browser/services/SelectionService.ts:156-160` |
| **Neither — the staleness is in the type.** A selection's bounds are `tracked` or `untracked`, and the doc states the hazard directly: *"Untracked bounds are unsafe beyond the point the terminal screen may be modified, since they may point to invalid memory."* A tracked bound is a pin the pagelist updates, so it cannot go stale | ghostty | `src/terminal/Selection.zig:32-34` |
| …and a **screen swap** clears it outright, exactly as justerm's `enter_alt_screen` / `leave_alt_screen` do | ghostty | `src/terminal/Terminal.zig:4271` `switchScreen` → `new.clearSelection()` |
| **All three also bound at *read* time, independently of whatever they do at resize** — the anchor fixups above are not the only defence any of them has. alacritty's `Selection::to_range` opens with *"Clamp selection to within grid boundaries"*: fully-out returns `None`, otherwise both endpoints go through `grid_clamp` | alacritty | `alacritty_terminal/src/selection.rs:283-288` |
| …xterm.js's line fetch is total by construction — `const line = this.lines.get(lineIndex); if (!line) { return ''; }` | xterm.js | `src/common/buffer/Buffer.ts:554-559` |
| …and ghostty clamps a pin's column against its own page in both corner accessors, `p.x = @min(…, p.node.cols() - 1)`, on top of pins that keep the row valid | ghostty | `src/terminal/Selection.zig` `topLeft` / `bottomRight` |
| **On ED, alacritty drops the selection outright** — `self.selection = None` in the clear arm. justerm deliberately does not (in-place erases *"stale in place … exactly like xterm's decorations"*, `term/search.rs`), so alacritty is the stricter outlier here and the recorded justerm rationale cites only xterm | alacritty | `alacritty_terminal/src/term/mod.rs:1803` |

**Why the read-time row matters more than it looks.** It is the half a fix aimed at the *anchor* never
reaches: justerm's `selection_range` walked its resolved range calling `abs_line` before its own
visibility filter, while its sibling `match_spans` bounded first — so the outlier was internal, not
against the references. 3/3 convergence on bounding at the read, and justerm had it in one projection
and not the other.

**The direction, and why justerm is only half-diverged.** justerm's *primary* pane is already
alacritty's rotate branch and does it better — user-authored points ride `reflow_pane` and come back
mapped. The *alt* pane is the one with no answer, and dropping there is xterm.js's answer applied to
xterm.js's own axis; the repro was a rows shrink.

⚠ **Not because the alt pane "cannot" track points — it can, and the first version of this section
said otherwise.** `reflow: false` disables the column re-split, not the tracking: the alt branch makes
its own `reflow_pane` call with tracked points and already uses the returned `extras` / `evicted` to
rotate and dispose alt markers. What makes dropping the right *trade* is shape, not capability — a
marker is one point with a binary fate, a selection is two ordered endpoints, and a shrink that
destroys the row under one but not the other has no "dispose" answer. This correction is recorded
because writing a false *"cannot"* as the justification for fixing a false *"cannot"* is #660's own
failure mode, one layer up.

**What the third row would cost, recorded so it is not re-proposed cheaply.** Ghostty's design removes
the question rather than answering it, and it is the better model — but it is a *storage* change:
justerm's anchors are `BufferPoint { line, col }` absolute indices (`docs/map/territory/selection.md`),
and four separate fixups (eviction, region rotation, below-margin shift, reflow) exist precisely
because the coordinate does not update itself. Adopting pins would retire all four and is a
whole-territory rewrite, not a fix.

**The trap this section exists to stop.** Reading only ghostty's `switchScreen` → `clearSelection`
gives *"a selection is cleared when you enter the alt screen, so there is never one there"* — which is
exactly the sentence justerm's own code carried, and exactly what #660 falsified. The clear happens at
the **swap**; it says nothing about a selection made while that screen is up.

## Scrolling a region by more than its own height (#661, verified 2026-07-31)

The question behind justerm's `ScrollOp::count` cap: **is a shift larger than the region equivalent
to a shift of exactly the region?** If it is, capping the reported count is lossless and the wire's
`i16` never overflows. Two of three references state the equivalence in their own code.

| Fact | Ref | Site |
|---|---|---|
| The scroll amount is **clamped to the region height** before anything else happens: `lines = cmp::min(lines, (self.scroll_region.end - self.scroll_region.start).0 as usize)`. The down-relative sibling clamps twice — region height *and* the distance from `origin` (`:745`) | alacritty | `alacritty_terminal/src/term/mod.rs:773` |
| ⚠ The equivalence stated outright, one layer down, as a fast path rather than a clamp: *"When rotating the entire region with fixed lines at the top, just reset everything"* — `if region.end - region.start <= positions && region.start != 0` resets every row and returns. **Read the guard, not just the comment**: it is `region.start != 0`, so a region anchored at the top does *not* take this path — it goes on to `increase_scroll_limit(positions)` because those rows enter scrollback. The equivalence holds for what lands on screen; it is not a statement that the operation is free | alacritty | `alacritty_terminal/src/grid/mod.rs:257` |
| The same clamp for IL, with the reason in the comment: *"We can only insert lines up to our remaining lines in the scroll region. So we take whichever is smaller"* — `const adjusted_count = @min(count, rem)`, where `rem` is the distance from the cursor to the region bottom | ghostty | `src/terminal/Terminal.zig:2704` (comment at `:2702-2703`) |
| **No clamp.** `insertLines` runs `while (param--)` and does one `splice` + one insert per iteration, so a `param` far past the region costs that many iterations and arrives at the same blanked region by brute force. The equivalence is true of the *result* and absent from the *code* | xterm.js | `src/common/InputHandler.ts:1357` |

**Direction this settles.** 2:1 for stating the bound explicitly, and the third agrees on the
outcome — convergence, so justerm's cap is not an arbitrary constant. What no reference supplies is
the *other* bound: none of them serializes a scroll delta, so `i16` and `MAX_SCROLL_COUNT` are this
repo's own question (theflow's tie-breaker table routes wire shape to this repo's precedent).

**Not searched.** Whether any reference bounds an *accumulated* delta. They do not accumulate at
all — each applies its scroll immediately — so the accumulator justerm's flow control needs
(`Term::record_scroll`) has no prior art here, and neither does the choice to cap it on read rather
than on write.
## Who bounds a pointer coordinate — the producer, not the engine (#667, verified 2026-07-31)

The sibling question to the one above, on the **write** side. #660 made justerm's engine total against
an out-of-grid *row*, and its own doc calls that a **backstop**; what this answers is whether a
consumer may lean on such a backstop or owes the bound itself. A pointer leaves the grid whenever a
drag does, so this is ordinary input rather than an edge case — and `Math.floor` on a negative offset
yields a *negative* cell, a different failure from overshooting the far edge.

**3/3, and unanimous in a way the read-time rows above are not**: all three bound *both* axes at
*both* ends in the consumer-side converter, before anything reaches their engine — including alacritty
and ghostty, whose engines clamp again at read time anyway.

| Fact | Reference | Site |
|---|---|---|
| **One converter serves mouse reporting, selection *and* linkification**, and it clamps both axes: `Math.min(Math.max(coords[0], 1), colCount + (isSelection ? 1 : 0))` / `…, rowCount)`. The `isSelection` flag widens only the *column* bound — a selection endpoint must be able to name the boundary after the last cell | xterm.js | `src/browser/input/Mouse.ts:46` |
| …and it refuses to answer at all when the cell has not been measured — `if (!hasValidCharSize) return undefined` | xterm.js | `src/browser/input/Mouse.ts:35` |
| `saturating_sub` for the padding (so the negative end cannot exist in the type), then `min(Column(col), size.last_column())` and `min(line, size.bottommost_line())` — *before* `viewport_to_point` | alacritty | `alacritty/src/event.rs:1812` |
| `@max(0, term.x)` / `@max(0, term.y)`, then `@min(col, grid.columns - 1)` / `@min(row, grid.rows - 1)`, under a comment stating the obligation outright: *"We need our grid to clamp"* | ghostty | `src/renderer/size.zig:136` |
| **Which cell *edge* the pointer is nearest is a separate computation, and alacritty needs an explicit extra rule for the overshoot** — its side comes from the **raw** x (`x.saturating_sub(padding) % cell_width`), so leaving the window needs its own arm — a second disjunct `x as f32 >= end_of_grid` yielding `Side::Right`, where `end_of_grid` subtracts the remainder strip | alacritty | `alacritty/src/input/mod.rs:534` |

**What justerm takes from this, and where it deliberately differs.** `justerm-web`'s three converters
disagreed: `input.ts` clamped (#266), `a11y-selection.ts` clamps its own DOM-tree endpoints, and the
selection converter did neither — this layer alone diverged, so the direction was *toward* the
references (#667). Two details did **not** transfer:

- **xterm's `colCount + 1` would be off by one here.** That extra column is how a converter with no
  `side` expresses "the boundary after the last cell"; justerm's `Side` already does, and
  `(cols - 1, Right)` **is** that boundary. Checked against xterm's own consumers, which convert to
  0-based immediately (`src/browser/services/SelectionService.ts:404-405`) and pass
  `isSelection: false` from the alt-click caller (`:729-735`) — the same path justerm's alt-click
  takes off its shared converter.
- **justerm gets alacritty's `end_of_grid` arm for free** by deriving the side from the *clamped*
  column: past the right edge the within-cell offset is `>= cellWidth`, hence `Right`; left of the
  origin it is negative, hence `Left`. Same two answers, one fewer rule — and the reason the clamp
  belongs *before* the side, not after.

**A cleared concern, with its condition.** The clamp is saturation in boundary space, so it changes no
selection that was previously correct: `px ∈ [-cellWidth/2, 0)` used to yield `(-1, Right)` and now
yields `(0, Left)` — the same boundary. This holds as long as the side stays a function of the
within-cell offset; a side derived from the raw pixel (alacritty's shape) would need the explicit
overshoot arm back.

**What none of them settles: `NaN` — adjudicated (#672, verified 2026-07-31).** The row above recorded
this as unadjudicated on the strength of one line (`hasValidCharSize` → `undefined`). Reading the rest
of that predicate's call graph changes the answer, so the retraction is the useful part: **xterm's
guard is not a converter guard, it is half of a repair loop**, and the half justerm can copy is the
half that does nothing on its own.

| Fact | Reference | Site |
|---|---|---|
| The validity predicate is `width > 0 && height > 0` — which **rejects `NaN` for free**, since `NaN > 0` is false. Positivity and non-NaN are one check, not two | xterm.js | `src/browser/services/CharSizeService.ts:18` |
| The *same predicate* triggers a **re-measure** when a resize is otherwise a no-op — so dropping the gesture is "defer until measurable", not "give up" | xterm.js | `src/browser/CoreBrowserTerminal.ts:1058` |
| …and again when a terminal that was hidden at `open()` becomes visible (comment: *"Terminal was hidden on open"*) — the second half of the loop | xterm.js | `src/browser/services/RenderService.ts:145` |
| A converter that meets a `NaN` coordinate **warns on the bare console and returns null** — not through xterm's own `LogService`, which defaults to `OFF` and would be silent exactly when a defect needs seeing | xterm.js | `src/browser/AccessibilityManager.ts:332` |
| ghostty's cell is an **integer type** at the conversion (`@floatFromInt(size.cell.width)`), so a zero/NaN cell is unrepresentable rather than guarded — total by typing, one rung below alacritty's total-by-saturating-cast | ghostty | `src/renderer/size.zig:140` |
| ⚠ **The loop above is about the CELL, and xterm arranges for it never to run on a hidden element.** `_validateAndSet` **retains the previous positive width/height** when a measurement reads zero, with the comment *"If values are 0 then the element is likely currently display:none, in which case we should retain the previous value"* — so `hasValidSize` stays true through a `display: none` and the `getCoords` guard cannot fire. The default strategy measures `'W'` on an `OffscreenCanvas`, which has no DOM box at all | xterm.js | `src/browser/services/CharSizeService.ts:64` |
| …and its origin is rect-derived and zeroes with the box (`const rect = element.getBoundingClientRect()`), while the drag listens on the **document** so it outlives the element — the same exposure `justerm-web` has | xterm.js | `src/browser/input/Mouse.ts` `getCoordsRelativeToElement`, `src/browser/services/SelectionService.ts:506` |
| **No measured origin term exists at all** — the position arrives window-relative from winit and nothing subtracts a measured box | alacritty | `alacritty/src/input/mod.rs:454` |
| Same: surface-relative from the toolkit (`pos: apprt.CursorPos`) | ghostty | `src/Surface.zig:4513` |
| An origin *is* subtracted, but it is a **configured constant** (border + scrollbar), so it cannot go to zero by absence | xterm | `ptyx.h:3856` |

**What justerm took, and the one thing it deliberately did not.** `CellGeometry` states its
preconditions and the converters *signal* a violation; they still answer. The refusal did not
transfer because its recovery half cannot: xterm owns the measurement (`CharSizeService`), while
`justerm-web` is handed the geometry per event by the consumer's `getGeometry()` (#578, ADR-0017), so
a dropped gesture would not come back on its own. ghostty's route — make it unrepresentable — is
closed for a recorded reason: the CSS cell is a **float on purpose** (ADR-0022, so `cols *
cssCellWidth()` scales back exactly). The dedupe on the warn is justerm's own, not xterm's: the reach
here is every event at pointer rate, where xterm's warn sites are selection-change.

**Scope correction — #819 (2026-08-26).** Everything in the paragraph above is about the **cell**, and
it stands there. It does **not** transfer to the **origin**, and reading it as if it did would be the
mistake the rows above now prevent:

- *"a dropped gesture would not come back on its own"* is **measured false** for a drag. `getGeometry`
  is pulled per event, so a pane shown again under a held button resumes on the correct cell — driven
  in a real browser on the `#819` probe.
- *"xterm's guard is half of a repair loop"* is true and **irrelevant here**, because on a hidden
  element xterm's guard never fires at all: it keeps the last positive cell on purpose. What xterm
  ships on this axis is a cached cell plus a zeroed origin — the exact shape justerm measured as
  defective.
- So on the origin axis **no reference arbitrates**: three have no measured origin term, and the
  fourth diverges into the defect. That is what put #819 on the *consumer-facing API shape → our own
  API's internal coherence* tie-breaker row rather than on a reference.

## Who guards a wheel accumulator (#675, verified 2026-07-31)

The odd one out among the rows above: **here the references do not arbitrate, because they share the
divergence.** justerm's `WheelScroller` mirrors xterm's wheel handling by design (its module doc says
so), and it inherited the shape rather than drifting from it — so theflow's Step 5 direction rule puts
this on *our own grounds*, not on parity.

| Fact | Reference | Site |
|---|---|---|
| The **byte-identical accumulator, with no non-finite guard** — `this._wheelPartialScroll += amount` then `%= 1`, where `Infinity % 1` is `NaN` and nothing clears it but `reset()` | xterm.js | `src/browser/services/MouseService.ts:478` |
| Its **only** bail is on an *absent* measurement — `if (cellHeight === undefined || dpr === undefined) return 0` — which does not catch a cell of `0`, the state a hidden or unlaid-out terminal is actually in | xterm.js | `src/browser/services/MouseService.ts:463` |
| alacritty and ghostty have **no equivalent**: neither turns a wheel delta into lines through a consumer-supplied cell, so there is nothing to compare | — | — |

**The ground justerm decided on, since the reference could not supply one.** xterm is an application
that owns and can re-measure its cell (see the `hasValidSize` rows above); `justerm-web` is a
published library whose failure here was silent *and* unrecoverable — measured, an unmeasured cell
killed the wheel until an alt-screen switch, and the widget then handed the consumer a non-finite
offset that came back on the next frame. So every producer on that path was made total instead:
`consumeWheelEvent`, `wheelScrollTarget`, `routeWheel`, `dragScrollSpeed`.

**A detail worth keeping, because a plausible fix hides it.** Guarding the *output* is not equivalent
to guarding the *input* where a clamp sits between them: `Math.max(0, Math.min(100, 10 - Infinity))`
is `0` — finite, and a silent jump to the live edge. Only `NaN` survives to the output, so an
output-side check fixes half the cases while reading as if it fixed all of them.

## Where a drag-scroll amount gets its viewport height (#680, verified 2026-07-31)

`dragScrollSpeed` is a port of xterm's `_getMouseEventScrollAmount`, down to the sign term, so the
interesting difference is not the algorithm — it is the **input**.

| Fact | Reference | Site |
|---|---|---|
| The height is read as **one number with one meaning**: `this._renderService.dimensions.css.canvas.height`, the measured canvas box | xterm.js | `src/browser/services/SelectionService.ts:419` |
| The ramp is otherwise identical to justerm's, sign term included — `(offset / Math.abs(offset)) + Math.round(offset * (DRAG_SCROLL_MAX_SPEED - 1))`, and equally unguarded | xterm.js | `src/browser/services/SelectionService.ts:429` |

**The ambiguity was justerm's own, and naming it is what solved #680.** `SelectionController` does not
have a canvas height; it reconstructs one as `getRows() * geom.cellHeight`, and a product loses
*which factor* was zero. That is why guarding the product would have re-decided #667 (whose reading
of a 0-row viewport is load-bearing for `tick()`'s edge-row floor) rather than fixing a defect. The
factors carry different contracts — `cellHeight` has a documented precondition since #672, `getRows()`
does not — so the guard belongs at the caller that still holds them separately, and the published
`dragScrollSpeed` keeps both its signature and xterm's semantics.

## What the engine does with a column it was handed anyway (#671, verified 2026-07-31)

The read-side half of the section above. Once a selection anchor **is** out of range — because the
producer did not bound it, or because a caller reached the engine API directly — the question is
whether the engine can be made to *observe* the difference. All three references answer "no", by
three different mechanisms, which is why the property converges while the code does not.

| Fact | Reference | Site |
|---|---|---|
| **Clamps both endpoints, and does it before any side arithmetic** — `Selection::to_range` opens with *"Clamp selection to within grid boundaries"* and runs `grid_clamp` on `start.point` **and** `end.point`, then dispatches to the per-type range builders | alacritty | `alacritty_terminal/src/selection.rs:283` |
| …so its own `+ 1` cannot overflow, and it pairs that `+ 1` with an explicit rule for the boundary it lands on: *"Wrap to next line when selection starts to the right of last column"* → `column = 0; line += 1` | alacritty | `alacritty_terminal/src/selection.rs:351` |
| **Does not clamp — the reader is simply total.** `translateToString` runs `while (startCol < endCol)`, so a `startCol` past the end yields `''` rather than an index error, and the only producer is the already-clamped `_getMouseBufferCoords` | xterm.js | `src/common/buffer/BufferLine.ts:559` |
| **Does not clamp on the general path either** — `topLeft`/`bottomRight` apply `@min(…, cols - 1)` only in the `mirrored_*` (rectangle) arms; `forward`/`reverse` return the pins unchanged, because a pin cannot hold an out-of-range position in the first place | ghostty | `src/terminal/Selection.zig:152` |

**The property converges; the mechanism does not.** Clamp (alacritty), a guaranteed producer plus a
total reader (xterm.js), or a type that cannot express it (ghostty). justerm was the only one where
the value was both *representable* and *observable* — and the observable difference was not uniform,
which is what made it hard to see:

| justerm before #671, 4-column grid `abcd` | result |
|---|---|
| `begin(0, 80, Left)` | the **anchor row disappears** from `selection_range` *and* the copy |
| `extend(2, 80, Left)` | selects **one cell more** than asked |
| either endpoint with `Side::Right` | unchanged — correct by accident |
| `usize::MAX` with `Side::Right` | **panics** on the `+ 1` |

**`Side`, not the endpoint, is the axis.** `Side::Right` adds one and the readers' `to.min(len)` /
`right_excl > left` clip the result to the same place the clamp would; `Side::Left` has no `+ 1` to
clip, so the raw column survives into a `left` that **no reader bounds**. Both an earlier lens report
("it yields an empty selection") and the first correction to it ("it already resolves correctly")
were single points in that 2×2 read as the whole table.

**What justerm kept rather than copied.** The bound is at the **write** site (`viewport_to_abs`,
beside #660's row clamp) rather than alacritty's read site: one function answers "what does a
viewport coordinate mean", and the alternative is five clamps for one rule — `resolve` has five
`Side`-dependent `+ 1`s. The consequence is that justerm has no equivalent of alacritty's
wrap-to-next-line arm and does not need one: an in-range `Side::Right` on the last column resolves to
`from == cols`, and the reader's `right_excl > left` drops that row — the same outcome alacritty
reaches by moving the start to `(line + 1, 0)`. Pinned as unchanged in
`tests/selection_column_bound.rs`.

**A cleared concern, with its condition — and the condition is stronger than the obvious one.** The
bound is the **grid width**, not the line length, because `SelectionType::Line` already resolves `to`
as `grid.cols()`. The completeness pass sharpened why that is safe: every row *is* exactly
`grid.cols()` wide, because both row producers resize it there (`grid.rs`, `set_screen` and `reflow`),
so the readers' `to.min(len)` is identically `to.min(cols)`. The clearance therefore rests on
`len == cols` being an invariant of those two producers, not merely on no type happening to resolve
against `abs_line(..).len()` today.

## Search: who may hand the engine a match, and what happens to its columns (#678, verified 2026-07-31)

The first entry for this territory — `docs/map/territory/search.md` recorded `## Reference behaviour`
as **None**. Two questions had to be separated, and separating them is what makes the answer legible:
who may *supply* a match, and what the projection does with a column that is out of range.

> **This section was rewritten once, on 2026-07-31, and the first version was wrong in the direction
> that flatters the change.** It claimed no reference arbitrates and that xterm's loop stops
> terminating. Both were false, both were pinned `file:line` claims, and the correction is below —
> the class of error this file exists to prevent, produced by reading a loop instead of running it.

**Nobody else lets a consumer hand the engine a match.** justerm's intake is not an oversight —
ADR-0017 puts query policy in the consumer, so the consumer owning `Vec<Match>` and handing it back
**is** the frame-mode design.

| Fact | Reference | Site |
|---|---|---|
| The search addon's public surface takes a **term**, not a match — `findNext(term, searchOptions)`; the addon runs the search itself | xterm.js | `addons/addon-search/src/SearchAddon.ts:101` |
| `Match` is a `RangeInclusive<Point>` produced by `regex_search_left` / `regex_search_right` and consumed in the *app* layer; `alacritty_terminal` exposes no public method taking one | alacritty | `alacritty_terminal/src/term/search.rs:21` |
| Matches come from the `PageList` search iterator | ghostty | `src/terminal/search.zig` |

**But the *guard* question is arbitrated, and the answers split 1–1.** The comparable surface is
xterm's `registerDecoration({x, width})` — a public intake for a consumer-supplied span with the same
column semantics:

| Fact | Reference | Site |
|---|---|---|
| **Hide.** An explicit, commented arm for exactly this input: `const x = decoration.options.x ?? 0; if (x && x > cols) { /* exceeded the container width, so hide */ element.style.display = 'none'; }` | xterm.js | `src/browser/decorations/BufferDecorationRenderer.ts:83` |
| …and its public intake validates *shape* but not *range* — `_verifyPositiveIntegers` throws on a negative, fractional or `NaN` x, and imposes no upper bound. Reject malformed, accept out-of-range, be total downstream | xterm.js | `src/browser/public/Terminal.ts:173` |
| …the colour path reaches the same outcome by inverting the loop: `forEachDecorationAtCell` walks **real cells** and tests membership, so an out-of-range `x` matches none | xterm.js | `src/common/services/DecorationService.ts:100` |
| **Clamp.** `Point::grid_clamp` clamps the column unconditionally — `self.column = min(self.column, last_column)` — and `Selection::to_range` runs it on *both* endpoints before any per-type arithmetic. `to_range` returns `None` on the **line** axis only; an off-grid column is clamped, never dropped | alacritty | `alacritty_terminal/src/index.rs:97` |
| **Cannot represent it** — a pin holds no out-of-range position | ghostty | `src/terminal/Selection.zig:152` |

**justerm's pre-#678 behaviour was neither answer**, and that is the argument the split does not
weaken: it dropped the match's *start row* and painted the continuation rows, because `left` was
unbounded while `right` was not. xterm hides the whole decoration; alacritty clamps the whole range;
justerm did half of one. The choice was therefore between two coherent answers and a third nobody
holds — and it went to **alacritty's**, on theflow's tie-breaker for API shape (this repo's own
precedent: #660 and #671 both bound a coordinate arriving from outside).

**The cost of that choice, recorded rather than discovered later.** Clamping paints wrong content
*visibly* where hiding paints nothing; on a grid ending in a wide glyph the clamped column can be the
pair's **trailing spacer**, so the span covers half a glyph — a bisection the dropped row could not
produce (the #454 class, with `match_spans` as a producer that issue does not name). Hiding would
have avoided that and lost the "visible so the consumer can notice" property instead.

**The projection mechanism converges exactly, and there the reference is unguarded.** xterm splits a
wrapped match into per-row ranges with justerm's shape, continuation rows starting at column 0:

| Fact | Reference | Site |
|---|---|---|
| The per-row split — the same model as `Term::match_spans`'s `left = if line == start_line { start_col } else { 0 }` | xterm.js | `addons/addon-search/src/DecorationManager.ts:123` |
| …with **no column bound**, and the degradation traced by running it rather than reading it: for `cols = 4, col = 80, size = 2` it emits `[[80, -76], [0, 4] × 19, [0, 2]]` — one negative-width range (which the hide arm above catches) followed by **nineteen spurious full-width highlight rows** (which it does not, their `x` being 0). It terminates; `currentCol = 0` is reset inside the loop | xterm.js | same |

So on its *own* search path xterm is worse than either deliberate answer — an accident nobody guarded,
because the producer is always its own engine. That is a negative result worth pinning: the model
converged independently, and the missing guard is evidence nobody has had to answer this, not that
the answer is "leave it".

**Where justerm bounds it, and why not where #671 did.** `match_spans`, the read site — `right` is
already bounded in the same expression, so this restores a symmetry rather than adding a rule, and
the write side is three storing intakes, one taking a whole `Vec`. #671 is **not** the same shape and
the first version of this section said it was: it did not touch `selection_range`, whose `left` is
still unbounded (`term/selection.rs`). What #671 did was clamp selection's *producer*
(`viewport_to_abs`), making the read-site asymmetry unreachable. Search has no producer to clamp —
the coordinate **is** the consumer's — which is exactly why the same asymmetry stayed live here and
why the bound has to sit at the read.

**A cleared concern, with its condition.** The bound is `abs_line(line).len() - 1`, the **grid** width
rather than the printed text, because both row producers resize every row to `grid.cols()`. It holds
while `len == cols` is an invariant of those producers.

## Search: what carries the current match across a re-search (#437/#441, verified 2026-08-03)

The second entry for this territory, and it is about *memory* rather than geometry: the set is
re-derived on every re-search, so what does the emphasis hold onto in between? justerm-web held the
**ordinal** (`min(index, total-1)` on output; `0` while typing). No reference does.

**All three keep the same text occurrence — 3–0 — and they do it three different ways.** The
mechanisms disagree; the invariant does not, which is what makes the tally usable.

| Fact | Reference | Site |
|---|---|---|
| The anchor **is the selection**: every re-find starts from `getSelectionPosition()`, because the addon *selects* each result it lands on. The emphasis has no separate memory — the terminal's selection is it | xterm.js | `addons/addon-search/src/SearchEngine.ts:103` |
| …and `_selectResult` is what stores it — `this._terminal.select(result.col, result.row, result.size)` | xterm.js | `addons/addon-search/src/SearchAddon.ts:238` |
| The **"n of m" index is derived from the position, never carried**: `findResultIndex` scans the result list for a row/col/size equal to the selected decoration's match, and `fireResultsChanged` reports `-1` when it is not in the (capped) list | xterm.js | `addons/addon-search/src/SearchResultTracker.ts:85` |
| The anchor stores a **`Point`**: `search_state.origin`, and `goto_match` searches from it (grid-clamped) every time | alacritty | `alacritty/src/event.rs:1565` |
| …and `advance_search_origin` re-parks the origin at the focused match *after* navigating, with the reason in the comment: *"after modifications to the regex the search is started without moving the focused match around"* | alacritty | `alacritty/src/event.rs:1152` |
| The anchor is an index **plus a tracked pin** — `selected: ?SelectedMatch` holds `{ idx, highlight }`, and `select()`'s own doc says it needs write access *"since we utilize tracked pins to ensure our selection sticks with contents changing"* | ghostty | `src/terminal/search/screen.zig:57`, `:797` |
| …so the index is *maintained*, not trusted: prune, append and resize each shift `m.idx` with the stated intent *"Moving the idx should not change our targeted result"* | ghostty | `src/terminal/search/screen.zig:633` |

**On output, xterm re-finds at the anchor and does not scroll.** `onWriteParsed` / `onResize` →
200 ms debounce → `findPrevious(term, { …, incremental: true }, { noScroll: true })`, and because
`_updateMatches` clears the cached term first, `findPreviousWithSelection` takes its *"Try to expand
selection to right first"* arm — a **forward** find starting at the old selection's exact start, which
re-lands on the same occurrence when the query is unchanged.

| Fact | Reference | Site |
|---|---|---|
| The debounced, non-scrolling incremental re-find | xterm.js | `addons/addon-search/src/SearchAddon.ts:76` |
| The expand-at-the-anchor arm it takes | xterm.js | `addons/addon-search/src/SearchEngine.ts:191` |
| The active decoration is built from the **found result**, so it is a position and lives outside the highlight cap (this is the #436 fact, recorded here for the anchor's sake) | xterm.js | `addons/addon-search/src/SearchAddon.ts:240` |

**While TYPING the references split 2–1, and the odd one out is not "jump to the first match".**

| Fact | Reference | Site |
|---|---|---|
| **Anchored.** With the term changed, `findNextWithSelection` starts at the selection's *start* (unchanged term → its *end*, which is what makes `next()` advance) — so extending a query re-finds at the same place | xterm.js | `addons/addon-search/src/SearchEngine.ts:110` |
| **Anchored.** Each keystroke runs `goto_match(MAX_SEARCH_WHILE_TYPING)` from the stored origin | alacritty | `alacritty/src/event.rs:1523` |
| **Designates nothing.** `changeNeedle` tears the search down and emits `selected_match = null`; ghostty has no current match while typing, only highlights, and its label reads 0 until the user navigates | ghostty | `src/terminal/search/Thread.zig:312`, `:334` |

**What is NOT arbitrated: where a *first* search lands.** xterm starts at `(0, 0)` with no selection
(buffer top); alacritty's origin comes from the vi cursor / display offset (viewport). justerm lands on
match 0, which is xterm's answer, and this entry does not settle whether that is the right one — it was
not the question either issue asked.

**The fallback direction, when the anchored occurrence is gone, is NOT arbitrated — and xterm
disagrees with itself across its own two paths.** Recorded loudly because the first draft of this
section claimed convergence here and was wrong, in the direction that flattered the change.

| Fact | Reference | Site |
|---|---|---|
| The **typing** path falls forward and wraps downward: `findNextWithSelection` walks the rows below the anchor, then re-enters from row 0 | xterm.js | `addons/addon-search/src/SearchEngine.ts:139` |
| The **output** path falls *backward*: `findPreviousWithSelection` tries forward within the anchor's own line only, and on failure runs a reverse search walking rows **upward**, then wraps from the bottom | xterm.js | `addons/addon-search/src/SearchEngine.ts:204` |
| Neither — alacritty searches from a **fixed** origin in the user's last search direction, so its fallback is not relative to the previous match at all | alacritty | `alacritty/src/event.rs:1566` |
| Neither — ghostty requires exact tracked-pin equality (`start.eql` **and** `end.eql`) and otherwise drops the selection and re-selects the first match: *"No match, just go back to the first match."* | ghostty | `src/terminal/search/screen.zig:759` |

justerm-web's `SearchPort.anchoredIndex` takes "first occurrence at or after the anchor, wrapping to
the top" on **both** paths — xterm's `findNext` rule applied uniformly. That is a choice, not a
convergence; what it claims is self-consistency, which the reference does not have.

**One consequence of anchoring at designation time, and the references split on it: the ratchet.**
xterm selects each incremental result (`SearchAddon.ts:238`) and that selection is the next anchor, so
its emphasis walks **forward** while typing and backspacing does not walk it back. alacritty's does
come back, because `search_state.origin` is written only at search start and by next/prev — never by
`update_search`. 2–1 for the ratchet, and a backend whose anchor is a by-product of designating is on
the majority side by construction.

| Fact | Reference | Site |
|---|---|---|
| `origin` at search start — the **viewport edge**, picked by search direction (`Right` → viewport top, `Left` → bottom); the vi-mode branch above it uses the vi cursor instead | alacritty | `alacritty/src/event.rs:970` |
| `origin` re-parked at the focused match, by next/prev only | alacritty | `alacritty/src/event.rs:1143` |
| …and `update_search`, the typing path, does not touch it — it only rebuilds the DFAs and re-runs `goto_match` | alacritty | `alacritty/src/event.rs:1523` |

## Trimming a line's end (#685, verified 2026-08-03)

Every row grepped at the pinned SHAs that day. The question: when a reference turns cells into
text, **what does it strip from the end, and by which predicate?** justerm used
`str::trim_end()` — the Unicode `White_Space` *property* — and no reference does that on its
primary path.

Two things this section exists to keep straight, because a count gets both wrong:

- **The predicate and the *shape* are different answers.** Two references never trim a string at
  all: they bound the row's written extent and let the selection's own end column clip against it.
  Only ghostty accumulates and then decides, which is justerm's shape — so ghostty is the one whose
  mechanism transfers.
- **Two of the three contradict themselves**, on a secondary path each. A row that said only
  "alacritty strips `' '`" would be true of the arm justerm does *not* mirror and false of the arm
  it does.

| Fact | Reference | Site |
|---|---|---|
| Primary path is a **row-extent bound**, not a trim: `line_to_string` clips at `min(line_length(), cols.end + 1)` | alacritty | `alacritty_terminal/src/term/mod.rs:572` |
| …and `line_length()` scans back while `cell.c != ' '` (plus a non-empty `zerowidth`), short-circuiting to the full row on `WRAPLINE` — so a written **NBSP is kept**, a trailing ASCII space is not distinguishable from padding | alacritty | `alacritty_terminal/src/term/cell.rs:271` |
| ⚠ **The `SelectionType::Block` arm additionally applies Rust `.trim_end()`** on top of the already-bounded string, so alacritty drops a trailing NBSP on a *block* copy and keeps it on every other kind. These are the **only two** `trim_end` / `is_whitespace` uses in `alacritty_terminal/src/` — grepping the crate for a trim finds this arm first and the primary path not at all | alacritty | `alacritty_terminal/src/term/mod.rs:540`, `:544` |
| **Deferred-blank accumulator** — a cell with no text, or `codepoint() == ' '`, is counted into `blank_cells` and emitted only if a non-blank follows. justerm's shape, and the only reference with **one** rule | ghostty | `src/terminal/formatter.zig:1145` |
| …stated in the option's own doc: *"Whitespace is currently only space characters (0x20)."* `Screen.selectionString` routes through this formatter, so selection copy uses the same predicate | ghostty | `src/terminal/formatter.zig:95` |
| ⚠ `selection_codepoints.default_line_whitespace = { 0, ' ', '\t' }` is **not** the extraction predicate — it belongs to `selectLine`, which moves the selection's *pins* past whitespace. Citing it for the trim is the mistake #685's own first correction made | ghostty | `src/terminal/selection_codepoints.zig:31` |
| **Column clip to written extent**: `translateToString` does `endCol = min(endCol, getTrimmedLength())` | xterm.js | `src/common/buffer/BufferLine.ts:553` |
| …and `getTrimmedLength` scans back for `HAS_CONTENT_MASK`, i.e. any *written* cell — so xterm keeps a written trailing **ASCII space** too, which the other two cannot | xterm.js | `src/common/buffer/BufferLine.ts:484` |
| ⚠ **The string-cache-hit path returns `value.trimEnd()` instead** — JS `trimEnd()` is Unicode `White_Space`, disagreeing with the clip above on a trailing NBSP. Same line, two answers, decided by cache state. Reachable only on a *canonical* request (no `startCol`/`endCol`/`outColumns`), which selection and a11y never make; search / web-links / the character joiner do | xterm.js | `src/common/buffer/BufferLine.ts:544` |

**What justerm did with this (#685).** Adopted ghostty's predicate at all four of its own trims,
because ghostty is both the self-consistent reference and the one sharing justerm's
accumulate-then-decide shape. The Block arm — a verbatim copy of alacritty's contradictory one — was
settled by **intra-surface consistency** rather than by the references, since alacritty's two arms
disagree: a Linear and a Block selection over the same cells must return the same text.

**What it did not buy.** xterm's extra case — a *written* trailing ASCII space — needs a bit
distinguishing a written `' '` from a blank, and justerm's blank cell packs `' '`
(`justerm-core/src/cell.rs`, `Cell::default`). Recorded as the open axis, not fixed: the references
split 2–1 there, where they are 3–0 on the property.

## Search: dropping the paint vs ending the session (#687, verified 2026-08-03)

The third entry for this territory, and the one that decides how many **clear verbs** a search
surface has. #687 was filed with *"no recorded tie-breaker for this layer"* — true of the question
it asked (what to do about an *invalid regex*, a state only justerm has) and false of the one that
decides the design (may a query change drop the paint without ending the session?). The concept
layer is silent; the mechanism layer is **3–0**. Reading only the first is what the
concept≠mechanism rule exists to prevent.

**Nobody ends the search session because the query changed, and two of the three keep the anchor
through a query the engine cannot run.**

| Fact | Reference | Site |
|---|---|---|
| A **malformed** pattern leaves the anchor entirely alone: in `update_search` a non-empty regex takes the `else` arm, `dfas = RegexSearch::new(regex).ok()` is `None` on failure, and the `search_reset_state` call sits in the empty-regex branch *above* it, which this arm never reaches | alacritty | `alacritty/src/event.rs:1520` |
| …and the `goto_match` that arm then calls returns before its body when `dfas` is `None`, so nothing downstream runs — `origin` is written only by `start_search` and `advance_search_origin` | alacritty | `alacritty/src/event.rs:1557` |
| xterm's paint drop is **anchor-neutral unconditionally**: `clearDecorations` clears the selected decoration, the highlight decorations and the result list, and touches the selection — which *is* its anchor — in neither variant | xterm.js | `addons/addon-search/src/SearchAddon.ts:81` |
| The anchor dies by a **separate call on a different path**, `this._terminal.clearSelection()`, reached on an emptied term or a find that returned nothing | xterm.js | `addons/addon-search/src/SearchAddon.ts:166` |
| `changeNeedle` tears the whole search down and emits `total_matches = 0` + `selected_match = null` — ghostty has nothing to spare here, because it designates nothing while typing (see the #437/#441 entry) | ghostty | `src/terminal/search/Thread.zig:334` |

**Read `retainCachedSearchTerm` carefully — it is not the anchor flag, and reading it as one is an
error this file made and had to retract.** The flag gates exactly one statement,
`this._state.clearCachedTerm()`. `_cachedSearchTerm` is a *re-highlight cache key* and a
same-term/different-term bit that picks `prevSelectedPos.end` over `.start` on the next find; it is
not what carries the emphasis. So xterm supports #687's split **more** strongly than the flag
suggests: its paint drop never had the power to end a session in the first place.

| Fact | Reference | Site |
|---|---|---|
| The flag's entire body — one call, to `clearCachedTerm` | xterm.js | `addons/addon-search/src/SearchAddon.ts:85` |
| What the cached term is *for*: `shouldUpdateHighlighting`, i.e. whether to recompute highlights at all | xterm.js | `addons/addon-search/src/SearchState.ts:83` |
| The anchor itself — the selection the addon puts on each result (already recorded under #437/#441) | xterm.js | `addons/addon-search/src/SearchEngine.ts:103` |
| A separate public verb drops **only** the designation, and its doc names the occasion: *"intended to be called on the search textarea's `blur` event"* | xterm.js | `addons/addon-search/src/SearchAddon.ts:90` |

**xterm's "invalid" is not justerm's, which is the row the issue's premise turned on.**
`isValidSearchTerm` is `!!(term && term.length > 0)` — *empty*, nothing more. The one place xterm
ends a session on a rejected term is the case where every implementation would, and it says nothing
about a non-empty pattern the engine refuses. xterm validates no dialect at all: the addon takes a
JS `RegExp` and the host (VS Code) validates upstream. **alacritty is the only reference that has
justerm's situation**, and it keeps the anchor through it — which is the row that decides #687.

| Fact | Reference | Site |
|---|---|---|
| `isValidSearchTerm(term)` = `!!(term && term.length > 0)` | xterm.js | `addons/addon-search/src/SearchState.ts:49` |

**One divergence, deliberate: the designation.** alacritty's invalid branch does not call
`search_reset_state`, so its `focused_match` marker survives a malformed pattern; justerm's
`clearHighlights` drops the designation with the highlights. That is not drift — #316 D2 requires
the screen to stop showing a query the box has rejected, and core's own hand-over already voids the
designation whenever the set is replaced (`set_search_highlights`). Core, the demo backend and the
port agree, so the family is consistent against the reference on this one axis and holds.

**Where justerm's shape legitimately differs, and why it is not a divergence either.** xterm keeps
its retain/end distinction *private* — the published `.d.ts` exposes only `clearDecorations(): void`
— because the object holding the positions is in the same address space. justerm's is across a
process boundary, so the same distinction has to be a **port method**
(`SearchPort.clearHighlights`). The seam location derives the shape; the rule underneath is shared.

| Fact | Reference | Site |
|---|---|---|
| The published API has `clearDecorations(): void`, no parameter | xterm.js | `addons/addon-search/typings/addon-search.d.ts:145` |

**And the sibling corpus agrees, which is what makes this a direction rather than a difference.**
`justerm-core` already draws the same line on its own side of the wire: highlights are
*query-derived* state and are **invalidated**, while user-authored state is **re-anchored** — the
rule in `justerm-core/src/term/search.rs`'s module doc, recorded in
`docs/map/territory/search.md` § Design model. The anchor is where the user navigated to; the
highlight set is a function of the query. `clear()` conflating them was the defect.

## How a comparable project structures a Playwright suite's page setup (#733, verified 2026-08-06)

The first entry for a **test-harness** question, and the reason it is worth a section is that
the harness is where the family's browser proofs live — a suite that races its own boot reports
machine speed as a defect, which is what #653 cost three CI runs. Read it with rule 5 doubled:
the tie-breaker table in `theflow.md` has **no row for this layer**, so nothing here can make a
justerm shape wrong. It converged with ours independently, which is the only claim it supports.

This section needed `test/` in the xterm.js sparse checkout; the clone recipe in `theflow.md`
§ "Step 1" now sets it. The pin is unchanged.

| Fact | Reference | Site |
|---|---|---|
| The console listener is attached **before** the navigation it is meant to observe, two lines above it, in the shared context builder | xterm.js | `test/playwright/TestUtils.ts:27` |
| That `goto` is the **only** one in the whole Playwright suite — `rg '\.goto\(' test/playwright addons` returns exactly one hit, and it is reached from `beforeAll`, never from a test body | xterm.js | `test/playwright/TestUtils.ts:29` |
| Per-test isolation is done **in-page**, by resetting the object under test rather than by navigating again | xterm.js | `test/playwright/TestUtils.ts:252` |
| The boot gate waits on a node the object under test **emits** (`.xterm-rows`), not on a sibling widget that merely mounts nearby | xterm.js | `test/playwright/TestUtils.ts:515` |
| ⚠ **CORRECTED 2026-08-10 (#731).** `pollFor` waits for browser-side state by **re-evaluating in the page** on a 10ms `setTimeout` recursion (`:566`, default `maxDuration` 2000 at `:552`), not by awaiting one long-lived promise. That is true **of `pollFor`**; the row said it as a project-wide absence and that is false at the same SHA — the addon suites await in-page promises directly (`addons/addon-image/test/ImageAddon.test.ts:223`, `KittyGraphics.test.ts:203`). xterm.js does **both**, and the absence must be scoped to this helper | xterm.js | `test/playwright/TestUtils.ts:529` |
| **`writeSync` is justerm's park-and-harvest shape verbatim** — one `evaluate` sets `window.ready = false` and kicks off `term.write(data, () => window.ready = true)` **returning nothing**, then `pollFor(page, 'window.ready', true)` harvests. The closest thing to a reference for #731's repair, and a stronger citation than the `pollFor` row above because it is the *whole* shape rather than the polling half | xterm.js | `test/playwright/TestUtils.ts:599` |
| It has **no** custom fixtures at all: `rg 'test\.extend|test\.use' test addons` returns zero, so it is silent on how to make a listener precede a hook | xterm.js | — (absence, grepped 2026-08-06) |
| **The whole first boot happens inside `beforeAll`, off the `browser` fixture** — `test.beforeAll(async ({ browser }) => { ctx = await createTestContext(browser); await openTerminal(ctx); })`, where `openTerminal` (`:449`) is what waits for the emitted node. So the navigation, the bundle fetch and the terminal's first mount are all charged to the **hook**, and no per-test assertion clock ever sees them | xterm.js | `test/playwright/Terminal.test.ts:11` |
| The budget that hook runs under is the **test timeout, `10000`** — twice the `expect` timeout it leaves at playwright's default 5000. The same asymmetry justerm's #735 warm-up trades on, reached from a different design (page reuse) rather than from a cold-cache measurement | xterm.js | `test/playwright/playwright.config.ts:5` |

**What this does and does not settle for justerm.**

- **Converges** with #733's one-navigation-per-test rule, and independently — justerm reached it
  from #653's measured flake, xterm.js from a shared-page design. Convergence is the
  non-arbitrariness signal, not an authority.
- **Cannot arbitrate** the fixture question at all (zero `test.extend` in the tree). justerm's
  `{ auto: true }` `consoleLines` is a first-principles answer to a Playwright ordering fact
  measured locally: a fixture the *test* declares is set up **after** `beforeEach`.
- **Is a design proposal, not a defect, where it differs.** xterm.js shares one page per test
  *file* and resets in-page; justerm takes a fresh context per test. Reading that as a finding
  fails the reference-free restatement test — nothing in justerm's own record asks for page reuse.
- **The poll row was recorded here before it was acted on, and #731 is where it landed — after being
  corrected.** justerm-web's e2e awaited an in-page probe's promise across the CDP boundary; when
  that promise's handler was lost on CI (2026-08-05, run `30979831545`), playwright rewrote the
  protocol error into "Execution context was destroyed … because of a navigation" and the hunt went
  to page lifecycle. Two things the application taught, both now in the rows above:
  - the original row **over-claimed** — it read as *"xterm.js never awaits an in-page promise"*, and
    its addon suites do. Only `pollFor` holds the rule;
  - the *better* citation was one nobody had opened: `writeSync` is the whole park-and-harvest
    shape, not just its polling half. A section can be right about every row it has and still be
    missing the one that matters, which is what "start from the map, then go to source" is for.

  The repair is **not** cited to either row. This repo's own `justerm-renderer/e2e/proofs.spec.mjs`
  already waited on `__done` and then read `__proof` while its sibling `screen-composited.spec.mjs`
  did not, so the finding survives the reference-free restatement (*one of our two harnesses drifted
  from the other*). xterm.js converging is the non-arbitrariness signal and nothing more.
  **And the rule that came out is not the obvious one:** "never await across CDP" is false —
  `waitForFunction` is itself `awaitPromise: true` (traced, `DEBUG=pw:protocol`, 2026-08-10), as is
  every `expect(locator).toBeVisible()`. What differs is whether playwright retains the awaited
  promise's owner by `objectId`. A rule written from the reference's *shape* alone would have
  forbidden the harness.
- **Points at one hazard class worth owning**, which is the row that earns its keep: their boot
  gate is a node the subject emits, ours is the demo's control bar — a **proxy** that mounts ~350
  lines before the `window.__*Probe` assignments every spec reaches for. Ours is sound only
  because the `justerm-wasm-decode` import between them resolves on the microtask queue. That
  finding stands with xterm deleted from the sentence, which is why it is recorded as a validity
  condition in `e2e/demo.spec.ts`'s `beforeEach` rather than as a parity fix.
- **Converges on the budget, and #735 is where that landed — but the convergence is on the
  destination, not the reason.** Both suites charge a browser process's first boot to `beforeAll`
  rather than to a per-test assertion clock. xterm.js arrives there because it reuses one page per
  file, so `beforeAll` is simply *where the page is*; justerm arrives there because a measured cold
  boot had to go somewhere with headroom, and its warm-up context is **discarded** immediately. The
  measurement, stated so it cannot be compressed into something stronger: the suite's first test ran
  10084ms (**passed** — a test's duration is not the `expect` budget, since `page.goto` has its own)
  and 15277ms (**failed**, at `beforeEach`'s 5000ms `expect` budget), under 112 spinners on a box
  also carrying other real work — nominally the same 4x oversubscription as #735's own 4024ms sweep,
  on a noisier host, which is why the numbers do not line up with that table.
  The rows above are therefore worth exactly one thing: a shape two projects reached independently
  is not arbitrary. They license nothing about *why* — in particular, xterm.js's config says nothing
  about cold caches, and this repo's own measurement is the only evidence for that half.

### Whether any reference's browser suite puts two terminals on one page (#776, verified 2026-08-24)

Asked for #776 (Epic #287 S8), which is the consumer-side browser proof that one canvas serves N
terminals. The question is narrower than #774's and does not follow from it: #774 established that no
reference **shares a context** between terminals, and this asks whether any of them nevertheless
constructs two terminals **on one page** in its own suite — because the harness shape would transfer
even where the architecture does not.

| Fact | Reference | Site |
|---|---|---|
| One case exists, and it is incidental: a **throwaway** second terminal in a *font-loading* test, constructed inside a `page.evaluate` string, opened into the **first terminal's own element**, and disposed at the end of the same test | xterm.js | `addons/addon-web-fonts/test/WebFontsAddon.test.ts:100` |
| …with `open(term.element)` — so it is not even placed anywhere of its own; the arrangement is "a second widget inside the first", not two panes | xterm.js | `addons/addon-web-fonts/test/WebFontsAddon.test.ts:101` |
| Nowhere else **in a browser**. `rg 'new Terminal\(' test addons` over the pinned tree returns 12 hits across 7 files. Three of those files are browser (playwright) suites — `test/playwright/Terminal.test.ts`, `addon-fit`, and this one — and only this one builds a second instance. The other four are node unit tests under `addons/*/src/`, and two of them *do* build a `terminal2`, but under **jsdom** (`addon-serialize/src/SerializeAddon.test.ts:6`): no canvas, no context, nothing placed | xterm.js | (grepped 2026-08-24) |
| No comparand at all: neither runs a browser suite, and neither has a two-terminals-one-surface arrangement to run one against | alacritty · ghostty | (absence) |

**What this settles: nothing about justerm's shape, and that is the useful part.** Two independent
reasons, either sufficient. First, the tie-breaker table has **no row for test-harness structure** —
stated in as many words in the section above, which is where this question's layer already got its
answer. Second, the arrangement itself has no comparand: xterm's helper terminal has its own canvas
and its own context (`addons/addon-webgl/src/WebglRenderer.ts:91`, in the #774 section), so even the
one case that exists is not the case being proven. A lens reporting the difference is `DELIBERATE`
with this row.

**Recorded so it is not searched again.** The negative result cost a targeted grep across the pinned
tree, and the shape of the answer — *"one hit, and it is not the thing"* — is exactly the kind that
reads as "I must have missed it" to the next person to ask. The corresponding justerm arrangement is
`justerm-web/demo/shared-surface.html`, whose design therefore rests on this repo's own precedent —
the renderer's `demo/per-grid-state.html` and `demo/context-loss-grids.html`, which prove the same
claims one layer down and supplied the differential (compare a sibling's whole rect byte-for-byte)
that the consumer page reuses.

## Dating an anchor across a non-uniform move — the one reference with a generation, and why it may compare with `<` (#741, verified 2026-08-06)

justerm serialises a marker line to a stateless consumer, so it needs the coordinate to say *which
buffer it is true of*. No reference has that problem — none of them serialises an anchor across a
boundary — so this cannot be cited to justify the design (**Wire / frame / API shape → this repo's own
precedent**). What it *can* settle is a mechanism question inside the design: **equality or order?**

| Fact | Reference | Site |
|---|---|---|
| ghostty **does** carry a generation: a per-page `page_serial` plus a `page_serial_epoch` floor, so a pointer can be checked against the page it was taken from — *"The serial number can be used to detect whether the page is identical to the page that was originally referenced by a pointer"* | ghostty | `src/terminal/PageList.zig:372` (the doc), `:392` (the epoch field) |
| It compares with `<` — `if (serial < self.page_serial_epoch) return false` — and it may, **because the counter is `u64` and is stated never to wrap**: *"If we created a new page every second it'd take 584 billion years to overflow. We're going to risk it."* justerm's `marker_epoch` is a `u32` moved by `wrapping_add`, so the same comparison is unavailable and equality is *forced*, not stylistic | ghostty | `src/terminal/PageList.zig:5010` (the compare), `:379` (the overflow note) |
| ⚠ Even there, order is only a **definitely-invalid floor**, never a validity answer: *"generations are not monotonic in list order, so older live successors may have lower generations. The epoch only advances when reset invalidates the entire list."* A live pin still needs the exact check. Read this before proposing that justerm's epoch could answer "how stale" rather than "stale or not" | ghostty | `src/terminal/PageList.zig:3623-3625` |
| ghostty's generation is **per page**, so one mangled page does not invalidate anchors elsewhere; justerm's single counter invalidates the whole index. That is the named prior art for anyone trying to shrink the #738 outage — it is a *design input*, not a defect | ghostty | `src/terminal/PageList.zig:392` |
| xterm.js keeps an anchor valid across a non-uniform move with **no generation at all**, by decomposing the move into splices the anchor applies itself: `onTrim(amount)` → `marker.line -= amount`, `onInsert({index, amount})` → `if (marker.line >= event.index) marker.line += event.amount`, and the mirror for delete. Unavailable to frame mode for the reason ADR-0020 records — the consumer holds a *copy*, and an `O(k)` splice list is a per-move payload on a channel that has none | xterm.js | `src/common/buffer/Buffer.ts:539` (the emit), `:646` (trim), `:654` (insert), `:666` (delete) |
| alacritty is **silent, not an outlier** — it has no anchor primitive to compare: `rg -i 'marker|epoch|generation' alacritty_terminal/src/grid/mod.rs` returns zero hits | alacritty | — |

## Relating a coordinate to the document it names — how the references make "which buffer" unaskable (#743, verified 2026-08-07)

`CommandLine::line` is a **document** line into `Engine::accessible_text`, and on the alt screen the
two queries answer about different buffers at the same instant. As with #741 the *design* question is
not citable — no reference serialises a coordinate to a stateless consumer, so none has ever had to
relate two separately-fetched answers (**Wire / frame / API shape → this repo's own precedent**). What
the references do settle is the **mechanism**: both make the question structurally unaskable rather
than answering it, and neither shape is importable into frame mode. Read this before proposing that
justerm "just do what xterm does" — that shape is on the deliberate-divergence list twice over.

| Fact | Reference | Site |
|---|---|---|
| Which buffer a coordinate names is answered by **typing the accessor**, never by an integer: `IBufferNamespace` exposes `active`, `normal` and `alternate` as three separate handles, so a caller reaching a line reaches it *through* the buffer that owns it and cannot pair a coordinate from one with text from another | xterm.js | `typings/xterm.d.ts:1701` (`active`), `:1706` (`normal`), `:1712` (`alternate`) |
| And a buffer **names its own kind** — `readonly type: 'normal' \| 'alternate'` — so a caller holding one can tell which it has. justerm's `[scrollback ++ grid]` is one integer space that both screens occupy, which is precisely why the question exists here and not there (the same root as `alt-screen-buffer-floor.md`'s `tracked_point` entry: a floor is a bound, and this needed an identity) | xterm.js | `typings/xterm.d.ts:1635` |
| The alt-screen consequence is stated on the **public** surface, not left to the reader: *"Get all markers registered against the buffer. If the alt buffer is active this will always return []."* The implementation is that literal reading — `this.buffer` is the **active** buffer, so the marker population follows the screen | xterm.js | `typings/xterm.d.ts:962-963` (the doc), `src/browser/CoreBrowserTerminal.ts:769` (the impl) |
| ⚠ **That posture is not available to `command_lines`, and the reason is ours rather than theirs.** Emptying the answer on the alt screen makes absence ambiguous between *disposed* and *wrong screen*, which fails ADR-0029 D3.2 and forfeits the re-ask discharge — and the carry discharge is unavailable to a document line. So the reference's answer is closed here **by our own record**, not by taste. Restated with xterm deleted, the finding is still whole: an empty answer must keep meaning one thing | xterm.js | as above |
| A **re-ask declaration on the public typings** is the reference's own shape for a coordinate-bearing result that must not be held: *"Note that the result of this function should be used immediately after calling as when the terminal updates it could lead to unexpected behavior."* This is corroboration for ADR-0029 D6's *state it where the caller reads it* — a mechanism inside the design, which the tie-breaker does allow a reference to settle | xterm.js | `typings/xterm.d.ts:1670` (`getLine`), `:1741` (the same note on `IBufferLine`) |
| ghostty makes it unaskable a second way: prompt-to-prompt navigation never crosses buffers, because it is scoped to the active screen — `self.terminal.screens.active.scroll(.{ .delta_prompt = delta })`. There is no coordinate to relate to a document, since the jump *is* the operation | ghostty | `src/termio/Termio.zig:609` (the fn), `:613` (the scoping) |
| alacritty is **silent, not an outlier**: it has no marker or line-mark concept to compare — `rg -ci marker` over `alacritty_terminal/src/term/mod.rs` and `grid/mod.rs` returns zero in both. Consistent with the #741 row above | alacritty | — |

## What retires a line-anchored mark when the line's content is destroyed in place (#750, verified 2026-08-07)

The question #750 could not answer from justerm alone: three fixups repair a mark when the buffer
*moves*, and nothing repairs it when the row's content dies where it stands. The two references
that have a line-anchored mark at all **converge**, including on the edge that looks like an
oversight — so the split below is a rule to port, not one to invent.

| Fact | Reference | Site |
|---|---|---|
| `Buffer.clearMarkers(y)` disposes every marker on line `y`, firing each one's `onDispose`. Its **only** caller is `_resetBufferLine`, the whole-row reset helper | xterm.js | `src/common/buffer/Buffer.ts:619`; caller `src/common/InputHandler.ts:1200` |
| `_resetBufferLine` is reached from `eraseInDisplay` and from nowhere else — ED 0's rows *below* (`:1238`), ED 1's rows *above* (`:1255`), ED 2's whole screen (`:1277`) | xterm.js | `src/common/InputHandler.ts:1238`, `:1255`, `:1277` |
| ⚠ **`eraseInLine` and `eraseChars` retire nothing**, whatever they blank — both go through `_eraseInBufferLine` / `replaceCells`, which touches cells and `isWrapped` and no marker | xterm.js | `eraseInLine` `src/common/InputHandler.ts:1323`, `:1326`, `:1329`; helper `:1175` |
| ⚠ **The rule is the helper's identity, not the erased range**, and xterm contradicts itself at one edge: ED 1's *cursor* row erases `[0, x+1)` through the partial helper, so at `x + 1 == cols` a full-width row is blanked and its markers survive | xterm.js | `src/common/InputHandler.ts:1246` (the erase) beside `:1249-1252` (the arm that *does* handle the full-width case, for `isWrapped` only) |
| Disposal is **observable to a holder**, not merely an absence: `dispose()` sets `line = -1`, and VS Code tests `marker.line === -1`. justerm's `TermEvent::MarkerDisposed` is the equivalent channel | xterm.js | `src/common/buffer/Marker.ts:32` |
| ghostty reaches the same split from the opposite storage. `semantic_prompt` is a field on `Row`; `Screen.clearRows` **whole-struct-resets** the row in its non-protected branch (`row.* = .{ .cells = cells_offset }`), so the prompt state goes to `.none` — and its protected branch deliberately does not, with the comment *"We need to preserve other row attributes since we only cleared unprotected cells"* | ghostty | field `src/terminal/page.zig:1976`; reset `src/terminal/Screen.zig:1656`; protected branch `:1650-1652` |
| ⚠ `Screen.clearCells` walks graphemes, hyperlinks and styles cell-by-cell and **never touches `row.semantic_prompt`**. `eraseLine` and `eraseChars` use it; only `eraseDisplay` calls `clearRows` — and it too routes its *cursor* row through `eraseLine`, reproducing xterm's edge | ghostty | `clearCells` `src/terminal/Screen.zig:1667`; `eraseLine` `src/terminal/Terminal.zig:3255`; `eraseDisplay`'s `clearRows` `:3341`, `:3370`, `:3387` |
| **Negative result:** alacritty has no line-mark concept — `rg -c "133"` over `alacritty_terminal/src/` is 0, and every `marker` hit is `std::marker::PhantomData`. It cannot arbitrate | alacritty | — |

### The consequence for search highlights, which is not obvious from either file (#750)

In xterm.js a search highlight **is** a marker: the addon registers one marker per match and hangs
a decoration on it, and the decoration service disposes a decoration when its marker disposes. So
the command mark and the search highlight are retired by *one* act there and cannot diverge —
which is why justerm's split (a marker list plus a flat `Vec<Match>`) produces two independent
staleness questions with two different answers. Recorded because the pre-#750 rationale in
`invalidate_search_highlights` cited "xterm's decorations" for the claim that an erase leaves them
alone, and that is **false for ED**.

| Fact | Reference | Site |
|---|---|---|
| One `registerMarker` per match, then `registerDecoration({ marker, … })` | xterm.js | `addons/addon-search/src/DecorationManager.ts:133-134` |
| A decoration is disposed with its marker | xterm.js | `src/common/services/DecorationService.ts:60` |

### What the only system that recovers *command text* does (#750)

Neither reference terminal stores it — ghostty's `semantic_prompt` is a per-row `enum(u2)` with no
column and no `B`/`C` distinction, so it structurally cannot re-borrow. The consumer that does is
VS Code's shell integration, read in the installed bundle (minified, so no line cite is possible):
`resources/app/out/vs/workbench/workbench.desktop.main.js`.

- `extractCommandLine()` walks `buffer.getLine(…).translateToString(…)` from
  `commandStartMarker/commandStartX` to `commandExecutedMarker/commandExecutedX` — the same
  algorithm as `Term::extract_lines` with its `[b_col, c_col)` clip.
- It is called from the **OSC 133 `C` handler**, and the result is stored on the command object;
  the promoted history entry's own `extractCommandLine()` is `{ return this.command }`, a captured
  string that never re-reads a cell.
- Markers are kept alongside it, for *positions* — pruned by `_clearCommandsInViewport()` and
  checked for `marker.line === -1`.

So the real consumer runs capture **and** disposal, which is what #750 landed.

## The overview ruler — who has one, how a mark is merged, and how big it is (#500, verified 2026-08-10)

The decoration territory had **no** rows here at all, while ADR-0024 carried a comparison made once
inside a record. These are the pinned half, added while working #500 (ruler mark merging / heights /
centring).

**Read the first two rows before the rest.** This area is the thinnest reference corpus in this file:
one implementation, and it is the one the tie-breaker table gives *no vote* on the questions #500
asks (`Consumer-facing API shape / units`). Everything below is therefore a search index, not an
arbiter — which is exactly why the negatives are recorded as rows rather than left as absences.

| Fact | Reference | Site |
|---|---|---|
| **Negative — alacritty has no scrollbar and no ruler of any kind.** `rg -ci "overview.?ruler\|minimap\|color.?zone"` and `rg -ci "scroll_?bar"` are both **0** across the tree. It cannot arbitrate anything on this surface | alacritty | — |
| **Negative, but not the one it looks like — ghostty HAS a scrollbar and still has no ruler.** `overview.?ruler\|minimap\|color.?zone` is 0, while `scroll_?bar` is not: the engine produces `PageList.Scrollbar { total, offset, len }` and hands it out as an apprt action, and the frontend draws a **native GTK** bar. A platform scrollbar has no per-line mark surface, so the *absence* of an overview ruler there is a consequence of that choice rather than an omission | ghostty | struct `src/terminal/PageList.zig:3347`; producer `:3396`; config `src/config/Config.zig:1422`; GTK host `src/apprt/gtk/class/surface_scrolled_window.zig:15` |
| ⚠ **Corollary worth more than the negative**: ghostty's three scalars are justerm's `ScrollPosition` — `total` = `scrollbackLen + rows`, `offset` = the viewport's top line, `len` = `rows` — engine-produced, consumer-drawn. So on *who owns scroll geometry* the corpus is 2 of 3 with us; it is only the **marked** ruler that is xterm-only | ghostty | as above |
| A ruler mark merges with an existing zone only when **colour AND position** both match — not "same colour" | xterm.js | `src/browser/decorations/ColorZoneStore.ts:64` |
| The merge threshold is `floor(lines.length / (canvas.height - 1) * drawHeight[position])` — *the number of buffer lines one mark's own pixel height spans*. So a merge fires exactly when two boxes would already touch, and merging is therefore near-invisible rather than a fidelity feature. It is **per position class** (a gutter mark is 3–6× taller, so it merges far more aggressively) and recomputed on canvas resize and on any scroll that changed the buffer length | xterm.js | `src/browser/decorations/OverviewRulerRenderer.ts:134`; applied at `ColorZoneStore.ts:109` |
| ⚠ **Upstream states its own motivation for merging, and it is cost, not appearance** — the accompanying zone pool exists *"to keep zone objects from being freed … reduce GC pressure since the color zones are accumulated on potentially every scroll event"* | xterm.js | `src/browser/decorations/ColorZoneStore.ts:35` |
| Merging is order-independent upstream **only because its input is sorted by buffer line** — zones are accumulated from `DecorationService.decorations`, a `SortedList` keyed on `marker.line`. Coalescing the same lines in registration order is order-*dependent* | xterm.js | consumption `src/browser/decorations/OverviewRulerRenderer.ts:167`; the list `src/common/services/DecorationService.ts:45` |
| Mark height is class-dependent: `full` is `round(2 * dpr)` **device** px; the gutter classes share `round(clamp(canvas.height / lines.length, 6, 12) * dpr)` | xterm.js | `src/browser/decorations/OverviewRulerRenderer.ts:124`, `:128` |
| ⚠ **That formula is not dpr-invariant, and the units are a canvas artefact.** The `[6,12]` clamp is applied to an already-device-px quantity and the result multiplied by `dpr` *again*, so one CSS layout yields a 10 CSS px gutter mark at dpr 1 and 12 at dpr 2. Upstream's own **public** option for this surface is documented "in CSS pixels" — the device px never reaches the API | xterm.js | formula `src/browser/decorations/OverviewRulerRenderer.ts:128`; canvas is device-sized `:153`; public unit `typings/xterm.d.ts:753` |
| A mark is **centred** on its line (`- drawHeight / 2`), over a track scaled by `canvas.height - 1` *"to ensure at least 2px are allowed for decoration on last line"* | xterm.js | `src/browser/decorations/OverviewRulerRenderer.ts:204`, comment `:203` |
| ⚠ **Its containment does not transfer to a DOM ruler.** Centring puts the first line's box at a negative `y` and the last line's past `canvas.height`; both are contained only because `ctx.fillRect` is clipped by the backing store. An absolutely-positioned element gets no such clip, and `getBoundingClientRect` reports its box whether an ancestor clips it or not | xterm.js | `src/browser/decorations/OverviewRulerRenderer.ts:198`-`:212` |

## Search on the overview ruler, and how a match position stays valid (#440, verified 2026-08-13)

Two separable questions, and the second one is the reason this section exists: #440 is the first
feature in this family where the **consumer caches query-derived coordinates across frames**, so the
lifetime question had to be asked of the references rather than assumed.

### What a search puts on the ruler

| Fact | Reference | Site |
|---|---|---|
| Search marks are `position: 'center'` — a **gutter** class — for the active and non-active alike; only the colour differs | xterm.js | `addons/addon-search/src/DecorationManager.ts:140`-`:143` |
| **At most one ruler mark per buffer LINE**: the mark is suppressed when the line already carries one, tracked in a `Set` cleared with the decorations. This is the same cardinality ADR-0024 R1 already states, arrived at independently | xterm.js | set `:33`; cleared `:78`; populated `:87`; suppression `:140` |
| ⚠ **The active match's ruler mark is therefore suppressed in the normal flow.** Every result is decorated plain *first* (`createHighlightDecorations`, which is the only caller of `_storeDecoration`), and the active decoration is created *after*, from a path that never populates the set — so by then its line is already marked. `activeMatchColorOverviewRuler` is a **required** option whose colour upstream's own flow does not paint | xterm.js | plain pass `:44`-`:56`; active pass `:64`-`:70`; required option `addons/addon-search/typings/addon-search.d.ts:75` |
| A wrapped match registers a marker + decoration **per covered row**, so it feeds a mark on every row it touches | xterm.js | `addons/addon-search/src/DecorationManager.ts:120`-`:133` |
| The whole search is capped at `highlightLimit` (default **1000**) by breaking the find loop, so upstream's *count* is capped too — it never has justerm's "full count, capped highlights" split, and therefore never has to decide whether the ruler marks the capped set or the full one | xterm.js | break `addons/addon-search/src/SearchAddon.ts:140`; default `:30` |

### How a match position survives a buffer mutation — 3–0, and nobody holds a raw coordinate

The question justerm has and no reference does. All three keep positions **live**; the mechanisms
differ, and every one of them needs a live reference *into the buffer* — a marker object, a pin, or
the buffer itself. A frame-mode consumer across a process boundary has none, which is ADR-0018/0020's
deliberate trade rather than a gap. So this row set **cannot arbitrate** justerm's design; it is
recorded because the decision it bears on was taken without it.

| Fact | Reference | Site |
|---|---|---|
| Search decorations ride **markers the Buffer moves in place**, so a decoration's line is never stale; the result *set* is separately re-found on a **200 ms** debounce after `onWriteParsed` / `onResize` — the same interval justerm's `SearchController` uses | xterm.js | listeners `addons/addon-search/src/SearchAddon.ts:65`-`:66`; debounce `:70`-`:79` |
| **Stores no match coordinates at all** — the regex is re-run over the *visible* region inside `RenderableContent::new`, which `draw` constructs every frame. What survives a frame is the compiled `dfas` and the focused match, not a list of positions | alacritty | `alacritty/src/display/mod.rs:775` (`draw`), `:784`; `display/content.rs:47`, `:525` |
| **Tracked pin**, maintained by the `PageList` itself — the search iterator holds `list.trackPin(...)` and the comment states the guarantee: *"if the pagelist prunes pages, the tracked pin will be moved somewhere safe"* | ghostty | `src/terminal/search/pagelist.zig:54`-`:62` |

**Measured on our side, for the same question** (probe, 2026-08-13, recorded in #440): a region scroll
(SU/SD/IL/DL/RI) invalidates the held search highlights while **neither** `evicted_total` **nor**
`marker_epoch` moves, so the shift is not observable by a consumer holding match lines. `marker_epoch`
is gated on a *surviving marker* having moved (`markers.rs`, `markers_rotate_region`) — with no markers
in the buffer it never moves at all, and with markers present the same scroll moves it. It therefore
answers *"did a marker move"*, not *"did a coordinate shift"*, and must not be read as the latter.
Only **on-screen** matches actually move; scrollback matches keep their absolute index exactly.

## Multi-viewport resource tiering — how the one reference that shares font machinery splits it (#768, verified 2026-08-18)

Read for ADR-0021's tier rule. **What the reference's word is worth here:** `theflow.md`'s tie-breaker
table has no row for renderer resource tiering, and ADR-0021 itself records that *"the three-tier
keying is justerm's own synthesis — no cited reference splits resources global / per-config /
per-grid"*. So these rows are a **convergence check on a derived rule**, not an authority over it: the
rule stands on what keying costs, and removing ghostty from the argument leaves it standing. Only the
*middle* tier has direct precedent.

Note the noun inversion, which is the easiest way to misread every row below: ghostty's `Surface` is
**one per terminal** and owns a renderer, a GPU atlas texture and a render thread; ADR-0021's
`TerminalSurface` is **one per app**. Read ghostty's "surface" as justerm's *grid*.

| Fact | Reference | Site |
|---|---|---|
| The shared set is **keyed by font configuration**, and states its own reason — *"allows expensive font information such as the font atlas, glyph cache, font faces, etc. to be shared"*. Refcounted (`ref` / `deref`) | ghostty | `src/font/SharedGridSet.zig:1-12` |
| A shared grid is **immutable**: it *"does NOT support resizing, font-family changes …"* because *"increasing the font size in one would increase it in all"*; a config change means a **new** grid that surfaces switch over to | ghostty | `src/font/SharedGrid.zig:1-22` |
| A surface holds the **selector and the key side by side** — `font_grid_key`, `font_size`, `font_metrics` — so the setting is per-terminal while the machinery it selects is not | ghostty | `src/Surface.zig:75-77` |
| `setFontSize` writes the selector **per-surface** (`self.font_size = size`) and then `ref`s the shared keyed grid — *selector per-grid, resource per-config*, implemented rather than theorised | ghostty | `src/Surface.zig:2441`, `:2444` |
| The surface **copies the shared grid's product onto itself** (`self.size.cell = size`, `self.font_metrics = font_grid.metrics`) — a tier names the owner, not the only place a value may sit | ghostty | `src/Surface.zig:2413`, `:2469` |
| The 256-colour **palette stays per-surface** and is read from `state.colors.palette` at draw time, although two surfaces on one theme could share it byte-for-byte — sharing is bought for expensive things only | ghostty | `src/renderer/generic.zig:2024-2026` |
| **The atlas grows; it never evicts.** `grow` preserves *"all previously written data"*, and a full atlas raises `Error.AtlasFull` rather than repointing a slot — so a slot handed to one surface can never be reused under another | ghostty | `src/font/Atlas.zig:313-314`, `:78` (raised at `:177`) |

**The last row is the one that does not transfer, and it is why ADR-0021 owes a Consequence rather
than an import.** `justerm-renderer`'s glyph cache is LRU-**evicting**, so sharing an atlas across
grids creates a hazard ghostty's arrangement cannot have: one grid's pack can repoint a slot another
grid's instance buffer still refers to. Adopting the arrangement without this precondition is the
error the row exists to prevent.

**Cannot be checked from here:** ADR-0021 also cites **WezTerm** (per-window `RenderState`, per-pane
state with no GPU resources) and **three.js** (`webgl_multiple_elements`), and neither has a pinned
tree in `../.refs/`. WezTerm is the record's only cited precedent for a bottom tier holding no GPU
resources — i.e. the unverifiable citation is the one carrying the load ghostty explicitly does not.

## A terminal registry, and what "registered but not drawn" is made of (#770, verified 2026-08-19)

Read for #770's grid registry (Epic #287 S2). Same caveat as the section above, and it is the reason
these rows are here rather than in an argument: `theflow.md`'s tie-breaker gives **renderer resource
ownership the project's own model**, so ghostty is a convergence check on the *state* and has no vote
on its *representation*. It converges on the load-bearing half — a hidden terminal stays registered
and keeps its resources — and diverges on how the hidden-ness is stored, for a reason that is in its
own architecture rather than in a preference (last row).

Read ghostty's "surface" as justerm's *grid* throughout (the noun inverts — see the section above).

| Fact | Reference | Site |
|---|---|---|
| The registry is a flat list of surfaces the app appends to; there is **no id** — identity is the pointer | ghostty | `src/App.zig:170-173` |
| Removal is a **`swapRemove`**, so the list's order is not preserved | ghostty | `src/App.zig:202` |
| Not-drawn is an explicit **`visible: bool`** on the render thread — *"true when the view is visible … used to determine if we should be rendering or not"* — defaulting to `true` | ghostty | `src/renderer/Thread.zig:108-110` |
| The flag gates the **draw**: *"If we're invisible, we do not draw"* | ghostty | `src/renderer/Thread.zig:526-531` |
| …and separately gates the **CPU cell rebuild**: *"If we're not visible there's no point spending CPU rebuilding cells — we'll catch up when the `.visible` mailbox message flips us back on"* | ghostty | `src/renderer/Thread.zig:644-650` |
| Becoming visible again **immediately rebuilds cells and draws** — *"renderCallback skips updateFrame while invisible"* — rather than rebuilding any GPU resource | ghostty | `src/renderer/Thread.zig:380-388` |
| The other reference gates **less**, and the ordering is the fact: a hidden window returns from `draw` **before** `self.dirty = false`, so the frame is deferred rather than dropped and it repaints on return | alacritty | `alacritty/src/window_context.rs:365-376` |

**The two references disagree about how much a hidden terminal stops doing**, which is why the last
two rows are here rather than in an argument: ghostty skips the CPU cell rebuild as well as the
paint, alacritty skips only the paint. **#771 chose ghostty's amount** — the draw loop skips a grid
with no viewport before it packs it, so a hidden grid that is still being fed costs the scatter and
nothing after it. Written here as a record of what the choice was between; the reasoning and the
measured price are in `docs/map/territory/multi-viewport.md`. (This paragraph said "justerm gates
neither yet" until that slice landed.)

**What transfers and what does not.** The state transfers: an invisible surface is never
unregistered and nothing it owns is released, which is exactly the guarantee penterm's adoption PRD
asks for (*"hidden workspaces' grids stay registered, viewport cleared"*). Two things do not:

- **The id.** justerm hands a grid handle across the wasm boundary as a number, so a pointer-identity
  registry has nothing to lend and a `swapRemove`'s freed slot must never be reused — a stale handle
  in JS has to fail loudly rather than address whichever grid landed there.
- **The retained rect.** ghostty can keep a hidden surface's rect because the surface *owns the OS
  window that produces it*; justerm's rect is the consumer's DOM box, which is unmeasurable while
  hidden (`display:none` reads back zero). So justerm carries the not-drawn state as the **absence of
  a viewport** rather than as a flag beside a retained one, and the consumer re-supplies the rect on
  the way back — which it must anyway, since the layout it is returning into is why it was hidden.

## Drawing N views on one canvas — the mechanism reference, and the two places it does not reach (#771, verified 2026-08-19)

Read for #771's draw loop (Epic #287 S3). three.js is the **mechanism** citation ADR-0021 makes for
"N grids, one context", and #771 is the slice that needed a fact from it, so it now has a pinned tree
(the routing table in `theflow.md`). It is cited for the *shape of the loop* and nothing else: the
tie-breaker gives renderer resource ownership justerm's own model, and units their own API's
coherence, so both rows below where the two differ are recorded as differences, not as corrections.

| Fact | Reference | Site |
|---|---|---|
| The multi-view loop is per view: **viewport, then scissor, then `setScissorTest(true)`, then a per-view clear colour**, and only then a render — one rect at a time on one canvas | three.js | `examples/webgl_multiple_views.html:266-276` |
| The **projection is sized to the view's rect**, not to the canvas: `camera.aspect = width / height`, re-derived per view per frame | three.js | `examples/webgl_multiple_views.html:273-274` |
| There is **no full-canvas clear** anywhere in the loop or before it — the example's views tile the canvas, so no pixel is outside every rect and it never has to answer for one (`renderer.clear` and `autoClear` do not appear in the file at all) | three.js | `examples/webgl_multiple_views.html:252-258` |
| `setViewport` takes a rect in **CSS px with a bottom-origin y**, and the renderer multiplies by the pixel ratio it owns and rounds | three.js | `src/renderers/WebGLRenderer.js:804-816` |

**What transfers.** The loop's shape, whole: per-rect viewport + scissor + clear + draw, with the
projection re-sized to the rect. justerm's `draw` is that loop. The scissor is load-bearing for the
same reason in both: `clear` ignores the viewport, so without it each view's background clear wipes
the whole buffer.

**What does not, and why each is a difference rather than a defect.**

- **The full-canvas clear.** justerm has one and three.js does not, because justerm's grids are not
  obliged to tile — a consumer places them where its layout says, and the area between two rects
  belongs to the page behind a canvas that is a transparent overlay plane (ADR-0021's z-order
  constraint). The reference is *silent* here rather than opposed: it has no uncovered area to have
  an opinion about.
- **The rect's units and origin.** three.js takes CSS px and flips nothing, because its caller hands
  it fractions of a canvas the renderer itself scales (`setPixelRatio`). justerm takes **device px,
  top-left origin**, and flips at the `gl.viewport` site: its caller hands it a measured DOM box, and
  the buffer height to flip against is one this renderer owns and may have been granted less of than
  it asked for (#339). The consumer-facing-units tie-breaker is our own API's coherence, and
  `cell_width()`'s contract already puts *"anything that addresses the drawing buffer — `readPixels`,
  GL interop, a picking rect"* in device px. Taking CSS px would also import three.js's rounding step,
  which #337 is the local record of not wanting.

## Sharing font machinery between terminals — how the one reference that does it refcounts, keys and invalidates (#772, verified 2026-08-19)

Read for #772's atlas registry (Epic #287 S4). This is the **one tier of ADR-0021's three that has
direct precedent**, so unlike the two sections above it these rows are read for *mechanism* rather
than only for convergence — but the tie-breaker is unchanged: `theflow.md` gives renderer resource
ownership justerm's own model, so a difference below is `DELIBERATE` unless the defect stands with
ghostty deleted from the sentence. The #768 section above carries the rows about *what* is shared;
these are about the **lifetime** — how an entry is joined, left and rebuilt.

Read ghostty's "surface" as justerm's *grid* throughout (the noun inverts — see the #768 section).

| Fact | Reference | Site |
|---|---|---|
| `ref` is **find-or-build**: an absent configuration *"will be initialized with a ref count of 1"*, a present one has its count incremented — one call, and the caller cannot tell which happened | ghostty | `src/font/SharedGridSet.zig:89` |
| The set **owns** what it hands back — *"the returned data (key and grid) should never be freed … the memory is owned by the set and will be freed when the ref count reaches zero"* | ghostty | `src/font/SharedGridSet.zig:94` |
| `deref` destroys **immediately** at zero — *"we are at a zero ref count so deinit the group and remove"* — with no free pool, no deferred reap and no grace period | ghostty | `src/font/SharedGridSet.zig:408` |
| The set exposes its **entry count** — *"returns the number of cached grids"* — so sharing is observable rather than inferred | ghostty | `src/font/SharedGridSet.zig:81` |
| The key hashes the **font size**, which carries the screen density: `DesiredSize` is `{ points, xdpi, ydpi }`, *"the DPI of the screen so we can convert points to pixels"* | ghostty | `src/font/SharedGridSet.zig:564`, `src/font/face.zig:50` |
| Switching a terminal to a different shared grid **forces a full cell rebuild** — *"force a full rebuild, because cached rows may still reference an outdated atlas from the old grid and this can cause garbage to be rendered"* — and separately resets the texture-sync watermarks so the whole atlas re-uploads | ghostty | `src/renderer/generic.zig:1110`, `:1081` |

**What transfers, and it is most of it.** find-or-build behind one call, ownership by the set,
destruction at zero with no pool, a countable entry set, and a forced re-pack on the way into a new
entry: `acquire_config` / `release_config` / `atlasCount` / `select_config`'s `needs_repack` are each
the same shape, arrived at from the same pressure. The last row is the one that would have been a
silent bug: justerm's packed instances address slots in the entry a grid is *leaving*, and the entry
it joins has a cache of its own, so a grid that does not re-pack draws whatever happens to sit at
those slot indices in the new atlas.

**What does not, and it is the density.** ghostty puts the DPI **inside the key**; justerm keys only
the four consumer selectors and treats the density as a global input every entry is baked at. The
reason is in the architecture rather than in a preference, and it is the same one
`docs/map/territory/multi-viewport.md` already records for the cell: a ghostty `Surface` is an OS
window that can be dragged onto a monitor of its own density, so its DPI genuinely is per-surface,
while N viewports on one canvas share one drawing buffer and therefore one `devicePixelRatio`. A key
component that is globally constant cannot separate two keys — it can only make every key wrong the
moment it changes. That is why a DPR change is the *rebuild-all* path ADR-0021 names as separate from
re-keying, rather than a mass re-key.

**And one hazard ghostty's arrangement cannot have**, restated here because it is the reason this
section is not simply an import: its atlas **grows and never evicts** (row in the #768 section), so a
slot handed to one surface can never be reused under another. justerm's glyph cache is LRU-evicting,
so the guarantee ghostty gets for free is one this design still owes — see ADR-0021's *Consequences*
and the multi-viewport territory's known holes.

## Recovering a context loss when the resource is SHARED between terminals (#774, verified 2026-08-20)

Read for #774 (Epic #287 S6): one context means one loss for **every** registered grid, so `restore`
walks two registries rather than acting on a grid. The question asked of the trees was whether any of
them recovers a resource that two terminals hold *together*.

**One does, and it is the closest analogue in any reference to this repo's `ConfigRegistry`.**

| Fact | Reference | Site |
|---|---|---|
| A texture atlas is cached **across terminals** and refcounted by an owner list — `ownedBy: Terminal[]`, with the comment that the implementation may hold terminals forever | xterm.js | `addons/addon-webgl/src/CharAtlasCache.ts:13-21` |
| A terminal **joins** an existing entry when its generated config compares equal, rather than baking its own | xterm.js | `addons/addon-webgl/src/CharAtlasCache.ts:60-68` |
| Release disposes the entry **only when the leaver is its sole owner**; otherwise it drops one reference and leaves the shared atlas standing | xterm.js | `addons/addon-webgl/src/CharAtlasCache.ts:85-99` |
| On `webglcontextrestored` the restoring terminal drops **its own** cache reference, re-initialises its WebGL state, and **asks for a full viewport redraw** | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:137-145` |
| The other reference asks the driver's reset status at the point of use, per window, and only when robustness is available | alacritty | `alacritty/src/renderer/mod.rs:283-295` |
| The third has **no GPU-loss concept to compare**: `rg -i 'lost\|robustness\|device.?removed\|resetstatus'` over the pinned `src` returns hits only about *focus* being lost | ghostty | (a negative result — the grep, not a file) |

**Why the release rule does not transfer, and the direction is not "we drifted".** What xterm shares
across terminals is a **CPU-side** atlas — a canvas the `TextureAtlas` rasterises into — while each
renderer owns its own context and uploads its own GL texture from it. So a loss there is *per
terminal*, and dropping only that terminal's reference is exactly right. justerm's shared entry holds
the **GPU texture**, on the one context every grid draws through, so the same event takes every entry
down at once and the restore has to walk the whole registry. The shapes differ because the tier does
(`theflow.md`'s divergence row: the per-grid tier holds GPU state, against both references). A lens
reporting the difference as a defect is `DELIBERATE` with that row.

**The last column of that fourth row is the one #774's proof turns on.** xterm can end its restore
with `_requestRedrawViewport()` because its consumer retains the whole terminal state and can simply
be asked again. justerm's cannot — the recorded divergence is that the consumer holds *only the
current frame* (ADR-0020 R3) — so `restore` repaints from the renderer's own retained grid instead,
and "recovered" has to mean *with no re-feed from the consumer*. That is why
`demo/context-loss-grids.html` places a hidden grid after the restore and asserts its rect
byte-for-byte with no `apply_damage` in between: the assertion is stricter than the reference's
contract because our consumer cannot supply the reference's fallback. The reference corroborates the
*problem* and has nothing to arbitrate about the solution.

**And the negative result that bounds all of it:** no reference loses **one** context across **N**
terminals, because none of them shares one. xterm is one context per terminal, alacritty one display
per window, ghostty one renderer per surface (`Surface.zig:86-92`, in the #768 section). So the case
#774 exists for — a terminal that is *registered and not drawn* when the context dies — has no
comparand anywhere, and the tie-breaker gives this layer to justerm's own model in any case.

**The refcount makes xterm's restore a no-op for the atlas whenever a sibling shares the
configuration**, which is worth stating because it looks like a rebuild and is not: the restoring
terminal drops its reference, then `_initializeWebGLState` re-acquires and `configEquals` matches the
sibling's surviving entry, so it rejoins the entry it just left. Only the *sole owner* case actually
rebuilds — and that rebuild throws away every cached glyph, re-warming ASCII alone. Transposed here
that shape is the mutation this slice already measured red (*"bake only configurations with a drawn
holder"*): on one context every entry's texture died, so a refcount-conditional restore leaves the
untouched entries dead. It is a **negative** corroboration of `restore`'s unconditional per-config
bake, not a source that could make it wrong.

**three.js is the other half of the answer, and it points the opposite way from what a viewport
renderer might hope.** `onContextRestore` calls `initGLContext()`, which re-instantiates the property
and resource managers — `properties = new WebGLProperties()` — so every GPU resource is re-created
**lazily, at next use**.

| Fact | Reference | Site |
|---|---|---|
| The restore handler resets the lost flag and re-runs the whole GL init rather than replaying resources | three.js | `src/renderers/WebGLRenderer.js:1119-1131` |
| …and that init throws away the per-object GPU property cache, so each resource is rebuilt the next time something draws it | three.js | `src/renderers/WebGLRenderer.js:458` |

Lazy works there because N views draw one scene and every resource is reached every frame. It is
exactly the shape a **registered-but-not-drawn** grid defeats: a hidden grid has no next use, so a
lazy restore would leave it dead until something placed it — and placing it is a consumer action that
may never come. That is the structural reason this renderer's restore is **eager**, and it is the
closest any reference gets to #774's question. Recorded as a divergence with a reason rather than as
prior art to follow.

## Whether a glyph's ink may leave its cell — 3–0, and the single guard that withdraws it (#791, verified 2026-08-20)

Read for #791. justerm-renderer builds its glyph quad from the cell-size uniform, so a glyph's ink is
destroyed at the cell boundary rather than drawn over the neighbour. The question asked of the trees
was whether that boundary is a shared convention or ours alone.

**It is ours alone.** All three let the quad be the glyph.

| Fact | Reference | Site |
|---|---|---|
| The glyph quad **is the glyph's own bounding box**, not the cell: `zeroToOne = (a_offset / u_resolution) + a_cellpos + (a_unitquad * a_size)`, where `a_size` is the rasterised glyph's size and `a_cellpos` only places the cell's origin | xterm.js | `addons/addon-webgl/src/GlyphRenderer.ts:53` |
| The atlas bakes into a canvas **taller than the cell** — `deviceCellHeight + TMP_CANVAS_GLYPH_PADDING * 4` — and the bounding-box scan runs over that whole area, so ink above and below the cell survives into the atlas instead of being clipped at bake time | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:485` |
| **The one withdrawal, and it is conditional:** left overflow is clipped only when the neighbouring cell's background differs (`bg !== lastBg`). Overflow over a same-background neighbour is drawn | xterm.js | `addons/addon-webgl/src/GlyphRenderer.ts:263` |
| Backgrounds are a **separate pass drawn before glyphs**, which is what lets an overlapping glyph quad survive its neighbour's fill | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:399` |
| A single-width glyph whose ink exceeds **1.5 cells** is rescaled rather than clipped — behind a user option, and never for ASCII, emoji, powerline or nerd-font codepoints | xterm.js | `src/browser/renderer/shared/RendererUtils.ts:47` |
| The instance carries the glyph's own `width`/`height` with `top`/`left` bearings — again the quad is the glyph, positioned by its bearing | alacritty | `alacritty/src/renderer/text/glsl3.rs:357` |
| Same shape at the **text** glyph site: `glyph_size = .{ render.glyph.width, render.glyph.height }` with `bearings` carrying the shaper's x-offset | ghostty | `src/renderer/generic.zig:3202-3204` |

**Two traps in this area, both hit while recording these rows, both caught by rule 2/3 rather than by
reading.** The `bg !== lastBg` guard is at `:263`, not `:262` — an issue body had already been written
with the wrong number. And ghostty repeats `glyph_size = .{ render.glyph.width, render.glyph.height }`
**six** times: `addUnderline` (`:3077`), `addOverline`, `addStrikethrough`, `addGlyph` (`:3202`),
`addCursor`, `addPreeditCell`. A `--find` on the pattern lands on the *decoration* site first, and a
row citing it would be a correct `file:line` supporting a claim about text rendering that the line does
not make — rule 3's more dangerous class. Pick the site by enclosing function, not by pattern.

**What the references do not settle.** They agree that overflow is allowed and they do not agree on how
much room to reserve for it, nor on which edges the withdrawal covers — xterm.js guards only the left
edge, and its own reason (it walks cells left to right and therefore knows only the *previous*
background) is an artefact of its loop rather than a principle. So the *permission* is 3–0 prior art;
the *rule* is a design decision this repo still owns.

**PuTTY is not in `../.refs` and therefore has no row here.** It was read at `dc472b18` (2026-08-17)
while investigating #791 and it corroborates the permission from a fourth angle — it clips per *run*
(`windows/window.c`, the `line_box` handed to `ExtTextOut` spans `char_width * len`), so ink bleeds
freely between cells inside a run and is cut only at the run's edge and at the row. Those citations
live in #791's body, not here, because this file's line numbers are only meaningful at the pinned SHAs
in `theflow.md` § "Step 1" and PuTTY is not one of them. Adding it to the routing table is a
`/grill-the-flow` decision, not a unilateral one.

## What a scrollbar drag reads on every move — 1 comparable reference, and it reads nothing (#814, verified 2026-08-25)

Read for #814. `justerm-web`'s `Scrollbar.dragTo` re-reads the track's `getBoundingClientRect()` on
every `mousemove` and turns the pointer's absolute position into a ratio — which is why an element
that loses its box poisons the drag at all. The question asked of the trees was whether re-reading
per move is the convention.

**Only one tree can answer**, and the negatives are the useful half:

- **alacritty has no scrollbar.** `rg -ci "scroll_?bar"` over the pinned tree is `0`. It cannot
  arbitrate.
- **ghostty has one and computes no ratio.** It pushes `value` / `upper` / `page_size` in **line**
  units into a GTK adjustment and lets the platform own the pointer→value conversion, which is also
  why it has no overview ruler (already recorded above).

So the comparison is one-to-one against the scrollbar xterm.js vendors from VS Code, and it does not
re-read anything.

| Fact | Reference | Site |
|---|---|---|
| The drag **clones the scrollbar state at pointerdown** and every later move works from that snapshot | xterm.js | `src/browser/scrollable/abstractScrollbar.ts:239` |
| Each move is a **delta from the initial pointer position** — `getDesiredScrollPositionFromDelta(pointerDelta)` — so no box is read during the gesture at all | xterm.js | `src/browser/scrollable/abstractScrollbar.ts:257` |
| Its sizes are **pushed in**, never measured from the DOM: `setVisibleSize` / `setScrollSize` on the state object | xterm.js | `src/browser/scrollable/abstractScrollbar.ts:133` |
| The one divide it does keep is guarded on that pushed state (`_computedIsNeeded`) before dividing by the slider ratio | xterm.js | `src/browser/scrollable/scrollbarState.ts:214` |
| The gesture is held by **pointer capture** on the target element, with `pointermove` / `pointerup` bound to the capture target rather than to `window` | xterm.js | `src/browser/scrollable/globalPointerMoveMonitor.ts:58` |
| **Negative worth having:** no `lostpointercapture` handler is registered, so a drag whose capture is lost ends without running `onStopCallback` — `handleDragEnd` never fires and the slider keeps its active class | xterm.js | `src/browser/scrollable/globalPointerMoveMonitor.ts:58` |

**What this settles and what it does not.** It settles that snapshot-plus-delta is available prior
art for a drag that survives its element losing a box. It does **not** argue that justerm's
absolute-position model is a defect: restate that finding with the reference deleted and nothing
remains, so it is a **design proposal**, and this layer's tie-breaker (our own API's internal
coherence) gives the reference no vote. #814 fixed the totality of the existing model instead, and
the recorded difference is that our drag goes *inert* while hidden where VS Code's keeps scrolling —
which is the behaviour #801 wants, since the pane the user cannot see should not scroll.

## The VT-gap sweep of 2026-08-26 — five decisions the references settled (#823–#832, verified 2026-08-26)

A full sweep of the Engine's VT dispatch against all five pinned trees produced seven filed gaps.
These are the rows that **decided** something — each one is the reason a spec says what it says, and
each was reached by reading source rather than by recalling a convention. They live here because
they are stable and because two of them are traps: a naive reading of the obvious reference gives
the wrong answer.

### Empty parameters are not empty values

The sharpest lesson of the sweep, and it cost a defect in a filed spec before it was learned: the
degenerate form of a sequence is often the **only** form real applications emit, and every
reference has a deliberate rule for it that a "normal case" reading never reaches.

| Fact | Reference | Site |
|---|---|---|
| **An empty dynamic-colour spec sets nothing** — the slot's name is left null and only a non-null name is set or queried. This is the shared `OSC 10`-family path, so it governs foreground, background **and** cursor | xterm | `misc.c:3685` (no names at all), `:3687` (`names[0] == ';'` → an empty *field* is also no name) |
| **An empty `OSC 52` clipboard target defaults to the system clipboard**, not to "unrecognised". Branches on the first byte being the separator | ghostty | `src/terminal/osc/parsers/clipboard_operation.zig:24` |
| …and it is pinned by a test named for the case, asserting `52;;?` yields kind `c` | ghostty | `src/terminal/osc/parsers/clipboard_operation.zig:64` |

⚠ **Measured consequence, not theory.** `tmux` 3.2a emits `ESC ] 52 ; ; <base64>` — the *empty*
target — for both `set-buffer -w` and an ordinary copy-mode copy. `nvim` 0.8.0 emits
`ESC ] 12 ; BEL` — the *empty* spec — four to five times per session. Both were captured on the
RHEL 9 VM. A spec that ignores unrecognised targets, or that forwards a raw spec unconditionally,
fails against the only emitters that exist. #828's target rule was written the wrong way from the
alacritty reading (which matches a single target byte and has **no** branch for an absent field)
and was corrected from this row.

### REP's retained character — the two obvious references disagree, and xterm breaks the tie

| Fact | Reference | Site |
|---|---|---|
| **`REP` repeats through the ordinary print path**, in a loop, so wrap / wide pairing / pen all apply. Count defaults to one; no retained char means no-op | ghostty | `src/terminal/Terminal.zig:452-456` |
| The retained char is armed **only by printing** | ghostty | `src/terminal/Terminal.zig:1365` |
| ⚠ **…and ghostty clears it only on full reset** — so a `REP` after a cursor move repeats a character from earlier in the stream | ghostty | `src/terminal/Terminal.zig:4467` (inside `fullReset`) |
| **xterm's rule is the opposite, and it is structural rather than a list of clearing sites**: at the end of each byte, `lastchar` is assigned `thischar` **only when the parser is back in the ground state** — and `thischar` is `-1` unless a graphic character was printed. So any completed escape sequence lands in ground with `-1` and disarms the repeat | xterm | `charproc.c:6478` (the ground-state gate), `:3364` (printing arms both) |
| `REP` then guards on the retained char having **positive width** — a control or zero-width char is not repeatable | xterm | `charproc.c:6154` |
| **xterm.js agrees with xterm**: its `precedingJoinState` is zeroed on every executed C0 control and at each escape-sequence entry — eleven sites in the parser, not one | xterm.js | `src/common/parser/EscapeSequenceParser.ts:690` (execute fast-path), `:310`, `:502`, `:676`, `:732`, `:775`, `:810`, `:849`, `:878`, `:902`, `:928` |

**The adjudication, since the trees split 2:1.** ADR-0004 gives the spec — and xterm as its closest
readable proxy — authority over the whole VT layer, above any implementation including ours. xterm
and xterm.js agree; ghostty is the outlier. So justerm follows **ghostty's mechanism** (repeat by
re-entering print) with **xterm's lifecycle** (the retained char does not survive a completed
sequence). Those are two different questions and picking one reference for both is the error #825's
body made before this row existed.

### Secondary device attributes — report yourself, do not impersonate

| Fact | Reference | Site |
|---|---|---|
| **alacritty reports its own version**, derived from its crate version at compile time | alacritty | `alacritty_terminal/src/term/mod.rs:1266-1267` |
| xterm.js instead **impersonates** whichever terminal it is configured to emulate, branching to xterm / rxvt-unicode / screen replies | xterm.js | `src/common/InputHandler.ts:1737`, the xterm reply at `:1745` (`ESC[>0;276;0c`) |

justerm follows alacritty. Impersonation is self-defeating for an engine that does not implement
what it would be claiming, and it contradicts the existing DA1, written to advertise only the levels
the Engine genuinely has.

### The title stack

| Fact | Reference | Site |
|---|---|---|
| **Two stacks, not one** — window title and icon name are pushed independently, selected by the second parameter | xterm.js | `src/common/InputHandler.ts:2952` |
| Depth is bounded at **10**… | xterm.js | `src/common/InputHandler.ts:47` (`STACK_LIMIT`) |
| …and overflow drops the **oldest** while the push still succeeds | xterm.js | `src/common/InputHandler.ts:2954` |
| A pop calls the **same title setter** the OSC handler calls, so a restore is an ordinary title change to every consumer | xterm.js | `src/common/InputHandler.ts:2967` |
| **Negative:** ghostty has no title-stack handling in its terminal core, and alacritty does not implement XTWINOPS at all | ghostty / alacritty | `ghostty/src/**` grepped for `title_stack`/`push_title`/`pop_title` — 0 matches; `alacritty_terminal/src/**` for the `t` final — 0 matches |

### Cursor colour

| Fact | Reference | Site |
|---|---|---|
| `OSC 12` is a set-or-report on the same dynamic-colour path as 10/11 — so the empty-spec rule at the top of this section governs it | xterm.js | `src/common/InputHandler.ts:3210` |
| `OSC 112` restores it | xterm.js | `src/common/InputHandler.ts:3268` |

| The slot order is fixed and the cursor is third: `OSC_TEXT_FG = 10`, then BG, then CURSOR — all above the first `#if` guard, so a build option cannot renumber them | xterm | `ptyx.h:1018`, `:1020` |
| **The stack caps at the cursor in a second reference too** — the next slots are the pointer colours, and it refuses rather than mis-addressing them | alacritty (via `vte`) | `vte-0.15.0/src/ansi.rs:1431` (`if index > NamedColor::Cursor as usize { unhandled(params); break; }`) |

#### The empty-field *advance* splits 3–1, and the outlier believes it does not

Added 2026-08-26 while implementing #832. This is the row that costs a re-derivation if it is
missing, because reading any single reference gives a confident answer and two of the four give
opposite ones.

An empty field **skips its slot and still advances to the next**, so `OSC 10 ; ; <spec>` addresses
the *background*:

| Reference | Advances past an empty field? | Site |
|---|---|---|
| xterm | **yes** — the null name is skipped, then `strchr`/`*names++` steps past the separator | `misc.c:3687-3692` |
| xterm.js | **yes** — `++offset` is in the `for` header, outside the `parseColor` guard | `src/common/InputHandler.ts:3156` |
| alacritty (via `vte`) | **yes** — `dynamic_code += 1` is the last statement of the loop body, after the `unhandled` else-branch | `vte-0.15.0/src/ansi.rs:1447` |
| **ghostty** | **no** — `std.mem.tokenizeScalar` never yields an empty token, so the gap is invisible and `OSC 10 ; ; <spec>` sets the **foreground** | `src/terminal/osc/parsers/color.zig:130`, pulled at `:293`, bumped at `:307` |

**Ghostty's divergence is unintentional and unpinned**, which is what makes it dangerous to read: the
loop opens with `// This matches the xterm behavior (see misc.c ChangeColorsRequest)`
(`src/terminal/osc/parsers/color.zig:288`), and no test in `src/terminal/` exercises a leading or
embedded empty field in the dynamic-colour family. A pass that consults ghostty first gets the
opposite answer *with a comment asserting it is xterm's*.

ADR-0004 settles it without needing the tally: `ctlseqs.txt:2082` indexes the stack by **parameter**
(*"Each successive parameter changes the next color in the list"*), not by value. justerm follows the
three.

#### Two more places this family splits, neither of which justerm follows xterm on

| Fact | Position | Site |
|---|---|---|
| **`OSC 112 ; <payload>` is discarded entirely** — the reset is in the `need_data = False` list, and unwanted data returns before dispatch | xterm **and** ghostty | xterm `misc.c:4156-4158`; ghostty `src/terminal/osc/parsers/color.zig:320` (with a named test at `:759`) |
| …while it **still resets**, the payload ignored | xterm.js **and** alacritty (via `vte`), and justerm | xterm.js `src/common/InputHandler.ts:3268`; `vte-0.15.0/src/ansi.rs:1521` |
| **A colour reply echoes the terminator the request arrived with** (BEL in → BEL out) | xterm, ghostty, alacritty | xterm `misc.c:3567` → emit at `:3593`; ghostty `src/terminal/osc/parsers/color.zig:97`; `vte-0.15.0/src/ansi.rs:1330` |
| …while the reply is **always ST**, whatever arrived | xterm.js, and justerm | `src/browser/CoreBrowserTerminal.ts:239` |

Neither is a defect justerm can state without naming a reference, so neither was changed by #832 —
but both are now *known* rather than merely un-looked-at. The terminator one has a measured wrinkle:
`justerm-core/tests/fixtures/cursor_color_nvim.raw` shows a real `nvim` asking with **BEL**
(`ESC ] 11 ; ? BEL`) and being answered with ST, and the engine discards `bell_terminated` before any
event is queued — so a consumer could not echo even if it wanted to.

#### A routing fact: alacritty's OSC dispatch is not in the alacritty tree

`alacritty_terminal` delegates its whole ANSI layer to the **`vte` crate**
(`alacritty_terminal/Cargo.toml:27`), so grepping the pinned alacritty tree for `osc_dispatch`
returns **zero hits** and reads exactly like "alacritty does not implement this". Its dynamic-colour
semantics live in `vte-0.15.0/src/ansi.rs`, which is **the same crate and the same version justerm
itself depends on** — so this reference is already on disk in the cargo registry, needs no pinned
tree, and is immutable by virtue of being a published crate version.
