# Terminal engine / renderer architectures — primary-source survey

Research for justerm. justerm-core is a **render-free** terminal-state engine that emits a
**serializable** viewport-snapshot + damage wire, consumed by a **separate** renderer that holds
no engine state ("frame-mode"). This note reads the actual source of the projects justerm names as
prior art, to place justerm in two lineages and to check whether its #482 marker-correlation cost
has any precedent.

All claims below are cited to a repo + file path + symbol. Everything I could not verify from
source is marked **GAP**.

---

## 1. `alacritty_terminal` — render-free engine, in-process render (justerm's named model reference)

**Public model.** `Term<T>` is the terminal; it owns a `Grid<Cell>` and exposes it read-only.

- `alacritty_terminal/src/term/mod.rs`: `pub struct Term<T>` (definition at `pub struct Term<T> {`),
  with `damage: TermDamageState`, and accessors `pub fn grid(&self) -> &Grid<Cell>` and
  `pub fn renderable_content(&self) -> RenderableContent<'_>`.
- `alacritty_terminal/src/grid/mod.rs`: `pub struct Grid<T>` — rows + scrollback + cursor.

**Damage model.** Damage is per-line column bounds, reset each frame by the consumer:

- `LineDamageBounds { pub line: usize, pub left: usize, pub right: usize }` with
  `is_damaged(&self) -> bool { self.left <= self.right }` (an inverted-bounds sentinel:
  `undamaged` sets `left: num_cols, right: 0`). (`term/mod.rs`)
- `pub enum TermDamage<'a> { Full, Partial(TermDamageIterator<'a>) }` — the whole-terminal escape
  hatch, or an iterator over damaged viewport lines.
- `pub fn damage(&mut self) -> TermDamage<'_>` collects damage since the last reset; it always
  damages the old + new cursor cell (`self.damage.last_cursor` swap, then `self.damage_cursor()`),
  and returns `TermDamage::Full` whenever `self.damage.full` is set (e.g. scroll / display-offset
  change → `mark_fully_damaged()`). `pub fn reset_damage(&mut self)` clears it. The renderer is
  expected to call `damage()` then `reset_damage()` each frame.

**What a renderer consumes per frame.** `RenderableContent<'a>`:

```rust
pub struct RenderableContent<'a> {
    pub display_iter: GridIterator<'a, Cell>,   // BORROWED iterator over grid cells
    pub selection: Option<SelectionRange>,
    pub cursor: RenderableCursor,               // { shape, point }
    pub display_offset: usize,
    pub colors: &'a color::Colors,              // BORROWED palette
    pub mode: TermMode,
}
```

The `'a` lifetime and the borrowed `display_iter` / `colors` are decisive: **this is consumed
in-process by borrow, not serialized.** The renderer lives in the same address space and reads the
grid directly. There is no wire.

**Decoration / mark concept — absent.** Grepping the crate, the only per-content annotation is
`set_hyperlink(&mut self, hyperlink: Option<Hyperlink>)` (`term/mod.rs`) which stores an **OSC 8
hyperlink on the cell template** — a per-cell attribute, not a line-level mark. "semantic" in the
crate means only **selection word-class boundaries**: `SEMANTIC_ESCAPE_CHARS`,
`semantic_search_left/right` (`term/search.rs`), `ViMotion::SemanticLeft` (`vi_mode.rs`). There is
**no OSC 133 prompt mark, no line marker, no decoration** type in `alacritty_terminal`. (GAP-adjacent:
alacritty *the app* also does not track OSC 133 in the crate; anything mark-like lives outside the
reusable engine.)

---

## 2. libvterm — pure bytes→screen-state + damage callbacks, zero rendering

Read from neovim's vendored, readable copy (`neovim/neovim`, `src/nvim/vterm/`), which is libvterm
with `nvim`-namespaced includes; the public callback/rect structs match upstream libvterm. (GAP: I
verified against neovim's vendored tree, not the canonical `leonerd` libvterm repo — the struct
layout is the same but I did not diff the canonical mirror line-for-line.)

