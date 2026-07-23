# justerm — architecture & contract

The detailed spec an implementer references. justerm is a **pure terminal engine**: bytes in →
terminal state → viewport/damage/scroll/selection out. No I/O, no IPC, no rendering, theme-agnostic.
See `CLAUDE.md` for the boundary invariants and `CONTEXT.md` for vocabulary. Key decisions with
rejected alternatives are in `docs/adr/`. This contract was grilled and cross-validated against prior
art (Mosh / Alacritty / Warp / VS Code / beamterm); the origin/rationale record is PenTerm's
`.scratch/rust-terminal-engine/PRD.md` (history only — this file is justerm's authoritative spec).

**What this file owns — and what it must not restate.** Only what the code cannot state for itself:
the consumer-facing *contract* (cadence, the wire format, damage/viewport semantics) and the terminal
behaviour that is not visible from the source because it is not implemented yet (§Hidden VT state).
The API *shape* belongs to `justerm-core/src/lib.rs` and its docs.rs page; the *rationale* for a
decision belongs to `docs/adr/`. Copying either into here produces a second, ungated version of
something another artifact already owns, and it drifts — which is exactly what happened to the API
list this file used to carry (theflow Step 6).

## Engine API

**Deliberately not listed here — read `justerm-core/src/lib.rs` or its
[docs.rs](https://docs.rs/justerm-core) page.** The compiler owns that shape and keeps it honest; a
prose copy has no gate. The copy that stood here is the cautionary case: it advertised
`viewport_snapshot(rows) -> Grid`, which has never existed in this repo, described `damage()` as an
iterator when it returns a `TermDamage` enum whose `Full` variant the shape could not express, and had
`hyperlink()` resolving a `Cell.link` field that moved to a per-row map in #45/#46 — all while omitting
`frame()`, the entry point this file's entire §Serialization is about, and ~25 other public methods.

The parts of the surface that are genuinely *contract* rather than signature are below and stay here:
the frame/damage cadence (§Cadence), the wire format (§Serialization), and the mode-gating that decides
what `encode_key` / `encode_mouse` / `encode_paste` / `encode_focus` emit (§Hidden VT state, "Input
encoding is mode-gated").

## Cell

Fixed-width record for fast typed-array decode. Two layers wear this name and they store the overflow
differently — say which you mean: the **in-memory** cell (`justerm-core`'s `Cell`, a packed
fixed-width value) and the **wire** cell record (§Serialization, 18 bytes). The fields below are the
model both share.

- `content`: a **grapheme cluster** (combining marks/emoji-correct). The primary code point is inline
  and the cluster overflow is kept out of the cell so it stays fixed-width — in memory via a presence
  bit (`COMBINED_PRESENT`) with the cluster in the row's column-keyed map (#45/#46), on the wire via a
  frame-local index into the grapheme side-table.
- `fg` / `bg`: **color references** — `Default | Indexed(u8 0..255) | Rgb(u8,u8,u8)`. **Never resolved
  hex** — the consumer/renderer maps indices→hex via its (frozen) scheme. Engine is theme-agnostic.
- `attrs`: standard 8 (bold/dim/italic/underline/blink/inverse/hidden/strikethrough). The record
  **reserves room** for underline style+color and an OSC 8 hyperlink id so adding them later is not a
  format change.
- `width`: 1 (normal) or 2 (wide / CJK fullwidth) — **derived, not stored**: it reads out of
  `flags & WIDE_CHAR` (the trailing column of a wide char carries its own spacer marker). Neither the
  in-memory cell nor the wire record spends a field on it.

## Damage = line + column span (+ scroll op)

Emit changed line ranges, each carrying the changed column span (`{line, left, right}` — Alacritty's
`LineDamageBounds` grain). **Not** full-frame (wastes IPC/idle power on small updates), **not**
cell-level (finer than terminals mutate; gratuitous). Scroll is a **first-class op** (shift rows + new
rows) so moderate scrolling moves content instead of redrawing; degrades to all-rows-dirty on
floods/resize. Damage is an *efficiency* axis, not quality — same pixels either way. The model
(incremental line+column bounds, **ack-gated** reset, a *recorded* — not diff-detected — scroll op)
and why not baseline-diff (Mosh) or per-line seqno (wezterm): **ADR-0003**.

## Viewport / scrollback / scroll

- The **engine** owns the full screen + scrollback ring + alt-screen + **scroll offset + follow-bottom**.
  The consumer sends scroll *intents* (wheel/page/jump-to-bottom); the engine resolves them to a window.
  ("new output while scrolled up — follow or stay?" is bound to an output event → must live here.)
- The consumer/renderer may cache a transient **overscan band** (viewport ± a screen) for instant
  small scrolls — a cache, not ownership; the engine stays authoritative.
- **Alt-screen** is an internal DEC mode; transparent — the engine emits whichever screen is current.

## Cadence — ack-paced state-diff (the consumer protocol)

The engine remembers the consumer's **last-acked screen state**; the diff it produces is
`last-acked → current` (line+column spans). The consumer applies a frame, then **acks**; the engine
sends the next diff only after the ack (≤1 in flight). Everything falls out of the last-acked baseline:
intermediate-state skip (a slow consumer's missed frames collapse into one diff), flow control (a slow
consumer gets larger diffs less often — never a pile-up, never discards), and pacing (the ack
round-trip is the collection interval, phase-aligned to the consumer's vsync). No separate timer.

**Boundary — the engine provides the diff, the consumer paces it (#13).** Cross-checked against Mosh
(SSP splits `diff_from(acked→latest)` from `calculate_timers`), Alacritty (damage state vs the render
loop), and xterm.js (`RenderDebouncer` coalesces dirty rows and flushes one frame per
`requestAnimationFrame`): the **state diff** (damage + scroll op + ack-gated reset) is engine-side and
already built in #4; the **pacing** (when to pull, RTT/vsync timing, ≤1-in-flight wire, retransmit)
lives in the consumer's transport (PenTerm's Tauri Channel) — NOT in justerm (CLAUDE.md: no IPC). So
#13's engine work is narrow: the viewport-vs-screen damage mapping above.

**Open question — viewport-vs-screen damage (carried over from #4 / ADR-0003).** Damage (#4) is
recorded against the *screen*, but the consumer renders the *viewport*. While scrolled up under
follow-bottom "stay" (#3), the screen scrolls yet the viewport is unchanged — so a screen scroll op
must NOT be applied to a scrolled-up viewport (it would shift a frozen view). The cadence work owns
mapping screen damage → viewport damage (and suppressing/ translating scroll ops while
`display_offset > 0`). Tracked in **#13** (cadence).

## Selection

Engine-owned. Type = char / word / line / **block**; anchor = point + **side (left/right)**.
`selection_range()` → highlight; `selection_text()` → copy text (respects type, wide chars,
wrapped-line joining, trailing-whitespace trim, **across scrollback** — the engine holds all cells).
Cursor blink is *not* an engine concern (consumer-local animation); the engine only reports cursor
position/style/visibility.

## Serialization (the wire format the engine offers)

Binary, **reference-based** (matches the Cell above — references, not RGB), little-endian,
**fixed-width 18-byte cell records** (`CELL_RECORD_LEN`; the field-by-field arithmetic is below) + a
grapheme side-table for rare multi-code-point clusters.
Designed for a consumer to ship over its own transport (e.g. a Tauri Channel) and decode straight into
typed arrays (a fixed stride → one contiguous view, no per-field parse). The engine provides the
*format* **and both directions** (`encode` a damage frame / `decode` it — the round-trip is the test);
*transport* is the consumer's job. Rationale — binary fixed-width vs Mosh's protobuf baseline-diff vs
xterm.js's escape-sequence re-emit (which a non-parsing GPU renderer like justerm-renderer cannot consume):
**ADR-0005**.

A **frame** serializes one damage cycle (`damage()` + `scroll_delta()`):
- **header** — magic, version, flags, `cols`/`rows`, cursor (`cursor_row`/`cursor_col` u16 +
  `cursor_visible` u8 — v3, #38; `cursor_shape`/`cursor_blink` — v4, #81), scroll position
  (`display_offset`/`scrollback_len` u32 — v5, #112/ADR-0013, for the consumer's scrollbar), the mouse
  wanted-events mask (`mouse_events` u8 — v8, #129/ADR-0016, the routing bits DOWN/UP/WHEEL/DRAG/MOVE the
  active tracking mode reports; the consumer routes a mouse event to the app vs. local on it), the
  alt-screen flag (`alt_screen` u8 — v9, #149, whether the alternate screen is active; the a11y announce
  policy #119 suppresses output reads on it), kind (`Full` | `Partial`).
- **scroll op** (optional) — `{top, bottom, count}` (ADR-0003); the decoder applies it *before* the spans.
- **spans** — for `Partial`, each `{line, left, right}` then `(right−left+1)` cell records; `Full` = all rows.
- **side-table** — only the clusters referenced *this frame*, renumbered frame-local; each cell's
  `extra` rewritten to the local index.
- **overlay** (v6, #108/ADR-0014; v7, #118/ADR-0015) — interaction state as *viewport* coordinates, five
  groups: a selection-span group then a search-match-span group (each a `u16` count + `(row, left, right)`
  `u16` triples), then a marker group (`u16` count + `(marker_id u32, row u16)` pairs; v10, #159, appends a
  kind discriminant `u8` and — for `CommandFinished` — a presence byte + `i32` exit), then a marker-lines
  group (`u16` count + `(marker_id u32, line u32)` pairs — v11, #120 S3, every live marker's *absolute*
  buffer line for the overview ruler), then an active-match-span group (same count + triple shape — v12,
  #428, the consumer-designated *current* search match; usually it also stays in the match group and the
  renderer's highlight ranking resolves the overlap #424 — a span-designated past-cap match rides this
  group alone, #436). Positions only (colour is the consumer's); `frame()`
  re-projects them against the scroll offset, the single anchoring authority. Append-only. Highlights are
  projected from the engine-owned selection + the consumer-supplied search set (the active one designated
  by index, `set_active_search_highlight`, or by absolute span, `set_active_search_match` #436 — the
  past-cap path); markers are persistent line anchors re-anchored like the
  selection — their *disposal* rides the event queue (`TermEvent::MarkerDisposed`), not the frame, so
  absence here means off-screen, not gone.

The **cell record** (little-endian): `c` (u32 Unicode scalar — *not* the renderer's atlas glyph
id), `fg`/`bg` (u32 each = tag byte `Default|Indexed|Rgb` + 24-bit payload; the tag is mandatory so
`Default ≠ Indexed(0) ≠ Rgb(0,0,0)`), `flags` (u16, incl. layout markers), `extra` (u16 frame-local
grapheme index, `0` = none), and — since wire **v2** (#26) — `link` (u16 frame-local hyperlink index,
`0` = none) = **4+4+4+2+2+2 = 18 bytes**, 2-aligned. Width is derived from `flags & WIDE_CHAR`. The
hyperlink id was added exactly as the format promised — a **versioned** addition with its own index +
side-table (`link_table`), never an overload of a live field; the `VERSION` byte gates it. **Underline
colour** (SGR 58, #520) follows the *same* path, not the spare-bits one: a full `Color` reference is
26 bits — too big to ride `flags` — so it is stored engine-side in a per-row map like the hyperlink
(gated by a `UCOLOR_PRESENT` cell bit) and will reach the wire as its own versioned group (core landed
first; the wire group is a later slice, so the 18-byte record above is unchanged for now). What the
`flags` bits 11–15 still genuinely reserve is the underline **style** (single/double/curly/dotted) —
a small enum that *does* fit spare bits — plus the colour tags' spare 6 bits.

## Hidden VT state — model these (and grow this list)

A correct-*looking* model (cell + cursor + advance + wrap) silently omits subtle state real terminals
track. These are invisible from first principles / this contract — only a reference impl (`vte` /
alacritty / xterm) or vttest reveals them. **Before implementing any VT-semantics slice (#2, #3, #4,
#6, #7, #8, #10): read how a reference terminal handles that area and enumerate the hidden state (flags,
deferred behavior) it tracks — then add what you find here.** Seeds (caught in #2 review, 2026-06-16):

- **Pending-wrap (deferred last-column wrap).** Printing into the last column does *not* advance to
  the next line — the cursor stays put with a `wrapnext` flag, and wrap happens on the *next* print.
  Eager wrap is a classic off-by-one bug (lines shift). [#2]
- **Wide-char spacer is a distinct marker, not a blank.** The trailing column of a width-2 char must
  carry a "wide-char spacer" marker (flag/variant), not a plain blank — else overwrite, erase,
  selection, and cursor positioning go wrong. [#2]
- **Background Color Erase (BCE).** Erase (ED/EL) fills cleared cells with the *current SGR
  background*, not default. [#8; note in #2 if deferred]
- **Cursor-move damage (the previous cursor cell is hidden damage).** Moving the cursor changes *no
  cell content*, so a content-only damage model records nothing — yet the rendered output changed. How
  the cursor is *drawn* is the renderer's choice: justerm-renderer draws it as a **native overlay**
  (#270), while a renderer with no cursor primitive would **cell-invert** (swap fg/bg on the cursor
  cell). The engine can't assume either, so it treats a cursor move as damage to **both the old and new
  cursor cells**: with incremental (`Partial`) damage a cell-invert renderer would otherwise leave the
  old cell inverted (a ghost) and the new cell un-inverted. Alacritty tracks this as
  `TermDamageState::last_cursor` (damages the cell the cursor left + the cell it lands on). This is
  damage-layer hidden state a "cursor is just (row, col)" model omits; it is the engine's job (it owns
  damage), *not* "drawing" (which stays the renderer's). [#38]
- **Tab stops are explicit per-column state, not a fixed modulo.** A bool-per-column set: HTS
  (ESC H) sets a stop at the cursor, TBC (CSI g) clears one (param 0) or all (param 3), and HT
  advances to the next *set* stop — or the last column if none remain (no wrap). Default = every
  8th column (incl. col 0). Resize must re-init/extend the set (#7). [#8]
- **Scroll region (DECSTBM) redefines what "scroll" means.** top/bottom margins (0-based,
  inclusive) stored as state; a line-feed at the *bottom margin* scrolls only rows `[top..=bottom]`,
  leaving rows outside fixed — `linefeed` must consult the margins, not the screen edge. DECSTBM
  homes the cursor (absolute (0,0); origin-relative under DECOM, a later slice), ignores an invalid
  region (top ≥ bottom), and defaults to the full screen. A line-feed below the region just
  descends; no scroll happens outside it. [#8]
- **IND / RI (ESC D / ESC M) scroll at the margins.** IND moves the cursor down — at the bottom
  margin it scrolls the region up (a line-feed without the carriage return). RI moves up — at the
  *top* margin it scrolls the region *down* (a blank line appears at the top, the bottom region line
  is lost). Off the margin, each just moves the cursor. [#8]
- **Alt-screen is a second grid swapped in (DEC 1049).** `?1049h` saves the cursor and swaps to a
  fresh (cleared) alternate grid; `?1049l` swaps back and restores the cursor. Guarded so a
  double-enter/leave is a no-op. The alt screen has no scrollback, and tab stops + scroll margins are
  *not* per-screen — they persist across the swap. The engine emits whichever grid is active; the
  switch is transparent to consumers. DEC private modes arrive as a `?` in the CSI `intermediates`. [#8]
- **Anchor lifecycle: selection/marker anchors are absolute-line coords shifted in lockstep with
  buffer mutation — and share the alt grid's line range.** Selection endpoints (#3) and decoration/
  command markers (#118/#158) store `[scrollback ++ screen]`-absolute lines. Eviction
  (`markers_evict_oldest`), region scroll (`markers_rotate_region`), and reflow (`iter_mut` over
  `m.line`) shift them so they track content; an anchor on a dropped line is disposed. **Hazard: on the
  alt screen the primary anchors are *retained* (that is the #118/#158 contract — a mark must survive a
  vim/less excursion), yet the alt grid occupies the *same* absolute-line range `[scrollback.len(),
  +rows)`. So an alt-screen scroll must NOT rotate primary markers or it silently disposes them.** The
  selection dodges this only because it is *cleared* on alt enter; markers are guarded explicitly
  (`if !self.on_alt` around `markers_rotate_region` in `linefeed`/`reverse_index`). [#158]
- **Anchor rotation gap (tracked): the CSI line-editing verbs don't move anchors.** Only
  `linefeed`/`reverse_index` rotate anchors today; `scroll_region_lines` (SU/SD/IL/DL) moves content
  via `grid.scroll_*_region` + `record_scroll` but calls *neither* `markers_rotate_region` nor
  `selection_rotate_region`, so a primary-screen IL/DL (zsh/fish multi-line prompt redraw, completion
  menus) leaves marks + live selection pointing at the wrong line. Pre-existing (selection has it too);
  tracked in **#162**. [#158]
- **OSC 133 shell-integration marks: only `A/B/C/D` are parsed.** `133;A` prompt-start, `;B`
  command-start, `;C` output-start, `;D[;exit]` finished → a kinded marker at the cursor line; the exit
  field parses to `i32`, else `None` (matching VSCode's FinalTerm handler, safer than WezTerm's
  `unwrap_or(0)` false-success). Suppressed on the alt screen (marks anchor primary content). Pairing
  A↔D, prompt-to-prompt navigation and success/fail earcons are *consumer* policy (#160), not core.
  Tracked long-tail: WezTerm also recognizes `133;L/I/N/P` (fresh-line + B/D/A variants); VSCode ignores
  them too, so they are a deferred zero, not a silent one. [#158, #160]
- **Origin mode (DECOM ?6) makes cursor addressing region-relative.** When set, CUP/HVP (`goto`) is
  relative to the scroll region's top margin and clamped to the bottom margin; the column is
  unaffected. Setting DECOM homes the cursor to the region top; *unsetting* it leaves the cursor put
  — an xterm/alacritty asymmetry we follow (ADR-0001 gold reference), noting xterm homes on both. [#8]
- **Scrollback accrues only on a top-anchored, primary-screen scroll.** A line scrolled off enters
  history *only* when `scroll_top == 0` and not on the alt screen — NOT merely "the full screen". A
  top-anchored sub-region (`[0..k]`) still accrues; a region with `scroll_top > 0`, the alt screen,
  and reverse-index (scroll *down*) never do (verified against alacritty `region.start == 0`). The
  **explicit line-editing verbs (SU/SD/IL/DL via `scroll_region_lines`) also never accrue** — even a
  full-screen SU (`scroll_top == 0`) drops its top line rather than pushing it to history; justerm
  matches xterm.js here (which carries a `FIXME` to accrue) and *trails* real xterm/alacritty, which
  rotate the SU top line into scrollback. Consequence for anchors (#162): a marker/selection on that
  dropped edge is disposed/cleared, not shifted into history — the anchor rotation is deliberately
  *consistent* with whatever the grid does, so revisiting SU-accrual would move the anchor path too. The
  viewport windows into history via a `display_offset` clamped to `[0, history.len()]`. New output
  while scrolled up (`display_offset > 0`) **stays** put — the offset is bumped to hold the view, not
  yanked to the bottom (alacritty/xterm.js follow-bottom). History is a flat line ring; semantic
  grouping (Warp's command "blocks") is a *consumer* concern above the engine, never in it. [#3]
- **The per-newline scroll cost is the eviction's alloc/copy, not the row shift — recycle the row
  buffer (no ring).** `#41` profiled `feed` as the dominant flood cost and blamed `scroll_up_region`'s
  `rotate_left`. That was a **misdiagnosis** (ADR-0009 amendment): `lines: Vec<Row>` with `Row =
  Vec<Cell>`, so `rotate_left` moves 24-byte `Vec` *handles*, not cell data, over a *bounded* ~24–100
  screen rows (scrollback is a separate `VecDeque`) — sub-microsecond, never the bottleneck. The real
  cost was `Term::linefeed`'s eviction: `grid.row(0).to_vec()` (copy ~2 KB + **allocate**) every line,
  plus an alloc/free **pair** every line once scrollback is at its cap (a flood is at cap throughout).
  Fix: **move + recycle**, keeping `rotate_left`. `Grid::scroll_up_recycle(blank: Row) -> Row` rotates,
  then swaps a caller-supplied `blank` into the bottom slot and returns the evicted top row by **move**
  (no copy); the grid clears + fits `blank`, so a *dirty* recycled buffer is safe. `Term` parks the
  cap-`pop_front`ed row in a `recycled_row` spare and feeds it back as the next `blank` → **zero per-line
  alloc/copy** in steady state (xterm.js `recycle`). Hidden state to get right: **scrollback-accrual
  (`scroll_top == 0`) and the recycle handshake (`scroll_bottom == rows-1`) are distinct predicates** —
  a top-anchored *sub-region* (`[0..k]`) still accrues but keeps the copy + region scroll (it must scroll
  only its rows); only the *full-screen* case uses the handshake. Region scrolls and RI / `scroll_down`
  never accrue scrollback and stay plain in-region `rotate`. `record_scroll`/damage are in logical
  coordinates (rows never leave logical order), so `DecodedFrame` is identical (no `WIRE_VERSION` bump).
  *(An in-Grid **ring** — `zero` offset, O(1) scroll — was built first and **measured as a net
  regression**: it optimized the already-free `rotate_left` while taxing every cell access with a `phys()`
  mapping; reverted in `1fa3b14`. ADR-0009 amendment has the numbers. Lesson: profile the *kind* of cost
  before assigning a Big-O.)* [#41]
- **Soft-wrap (WRAPLINE) vs a hard line-end must be distinguished for reflow.** An auto-wrap (the
  deferred last-column wrap firing) marks the row it leaves as *soft-wrapped* — a `WRAPLINE` flag on
  its last cell (Alacritty's encoding; xterm.js instead flags the continuation row). An explicit
  CR/LF/NEL ends the line *hard*. Reflow (#7) merges soft-wrapped rows into one logical line and
  re-splits at the new width; without this flag every line looks identical and reflow corrupts
  content. [#7]

- **Selection coordinates are absolute-from-oldest, and only three events move them.** Anchors are
  stored as a line index into `[scrollback ++ screen]` counted from the oldest line — NOT viewport
  rows (those drift under new output). This index is *invariant* under a normal top-anchored scroll:
  the line evicted into scrollback grows `scrollback.len()` by exactly the screen shift, so existing
  content keeps its index (verified against the existing `display_offset` model, which bumps in
  lock-step). The index moves only on (a) **cap eviction** (`pop_front` → decrement anchors, clamp
  off-top), (b) **in-screen region/RI scroll** with `scroll_top > 0` or alt (rotate anchors within the
  region; an endpoint on the dropped line clears the selection — top-anchored scroll must NOT rotate),
  and (c) **resize reflow** (anchors reflow through `grid::reflow` alongside the cursor — it tracks N
  points). Alt enter/leave clears the selection (it is primary-only). [#5]
- **Selection text vs highlight need different grains.** `selection_text` joins soft-wrapped rows into
  one logical line and trims trailing blanks *only at the logical end* (spaces at a wrap boundary are
  real content), skips `WIDE_CHAR_SPACER` cells (emit the lead glyph once), and ends hard lines with
  `\n`; Block extracts each row independently. `selection_range` instead projects onto *viewport* rows
  (clipping off-screen parts) as inclusive column spans for the renderer. [#5]
- **The soft-wrap run walk is intentionally unbounded (scrollback-bounded), not capped.** `search`,
  `viewport_logical_lines`, and word-selection assemble a `WRAPLINE` run into one logical line with **no
  per-run length cap** — bounded only physically by the scrollback cap (`O(scrollback)` per call, never
  infinite). This is a deliberate completeness/a11y choice: an edge-spanning URL wrapped across many rows
  still matches (link detection), and the a11y view reads the whole logical line (#119). This *matches*
  xterm's structure (verified against real source): its **search** wrap-assembly
  (`SearchLineCache.ts::translateBufferLineToStringWithWrap`, a `while (isWrapped)` walk) is **uncapped**
  too — its only cap is a 1000-*result* count (`SearchAddon.ts`), and search does a *literal* match (no
  ReDoS). The 2048-char/direction + whitespace-stop cap lives **only in the link provider**
  (`WebLinkProvider.ts::_getWindowedLineStrings`), because *that* path runs a URL **regex** over the
  assembled text. So the bound belongs with the **regex-runner**, not the buffer walk. In justerm link
  detection is the consumer's job (ADR-0017), so if the pathological single-multi-KB-line case ever
  bites, the fix is the **consumer** capping its own regex input — exactly where xterm puts it — not a
  core cap. Deferred until profiling shows it matters. [#206]

- **Editing CSIs are BCE-filled and region/line-scoped — and must not orphan a wide-char half.**
  ICH (`@`, insert blanks), DCH (`P`, delete chars), ECH (`X`, erase chars) operate *within the
  cursor's line*; IL (`L`), DL (`M`) insert/delete whole lines; SU (`S`)/SD (`T`) scroll the region.
  All fill newly-blanked cells with the current SGR background (BCE), default param 1. IL/DL are
  **region-gated**: they act only when the cursor is inside the scroll region and scroll
  `[cursor_row..=scroll_bottom]` — a no-op when the cursor is outside (Alacritty's
  `scroll_region.contains(origin)` gate). SU/SD are keyed to the region *top*, cursor-independent.
  **None reset pending-wrap.** ICH/DCH shift cells and so can split a width-2 glyph at the boundary —
  unlike Alacritty (which ignores this), justerm clears the orphaned lead/spacer to keep the repo's
  no-orphan wide-char invariant (the same rule `clear_cells`/`write_glyph` already enforce), because
  selection's spacer-skip and the renderer both assume a spacer always has a lead to its left. [#8]
- **DECSC/DECRC save set includes origin mode; DECRC restores it.** `ESC 7`/`ESC 8` (and the
  `CSI s`/`CSI u` aliases) save and restore the cursor: position, pen/SGR, **origin mode (DECOM)**,
  and pending-wrap. Alacritty omits origin mode from its saved `Cursor`; justerm follows the DEC/xterm
  spec and restores it (charsets join the set when a charset slice lands). The general tie-break —
  Alacritty on genuine ambiguities, the spec where Alacritty merely omits a mandated behaviour — is
  **ADR-0004**. [#8]
- **A combining mark (width-0 code point) attaches to the previous base cell, not its own cell.**
  `print` must not drop a width-0 char (the current #2 behaviour). It appends to the cell the cursor
  just left: back up one column, and if that cell is a `WIDE_CHAR_SPACER` back up once more to the
  lead. The exception is pending-wrap — there the cursor still sits *on* the just-written last-column
  glyph, so the mark attaches in place without backing up (and without firing the deferred wrap). The
  extra code points live in a side-table referenced by a **1-based index in the cell** (`Cell.extra:
  Option<NonZeroU32>`), not a boxed list on the cell: this keeps `Cell` `Copy` — which the grid relies
  on (`copy_within` for ICH/DCH, reflow's `to_vec`), and which Alacritty's `Option<Box<CellExtra>>`
  would forfeit — and matches #6's index-referenced serialization more directly. The index travels
  with the cell through scroll/shift/reflow (it is plain data). Trade-off: a cell overwritten or reset
  drops its index, leaking a dead side-table entry (rare — only combining-mark cells; compactable on
  resize, a common-90% deferral). `selection_text` appends the marks after the base char; #6 encodes
  the side-table. Per-codepoint width means a true multi-emoji ZWJ sequence still splits at each
  width-2 glyph (a grapheme segmenter is a later slice); the ZWJ/VS code points themselves attach. [#8]

- **A wide-char's two halves both serialize, as flagged cells — never dropped, never a plain blank.**
  A span covering a width-2 glyph encodes *both* cells: the `WIDE_CHAR` lead (carries `c`) and the
  `WIDE_CHAR_SPACER` trailer (blank `c`, spacer flag). The consumer places one glyph across two columns
  and must know the trailer column is *owned* (cursor math, overwrite, selection). A column-bounded
  damage span can also *bisect* a glyph — start on a spacer or end on a lead whose partner is outside
  the span; cells ship as-is and the consumer's mirror already holds the partner from a prior frame, so
  the half is unambiguous against that mirror (do not "fix up" by widening the span). [#6]
- **Combining clusters live in a per-row, column-keyed map — a flag-gated cache, never read without the
  cell's `COMBINED_PRESENT` bit.** A `Row` carries a `BTreeMap<col, marks>` alongside its cells (#45,
  blueprint xterm.js `BufferLine._combined`); the cell holds only the presence bit, not an index. The map
  rides with the row through scroll/scrollback/reflow for free (the row is the unit that moves) and is
  cleared on row reuse, so there is **no global pool and no leak** (the old `grapheme_pool` did grow
  unbounded — an overwritten combining cell orphaned its slot). The load-bearing invariant: `map[col]` is
  read iff the cell at `col` is combined, which makes a stale entry (left by an overwrite/erase) harmless.
  Cells move by raw copy (the bit travels, the map does not), so the live entries must be **carried
  explicitly only where cells change column**: ICH/DCH re-key the map alongside the cell shift, and reflow
  re-keys per column when splitting/merging rows (xterm's `_copyCellMapsFrom`); print-overwrite, erase, and
  whole-row scroll need nothing. Serialization gathers each combined cell's cluster into the frame-local
  `side_table`, recording the index on the **span** (`Span.combining`, the per-cell `extra` lifted out of
  the cell); the wire bytes are unchanged. [#6, #45]
- **OSC 8 hyperlinks ride the *same* per-row-map machinery, gated by the `LINK_PRESENT` bit.** The `Row`
  carries a second `BTreeMap<col, hyperlink-pool index>` (the URI dedup pool, `hyperlink_pool`, stays
  global — only the per-column reference moved), gated by the cell's `LINK_PRESENT` bit, which reuses
  xterm's `BgFlags.HAS_EXTENDED` (`0x10000000`, bg bit 28) **exactly**. Carry/reflow/recycle treat it
  identically to combining (`Row::move_maps` re-keys both maps together; reflow threads both). Reads go
  through `Engine::link_at(row, col)` / `viewport_link_at` (the link is no longer on the `Cell`); the
  decoded index rides `Span.links`. With this `Cell` is **12 bytes** — three packed `u32`, no `Option`
  field (the #43 epic target, matching xterm.js's `BufferLine` cell). [#26, #46]
- **Underline colour (SGR 58) rides the *same* machinery — a third per-row map, gated by its own
  `UCOLOR_PRESENT` bit (#520).** A cell that draws a coloured underline stores a `Color` reference in
  `Row`'s `BTreeMap<col, Color>`, gated by bg bit 29. Carry/reflow/recycle/`move_maps` thread it exactly
  like the link and combining maps; read through `Engine::underline_color_at(row, col)` (`Color::Default`
  = follow the fg). **Where justerm diverges from xterm:** xterm's `HAS_EXTENDED` is a *shared* gate
  holding link **and** underline colour/style in one `ExtendedAttrs` object; justerm keeps a **separate
  map per concern** (as combining and links already are), gating each with its own bit — so the maps must
  be threaded in lockstep across every op (the coherence the shared object gives xterm for free). The
  colour is stored only where an `UNDERLINE` attribute is present (inert otherwise, and xterm likewise
  does not persist it) and does not yet reach the wire — a later slice. [#520]
- **The scroll op is recorded (not diff-detected), screen-relative, and ordered before the spans.**
  Per ADR-0003 the frame carries `{top, bottom, count}` *ahead of* the damage spans; the decoder shifts
  its mirror grid first, then applies spans — reversing the order lands spans on pre-scroll rows. #6
  serializes **screen** damage only; the screen→viewport *remap* (suppress/translate scroll while
  `display_offset > 0`) is the consumer/cadence concern in #13. The scroll *position* itself
  (`display_offset` + `scrollback_len`) **was** out of scope until a consumer needed it — the scrollbar
  (#112) did, so v5 now carries it in the header (ADR-0013); the remap logic stays #13's. [#6]
- **Colour needs an explicit tag in bytes; `Default ≠ Indexed(0) ≠ Rgb(0,0,0)`.** A "zero means
  default" packing collides with ANSI black (`Indexed(0)`) and true black (`Rgb(0,0,0)`). Each of
  `fg`/`bg` ships a tag + payload so the consumer's frozen-scheme resolver picks default vs palette vs
  truecolour. This is the theme-agnostic invariant projected into the wire: the engine ships the
  *reference*, never the resolved hex. [#6]
- **`flags` mixes SGR attrs with layout markers, and `c` is a codepoint — the consumer must split
  both.** The record ships the raw `CellFlags` u16: bits 0–7 (bold…strikethrough) map to the renderer's
  style/effect, bits 8–10 (`WIDE_CHAR`/`SPACER`/`WRAPLINE`) are *layout*, not font style (a renderer
  that packs bold/italic/underline into a glyph id would corrupt it if fed `WIDE_CHAR`).
  Likewise `c` is the Unicode scalar; mapping codepoint → atlas glyph id is the renderer's job, so the
  engine stays font/atlas-agnostic and reusable beyond any one renderer. [#6]
- **Empty, Partial, and Full are three distinct frames the ack cadence needs.** "Nothing changed since
  the ack" is a valid frame (0 spans, no scroll) so the consumer can ack without redraw — *not* the
  absence of a frame, and *not* `Full`. `Full` (resize / alt-screen clear) ships every row. Conflating
  empty with "skip" or with `Full` breaks the ≤1-in-flight ack loop (§Cadence). [#6]

- **Input encoding is mode-gated, and the modes are hidden state the engine owns.** `encode_key` /
  `encode_mouse` / `encode_paste` are the inverse of `feed`: a consumer event → the bytes an app
  expects, decided by DEC modes the engine tracks from the *output* stream. (a) **App cursor keys
  (DECCKM `?1`)**: when set, the cursor keys and Home/End encode as **SS3** (`ESC O A`); when reset, as
  **CSI** (`ESC [ A`). The catch: a key carrying *any* modifier always uses the **CSI `1;<mod>` form**
  regardless of DECCKM (xterm: "if the original did not start with CSI, the start is changed to CSI" —
  except keypad). Modifier param = `1 + (shift 1 | alt 2 | ctrl 4 | meta 8)`. (b) **Mouse** is two
  orthogonal axes: a **tracking mode** deciding *what* reports (`?1000` press+release, `?1002` adds
  motion-while-pressed, `?1003` adds all motion — so `encode_mouse` returns `None` for a bare move
  under `?1000`) and an **encoding** deciding *how* (`default` X10 `CSI M Cb Cx Cy` with each value
  `+32` — which **breaks past column 223** — vs `?1006` **SGR** `CSI < Cb;Cx;Cy M/m`, where final `M`
  is press/motion and `m` is release, coords unbounded). Coords are **1-based** in both; the button
  byte packs button low bits + motion `+32` + wheel `64` + modifiers (shift 4 | meta 8 | ctrl 16);
  default encoding has no separate release code (button 3 = "released"), SGR distinguishes via `M`/`m`.
  Three further encodings (#28) are stateless `encode_mouse` arms on the same `Cb`: `?1015` **urxvt**
  (`CSI Cb;Cx;Cy M`, the default `Cb` semantics as decimal params, always `M`), `?1005` **UTF-8**
  (default `CSI M` framing but each value UTF-8-encoded to pass the 223 ceiling), and `?1016` **SGR-pixels**
  (SGR framing but the coordinates are the consumer-supplied **pixels** in `MouseEvent::px`/`py` — the
  engine only formats them, it never computes pixels, so the boundary holds). `?1001` hilite tracking is
  excluded — a stateful interactive handshake, not a stateless encoding, with ~0 real usage.
  (c) **Focus reporting (`?1004`)**: emit `CSI I` on focus-in, `CSI O` on focus-out — only when set.
  (d) **Bracketed paste (`?2004`)**: wrap pasted text in `CSI 200~`…`CSI 201~` so the app never
  mistakes paste content for typed control sequences (a real injection-safety boundary, not cosmetic).
  (e) **Backspace is DEL (`0x7f`), not BS (`0x08`)** — the standard PC-keyboard convention apps assume.
  The kitty keyboard protocol (`CSI u` + a negotiated progressive-flag stack + key-release events) is a
  *stateful* superset deferred to #23; legacy here is a pure event→bytes function. (`?1016` SGR-pixel
  mouse — once mistakenly called out-of-bounds — is in scope: the consumer supplies the pixels, the
  engine only formats them; landed in #28. The genuinely-excluded mode is `?1001` hilite tracking, a
  stateful handshake, not an encoding.) [#11]
- **The kitty keyboard protocol is a negotiated flag stack that rewrites only what legacy can't express.**
  An app enables it via `CSI > flags u` (push the current flags, set new), `CSI = flags ; mode u` (set in
  place — mode 1 replace / 2 or-in / 3 and-not), `CSI < n u` (pop n), and queries with `CSI ? u` → the
  engine replies `CSI ? flags u` on the #27 channel. These route by their leading `>`/`=`/`<`/`?`
  intermediate, so a plain `CSI u` stays SCORC. The stack is depth-capped (oldest dropped). The five
  progressive flags gate `encode_key`: bit0 disambiguate, bit1 report-events (repeat/release), bit2
  alternate-keys (`codepoint:shifted:base`), bit3 all-as-escape (printable chars → `CSI u`), bit4
  associated-text (`…; text` codepoints). The load-bearing rule: kitty **only changes what legacy
  cannot express** — a plain unmodified press stays legacy; the `CSI u`/extended form appears only for a
  modifier legacy can't carry, a release/repeat event, or an ambiguous key. The per-key exceptions are
  spec-verified, not guessed: **Escape** disambiguates even unmodified (it introduces sequences), but
  **Enter/Tab/Backspace stay legacy** (the documented exceptions); functional keys (arrows/nav/F1–F12)
  keep their legacy terminator (`A`…/`~`) and only gain the `;mods:event` parameter. Modifiers carry the
  **kitty bit scheme** (Super=8/Meta=32/… — the superset), so `csi_param` remaps to the legacy
  Shift1/Alt2/Ctrl4/Meta8 while `kitty_param` uses the bits directly. The exotic functional keys
  (F13–F35, keypad, media, lock, modifier-as-key) are **deferred**: they need a `Key`-enum expansion the
  consumer must drive, have no dogfood (encode is inbound — no capture exercises it), and even the engine
  library `alacritty_terminal` does no key encoding at all. Verified against a real neovim+kitty session
  capture (`tests/fixtures/neovim_kitty.raw`). [#23]

- **Consumer events are pull-drained, and OSC 8 is not one of them.** Title (OSC 0/2), bell (BEL), and
  cwd (OSC 7) are point-in-time notifications: the engine queues them during `feed` and the consumer
  takes them via `drain_events` (emptying the queue — the pull counterpart to an ack). No callback is
  injected across the boundary — unlike alacritty's `EventListener` push model, which would couple the
  engine to the consumer's event loop and break the "feed in, pull out" symmetry. OSC 8 hyperlink is
  deliberately excluded: a hyperlink applies to *subsequently printed cells* until closed, so it is
  per-cell state (modelled like a grapheme side-table, versioned into the wire), not an event — its own
  slice (#26). OSC string terminator may be BEL or ST; vte consumes it and calls `osc_dispatch` once, so
  an OSC-terminating BEL is not double-counted as a bell. [#12]
- **An OSC 8 hyperlink is ambient pen-like state stamped onto cells — not an event, and not closed by
  an SGR reset.** `OSC 8 ; params ; URI` opens a link (the URI is interned into a `hyperlink_pool` and
  becomes "current"); `OSC 8 ; ; ` (empty URI) closes it. Every glyph printed while open carries a
  `Cell.link` index into the pool — both halves of a wide glyph, so a hover/selection over either
  agrees. The index is plain `Copy` data, so it rides the cell through scroll/scrollback/reflow exactly
  like the grapheme `extra`, and it renumbers frame-local into the wire's `link_table` the same way.
  The catch: a hyperlink is **orthogonal to SGR** — `CSI 0 m` (reset attributes) must *not* close it;
  only an empty-URI OSC 8 does (and it persists across line-feeds until then). It is cell state, not a
  point-in-time event, which is why it is here and not on the `drain_events` surface (alacritty agrees —
  hyperlink is a Cell attribute, not an `Event`). The OSC 8 `id=` param (multi-line link grouping) is a
  later refinement; the common-90% interns one pool entry per open. [#26]

- **Query replies are an outbound channel, drained pull-style and kept apart from events.** An app
  query (`CSI c` DA1, `CSI 5n`/`CSI 6n` DSR, `CSI ? Ps $ p` DECRQM) makes the engine *produce bytes the
  consumer must write back to the PTY* — justerm's first "engine → app" path. They queue during `feed`
  and the consumer takes them via `drain_replies` (raw `Vec<u8>`), separate from `drain_events` (typed
  notifications → UI; replies → PTY). This is alacritty's push `Event::PtyWrite` translated to justerm's
  pull cadence; xterm.js instead unifies replies with key output into one `onData` stream — justerm does
  not, because `encode_*` is a *synchronous* consumer-driven call while a reply is an *async* side-effect
  of parsing. Catches: **DA1 must advertise only what the engine implements** (`CSI ? 62;22 c` = VT220 +
  ANSI colour, not Sixel/printer it lacks — a lying DA makes apps call absent features); **DSR cursor
  position is region-relative under origin mode** (DECOM), 1-based; an unrecognised query emits *nothing*
  (no spurious bytes). The kitty `CSI ? u` query (#23) reuses this channel. [#27]

- **RIS and DECSTR are two reset strengths, and the split is the hidden state.** **RIS** (`ESC c`, full
  reset) is power-on reinitialisation: every screen/mode field to its default — clear screen + alt,
  **clear scrollback**, `display_offset` 0, primary screen, tabs default, cursor home, *all* modes (mouse
  tracking/encoding, focus, bracketed paste, app-cursor-keys, origin, autowrap, insert), charsets default.
  Implemented as a **reconstruct preserving only (cols, rows, scrollback_limit)** — so new state added
  later is reset for free — but it must (a) **preserve the `replies`/`events` queues** accrued earlier in
  the same `feed` (consumer-bound output, not terminal state) and (b) **signal full damage** (a fresh
  reconstruct has none, so the consumer would not repaint the blanked screen). The vte parser lives
  outside `Term`, so replacing `self` is safe. **DECSTR** (`CSI ! p`, soft reset) is a *subset* that does
  **not** destroy content: cursor-visible on, scroll margins full, SGR default, saved-cursor (DECSC) home,
  charsets default, and origin/app-cursor-keys/bracketed-paste/insert **off** — but it pointedly does
  **NOT** clear the screen/scrollback, move the *active* cursor, or reset mouse/focus tracking (so a stuck
  mouse is recovered only by RIS, never DECSTR). The load-bearing detail, source-verified against
  xterm.js (`CoreService` default `wraparound: true // xterm - true, vt100 - false`): **DECSTR sets
  autowrap back ON**, contradicting the VT510 manual's "no autowrap" — follow xterm. [#53]

- **VT52 mode (DECANM ?2) is a second escape *dialect*, mode-gated — not a second parser, and `ESC Y`
  coordinates are hidden state.** Resetting DECANM (`CSI ?2l`) enters the pre-ANSI VT52 dialect; `ESC <`
  returns to ANSI (default). Neither xterm.js (marks ?2 `#N`, no `case 2`) nor alacritty (no `vt52` at
  all) implement it, so the authority is the xterm `ctlseqs` "VT52 Mode" section + the DEC VT100 manual.
  Every VT52 sequence is `ESC <final>`, which vte already tokenizes, so VT52 is a **`vt52_mode` flag that
  re-routes `esc_dispatch`** into VT52 meanings (`A/B/C/D`=cursor, `H`=home, `I`=reverse-LF, `J/K`=erase,
  `Z`=identify→reply `ESC / Z`, `=`/`>`=keypad→`application_keypad`, `<`=exit, `c`=RIS), **not** a
  pre-vte sub-parser — a sub-parser would force byte-at-a-time vte feeding to catch mid-`feed` mode flips
  and re-own the ESC state machine vte already owns (ADR-0001). The load-bearing hidden state is **`ESC Y
  row col` direct addressing**: vte dispatches `Y` as a final and returns to ground, so the two
  coordinate bytes arrive as ordinary **`print` calls**, *not* part of the escape sequence. A 2→1→0
  `vt52_y_pending` counter (with `vt52_y_row` parking the first) consumes them in `print` before they
  would be drawn; each byte decodes as `value - 0x20` (so coords are always ≥ `0x20` — printable, never a
  C0 control routed to `execute`), and `goto` clamps out-of-range coordinates. The state lives on `Term`,
  so it survives `feed` boundaries (coords may split across calls) for free. RIS is honored *inside* VT52
  (`full_reset` rebuilds `Term`, clearing `vt52_mode`) so an app can always escape back to ANSI; DECRQM
  ?2 reports `!vt52_mode` (DECANM *set* = ANSI). Non-goal in the first cut: graphics `ESC F`/`ESC G` are
  no-ops — the VT52 graphics glyph set differs from DEC Special Graphics, so reusing that charset would
  render *wrong* glyphs (approximate-but-wrong is worse than an explicit non-goal). [#84]

The *systematic* catch for this whole class is #8's vttest harness + dogfood — this list is only the
famous few caught by review. Pull vttest early so VT-semantics slices verify against it from the start.

### Where to look (reference impls — grep symbols, not line numbers)

External paths drift; **symbol/flag names don't** — grep these in a fresh checkout rather than
trusting a path.

- **`vte`** — the parser we depend on: <https://github.com/alacritty/vte> (the `Perform` trait, params
  handling, the `ansi` module if present).
- **`alacritty_terminal`** — the gold state-model reference (we do *not* depend on it; read only):
  <https://github.com/alacritty/alacritty> under `alacritty_terminal/src/` —
  - pending-wrap → grep **`WRAPLINE`**, **`input_needs_wrap`** (in `term/mod.rs`).
  - wide-char → grep **`WIDE_CHAR`**, **`WIDE_CHAR_SPACER`** (in `term/cell.rs`).
  - BCE → the erase handlers that clear with the cursor *template* cell (carries current bg).
  - selection → `selection.rs` (`Selection`, `to_range`, `selection_to_string`).
  - grid/scrollback → `grid/` (`Grid`, `Row`, the scrollback ring).
- **`wezterm-term`** — alternative model: <https://github.com/wezterm/wezterm> under `term/src/`.
- **`xterm.js`** — the web/JS perspective (what PenTerm leaves behind): <https://github.com/xtermjs/xterm.js>.
- **Conformance suites** (for #8): **vttest** <https://invisible-island.net/vttest/> and iTerm2's
  **esctest** (very thorough) — these *are* the systematic net.

## How a consumer integrates (context, not justerm's work)

PenTerm (first consumer) wraps justerm: it feeds PTY/SSH bytes, ships the binary diff over a Tauri
Channel, and in the webview hands each decoded frame's cells + overlay spans + cursor to the
first-party **`justerm-renderer`** WebGL2 renderer (ADR-0018, superseding beamterm/ADR-0002). Unlike
the parser-agnostic third-party renderer it replaced, justerm-renderer does the compositing **in
wasm**: it resolves colour *references* → RGB (against the consumer's frozen scheme), maps attrs
(inverse/dim/hidden), blends the selection/search highlight, and draws the cursor as a **native
overlay** (#270 — block/underline/bar/hollow, no cell-invert). The consumer's remaining job is
*policy projection* — the frozen palette, the blink phase, the focus tint — pushed to the renderer
each frame (ADR-0017: mechanism in the renderer, policy in the consumer). The selection *model + text*
stay in justerm (so copy reaches scrollback), and the engine's frame still ships colour references +
the old+new cursor cells on a move (renderer-agnostic damage; see "Cursor-move damage" under Hidden VT
state). This integration is tracked in the consumer (`justerm-web` / PenTerm), not here — but it
defines what the engine's output must serve.

In the webview, the adapter does not hand-write the `decode` side of the wire format: justerm ships
the **canonical web decoder** as a separate `justerm-wasm-decode` crate (the native `decode` compiled to
WASM, version-locked to the crate), so encode (native backend) and decode (WASM webview) share one
implementation and cannot drift. The decoder stops at *references* (a zero-copy flat cell-buffer view
+ span directory); ref → RGB and codepoint → atlas are the **renderer's** job (justerm-renderer does
them in wasm), while the frozen palette + policy that drive them stay the consumer's. Decision +
shape: **ADR-0008** (#34).

## Prior-art basis (one line each)

- **Mosh (SSP):** server keeps screen state, syncs diffs, skips intermediates — our cadence's ancestor;
  its scrollback failure (synced only the screen) is why scrollback is engine-owned here.
- **Alacritty:** `LineDamageBounds` (damage grain) + the `Selection`/`to_range`/`to_string` model we
  mirror on `vte`. (We do *not* depend on `alacritty_terminal` — see ADR-0001.)
- **Warp:** forked Alacritty's model + native GPU render — confirms the model base; its render path is
  the full-native option we do not take.
- **VS Code:** raw-bytes-over-IPC + watermark flow control — the counter-example; our parse-in-engine +
  diff gives flow control for free.
- **beamterm:** the parser-agnostic WebGL2 grid renderer the first-party `justerm-renderer`
  reimplements — the original adopted renderer (ADR-0002), superseded by ADR-0018 (switch #273).
