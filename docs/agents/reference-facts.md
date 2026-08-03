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
| ⚠ **A preedit outranks DECTCEM there** — an explicitly hidden cursor is *shown* as a solid block during composition, *"because it shows an important editing state to the user"*. Nothing in justerm-web's chain can currently override `cursorVisible: false` (#592) | ghostty | `src/renderer/cursor.zig:47` |
| alacritty suppresses the blink during a preedit too, as a term in the same expression | alacritty | `alacritty/src/event.rs:1633` |
| **Negative result: xterm.js has no preedit rule for the caret.** Its only `isComposing` guard near the cursor is `_syncTextArea`, which stops *moving the hidden textarea* mid-composition — an IME-disturbance guard, unrelated to the caret | xterm.js | `browser/CoreBrowserTerminal.ts:337-339` |
| ⚠ **Measured, not read (#592, real browser, 2026-07-28)**: composition driven through the hidden textarea, cursor cell and a content cell sampled 5x over 1.4s. With the application silent — the default since #575 — the caret shows **one** distinct colour (already solid) and no content cell changes; with the application asking to blink, the caret shows **two**. So justerm-web adopted alacritty's suppression as a **no-op in the common case**, biting only where an application explicitly asked to blink. ghostty's stronger form was **rejected**: revealing a DECTCEM-hidden caret would invert `cursorCommand`'s contract for a rare case | justerm-web | `justerm-web/src/cursor.ts` `setComposing`, decision recorded on #592 |

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
precisely what xterm leaves to `FitAddon`. alacritty differs because it **owns its OS window** and can
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

| Fact | Reference | Site |
|---|---|---|
| **`handleResize` runs unguarded during a context loss** — no `isContextLost` check, no deferral; it recomputes dimensions and assigns the canvas straight through. The context-loss listeners sit in the constructor and touch only the restore timeout and the atlas rebuild | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:192` (the handler), `:125-146` (the two listeners, which it never consults) |
| …because **the canvas is sized from the grid, never from what the driver granted**: `_canvas.width = dimensions.device.canvas.width`, a `cols * cell` product. There is no read-back to return 0 | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:205` |
| ⚠ **`drawingBufferWidth` / `drawingBufferHeight` appear nowhere in the project** — `rg drawingBuffer src addons` returns **0 hits** at this pin. So the failure class justerm's adopt-what-fits creates (a lost context reports a 0x0 buffer, the loop floors the grid to 1x1) is structurally unreachable there, and xterm cannot arbitrate the guard | xterm.js | absence, reproducible: `rg -n drawingBuffer ../.refs/xterm.js/{src,addons}` |
| ⚠ **`isContextLost` appears nowhere either** — `rg isContextLost src addons` is also **0 hits**. So xterm has no position on the event-vs-state race that this territory turns on (a browser kills a context synchronously and only *queues* `webglcontextlost`); it consults neither the query nor its own listeners outside the constructor. The "cannot arbitrate" conclusion therefore reaches further than the buffer read-back — it covers the predicate too | xterm.js | absence, reproducible: `rg -n isContextLost ../.refs/xterm.js/{src,addons}` |
| **The comparison set does not extend.** alacritty and ghostty are not browser renderers and have no context-loss concept at all — matching `gl-context-lifecycle.md`'s recorded *"No reference comparison at all"* | alacritty · ghostty | n/a by layer, not by omission |
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
commits only after the re-bake succeeds; its in-repo siblings (`rebake_atlas`, `rebake_for_cell`,
`adopt_spacing`'s rollback) do the same. **Direction: this layer *and* its siblings agree against
the reference — a family position, held, and xterm must not be read as licence to half-commit.**

| Fact | Reference | Site |
|---|---|---|
| The `webglcontextrestored` handler calls `_initializeWebGLState()` **unguarded and without `try`/`catch`**, straight from the event listener | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:137-146` |
| That method assigns the rectangle renderer, **then** the glyph renderer — two sequential commits with no rollback between them | xterm.js | `addons/addon-webgl/src/WebglRenderer.ts:279-287` |
| …and `GlyphRenderer`'s constructor throws on a lost context (`throwIfFalsy(gl.getParameter(...))`). So a second loss mid-restore leaves it **half-committed** — new rectangle renderer, stale/disposed glyph renderer — with no retry latch, and the exception escapes into the listener | xterm.js | `addons/addon-webgl/src/GlyphRenderer.ts:128`, `:130` |
| justerm cannot reach that state by construction: `restore` deletes the half-built replacements and returns `Err` on a re-bake failure, leaving the live objects in place, and the state machine keeps `pending_rebuild` set so the next frame retries | justerm-renderer | `justerm-renderer/src/webgl.rs` `restore` (the `rebake` error path), `context_loss.rs` `a_failed_rebuild_is_retried_on_the_next_frame` |

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
(`webgl.rs` `bg_a = (!block && v_bg_default > 0.5) ? mix(u_bg_alpha, 1.0, cov) : 1.0`) converges with
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

## Renderer ink channels

| Fact | Reference | Site |
|---|---|---|
| The strikethrough draws in the **glyph foreground**, never in the SGR 58 underline colour — confirming #525's premise | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:758-762` |
| ⚠ **The mechanism is a `save`/`restore` bracket, and the obvious grep hit says the opposite.** The underline block opens with `save()` (`:565`), sets `strokeStyle` from `getUnderlineColor()` (`:576-583`), then assigns `fillStyle = strokeStyle` (`:585`) — read alone, that says the SGR 58 colour becomes the fill for everything after it. `restore()` at `:688` undoes it, so the glyph `fillText` (`:735`) and the strikethrough's `strokeStyle = fillStyle` (`:762`) both get the foreground back | xterm.js | `TextureAtlas.ts:565`, `:585`, `:688`, `:735`, `:762` |
| ⚠ Path note: `TextureAtlas.ts` lives under `addons/addon-webgl/src/`, **not** `src/browser/renderer/shared/` as #525 cites | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts` |

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

**What justerm took, and the one thing it deliberately did not.** `CellGeometry` states its
preconditions and the converters *signal* a violation; they still answer. The refusal did not
transfer because its recovery half cannot: xterm owns the measurement (`CharSizeService`), while
`justerm-web` is handed the geometry per event by the consumer's `getGeometry()` (#578, ADR-0017), so
a dropped gesture would not come back on its own. ghostty's route — make it unrepresentable — is
closed for a recorded reason: the CSS cell is a **float on purpose** (ADR-0022, so `cols *
cssCellWidth()` scales back exactly). The dedupe on the warn is justerm's own, not xterm's: the reach
here is every event at pointer rate, where xterm's warn sites are selection-change.

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
