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
2. **Verify before recording, not before quoting.** Every row below was grepped
   against the pinned tree on the day it was added. Copying a citation out of an issue
   body *is not verification* — several of those citations turned out to be off.
3. **Record the mechanism when the site alone misleads.** Two rows below carry a
   "read this too" note because the obvious grep hit gives the opposite answer.
4. **This file does not decide anything.** It records what a reference *does*.
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

**Verdict recorded so it is not re-litigated: justerm converges with alacritty, and the references are
1:1.** So "the walk crosses a separator" is not by itself evidence of a defect here — by the two-lens
divergence-direction rule, a split reference means this is a *product* choice, not a correctness fix.
Valid as long as justerm's word-boundary set keeps treating the start cell like alacritty does; #545
(injecting the boundary set instead of hardcoding it) is the issue that would revisit it.

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

## Renderer ink channels

| Fact | Reference | Site |
|---|---|---|
| The strikethrough draws in the **glyph foreground**, never in the SGR 58 underline colour — confirming #525's premise | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:758-762` |
| ⚠ **The mechanism is a `save`/`restore` bracket, and the obvious grep hit says the opposite.** The underline block opens with `save()` (`:565`), sets `strokeStyle` from `getUnderlineColor()` (`:576-583`), then assigns `fillStyle = strokeStyle` (`:585`) — read alone, that says the SGR 58 colour becomes the fill for everything after it. `restore()` at `:688` undoes it, so the glyph `fillText` (`:735`) and the strikethrough's `strokeStyle = fillStyle` (`:762`) both get the foreground back | xterm.js | `TextureAtlas.ts:565`, `:585`, `:688`, `:735`, `:762` |
| ⚠ Path note: `TextureAtlas.ts` lives under `addons/addon-webgl/src/`, **not** `src/browser/renderer/shared/` as #525 cites | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts` |