**The screen callbacks** — `src/nvim/vterm/vterm_defs.h`, `typedef struct { ... } VTermScreenCallbacks;`:

```c
typedef struct {
  int (*damage)(VTermRect rect, void *user);
  int (*moverect)(VTermRect dest, VTermRect src, void *user);
  int (*movecursor)(VTermPos pos, VTermPos oldpos, int visible, void *user);
  int (*settermprop)(VTermProp prop, VTermValue *val, void *user);
  int (*bell)(void *user);
  int (*resize)(int rows, int cols, void *user);
  int (*theme)(bool *dark, void *user);
  int (*sb_pushline)(int cols, const VTermScreenCell *cells, void *user);   // scrollback out
  int (*sb_popline)(int cols, VTermScreenCell *cells, void *user);          // scrollback in
  int (*sb_clear)(void *user);
} VTermScreenCallbacks;
```

- `VTermRect { int start_row, end_row, start_col, end_col; }` — damage is a **rectangle of
  cells**, half-open. The host reads the rect and pulls current cell state via
  `vterm_screen_get_cell` (the callback carries only the rect, not the cells).
- `VTermDamageSize { VTERM_DAMAGE_CELL, VTERM_DAMAGE_ROW, VTERM_DAMAGE_SCREEN, VTERM_DAMAGE_SCROLL }`
  is the merge granularity the host requests; the `VTermScreen` internally accumulates a single
  merged `VTermRect damaged` (`src/nvim/vterm/vterm.h`, `struct VTermScreen { ... VTermRect damaged;
  VTermRect pending_scrollrect; ... }`).
- **No rendering.** libvterm produces cell state + damage rects + scrollback push/pop and hands
  them to the host via function pointers; drawing is entirely the host's. `moverect` is a scroll
  optimization (copy a region rather than re-damage it). This is the C-callback analog of justerm's
  damage wire, but delivered **in-process via function pointers**, not serialized.

---

## 3. Mosh — State Synchronization Protocol: syncs SCREEN STATE, not the byte stream

**The screen-state model** — `src/terminal/terminalframebuffer.h`:

- `class Cell` (a grapheme + `Renditions` + optional `Hyperlink`), `class Row`, `class Framebuffer`
  (`class Framebuffer` holds `DrawState ds` + the rows). This is a plain grid-of-cells state object.
- `class Row { std::vector<Cell> cells; uint64_t gen; ... bool operator==(const Row& x) const {
    return ( gen == x.gen && cells == x.cells ); } }`. The comment is explicit: *"gen is a generation
  counter. It can be used to quickly rule out the possibility of two rows being identical; this is
  useful in scrolling."* — a **row diff accelerator**.

**The diff between two STATES** — `src/terminal/terminaldisplay.h` / `.cc`:

- `class Display { std::string new_frame( bool initialized, const Framebuffer& last,
    const Framebuffer& f ) const; ... }`. `new_frame` **diffs two Framebuffer states** and emits the
  minimal terminal-escape update:
  - scroll detection by comparing rows: in `new_frame` (`terminaldisplay.cc`) it walks rows and
    tests `*f.get_row(region_height) == *rows.at(lines_scrolled + region_height)` to find a
    scrollable region;
  - per-cell skip in `put_row`: `if ( initialized && !clear_count && ( cell == old_cells.at(frame_x) ) )`
    — unchanged cells emit nothing.

**It syncs state, not bytes** — `src/statesync/completeterminal.h` / `.cc`:

- `class Complete { Parser::UTF8Parser parser; Terminal::Emulator terminal; Terminal::Display
    display; ... const Framebuffer& get_fb() const; std::string diff_from( const Complete& existing )
    const; void apply_string( const std::string& diff ); }`.
