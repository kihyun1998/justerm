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
| ⚠⚠ **The shrink path's cursor reconciliation — the ONLY reference site that *sets* a wrap flag from a reflow's output, and the true prior art for anything like `Cursor::set_reflowed_point`.** Two guards, and both are load-bearing: the column must equal the new width **exactly**, and the reflowed last cell must **not already be `WRAPLINE`**. Arming on a property of the *content* instead (justerm's first attempt used "the point's offset reached the line's end") is far wider and deletes hard line breaks: measured on `"abcd\r\nWXYZ"` resized 6→2, alacritty and that rule agree at cursor column 2 and 3 and disagree at 4 and 5. **This row exists because the site was not in this file and so was not read** | alacritty | `alacritty_terminal/src/grid/resize.rs:374-384` |
| The **saved** cursor is *clamped*, never re-armed — the line immediately after the block above | alacritty | `alacritty_terminal/src/grid/resize.rs:386` |
| Preserving a blank row costs **no destination row**: on `cols_len == 0` the reflow returns early and only bumps a counter (`if (!src_row.wrap_continuation) self.new_rows += 1; return;`). A port that materialises a real blank row instead pays for it out of the active area — and on a screen with no scrollback to absorb the displaced row (the alt screen) that is content *destruction*, not scrolling | ghostty | `terminal/PageList.zig:1610-1616` |
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

## Renderer ink channels

| Fact | Reference | Site |
|---|---|---|
| The strikethrough draws in the **glyph foreground**, never in the SGR 58 underline colour — confirming #525's premise | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts:758-762` |
| ⚠ **The mechanism is a `save`/`restore` bracket, and the obvious grep hit says the opposite.** The underline block opens with `save()` (`:565`), sets `strokeStyle` from `getUnderlineColor()` (`:576-583`), then assigns `fillStyle = strokeStyle` (`:585`) — read alone, that says the SGR 58 colour becomes the fill for everything after it. `restore()` at `:688` undoes it, so the glyph `fillText` (`:735`) and the strikethrough's `strokeStyle = fillStyle` (`:762`) both get the foreground back | xterm.js | `TextureAtlas.ts:565`, `:585`, `:688`, `:735`, `:762` |
| ⚠ Path note: `TextureAtlas.ts` lives under `addons/addon-webgl/src/`, **not** `src/browser/renderer/shared/` as #525 cites | xterm.js | `addons/addon-webgl/src/TextureAtlas.ts` |
