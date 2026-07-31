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
the frame/damage cadence (§Cadence), the wire format (§Serialization), the mode-gating that decides
what `encode_key` / `encode_mouse` / `encode_paste` / `encode_focus` emit (§Hidden VT state, "Input
encoding is mode-gated"), and the minimum width immediately below — a clamp is behaviour the
signature cannot state.

**Minimum size: `MIN_COLUMNS = 2` columns × 1 row, applied silently (#547).** Construction and
`resize` widen `cols` up to two and `rows` up to one. Only the width is a published constant, and
only the width is a *contract change*: the row floor is one that `resize` always held (*"a terminal
is never 0-tall"*) and that the constructor merely failed to enforce, where it panicked on a
subtract overflow rather than yielding a degenerate screen. One column cannot hold a width-2 glyph — it needs a
`WIDE_CHAR` lead *and* the `WIDE_CHAR_SPACER` that stands for its second half — and a half-written pair
is the malformed state every repair path keys off, so the floor is what makes ADR-0025 D4 (*both halves
of a pair move together*) unconditionally satisfiable instead of true only above an unstated width.
**Consequences a consumer must plan for:** `resize(1, rows)` yields a **2**-column grid with no error
and no signal, so a consumer that drags a pane to one column must read the width back from the frame
header (or `grid().cols()`) rather than assuming the value it passed. It also must not report the
*requested* width to its PTY — a `TIOCSWINSZ` of 1 column would tell the application a width the buffer
does not have. Both references a terminal *engine* compares to forbid the width for the same reason
(alacritty `MIN_COLUMNS = 2`, xterm.js `MINIMUM_COLS = 2`); ghostty permits it and destroys the glyph.
justerm clamps in the core, where xterm.js clamps, because justerm has no app layer to clamp in.

## Cell

Fixed-width record for fast typed-array decode. Two layers wear this name and they store the overflow
differently — say which you mean: the **in-memory** cell (`justerm-core`'s `Cell`, a packed
fixed-width value) and the **wire** cell record (§Serialization, 14 bytes). The fields below are the
model both share.

- `content`: a **grapheme cluster** (combining marks/emoji-correct). The primary code point is inline
  and the cluster overflow is kept out of the cell so it stays fixed-width — in memory via a presence
  bit (`COMBINED_PRESENT`) with the cluster in the row's column-keyed map (#45/#46), on the wire via a
  cluster inlined at its column in the span's sparse combining group (v14, #621 — there is no
  grapheme side-table and no per-cell index).
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

**Viewport-vs-screen damage — settled in #13, and not by the mapping this section once predicted.**
The problem is real: damage (#4) is recorded against the *screen*, but the consumer renders the
*viewport*, and while scrolled up under follow-bottom "stay" (#3) the screen scrolls while the
viewport does not — so a screen scroll op applied to a scrolled-up viewport would shift a frozen view.
This section carried it as an open question owed a *translation layer* ("map screen damage → viewport
damage, suppress or translate scroll ops"). **No such layer was built, and none is needed.** What
`display_offset > 0` means is that nothing the consumer can see has changed, so there is nothing to
translate — only nothing to send:

- `Term::damage` and `Term::frame_damage` both return an **empty** `Partial` while `display_offset > 0`.
  A frozen viewport reports no damage rather than translated damage.
- A user scroll *moves* the viewport, and that path sets `full_damage` — so the frame after a scroll
  is a full redraw, which is also why no scroll op has to be suppressed for a scrolled-up view.

The asymmetry is the whole design: damage is defined against **what the consumer can see**, not
against the screen, and the screen is merely where it happens to be recorded. Anyone reaching for a
translation layer here is re-solving a solved problem.

## Selection

Engine-owned. Type = char / word / line / **block**; anchor = point + **side (left/right)**.
`selection_range()` → highlight; `selection_text()` → copy text (respects type, wide chars,
wrapped-line joining, trailing-whitespace trim, **across scrollback** — the engine holds all cells).
Cursor blink is *not* an engine concern (consumer-local animation); the engine only reports cursor
position/style/visibility.

## Serialization (the wire format the engine offers)

Binary, **reference-based** (matches the Cell above — references, not RGB), little-endian,
**fixed-width 14-byte cell records** (`CELL_RECORD_LEN`; the field-by-field arithmetic is below) + a
sparse per-span group for the rare cells carrying a multi-code-point cluster (v14, #621 — there is
no grapheme side-table any more).
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
- **combining group** (v14, #621) — one sparse `(col, cluster)` map per span, in span order, the
  cluster inline. No side-table and no per-cell index: nothing interned the clusters, so the
  indirection bought only a second count to overflow.
- **link group + `link_table`** (v14, #621) — one sparse `(col, index)` map per span, addressing a
  frame-local table of the URIs referenced *this* frame. This half stays interned because cells
  genuinely share a URI, and every reference does the same.
- **overlay** (v6, #108/ADR-0014; v7, #118/ADR-0015) — interaction state as *viewport* coordinates, five
  groups: a selection-span group then a search-match-span group (each a **`u32`** count + `(row, left, right)`
  `u16` triples — the count was widened in v14/#621 because these three groups are viewport-bounded and the
  header admits a viewport larger than `u16::MAX` cells; the *marker* counts below deliberately stayed `u16`,
  which is the asymmetry to read as intentional), then a marker group (`u16` count + `(marker_id u32, row u16)` pairs; v10, #159, appends a
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

**What `decode` validates, and the line it draws (#582).** A frame's *payload placement* is read
against the geometry the same frame declares: a span reaching past `cols` or sitting past `rows`, a
sparse group entry keyed outside its own span, and a scroll region whose `bottom` is past the last
row are all `DecodeError::BadSpan`. The reason is that each of those is a **write index** in a
consumer — `justerm-web`'s `cell-mirror.ts` keeps the viewport as one flat array, so a column past
`cols` does not throw, it overwrites the next row — and `decode`'s input is attacker-influenced
(ADR-0008). The frame's *annotations* — overlay spans, marker rows, the cursor — are **not** checked:
consumers resolve them by scan rather than by index, so clamping them is consumer policy under
ADR-0017 and core has nothing to reject. `encode` mirrors the rule in the direction it can: it drops
a group entry keyed outside its span rather than narrowing it onto a different live column, with a
`debug_assert!` naming the producer, since only justerm itself can build one.

The **cell record** (little-endian): `c` (u32 Unicode scalar — *not* the renderer's atlas glyph
id), `fg`/`bg` (u32 each = tag byte `Default|Indexed|Rgb` + 24-bit payload; the tag is mandatory so
`Default ≠ Indexed(0) ≠ Rgb(0,0,0)`), and `flags` (u16, incl. layout markers)
= **4+4+4+2 = 14 bytes**, 2-aligned. Width is derived from `flags & WIDE_CHAR`.

It was **18 bytes until v14** (#621): the record also carried `extra` and `link`, two u16 frame-local
indices for a grapheme cluster and an OSC 8 URI. Both were too narrow for values the engine
legitimately holds — a viewport can hold more cells than a `u16` can number, since the header stores
`cols` and `rows` as u16 *each* — and widening them in place would have inflated a record **every**
cell pays, which is the trade ADR-0008's Axis 4 rejected in the other direction. So they left the
record for sparse per-span groups instead, and the record shrank: measured, −20.9% on an ordinary
frame that carries neither. The
hyperlink id was added exactly as the format promised — a **versioned** addition with its own index +
side-table (`link_table`), never an overload of a live field; the `VERSION` byte gates it. **Underline
colour** (SGR 58, #520) follows the *same* path, not the spare-bits one: a full `Color` reference is
26 bits — too big to ride `flags` — so it is stored engine-side in a per-row map like the hyperlink
(gated by a `UCOLOR_PRESENT` cell bit) and reaches the wire as its own versioned group (**v13**, #520):
a per-span **sparse** group of `(col, Color)` pairs, so the per-cell record above is unchanged —
only cells that draw a coloured underline cost bytes, not every cell. **v14 (#621) generalised that
shape rather than adding to it**: combining clusters and hyperlink references took the same route out
of the record, which is why the record is now 14 bytes rather than 18. The sparse-group pattern is now
how *every* rare per-cell payload reaches the wire, not a special case for one of them. What the
`flags` bits 11–15 still genuinely reserve is the underline **style** (single/double/curly/dotted) —
a small enum that *does* fit spare bits — plus the colour tags' spare 6 bits.

## Hidden VT state — model these (and grow this list)

A correct-*looking* model (cell + cursor + advance + wrap) silently omits subtle state real terminals
track. These are invisible from first principles / this contract — only a reference impl (`vte` /
alacritty / xterm) or vttest reveals them. **Before implementing any VT-semantics slice (#2, #3, #4,
#6, #7, #8, #10): read how a reference terminal handles that area and enumerate the hidden state (flags,
deferred behavior) it tracks — then add what you find here.** Seeds (caught in #2 review, 2026-06-16):

The bullets below are the *seeds* — narrative descriptions of individual pieces of hidden state. The
**soft-wrap flag and the wide-char-spacer markers specifically** have a consolidated
**ownership + lifecycle** model — who stores each, which verb sets / clears / repairs it, how reads
gate — in **ADR-0025**. Read that first for those two; keep these bullets as the implementation detail
under it.

- **Pending-wrap (deferred last-column wrap).** Printing into the last column does *not* advance to
  the next line — the cursor stays put with a `wrapnext` flag, and wrap happens on the *next* print.
  Eager wrap is a classic off-by-one bug (lines shift). [#2]
- **Wide-char spacer is a distinct marker, not a blank.** The trailing column of a width-2 char must
  carry a "wide-char spacer" marker (flag/variant), not a plain blank — else overwrite, erase,
  selection, and cursor positioning go wrong. [#2]
- **A `WIDE_CHAR` lead can stand in the *last* column, so a repair that reads `lead + 1` needs its
  bound.** The print paths cannot produce this — `write_glyph` wraps rather than write a lead it
  cannot pair, and a mode-2027 promotion at the last column relocates instead. `Row::resize` can:
  it truncates cells with no wide repair, and since #567 the alt screen resizes **without**
  reflowing, so narrowing straight through a pair strands its lead at the new right edge (measured:
  `?1049h`, `한` at columns 1-2 of 4, `resize(2, 3)` → `cell(1, 1).is_wide()`). `MIN_COLUMNS = 2`
  does not cover this — the floor guarantees a pair *fits*, which is about **placing** one, not
  about what is already in the buffer. The consequence is asymmetric and worth stating plainly: a
  reader that skips the lead-less lead is merely wrong on screen, while a repair site that indexes
  `lead + 1` unbounded **panics inside the consumer's process** (#529's guard; #536 is the same
  class one function over). [#529, #567]
- **The column a wrapped wide glyph vacates is a blank *written with the current pen* — flagging
  a cell is not the same as writing one.** When a width-2 glyph cannot fit in the last column it
  wraps, and that column becomes a soft-wrap artefact (the row is marked wrapped, and the column
  gets the leading-spacer marker the
  text extractors skip). It must be **written**: a blank built from the pen, carrying the pen's
  fg/bg and its still-open hyperlink / armed underline colour. All three references do exactly
  this — xterm.js `setCellFromCodepoint(col, 0, 1, curAttr)`, ghostty `printCell(0, .spacer_head)`
  (which stamps the cursor's hyperlink), alacritty `write_at_cursor(' ')` under a
  `LEADING_WIDE_CHAR_SPACER` template. Two failure modes sit on either side of that: OR-ing the
  markers onto the previous occupant leaves a glyph the extractors skip but a renderer still
  draws — a character you can see and cannot copy, search or have announced (#528) — while
  `reset()`-ing to a *default* cell punches an uncoloured notch into a coloured run. Note the
  marker setter's name is a trap: `set_leading_spacer` only *records* that the column is blank.
  The ordinary pending-wrap soft wrap is a different case and must NOT be blanked — there the
  last column holds real content and the row is merely marked wrapped.
  Three consequences that only showed up once the column was written, each of which cost a
  regression before it was understood:
  ① **Writing the column makes the vacate an overwrite**, so it inherits the no-orphan obligation
  every other overwrite carries — if the column was a wide glyph's *spacer*, blanking it destroys
  the marker every repair path keys off (`is_wide_spacer`), stranding the lead **permanently**.
  Damage the repaired lead too: it is a second changed cell, and ghostty asserts the dirty bit on
  exactly that cell rather than the written one.
  ② **Commit nothing until the wrap is known to happen.** `wrapline` → `linefeed` silently stays
  put when the cursor is parked *below* a DECSTBM region on the last row. Both paths destroy
  content on the assumption the row is about to change — `write_glyph` blanks the column it
  leaves, and the mode-2027 relocation writes its cluster to `(cursor.row, 0..=1)` *after* the
  wrap, which is the same row when nothing advanced. Reasoning only about the vacated source
  column misses the second entirely.
  ③ **Position is part of the artefact's definition** — it is only ever the *last* column of a
  soft-wrapped row. The marker alone is not a sufficient test, because the row-shift verbs
  (ICH/DCH) move whole cells and carry it inward, where it describes nothing; treating a migrated
  marker as an artefact joins two visually separate words in the clipboard. [#528]
  ④ **The marker has a lifetime, and its claim has two clauses.** It asserts *"this row
  soft-wraps, and its last column is the blank **that** pair vacated"* — so it must be cleared when
  either clause goes false, and for a long time nothing in the crate cleared it at all.
  `Term::end_wrap` owns the first clause (a row that stops wrapping cannot hold an artefact), which
  is why `DCH` ends its wrap *before* the shift rather than after: the marker is a cell bit the
  shift would otherwise carry inward. `Term::void_wrap_artefact_above` owns the second, and the
  rule at its four call sites is one sentence: **the record survives only an in-place same-width
  overwrite** — a narrow write, an erase or a shift at columns 0/1 all end the pair the record was
  about, and a wide lead that arrives later by some other route did not *wrap* from anywhere. The
  clear is one-way; nothing re-arms it. The row above may be the last **scrollback** row, since the
  readers walk `[scrollback ++ grid]` as one buffer — including `shift_region`'s `top == 0` seam,
  the one wrap-ending path that is not a grid row and so has to couple the clear itself.
  Ported, not derived: ghostty gates the print path on `cell.wide != wide`
  (`Terminal.zig:1484`) and reaches up a row from `DCH`/`ECH` through
  `Screen.splitCellBoundary` (`Screen.zig:1873`); alacritty has the print half only and clears
  unconditionally, dropping records that are still true. Only justerm's `ICH` site has no
  counterpart.
  **Ask before the mutation, never after.** The post-state form (*"is a wide lead standing at
  column 0 now?"*) is the intuitive one and it is wrong three ways — a `DCH` that pulls the next
  wide glyph left satisfies it, two-step placements (VS16 promotion, IRM's insert-then-write)
  satisfy it only at the end, and running it inside `insert_chars` cleared the marker
  `vacate_for_wrap` had just set, because `write_glyph` routes IRM's gap-opener through there
  *between* the set and the write. That last one is the general lesson: **a repair keyed on a state
  predicate must not run while that state is mid-construction.** [#534]
> The three entries that follow are conformance cases of **ADR-0025** (row-scoped and
> wide-pair-scoped state has one owner and one lifecycle) — split storage, marker-is-established,
> and row-property. Read the ADR for the rule; these are its instances.

- **A cell's extended attributes are stored in two halves — a presence bit in the cell, the value in
  the row — so copying a cell copies only half of it.** Combining marks (#45), the OSC 8 hyperlink
  (#46) and the SGR 58 underline colour (#520) do not fit the packed 12-byte cell, so each keeps a
  *bit* in the cell and its *value* in a column-keyed sparse map on the `Row`. Every path that moves,
  relocates or synthesizes a cell must carry the map entries with it — the mode-2027 width promotions
  (`relocate_cluster_wide` / `promote_cluster_to_wide`), the second half of a wide glyph, ICH/DCH
  shifts, reflow. Miss one and the bit arrives with nothing behind it: the flag-gated read then returns
  the **default**, silently, with no crash — a hyperlink or underline colour that simply evaporates.
  Carry the whole family in one step (`Row::ext_attrs_at` → `Row::set_ext_attrs`) rather than naming
  each rider, the way xterm.js re-keys `_combined` *and* `_extendedAttrs` through a single
  `_copyCellMapsFrom` for every cell `copyCellsFrom` moves — a rider added later is then covered by
  construction. The dual hazard: the maps are deliberately left holding stale entries (harmless under
  the gate), so a carry must **clear** as well as set, or a new cell inherits the previous occupant's
  value. [#521]
  **The wire is the third mover, and it fails in the *opposite* direction.** A presence bit never
  travels as a bit: `encode_color` keeps only mode+value, so `BG_LINK` / `BG_UCOLOR` are dropped, and
  `CellFlags` carries none. So on decode every bit is **reconstructed** from whether its group carries
  an entry — and **since v14 (#621) that is true of all three of them, with no exceptions left**.
  `combined` / `linked` used to be the exception: `decode_cell` could set them inline from the cell
  record's own `extra` / `link`, so only `UCOLOR_PRESENT` needed a group loop of its own. #621 moved
  both references out of the record, so all three bits are now armed by their own group's loop and
  none arrives with its cell. Read the exception as *gone*, not as "mine might be like `extra`" —
  that reading is what produced #531. Where a missed grid-side carry leaves a bit with
  nothing behind it, a missed wire-side re-arm leaves the **value with no bit**: the gated read returns
  `Default` while the map holds the real colour, and the frame stops being a round-trip fixed point.
  A rider that reaches the wire as a group, not as a cell field, owes its own re-arm here. [#531]
- **Background Color Erase (BCE).** Erase (ED/EL) fills cleared cells with the *current SGR
  background*, not default. [#8; note in #2 if deferred]
- **A blank cell carries the current background — including one no app asked for.** BCE covers the
  blanks an *erase* creates. The same rule governs the blanks a **structural repair** creates: when
  an overwrite, erase or row shift destroys one half of a width-2 glyph, the engine frees the other
  half to keep its no-orphan invariant, and that cell is a blank in the pen's background, not a
  `Cell::default()`. Otherwise a coloured run gets an uncoloured notch wherever a wide glyph was
  broken. Note the repair is **not** an erase — the app asked for something at a *different* column
  — so two things follow that an erase gets for free. ① It **damages**: the freed cell lies outside
  the operation's own range by construction, so every function's own span misses it, and a
  frame-mode consumer keeps painting the destroyed glyph. Reset and damage are bundled in
  `Term::free_cell` precisely because the damage half has no compiler behind it. ② It takes the
  pen's **background only**. Taking the pen's full attributes would plant its hyperlink — and its
  DECSCA protection — on a cell the app never wrote; keeping the *cell's own* attributes would
  leave the destroyed glyph's hyperlink alive and clickable. Both were rejected; see #530 for the
  record and for the accepted cost (a freed cell loses DECSCA protection, as it does in ghostty).
  The value is not a compromise between references — it is xterm.js's `_eraseAttrData()` exactly
  (`DEFAULT_ATTR_DATA` plus `curAttr.bg & ~0xFC000000`), which is what its `replaceCells` /
  `insertCells` / `deleteCells` repairs are handed; only its *print* path uses the whole pen.
  **A row property must not live where a cell write or clear can reach it — fixed in #538.**
  Soft-wrap used to ride `WRAPLINE` in the row's last cell, and every whole-cell operation takes
  the whole content word with it: a plain overwrite of the last column, an erase, or a structural
  repair all silently broke the row's wrap link and split the logical line (measured: `"abcdZ"`
  became `"abc
Z"`, and a search across the wrap went from 1 hit to 0). It now lives on the
  `Row`, where no cell operation can reach it, matching both references (ghostty's `Row.wrap`,
  xterm.js's `BufferLine.isWrapped`, with `clearWrap` an explicit argument on its erase helper
  `_eraseInBufferLine` — *not* on `replaceCells`, which takes `respectProtect` there).
  The wire is unchanged: the bit is *derived* onto a span's last cell at encode time, so it is
  wire-only and never set on a live cell. **Ending the wrap is an explicit act, and it is
  per-verb, not derivable from the erased range.** `Term::end_wrap` is called by the verbs that
  destroy content *from the cursor rightward* — `EL 0`, `ECH`, `DCH` — at any column, because
  after them "this row continues past its last column" can no longer be asserted; `EL 1` and
  `ICH` leave it, because the tail (and whatever it flowed into) survives. This matches C xterm
  (`ClearRight` ends with an unconditional `LineClrWrapped`) and ghostty (`cursorResetWrap` in
  `eraseLine(.right)` / `eraseChars` / `deleteChars`) call site for call site — *not* xterm.js,
  whose `clearWrap` governs the opposite-polarity `isWrapped` flag and so answers a different
  question. A second caller class ends the wrap without erasing anything: the **row-shift verbs**
  (`IL`/`DL`/`SU`/`SD` and the region paths in `LF`/`RI`, all through `Term::shift_region`, #540).
  The wrap flag is a claim about *adjacency*, and a shift can falsify it without touching a cell —
  at the two seams where a row's next neighbour changed, and, when the region starts at the grid's
  top, on the last *scrollback* row (the readers walk `[scrollback ++ grid]` as one buffer). The
  interior of the region is safe by construction because whole `Row`s rotate, so a pair moves
  together.
  **Each seam carries exactly one exemption, and both are facts about the caller that the shift
  cannot see** — so they are parameters, not tests. The **top** seam is exempt when the linefeed
  evicts row 0 into scrollback: adjacency survives one row further back. The **bottom** seam is
  exempt when the shift is the one `wrapline` asked for: the blank it exposes is not a stranger that
  displaced a continuation, it *is* the continuation, about to be written into (#557). Geometry is
  not the discriminator — #540's guard first read "only when a stationary row sits below the region",
  which holds perfectly at a *region's* bottom while the scroll is still serving the wrap, and split
  the line the scroll existed to continue. **Why** the shift happens is the discriminator: a verb
  displaces a continuation that already existed, a wrap-serving linefeed creates one. xterm.js
  carries the same fact in the same place — `BufferService.scroll(eraseAttr, isWrapped)`, with only
  the auto-wrap branch of `_print` passing `true`. `end_wrap` damages the last column (the wrap rides the wire there, and nothing else
  would re-ship it), which is why `shift_region` records the scroll op *before* the seam clears:
  `record_scroll` rotates `line_damage` with the content, so a clear damaged earlier is carried to
  the wrong row. It does **not** drop the wide-wrap artefact marker in that column — the erase
  verbs above have already blanked it through the last column, and the leftward erases go through
  `drop_artefact_if_erased` instead; the marker's own lifecycle across every verb is #534. The leftward erases (`EL 1`, `ED 1`) keep the wrap but still drop
  an artefact marker they blanked, and `ED 1` ends the wrap when it covered the whole row (xterm.js
  does the same, `InputHandler.ts:1248`). **`EL 2` is a deliberate divergence, split 2:2:** justerm
  and alacritty end the wrap there, C xterm and ghostty do not (ghostty's source says it *"seems
  like complete should"* but xterm does not). justerm ends it because it *joins* logical lines for
  copy/search, so a blanked-but-still-wrapped row would visibly merge two lines — a cost xterm does
  not carry.
  **Still inconsistent, deliberately unfixed here:** `LF`/`RI` expose a *default* line while
  `SU`/`SD`/`IL`/`DL` expose a BCE one — the crate does not yet agree with itself everywhere. [#530]
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
- **The alt screen resizes but does not reflow.** Both grids take the new dimensions on every resize
  — that part is load-bearing, because leaving alt used to restore an old-sized grid and a damage-driven
  render could panic on the mismatch — but only the **primary** re-splits its content. Reflow re-wraps
  a long line so *history* stays readable at the new width, and it assumes the content is text that
  **flows**; the alt screen has no history, its content is a **layout** rather than a paragraph
  (re-wrapping a process table means nothing), and the application already knows the new size and
  repaints. All three references take the same position with the same shape — one flag on the same
  resize function: ghostty `alt.resize(.{ .reflow = false })`, alacritty `grid.resize(!is_alt, …)`,
  xterm.js gating on `_hasScrollback` with the alt buffer built as `new Buffer(false, …)`.
  Re-splitting it is not merely wasted work: measured on a real `htop` recording taken across a live
  `SIGWINCH`, it leaves debris in the cells htop does not overwrite, because htop repaints **without**
  erasing first (`vim` hides this by clearing). justerm re-split the alt grid from #8 until #567 —
  never by decision, but because the fix that made both screens take the new dimensions reached for a
  helper that re-splits when the column count changes. [#567, #8]
- **Anchor lifecycle: selection/marker anchors are absolute-line coords shifted in lockstep with
  buffer mutation — and share the alt grid's line range.** Selection endpoints (#3) and decoration/
  command markers (#118/#158) store `[scrollback ++ screen]`-absolute lines. Eviction
  (`markers_evict_oldest`), region scroll (`markers_rotate_region`), and reflow (`iter_mut` over
  `m.line`) shift them so they track content; an anchor on a dropped line is disposed. **Hazard: on the
  alt screen the primary anchors are *retained* (that is the #118/#158 contract — a mark must survive a
  vim/less excursion), yet the alt grid occupies the *same* absolute-line range `[scrollback.len(),
  +rows)`. So an alt-screen scroll must NOT rotate primary markers or it silently disposes them.**
  **How each side actually satisfies that has changed twice, and this entry described the state before
  both.** Markers are *not* guarded by `if !self.on_alt` any more — since #186/#187 the storage is
  per-buffer and `markers_rotate_region` routes through `markers_mut`, so an alt scroll rotates *alt*
  marks and leaves the frozen primary list alone; the code says "no guard needed" at the site. And the
  selection does **not** "dodge this by being cleared on alt enter" — that clearing is real
  (`enter_alt_screen` / `leave_alt_screen`) but happens only *at the swap*, so it says nothing about a
  selection made while the alt screen is already up, which is the ordinary act of copying out of vim.
  Reading it as a lifetime invariant is what shipped #660: `Term::resize`'s alt branch skipped
  re-anchoring on that basis, and since the alt pane does not reflow (#567) the anchor kept an
  absolute line the shrunk grid no longer had — `selection_range` then indexed past the end and
  `frame()` panicked. **The rule now: an anchor the alt pane cannot re-anchor is dropped** — `resize`
  clears the selection when the geometry actually changes on alt, which is what both references do
  (alacritty clears on a width change, xterm.js on a height change). [#158, #187, #660]
- **Anchor rotation: the CSI line-editing verbs move anchors too (closed by #162).** This entry was
  a gap and is kept as a *modelled* one, because it is the second half of the rule above and reads
  wrongly without it. `scroll_region_lines` (SU/SD/IL/DL) moves content via `grid.scroll_*_region` +
  `record_scroll` and now rotates both anchor sets alongside it — `selection_rotate_region` and
  `markers_rotate_region`, unguarded exactly as `linefeed`/`reverse_index` are (the `!on_alt` guard
  this line used to cite was retired by #186/#187's per-buffer marker storage — see the entry above), and with
  `up = !down` since "content moved up" is the non-`down` case. Before that, a primary-screen IL/DL
  (zsh/fish multi-line prompt redraw, completion menus) left marks and a live selection pointing at
  the wrong line. **Any new verb that moves rows owes the same rotation**; the tell is a call to
  `grid.scroll_*_region` or `record_scroll` without one beside it. [#158, #162]
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
- **Soft-wrap vs a hard line-end must be distinguished for reflow.** An auto-wrap (the deferred
  last-column wrap firing) marks the row it leaves as *soft-wrapped*; an explicit CR/LF/NEL ends the
  line *hard*. Reflow (#7) merges soft-wrapped rows into one logical line and re-splits at the new
  width; without the distinction every line looks identical and reflow corrupts content. The flag is
  a **property of the row** (`Grid::is_row_wrapped`) — it began on the last cell, Alacritty-style,
  and moved in #538 because a cell write there destroyed it (see the "row property" entry above).
  xterm.js flags the continuation row instead; either encoding works, but neither reference keeps it
  where a cell operation can reach it. [#7, #538]
- **A point reflow tracks is a position in the logical line, and "one past the last cell" is one of
  them.** `grid::reflow` carries the cursor, both selection anchors and every OSC-133 command mark
  through the re-split. All three can sit *just after* the content, and when the last row comes out
  full that is a column the grid does not have — so the returned point is allowed to leave the grid
  (`col == new_cols`, or a row the fit has not created yet) and the **seam in `Term::resize` resolves
  it per kind**: the cursor reads it as the next write position (the row after), a command mark keeps
  it as an **exclusive** bound meaning "all of this row" (`extract_lines` clips `[b, c)`), a selection
  anchor is clamped. Deciding it inside `reflow` picked one kind's answer for all three — a mark then
  landed on the first row of the *next logical line* and swallowed that line's newline. Marker columns
  therefore have domain `[0, cols]`. The bound that keeps a raw-written anchor from indexing a row
  that does not exist lives at the seam too, against the **final** geometry (`scrollback + rows`):
  bounding against what `reflow` emitted clamped away rows the caller's fit was about to create.
  **The row that position needs is bought at the seam, not created by `reflow`.** While the pane is
  shorter than the screen the fit supplies it for free; when the content already fills the pane the
  pane **scrolls** for it, which costs one line of history and is what a terminal does when content
  grows past the bottom. A pane with no history cannot pay — the displaced line would be destroyed
  rather than archived — so it clamps instead, and the gate is that budget (`limit > 0`) rather than
  a test for the alt screen: the alt panes carry a limit of `0` because that is what their history
  is, so they fall out of it by construction (#567).
  ghostty splits the same three ways inside its own reflow — every non-cursor pin is clamped before it
  can widen a row, the cursor pin never is. [#562, #549, #559]

- **Selection coordinates are absolute-from-oldest, and five events move them** (this said *three*
  until #660; the count has been wrong twice, so treat the list as the thing to check rather than the
  number). Anchors are
  stored as a line index into `[scrollback ++ screen]` counted from the oldest line — NOT viewport
  rows (those drift under new output). This index is *invariant* under a normal top-anchored scroll:
  the line evicted into scrollback grows `scrollback.len()` by exactly the screen shift, so existing
  content keeps its index (verified against the existing `display_offset` model, which bumps in
  lock-step). The index moves only on (a) **cap eviction** (`pop_front` → decrement anchors, clamp
  off-top), (b) **in-screen region/RI scroll** with `scroll_top > 0` or alt (rotate anchors within the
  region; an endpoint on the dropped line clears the selection — top-anchored scroll must NOT rotate),
  and (c) **resize reflow** (anchors reflow through `grid::reflow` alongside the cursor — it tracks N
  points), plus the two this list omitted: (d) a **top-anchored sub-region scroll** growing
  `scrollback.len()` while rows below the margin stay put, so their absolute index rises
  (`selection_shift_below_margin`, #449), and (e) a **shrinking resize on the alt screen**, where the
  pane does not reflow and the selection is therefore dropped rather than moved (#660).
  Alt enter/leave clears the selection — **but that is a clear at the swap, not a claim that the
  selection is "primary-only"**, which is how this sentence read until #660 and is what licensed
  `resize` to skip the alt anchors entirely. A selection made *while* the alt screen is up is
  ordinary (copying out of vim), and it is exactly what panicked. [#5, #449, #660]
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
  span's own sparse `(col, cluster)` group (`Span.combining`) — the cluster itself, at its column. It
  was a frame-local `side_table` + an index on the span until v14, and before #45 that index sat on the
  cell; each move took it further from the cell and the last one removed the index entirely, because
  nothing ever interned these clusters for it to point at. [#6, #45, #621]
- **OSC 8 hyperlinks ride the *same* per-row-map machinery, gated by the `LINK_PRESENT` bit.** The `Row`
  carries a second `BTreeMap<col, Arc<str>>` holding **the URI itself** (#628 — it held an index
  into a buffer-wide `hyperlink_pool` until then, and that pool was never reclaimed; `Arc` because
  cells genuinely share a URI, which is the one way links differ from combining marks), gated by the cell's `LINK_PRESENT` bit, which reuses
  xterm's `BgFlags.HAS_EXTENDED` (`0x10000000`, bg bit 28) **exactly**. Carry/reflow/recycle treat it
  identically to combining (`Row::move_maps` re-keys both maps together; reflow threads both). Reads go
  through `Engine::link_at(row, col)` / `viewport_link_at`, which hand back an **owned `Hyperlink`**
  rather than a borrow: the URI lives in the row's map, so a `&str` would be tied to `&Engine` and a
  hover handler could not hold it across the next `feed()`. Measured — the borrow reads at 0.75 ns but
  cannot be kept, and the caller's workaround (copying the string) costs 62.6 ns against the handle's
  17.9 ns, so the borrow saves nothing and moves a larger cost outward. A struct rather than a bare
  `Arc<str>` keeps `Arc` out of the published signature and gives `id=` (#635) somewhere to land
  without changing the return type again; alacritty's `Hyperlink` is the same shape for the same
  reasons. The decoded index rides `Span.links`. With this `Cell` is **12 bytes** — three packed `u32`, no `Option`
  field (the #43 epic target, matching xterm.js's `BufferLine` cell). [#26, #46]
- **Underline colour (SGR 58) rides the *same* machinery — a third per-row map, gated by its own
  `UCOLOR_PRESENT` bit (#520).** A cell that draws a coloured underline stores a `Color` reference in
  `Row`'s `BTreeMap<col, Color>`, gated by bg bit 29. Carry/reflow/recycle/`move_maps` thread it exactly
  like the link and combining maps — **and so does `decode`, which is the threading site this list
  omitted until #531**: the gating bit is not encodable, so the decoder re-arms it from the group, and
  a rider whose author reads only the in-memory rules ships a value its own gate hides. Note the
  regime *inverts* across the wire: in memory the bit is authoritative and the map is a flag-gated
  cache, while on the wire the map is authoritative and the bit is derived from it. Read through
  `Engine::underline_color_at(row, col)` (`Color::Default` = follow the fg). **Where justerm diverges from xterm:** xterm's `HAS_EXTENDED` is a *shared* gate
  holding link **and** underline colour/style in one `ExtendedAttrs` object; justerm keeps a **separate
  map per concern** (as combining and links already are), gating each with its own bit — so the maps must
  be threaded in lockstep across every op (the coherence the shared object gives xterm for free). The
  colour is stored only where an `UNDERLINE` attribute is present (inert otherwise, and xterm likewise
  does not persist it). On the wire it is a per-span **sparse** group of `(col, Color)` pairs (v13), not
  a per-cell field, so a plain-text frame pays nothing for it. [#520]
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
  per-cell state (versioned into the wire as its own group), not an event — its own
  slice (#26). Note the two stopped being modelled alike at v14: a URI is genuinely shared between
  cells and keeps its frame-local table, while a cluster is not and is now inlined (#621). OSC string terminator may be BEL or ST; vte consumes it and calls `osc_dispatch` once, so
  an OSC-terminating BEL is not double-counted as a bell. [#12]
- **An OSC 8 hyperlink is ambient pen-like state stamped onto cells — not an event, and not closed by
  an SGR reset.** `OSC 8 ; params ; URI` opens a link (one `Arc<str>` per open, shared by that open's cells and
  becomes "current"); `OSC 8 ; ; ` (empty URI) closes it. Every glyph printed while open is stamped
  into the row's link map — both halves of a wide glyph, so a hover/selection over either agrees.
  The cell carries only the `LINK_PRESENT` bit; the handle rides *the row*, which is the unit that
  moves through scroll/scrollback/reflow, and that is what makes the URI die with the last row
  holding it (#628 — there is no buffer-wide pool to reclaim, exactly as #45 arranged for combining
  marks). Per frame it is renumbered into the wire's `link_table` — which, unlike the cluster's
  vanished side-table, is still there and still interned (#621), keyed here by the `Arc`'s identity
  so one open is one entry however many cells it covers.

  **Two opens of an identical URI stay two links; an `id=` the application declared groups them.**
  That is one rule and not two — merging on URI alone would override the distinction OSC 8's `id=`
  parameter exists to express, and refusing to merge on a declared `id` would ignore the application
  saying *these runs are one link*. `params` is a `:`-separated key=value list, `id` may sit anywhere
  in it, and an empty value is **not** an id; the group key is `id` **and** URI together, so a reused
  id pointing at a new target does not merge. The URI is everything after the *second* `;` — it is
  rejoined from vte's split, so an unencoded `;` inside it is kept rather than truncating the target
  (#650, matching xterm's deliberately special-cased split-on-first-`;`). It is never decoded: a
  `%3B` stays a `%3B`. The group is held **weakly**: when the last row holding
  the link goes, the key is gone too, and a later open of the same id is genuinely a new link — which
  is why grouping did not reintroduce the pool #628 deleted (#635, xterm.js reaches the same lifetime
  by deleting its `_entriesWithId` entry on last-marker disposal).
  The catch: a hyperlink is **orthogonal to SGR** — `CSI 0 m` (reset attributes) must *not* close it;
  only an empty-URI OSC 8 does (and it persists across line-feeds until then). It is cell state, not a
  point-in-time event, which is why it is here and not on the `drain_events` surface (alacritty agrees —
  hyperlink is a Cell attribute, not an `Event`). [#26, #635]

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
