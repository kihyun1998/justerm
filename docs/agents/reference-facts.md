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
