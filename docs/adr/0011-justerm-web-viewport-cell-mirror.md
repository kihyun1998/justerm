# ADR-0011: justerm-web keeps a viewport cell mirror (frame mode) to apply scroll-op damage

Status: accepted (2026-06-29, #106 / S3)

## Context

`justerm-web`'s S2 renderer is **mirror-less**: `frameToDrawOps` walks a `DecodedFrame`'s span
directory straight into beamterm `batch.cell` calls, and beamterm **retains** untouched cells between
frames — so damage-only Partial frames work without the web holding any grid (S2, #105).

S3 (#106) breaks that for one frame kind. A **scroll-op** Partial frame carries
`(top, bottom, count)` plus spans for the newly-exposed line(s) only. To render it the consumer must
**move the region's cells to their new rows** and then paint the new spans — but the cells that moved are
*not* in the frame (the scroll op stands in for re-sending them; that compression is the whole reason the
op exists in the wire format, ADR-0005). beamterm cannot supply them either:

- it has **no scroll / shift / move primitive** (verified: `beamterm_renderer.d.ts` exposes
  `batch.cell` / `batch.clear` / `render`, `copyToClipboard`, modifier keys — nothing that relocates
  retained cells);
- its only readback, `getText(query: CellQuery): string` (`.d.ts:120`), returns **text only** — no
  fg/bg/flags — so cells read back from beamterm cannot be repainted with their styling.

In **frame mode** the authoritative grid lives in the *backend* (`justerm-core`, ADR-0009); only a
viewport snapshot + damage cross the wire (ADR-0003/0005). The web therefore has **no local styled cells
to shift** unless it keeps them itself.

### Prior art — read at source this session, three independent renderers converge

| renderer | local styled buffer | viewport scroll | source read |
| --- | --- | --- | --- |
| **xterm.js v6** | `Buffer.lines: CircularList<BufferLine>`; `BufferLine._data: Uint32Array`, `CELL_SIZE=3` → `[content, FG, BG]` per cell | `ydisp` / `ybase` offsets; renderer re-reads `lines` | `common/buffer/Buffer.ts:29-31`, `BufferLine.ts:22,62` |
| **alacritty** | `Grid.raw: Storage<Row>` (one ring, `zero` index) | `display_offset`; `scroll_display` adjusts it | `grid/storage.rs:33`, `grid/mod.rs:121,134,163` |
| **penterm terminal-native** | `CellGrid` (`grid[y][x]` cell objects) — **the exact frame-mode + beamterm case** | — (backend-owned) | `terminal-native/lib/renderToBeamterm.ts` |

All three keep a **local styled grid the renderer reads**; none rely on the GPU/DOM sink to re-supply a
shifted region. (alacritty/xterm were *also* read at source for ADR-0009's core scroll decision; this ADR
reuses that reading for the renderer-side question.)

**Shared-cause check** (CLAUDE.md; the discipline ADR-0009 applied to reject Route B). Surface
similarity is not enough — does the *cause* transfer? xterm and alacritty keep a buffer because their
renderer needs **random-access styled cells to repaint a scrolled/shifted region** (and for selection /
search). That exact cause is present for `justerm-web` + beamterm: beamterm cannot relocate retained
cells nor return their style, so a scroll-op frame is un-renderable without a local styled copy. The
cause genuinely transfers — and penterm, the *same* frame-mode + beamterm pairing, already resolved it
with a `CellGrid` mirror. This is a same-constraint precedent, stronger than the xterm/alacritty analogy.

**Where the cause stops — scope is viewport, not screen+scrollback.** xterm's `CircularList` and
alacritty's `Storage` hold screen **+ thousands of scrollback rows** in one ring. That part does **not**
transfer: `justerm-core` already **splits** screen and scrollback (ADR-0009), and *"the wire format
already erases the screen-vs-history distinction; the consumer never sees the boundary"* (ADR-0009,
Context). The backend owns the scrollback, the `display_offset`, and follow-bottom — the last verified
in alacritty's `Grid::scroll_up` (`mod.rs:267-268`: a new-output scroll bumps `display_offset` only when
the user has scrolled up), which is **engine-side**, not renderer-side. So the web needs only a copy of
the **current viewport**, not a scrollback duplicate. Mirroring screen+scrollback would be the *maximal*
grain; the *correct* grain is the viewport (CLAUDE.md; ADR-0009's same conclusion for the core ring).

## Decision

`justerm-web` (frame mode) maintains a **viewport cell mirror** — a `rows × cols` grid of styled cells
(structure-of-arrays, matching the decoder's `DecodedFrame` columns), optionally widened by an
**overscan band** of a few rows for smooth scrolling.

`applyFrame(frame)` becomes:

1. **scroll op** (`hasScroll`): shift the mirror's `[top, bottom]` rows by `count` in place; clear the
   newly-exposed rows. The shifted *and* exposed cells are the scroll damage.
2. **spans**: apply the frame's span cells into the mirror (the explicit damage).
3. **emit**: produce draw ops for the union of (1)+(2)'s changed cells by reading the mirror and running
   them through S2's per-cell mapper (`resolveRgb` → flags → wide-char → grapheme → `RenderPolicy`).
4. **full frame** (`kind = 0`): rebuild the mirror from the frame and repaint every cell.

The mirror is **frame-mode-specific**. In the in-wasm north star the engine runs in the browser and its
own grid *is* the local styled buffer (xterm/alacritty's situation); the `FrameSource` seam lets the
renderer read that grid directly and the mirror **collapses into the engine's grid** — it is replaced,
not extended.

`display_offset` / follow-bottom / alt-screen scroll reset stay **backend-owned** (frame mode); the web
computes scroll **intent** (wheel/key → lines, S3's pure half) and renders the frames the backend emits.

## Consequences

- **Scroll-op Partial frames render correctly** without a beamterm shift primitive, and the wire keeps
  its scroll-op compression — no full-viewport retransmit per scrolled line (which would undo ADR-0009's
  whole point on the backend and ADR-0003's damage minimisation on the wire).
- **S2 is reused, not discarded.** `frameToDrawOps`'s cell→op mapping survives intact as the mapper in
  step 3; the mirror is a new *model* layer in front of it. The signature shifts from "frame → ops" to
  "mirror damage → ops" ("what new code reveals about existing code", the TDD refactor step).
- **The mirror is the local styled store later slices need.** Selection (#109) and search highlight
  (#110) need cell positions/styling that beamterm's text-only `getText` can't give; they read the
  mirror.
- **Bounded cost.** Viewport-only: `rows × cols` cells (80×24 ≈ 1920) + overscan — kilobytes, not the
  scrollback. No unbounded growth.
- **Clean in-wasm migration.** Because the mirror is viewport-scoped and sits behind `FrameSource`, the
  in-wasm engine replaces it at one seam rather than the renderer having duplicated the engine's
  scrollback.

## Alternatives considered

- **Mirror-less; backend sends a Full frame on every scroll.** Rejected — O(rows × cols) cells on the
  wire per scrolled line, which contradicts the scroll-op's reason for existing (ADR-0005) and the
  damage-minimisation lineage (ADR-0003, Mosh). The scroll op is in the format precisely to avoid this.
- **Read cells back from beamterm via `getText` and repaint.** Rejected — `getText` returns text only;
  every scroll would drop fg/bg/flags/wide-char. Styling cannot survive a round-trip through beamterm.
- **Mirror screen + scrollback (xterm `CircularList` / alacritty `Storage` 1:1).** Rejected — the
  backend owns scrollback (ADR-0009 split) and the wire erases the boundary, so the web would be
  duplicating state it never needs to address. The analogy's *cause* covers a local styled grid, not its
  *scope*; viewport is the correct grain.