- `Complete::diff_from` (`completeterminal.cc`) computes the wire update as
  `display.new_frame( true, existing.get_fb(), terminal.get_fb() )` — i.e. **the wire payload is the
  diff between the sender's framebuffer STATE and the receiver's last-acked framebuffer STATE.** SSP
  synchronizes the `Complete` object (a terminal state), not the PTY byte stream. This is the
  canonical "serialized terminal state over a wire" prior art.

**But: not a reusable engine crate.** Mosh's terminal (`src/terminal/`) is *internal* to mosh and
coupled to the state-sync objects (`Complete`, `Display`). It is not published or consumed as a
general, embeddable terminal engine the way `alacritty_terminal` / `wezterm-term` / libvterm are.
Mosh has lineage (B) but not (A).

---

## 4. wezterm-term and libghostty — reusable, render-free engine libraries

### wezterm — `term/` crate (`wezterm/wezterm`, branch `main`)

- `term/src/lib.rs` doc, verbatim: *"This crate provides the core of the virtual terminal emulator
  implementation used by wezterm… This crate does not provide any kind of gui, nor does it directly
  manage a PTY; you provide a `std::io::Write` implementation… and supply bytes to the model via the
  `advance_bytes` method."* → a **render-free, reusable engine crate** (lineage A).
- **Dirty model = sequence numbers, in-process.** Everything threads a `SequenceNo` (`use
  wezterm_surface::SequenceNo`). `term/src/screen.rs`: `dirty_line(&mut self, idx, seqno)` calls
  `self.lines[line_idx].update_last_change_seqno(seqno)`; every mutation stamps
  `line.update_last_change_seqno(seqno)`. The GUI (a separate crate, in the same process) asks each
  `Line` "changed since seqno X?". No serialization in the `term` crate itself.
- **Has a semantic-mark concept (unlike alacritty).** `term/src/terminalstate/mod.rs`:
  `pub fn get_semantic_zones(&mut self) -> anyhow::Result<Vec<SemanticZone>>` — OSC 133 zones
  (`SemanticType` = Prompt / Input / Output). Notable: the semantic type is stored **per cell**
  (`self.pen.set_semantic_type(...)`, `line.semantic_zone_ranges()`), and zones are **recomputed by
  scanning every phys line on demand** (`screen.for_each_phys_line_mut(...)`) — O(cells), in-process,
  whole-buffer. It is never serialized and never correlated by a stateless consumer.

### libghostty-vt — extracted C library with an explicit "render state" API (`ghostty-org/ghostty`, branch `main`, `include/ghostty/vt/`)

- `include/ghostty/vt.h` mainpage: *"libghostty-vt is a C library which implements a modern terminal
  emulator, extracted from the Ghostty terminal emulator… parsing terminal escape sequences,
  maintaining terminal state, encoding input events… scrollback, line wrapping, reflow on resize."*
  → a **render-free reusable engine exposed over a stable-ish C ABI** (lineage A). (Header warns the
  API is WIP/unstable.)
- **`render.h` — the closest analog to justerm's model, but in-process.** `@defgroup render "Render
  State"`: *"Represents the state required to render a visible screen (a viewport)… updates from a
  single terminal instance and only updating dirty regions."* Two-layer dirty tracking:
  `GhosttyRenderStateDirty { DIRTY_FALSE, DIRTY_PARTIAL, DIRTY_FULL }` (global) **plus** per-row
  dirty flags; the caller unsets both after drawing. The renderer holds a **persistent** render-state
  object and calls `ghostty_render_state_update` to pull incremental changes under a lock.
  - **Decisively in-process, not serialized:** `GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR` is documented
    *"Row data is only valid as long as the underlying render state is not updated. It is unsafe to
    use row data after updating the render state."* — the row/cell data are pointers into
    engine-owned memory, shared by lock, never marshaled.
