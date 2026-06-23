# justerm — architecture & contract

The detailed spec an implementer references. justerm is a **pure terminal engine**: bytes in →
terminal state → viewport/damage/scroll/selection out. No I/O, no IPC, no rendering, theme-agnostic.
See `CLAUDE.md` for the boundary invariants and `CONTEXT.md` for vocabulary. Key decisions with
rejected alternatives are in `docs/adr/`. This contract was grilled and cross-validated against prior
art (Mosh / Alacritty / Warp / VS Code / beamterm); the origin/rationale record is PenTerm's
`.scratch/rust-terminal-engine/PRD.md` (history only — this file is justerm's authoritative spec).

## Engine API (shape)

- `feed(&mut self, bytes)` — push a VT byte slice (caller does the PTY/SSH I/O).
- `viewport_snapshot(rows) -> Grid` — current visible cells + cursor.
- `damage() -> Iterator<{line, left, right}>` — changed line ranges, each with a changed column span.
- `scroll_delta() -> Option<{count, region}>` — a first-class scroll op since last frame.
- `resize(cols, rows)` — re-layout / reflow.
- selection: `begin/extend/clear(point, side, mode)`, `selection_range()`, `selection_text() -> String`.
- `search(query) -> Matches` (grid + scrollback).
- `encode_key(event) / encode_mouse(event) / encode_paste(text) / encode_focus(focused) -> bytes` —
  mode-dependent input encoding (the engine owns the modes that decide encoding: DECCKM, mouse
  tracking/encoding, bracketed paste, focus reporting). `encode_mouse`/`encode_focus` return `Option`
  (nothing when the mode is off). See the "Input encoding" Hidden VT state entry.
- `drain_events() -> Vec<TermEvent>` — point-in-time consumer events (OSC 0/2 title, BEL bell, OSC 7
  cwd) accumulated during `feed`, drained pull-style (no callback across the boundary). OSC 8 hyperlink
  is *not* here — it is per-cell state (#26), not an event.
- `hyperlink(link) -> Option<&str>` — resolve a cell's `link` index (OSC 8) to its URI, so the renderer
  reads `Cell.link` then this to make a cell clickable (#26).
- `drain_replies() -> Vec<u8>` — bytes the engine produced answering app queries (DA1, DSR, DECRQM)
  during `feed`, for the consumer to write back to the PTY. Pull, and separate from `drain_events`
  (raw bytes → PTY vs typed notifications → UI). The first outbound "engine → app" path (#27).

## Cell

Fixed-width record for fast typed-array decode:

- `content`: a **grapheme cluster** (combining marks/emoji-correct; primary code point inline, rare
  multi-code-point clusters reference a side-table — keeps cells fixed-width).
- `fg` / `bg`: **color references** — `Default | Indexed(u8 0..255) | Rgb(u8,u8,u8)`. **Never resolved
  hex** — the consumer/renderer maps indices→hex via its (frozen) scheme. Engine is theme-agnostic.
- `attrs`: standard 8 (bold/dim/italic/underline/blink/inverse/hidden/strikethrough). The record
  **reserves room** for underline style+color and an OSC 8 hyperlink id so adding them later is not a
  format change.
- `width`: 1 (normal) or 2 (wide / CJK fullwidth).

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
**fixed-width 16-byte cell records** + a grapheme side-table for rare multi-code-point clusters.
Designed for a consumer to ship over its own transport (e.g. a Tauri Channel) and decode straight into
typed arrays (a fixed stride → one contiguous view, no per-field parse). The engine provides the
*format* **and both directions** (`encode` a damage frame / `decode` it — the round-trip is the test);
*transport* is the consumer's job. Rationale — binary fixed-width vs Mosh's protobuf baseline-diff vs
xterm.js's escape-sequence re-emit (which a non-parsing GPU renderer like beamterm cannot consume):
**ADR-0005**.

A **frame** serializes one damage cycle (`damage()` + `scroll_delta()`):
- **header** — magic, version, flags, `cols`/`rows`, cursor (`cursor_row`/`cursor_col` u16 +
  `cursor_visible` u8 — v3, #38), kind (`Full` | `Partial`).
- **scroll op** (optional) — `{top, bottom, count}` (ADR-0003); the decoder applies it *before* the spans.
- **spans** — for `Partial`, each `{line, left, right}` then `(right−left+1)` cell records; `Full` = all rows.
- **side-table** — only the clusters referenced *this frame*, renumbered frame-local; each cell's
  `extra` rewritten to the local index.

The **cell record** (little-endian): `c` (u32 Unicode scalar — *not* the renderer's atlas glyph
id), `fg`/`bg` (u32 each = tag byte `Default|Indexed|Rgb` + 24-bit payload; the tag is mandatory so
`Default ≠ Indexed(0) ≠ Rgb(0,0,0)`), `flags` (u16, incl. layout markers), `extra` (u16 frame-local
grapheme index, `0` = none), and — since wire **v2** (#26) — `link` (u16 frame-local hyperlink index,
`0` = none) = **4+4+4+2+2+2 = 18 bytes**, 2-aligned. Width is derived from `flags & WIDE_CHAR`. The
hyperlink id was added exactly as the format promised — a **versioned** addition with its own index +
side-table (`link_table`), never an overload of a live field; the `VERSION` byte gates it. Remaining
"reserve room for later" is *spare bits*, not padding: underline style+colour use `flags` bits 11–15
and the colour tags' spare 6 bits.

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
  cell content*, so a content-only damage model records nothing — yet the rendered output changed: the
  consumer draws the cursor by **cell-invert** (beamterm has no cursor primitive — it consumes flat
  `CellData`, so the caller swaps fg/bg on the cursor cell; see "How a consumer integrates"). With
  incremental (`Partial`) damage, a pure cursor move then leaves the old cell still inverted (a ghost)
  and the new cell never inverted. The engine must damage **both the old and new cursor cells** on a
  move — Alacritty tracks this as `TermDamageState::last_cursor` (damages the cell the cursor left +
  the cell it lands on). This is damage-layer hidden state a "cursor is just (row, col)" model omits;
  it is the engine's job (it owns damage), *not* "drawing" (which stays the consumer's). [#38]
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
- **Origin mode (DECOM ?6) makes cursor addressing region-relative.** When set, CUP/HVP (`goto`) is
  relative to the scroll region's top margin and clamped to the bottom margin; the column is
  unaffected. Setting DECOM homes the cursor to the region top; *unsetting* it leaves the cursor put
  — an xterm/alacritty asymmetry we follow (ADR-0001 gold reference), noting xterm homes on both. [#8]
- **Scrollback accrues only on a top-anchored, primary-screen scroll.** A line scrolled off enters
  history *only* when `scroll_top == 0` and not on the alt screen — NOT merely "the full screen". A
  top-anchored sub-region (`[0..k]`) still accrues; a region with `scroll_top > 0`, the alt screen,
  and reverse-index (scroll *down*) never do (verified against alacritty `region.start == 0`). The
  viewport windows into history via a `display_offset` clamped to `[0, history.len()]`. New output
  while scrolled up (`display_offset > 0`) **stays** put — the offset is bumped to hold the view, not
  yanked to the bottom (alacritty/xterm.js follow-bottom). History is a flat line ring; semantic
  grouping (Warp's command "blocks") is a *consumer* concern above the engine, never in it. [#3]
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
- **The grapheme side-table is re-indexed per frame — the engine pool is global, append-only, and
  leaky.** `Cell.extra` indexes the engine's `grapheme_pool`, which only grows and accumulates dead
  entries (an overwritten combining-mark cell orphans its slot). Serializing the whole pool would ship
  garbage and grow unbounded. A frame encodes *only* the clusters its cells reference, renumbered
  contiguous frame-local, rewriting each `extra` to the local index; the decoder rebuilds a per-frame
  table. (Side-effect: the wire never exposes the leak; pool compaction stays a deferred engine concern.) [#6]
- **The scroll op is recorded (not diff-detected), screen-relative, and ordered before the spans.**
  Per ADR-0003 the frame carries `{top, bottom, count}` *ahead of* the damage spans; the decoder shifts
  its mirror grid first, then applies spans — reversing the order lands spans on pre-scroll rows. #6
  serializes **screen** damage only; the screen→viewport remap (suppress/translate scroll while
  `display_offset > 0`) is the consumer/cadence concern in #13, out of this format's scope. [#6]
- **Colour needs an explicit tag in bytes; `Default ≠ Indexed(0) ≠ Rgb(0,0,0)`.** A "zero means
  default" packing collides with ANSI black (`Indexed(0)`) and true black (`Rgb(0,0,0)`). Each of
  `fg`/`bg` ships a tag + payload so the consumer's frozen-scheme resolver picks default vs palette vs
  truecolour. This is the theme-agnostic invariant projected into the wire: the engine ships the
  *reference*, never the resolved hex. [#6]
- **`flags` mixes SGR attrs with layout markers, and `c` is a codepoint — the consumer must split
  both.** The record ships the raw `CellFlags` u16: bits 0–7 (bold…strikethrough) map to the renderer's
  style/effect, bits 8–10 (`WIDE_CHAR`/`SPACER`/`WRAPLINE`) are *layout*, not font style (beamterm
  packs bold/italic/underline into its 16-bit glyph id — feeding `WIDE_CHAR` there would corrupt it).
  Likewise `c` is the Unicode scalar; mapping codepoint → atlas glyph id is the consumer's job, so the
  engine stays font/atlas-agnostic and reusable beyond beamterm. [#6]
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

PenTerm (first consumer) wraps justerm: feeds PTY/SSH bytes, ships the binary diff over a Tauri
Channel, and in the webview a **thin adapter** resolves color references → RGB (via the session's
frozen scheme) and maps attrs (inverse/dim/hidden → colour manipulation) before handing cells to the
**`beamterm`** WebGL2 renderer; the adapter draws the cursor. beamterm has **no cursor primitive** —
no overlay quad, no cursor uniform, no per-cell reverse bit; it consumes only flat `CellData
{ symbol, style_bits, fg, bg }`. So the adapter renders the cursor by **cell-invert**: swap fg/bg on
the cell at the engine-reported cursor row/col (beamterm's own terminal example does exactly this).
Because of that, the engine must include the old+new cursor cells in `Partial` damage on a cursor move
(see "Cursor-move damage" under Hidden VT state) — otherwise the inverted cell ghosts. Selection
highlight is rendered by beamterm but the selection *model + text* stay in justerm (so copy reaches
scrollback). This
integration is tracked in PenTerm, not here — but it defines what the engine's output must serve.

In the webview, the adapter does not hand-write the `decode` side of the wire format: justerm ships
the **canonical web decoder** as a separate `justerm-wasm` crate (the native `decode` compiled to
WASM, version-locked to the crate), so encode (native backend) and decode (WASM webview) share one
implementation and cannot drift. The decoder stops at *references* (a zero-copy flat cell-buffer view
+ span directory); ref → RGB, codepoint → atlas, and the per-cell adapter loop above stay the
consumer's. Decision + shape: **ADR-0008** (#34).

## Prior-art basis (one line each)

- **Mosh (SSP):** server keeps screen state, syncs diffs, skips intermediates — our cadence's ancestor;
  its scrollback failure (synced only the screen) is why scrollback is engine-owned here.
- **Alacritty:** `LineDamageBounds` (damage grain) + the `Selection`/`to_range`/`to_string` model we
  mirror on `vte`. (We do *not* depend on `alacritty_terminal` — see ADR-0001.)
- **Warp:** forked Alacritty's model + native GPU render — confirms the model base; its render path is
  the full-native option we do not take.
- **VS Code:** raw-bytes-over-IPC + watermark flow control — the counter-example; our parse-in-engine +
  diff gives flow control for free.
- **beamterm:** the adopted renderer (see ADR-0002).