- **Tracked grid refs — engine-owned pins the renderer resolves O(1) (the anti-#482).**
  `include/ghostty/vt/grid_ref_tracked.h`: *"Tracked grid references are owned grid references that
  move with the terminal."* `ghostty_tracked_grid_ref_has_value(ref)` and
  `ghostty_tracked_grid_ref_to_point(...)` let a renderer hold a persistent handle to a buffer
  position and resolve it to a viewport point each frame, **without re-indexing** — the engine keeps
  the pin correct as content scrolls/reflows. This is ghostty's marker/decoration substrate, and it
  works precisely because the consumer shares the engine's address space.

---

## 5. xterm.js decoration model (contrast: live markers + incremental index, in-process)

Repo `xtermjs/xterm.js`, branch `master`.

**Renderer path is O(D), reading `marker.line` live.** `src/browser/decorations/BufferDecorationRenderer.ts`:

```ts
private _doRefreshDecorations(): void {
  for (const decoration of this._decorationService.decorations) {   // iterate the D decorations
    this._renderDecoration(decoration);
  }
  ...
}
```

and each decoration's viewport row is read straight off its marker (O(1)):

```ts
// _refreshStyle:
const line = decoration.marker.line - this._bufferService.buffers.active.ydisp;
if (line < 0 || line >= this._bufferService.rows) { /* off-viewport → hide */ }
...
element.style.top = `${line * ...cell.height}px`;
```

The marker is a **live engine object** whose `.line` the engine mutates in place as the buffer
scrolls; the renderer never re-indexes — it just re-reads. There is no serialization; the renderer
shares the buffer.

**The incremental per-line index is maintained on register/dispose/trim, not per frame.**
`src/common/services/DecorationService.ts`:

- `private readonly _decorations: SortedList<IInternalDecoration>` (sorted by `e => e?.marker.line`),
  `public get decorations(): IterableIterator<...> { return this._decorations.values(); }`, and
  `private readonly _lineCache = this._register(new DecorationLineCache())`.
- `registerDecoration(...)`: on register it does `this._decorations.insert(decoration);
  this._lineCache.add(decoration);` and the marker's `onDispose` does `this._decorations.delete(...)
  ; this._lineCache.remove(decoration)`. **The index mutates on register/dispose, never rebuilt per
  frame.**
- `class DecorationLineCache`: `private readonly _decorationsByLine: Map<number,
  IInternalDecoration[]>`. Its doc: *"Per-logical-line index of decorations for fast cell lookup…
  The index is kept aligned with marker.line updates via buffer line trim/insert/delete events."*
  It subscribes to the buffer's own events: `attachToBufferLines(lines)` registers
  `lines.onTrim / onInsert / onDelete` handlers (`_handleBufferLinesTrim` shifts every bucket key by
  `-amount`). Lookup is O(1): `getDecorationsOnLine(line) { return this._decorationsByLine.get(line);
  }`.

Key point for the contrast: xterm's incremental index is **only possible because the consumer
receives incremental engine events** (`onTrim`/`onInsert`/`onDelete`) and shares the live marker
objects. It is not driven by a per-frame snapshot.

---

## 6. Synthesis for justerm

### 6.1 The two lineages — hypothesis test

| Project | (A) render-free **reusable** engine | (B) serialized state over a **wire** | Consumer holds engine state? |
|---|---|---|---|
| `alacritty_terminal` | **Yes** (`Term`, `RenderableContent<'a>`) | No — borrowed, in-process | Yes (shares grid) |
| libvterm | **Yes** (`VTermScreen` + callbacks) | No — C function-pointer callbacks | Yes (pulls cells) |
| `wezterm-term` | **Yes** ("does not provide any gui") | No — seqno dirty, in-process | Yes (reads `Line`) |
| libghostty-vt | **Yes** (extracted C lib, render-state API) | No — shared memory + lock; "unsafe after update" | Yes (lock-shared) |
| Mosh | **No** (internal to mosh, coupled to SSP) | **Yes** (`Complete::diff_from` → `new_frame`) | Yes (both ends keep a full `Complete`) |
| **justerm** | **Yes** (`justerm-core`) | **Yes** (serialized snapshot + damage wire) | **No** (fresh per-frame snapshot only) |

**Hypothesis (stated):** *"Composing BOTH is unusual — alacritty_terminal/libvterm/wezterm-term
split the engine but render in-process (no serialization); Mosh syncs state but exposes no reusable
engine crate; justerm does both."*

**Verdict: CONFIRMED, with two sharpening corrections from source.**

1. Correct as stated for the four render-free engines: all split engine from renderer but hand off
   **in-process** — by borrow (`RenderableContent<'a>`), by C callback (`VTermScreenCallbacks`), by
   seqno-stamped `Line`, or by lock-shared render state whose *"row data is… unsafe to use after
   updating"* (libghostty). None serialize. Correct for Mosh: it serializes state
   (`diff_from`/`new_frame`) but its terminal is internal, not a reusable engine.

2. **Sharpening #1 — libghostty is closer than the hypothesis implies, but still lineage-A-only.**
   libghostty-vt's `render.h` is almost exactly justerm's design *intent* (a separate render state,
   two-layer global+per-row dirty, "only update dirty regions"). The one axis that still separates
   them is the one that defines justerm: libghostty's render state is **shared memory in the same
   process** (explicitly unsafe to read across an update), whereas justerm's is **serialized and
   consumed by a process that holds no engine state.** So justerm ≠ libghostty precisely on lineage
   (B).

3. **Sharpening #2 — the true novelty is the STATELESS consumer.** Every reference consumer — even
   Mosh's receiver — **retains terminal state** (Mosh's client keeps a full `Complete`/`Framebuffer`
   to diff against). justerm's frame-mode consumer retains **only the current frame's snapshot**.
   That is the actually-unusual property, and it is what creates the #482 cost below.

### 6.2 The #482 cost — is there precedent for "correlate marks against serialized state"?

justerm renders marker-anchored decorations by, each frame, decoding the frame's flat `markerLines`
snapshot (every live marker's absolute buffer line, stride-2) into a `Map` and joining it against
the registry (`justerm-web/src/decorations.ts`, `decorationsForFrame`, `readMarkerLines`). Because
`markerLines` carries **all M live markers** and M is uncapped (`markers_evict_oldest` only drops on
scroll-out; one marker per OSC-133 prompt), the per-frame work is O(M) even for D≪M decorations.

**Does any reference pay an analogous cost? No — and the reason is instructive.** Two cases:

- **No mark concept at all (prior-art GAP):** `alacritty_terminal` and libvterm have **no
  line-level mark/decoration/OSC-133** primitive whatsoever (alacritty: only per-cell OSC 8
  hyperlink + selection word-boundaries; libvterm: only cell attrs + damage rects). The engine/render
  split in these classic libraries simply **omits semantic marks**. justerm carrying markers *on the
  wire* is beyond what these split.

- **Marks exist but live engine-side, referenced O(1) by an in-process consumer:**
  - **ghostty** — a mark is a `GhosttyTrackedGridRef`, an **engine-owned pin the renderer resolves
    O(1) per frame** (`ghostty_tracked_grid_ref_to_point`). No per-frame correlation, no re-index —
    the engine keeps the pin correct across scroll.
  - **xterm.js** — a mark is a **live `Marker` object** whose `.line` the renderer reads O(1)
    (`decoration.marker.line`), with an incremental `_lineCache` kept aligned by
    `onTrim`/`onInsert`/`onDelete` **events**. The renderer never rebuilds an index per frame.
  - **wezterm** — OSC 133 zones exist but are recomputed **engine-side** by scanning the whole buffer
    on demand (`get_semantic_zones`, O(cells)); the cost is paid by the engine, which has the whole
    buffer, not by a stateless consumer.

**Conclusion:** #482 is a **structurally self-inflicted cost of the lineage intersection**, with no
direct prior art. It arises *only* because justerm (a) serializes marks into every frame for (b) a
consumer that keeps no persistent marker state. Every reference avoids it either by not having marks
(alacritty/libvterm) or by keeping marks as persistent engine-side objects that an in-process
consumer references by handle (ghostty/xterm/wezterm). The **closest prior-art remedy is ghostty's
tracked-ref / xterm's live-marker+event-index model**: take markers *off* the per-frame snapshot and
give the consumer a persistent, event-updated marker structure.

### 6.3 Techniques justerm could adopt *within* frame-mode (consumer holds only a per-frame snapshot)

For each, "compatible?" means: does it work when the consumer re-receives a fresh snapshot every
frame and keeps no engine state?

**(a) Incremental line-index like xterm's `_lineCache` — NOT compatible as-is; compatible only if
markers leave the per-frame snapshot.**
xterm's `_lineCache` is cheap because it is *mutated by incremental engine events* (register/dispose,
`onTrim`/`onInsert`/`onDelete`) and never rebuilt. justerm's consumer has no such event feed for
marker *positions* — it receives the **whole `markerLines` snapshot afresh each frame**, so any index
built from it must be rebuilt each frame = O(M) = no win over the current code. The technique becomes
possible only under a **wire-model change**: move marker position updates onto an **out-of-band,
persistent event channel** (register / dispose / bulk line-shift-on-scroll), mirroring xterm's
buffer events, so the consumer maintains a durable `markerId → line` map updated in O(Δ). Precedent
already exists in-repo: `onMarkerDisposed` (#160) is exactly such an out-of-band marker event — the
positions are the missing half. This is the prior-art-informed direction (ghostty tracked refs /
xterm markers), but it is an architecture change, not an in-frame-mode optimization.

**(b) Mosh-style state diffing — compatible but limited; helps only the unchanged case.**
Mosh diffs two full states held on both ends, with `Row.gen` as a fast "rule out identical" check.
A justerm consumer *could* retain last frame's `markerLines` (plus a scroll generation) and skip the
whole projection when neither markers nor scroll changed — a coarse `gen`/hash guard. **Compatible**
(the consumer opts to keep one extra frame of state) and cheap, but it does **not** reduce the
steady-state cost: a single scroll changes every marker's viewport row, so a scrolling session — the
exact case where M is large — still pays O(M) every frame. Use it as an early-out, not a fix.

**(c) Damage-scoped work — NOT applicable to the marker join.**
justerm already emits per-line damage, and damage-scoping is right for *cell* work. But marker
anchors are keyed to **absolute buffer lines**: on scroll, every marker's viewport row changes while
its absolute line and its cell content do not, so damage (a cell-content signal) does not bound the
marker-correlation work. A *scroll-generation* guard (part of (b)) is the applicable lever here, not
line-damage.

**Bottom line for #482.** Within strict frame-mode, the tractable move is the one the issue already
proposes — make the join O(D) by iterating the registry (`byMarker`) rather than seeding an M-sized
`anchors` map (the decode of the wire snapshot stays O(M), because a serialized snapshot for a
stateless consumer is inherently O(M) to receive). The only way to get *below* O(M) per frame is the
prior-art model from ghostty/xterm: stop serializing marker positions into every frame and give the
consumer a persistent, event-updated marker index — accepting that this trades a slice of the
"stateless consumer" purity (§6.1 sharpening #2) for O(Δ) marker maintenance.

---

## References

Each source cited by repo + path (default branch noted where non-`master`).

- **alacritty_terminal** — `alacritty/alacritty`:
  - `alacritty_terminal/src/term/mod.rs` — `Term<T>`, `TermDamage`, `LineDamageBounds`,
    `TermDamageState`, `damage()`/`reset_damage()`, `RenderableContent<'a>`, `RenderableCursor`,
    `renderable_content()`, `set_hyperlink`, `SEMANTIC_ESCAPE_CHARS`.
  - `alacritty_terminal/src/grid/mod.rs` — `Grid<T>`.
  - `alacritty_terminal/src/term/search.rs` — `semantic_search_left/right` (selection boundaries).
  - `alacritty_terminal/src/vi_mode.rs` — `ViMotion::Semantic*`.
- **libvterm** (read via neovim's vendored copy) — `neovim/neovim`:
  - `src/nvim/vterm/vterm_defs.h` — `VTermScreenCallbacks` (`damage`, `moverect`, `movecursor`,
    `settermprop`, `bell`, `resize`, `sb_pushline`, `sb_popline`, `sb_clear`), `VTermRect`,
    `VTermDamageSize`.
  - `src/nvim/vterm/vterm.h` — `struct VTermScreen { ... VTermRect damaged; VTermRect
    pending_scrollrect; ... }`.
  - GAP: verified against neovim's vendored tree, not diffed line-for-line against the canonical
    upstream libvterm mirror.
- **Mosh** — `mobile-shell/mosh`:
  - `src/terminal/terminalframebuffer.h` — `Cell`, `Row` (with `gen` + `operator==`), `Framebuffer`,
    `DrawState`.
  - `src/terminal/terminaldisplay.h` / `src/terminal/terminaldisplay.cc` — `Display::new_frame(
    bool initialized, const Framebuffer& last, const Framebuffer& f )`, `put_row` (per-cell skip),
    scroll-region detection.
  - `src/statesync/completeterminal.h` / `src/statesync/completeterminal.cc` — `Complete`,
    `diff_from` → `display.new_frame(...)`, `apply_string`, `get_fb`.
- **wezterm-term** — `wezterm/wezterm` (branch `main`):
  - `term/src/lib.rs` — crate doc ("does not provide any kind of gui"), `advance_bytes` mention,
    `SequenceNo` import.
  - `term/src/screen.rs` — `Screen`, `dirty_line`, `update_last_change_seqno`.
  - `term/src/terminalstate/mod.rs` — `get_semantic_zones() -> Vec<SemanticZone>`,
    `set_semantic_type`, `for_each_phys_line_mut`.
- **libghostty-vt** — `ghostty-org/ghostty` (branch `main`):
  - `include/ghostty/vt.h` — library overview, `@ref render` / `@ref terminal` groups.
  - `include/ghostty/vt/terminal.h` — terminal options/callbacks (OSC 133/OSC 7 handling surfaced as
    host callbacks).
  - `include/ghostty/vt/render.h` — `GhosttyRenderStateDirty` (FALSE/PARTIAL/FULL) + per-row dirty,
    `GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR` ("unsafe to use row data after updating").
  - `include/ghostty/vt/grid_ref_tracked.h` — `GhosttyTrackedGridRef`, `..._has_value`, `..._to_point`.
- **xterm.js** — `xtermjs/xterm.js`:
  - `src/browser/decorations/BufferDecorationRenderer.ts` — `_doRefreshDecorations` iterating
    `_decorationService.decorations`, `_refreshStyle` reading `decoration.marker.line`.
  - `src/common/services/DecorationService.ts` — `_decorations: SortedList`, `_lineCache`,
    `registerDecoration` add/remove, `class DecorationLineCache` (`_decorationsByLine`,
    `attachToBufferLines` → `onTrim`/`onInsert`/`onDelete`, `getDecorationsOnLine`).
- **justerm (local)** — for grounding the synthesis:
  - `justerm-web/src/decorations.ts` — `DecorationRegistry`, `decorationsForFrame`,
    `rulerMarksForFrame`, `readMarkerLines` (`MARKER_LINE_STRIDE`).
  - GitHub issue **#482** — the O(M)-per-frame marker-walk regression this note investigates.
