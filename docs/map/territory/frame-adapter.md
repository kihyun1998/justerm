# Territory — frame adapter

## What it is

The join between what the engine sends and what the renderer packs. The packer reads a **dense
row-major** grid (`row * cols + col`); a decoded `Partial` frame — the common case after the first —
carries only the damaged cells in span order, with a directory saying where each span sits. The
adapter keeps one persistent dense `FrameGrid` per renderer grid and scatters each frame's damage
into it, so the packer always sees a coherent full viewport. It also owns the widget-side encoders
that assemble `apply_damage`'s header.

## Governing decisions

**None.**

- [ADR-0003 — damage model](../../adr/0003-damage-model-incremental-bounds.md) creates the shape
  this adapts from — spans with column bounds.
- [ADR-0011 — justerm-web keeps a viewport cell mirror](../../adr/0011-justerm-web-viewport-cell-mirror.md)
  decided the web's mirror, whose scatter (`cell-mirror.ts`: wipe on Full, shift before spans,
  scatter span-ordered cells) `FrameGrid` ports; it decides nothing about this module.
- [ADR-0027 — liveness is answered by the source that owns it](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  struck its `apply_damage` row (#774): the entry point holds no GL context and only scatters, so it
  has no liveness question. That records a property of this area; it does not govern it.
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) decides the crate, not this.

## Design model

Status: from the source, its tests and the comments in `serialize.rs`; no record decides it.

- **The mismatch is the whole territory.** Sparse span order versus dense indexing: feeding spans
  straight to the packer misaligns `bg[row * cols + col]` and silently repaints undamaged cells as
  Default — a wrong picture, not a crash (`frame_grid.rs` module doc, #277).
- **The grid persists across frames**, which is what makes a `Partial` meaningful: a frame of three
  spans states a change, and the unchanged remainder comes from here. A context loss invalidates GPU
  state but not this grid, so a restore re-packs without a re-send from the engine
  ([GL context lifecycle](gl-context-lifecycle.md)).
- **Scatter, then defer the pack.** `GridTier::apply_damage` scatters, stores the blink phase in
  `last_blink_on`, sets `needs_repack` and returns; the pack runs once in `render` however many
  setters surround it (#421). It never touches GL, which is why ADR-0027 struck its row.
  `apply_frame` is the other entry: dense, cluster-free, packs immediately, and is called by renderer
  demos and proofs only — `justerm-web` calls `apply_damage` alone.
- **Every index is bounded as a caller error, because the caller is JS** (#355). The wire caps
  `cols`/`rows` at `u16` (`MAX_COLUMNS`, `MAX_ROWS`), so no frame from core names more than
  65535 × 65535 cells, but `apply_damage` reads its header from JS and `apply_frame` takes bare
  `u32`s. `cell_count` is checked before any per-cell vector is reserved — in `resolve_and_pack` as
  well as in `resolve_frame`, because guarding only the pure layer left the panic one frame earlier;
  `resolve_frame` re-checks because it is a separately tested public surface. Span and scroll
  indices are validated in `FrameGrid::validate` **before the first write**: an unchecked
  `line == rows` (an off-by-one) used to trap the module (`RuntimeError: unreachable`) and poison it
  ("recursive use of an object"), and checking as the scatter went left a refused frame half-applied
  (#355). A refused frame leaves the grid untouched and is thrown as a string. That bound is on
  indices, not on memory: see Known holes.
- **A Full frame ignores its scroll op.** Core ships every row as a span, and
  `Term::mark_fully_damaged` sets full damage without clearing `scroll`, so an alt-screen switch can
  leave a stale scroll op on a Full frame. `validate` checks the scroll region only when
  `kind != 0`. The decoder is one notch stricter: `decode` rejects `bottom >= rows` on any frame,
  because `cell-mirror.ts` still shifts after blanking on a Full frame (`serialize.rs`, #582).
- **An over-height scroll `count` is tolerated, not refused**: `shift_row` bounds the source row by
  `[top, bottom]`, so every row blanks and the spans repaint it. Core stopped producing one in #661
  (`Term::scroll_delta` caps at the region height and `MAX_SCROLL_COUNT`); `decode` deliberately
  still accepts a foreign one, measured across the whole `i16` range to return `Ok` and blank the
  region (`serialize.rs`, #661).
- **A cluster index is resolved to text at scatter time**, because `extra` indexes the *frame's*
  `side_table` and the next frame's table differs. The table holds only trailing combining marks, so
  the stored text is base codepoint + marks. The column is `u32`, as core emits it (#621/#627): the
  old `u16` read index 65536 as "no cluster" and 65537 as the wrong one, silently.
- **The damage entry point's arguments** (`JustermRenderer::apply_damage`). Eight array arguments —
  the header, the span directory, five cell columns (`codepoints`, `fg`, `bg`, `flags`, `extra`) and
  the side table — arrive separately because each is a distinct typed-array or `Vec` view at the
  wasm-bindgen boundary; grouping them would need an array-of-structs rewrite that breaks the
  zero-copy struct-of-arrays transport. The underline colour column (#520, SGR 58) is optional and
  trailing, on `apply_frame` too: a caller that predates it keeps working, and omitted means every
  underline follows the fg. Whether `extra` is a zero-copy identity is decided by the **frame
  producer's** decoder: `justerm-wasm-decode` ≥ 0.12.0 hands a `Uint32Array`, 0.11.0 a `Uint16Array`
  that `asU32` widens with one copy (#633). The widget calls `decodeFrame` nowhere, so its own pin
  decides nothing here; PenTerm decodes in `penterm 7dadc3c95 src/blocks/terminal/lib/nativeFrameSource.ts
  — createNativeFrameSource` and pins `justerm-wasm-decode` 0.30.0.
- **The widget's wire encoders** (`justerm-web/src/renderer-wire.ts`). Pure, so the wire assembly is
  unit-testable with no backend. `damageHeader` builds the eight-slot header: a negative
  `scrollCount` rides its `u32` slot as two's complement and the renderer reads it back
  `as i32 as i16`; `blinkOn` defaults to `true` so a caller with no phase shows blinking text, while
  the widget's `applyFrame` passes `TextBlink`'s current phase (#576).
  `blinkPhaseHeader` is the **phase-only** re-issue: an empty damage carrying only the new phase
  makes the renderer re-pack its retained grid, so text blink needed no renderer or wire change. Its
  `kind` is **Partial**, and that is load-bearing: a Full header wipes the grid before scattering
  nothing. `cols`/`rows` must equal the grid the renderer holds, because `GridTier::apply_damage`
  allocates a fresh empty `FrameGrid` on any dimension mismatch — the same blank; the widget refuses
  the flip while the last frame's grid disagrees with the backend's. `carriesBlink` gates the
  re-pack and takes the flag column, so a `number[]` fixture and a `Uint16Array` share one path.
- **Input bound.** Span count, cell columns and side table are bounded by the caller's arrays; the
  scatter reads only what `validate` admitted. The grid's size is bounded only by `u32` (Known holes).

## Code

- `justerm-renderer/src/frame_grid.rs` — `FrameGrid` (`try_new`, `validate`, `apply`,
  `shift_region`, `shift_row`), `DamageFrame`, `DamageError`, `cell_count`, `SPAN_STRIDE`; test
  `a_span_pointing_outside_the_grid_is_refused_and_changes_nothing`
- `justerm-renderer/src/webgl.rs` — `GridTier::apply_damage` (header parse, grid re-create on a
  dimension change, `last_blink_on`, `needs_repack`)
- `justerm-renderer/src/webgl/grid_state.rs` — `JustermRenderer::apply_damage` (the wasm export)
- `justerm-renderer/src/webgl/draw.rs` — `JustermRenderer::apply_frame`,
  `JustermRenderer::resolve_and_pack` (the first `cell_count` check),
  `JustermRenderer::repack_from_grid`
- `justerm-renderer/src/glyph_resolve.rs` — `resolve_frame`, `ResolveError` (its grid-overflow
  and frame-shorter-than-grid refusals)
- `justerm-web/src/renderer-wire.ts` — `damageHeader`, `blinkPhaseHeader`, `carriesBlink`, `asU32`,
  `asU16`
- `justerm-web/src/justerm-renderer.ts` — `applyFrame`, `repackAtTextBlinkPhase`, `trackBlinkCells`
- `justerm-core/src/serialize.rs` — `decode` (the scroll-region check), `MAX_SCROLL_COUNT`
- `justerm-core/src/term.rs` — `Term::mark_fully_damaged`, `Term::scroll_delta`

## Reference behaviour

**None.** Whether the references carry an equivalent seam, or avoid it by having their frontend
hold the model, has never been checked.

## Cross-cutting invariants

**None.**

## Blast radius

- [damage](damage.md) — the span shape this adapts from; a change to what a `Partial` contains lands
  here first, and the failure is a wrong picture, not an error.
- [frame](frame.md) · [wire format](wire-format.md) — the decoded columns, the header scalars and
  their widths are this module's input contract; a column narrowed or reordered misreads silently.
- [wasm decoder](wasm-decoder.md) — the producer's decoder version decides the typed array `extra`
  arrives as, and the decoder's scroll check is deliberately stricter than `validate`.
- [cell compositing](cell-compositing.md) — packs the dense grid and would misread a sparse one
  without erroring.
- [GPU upload](gpu-upload.md) — the diff assumes this grid is current; a cell left out of the
  scatter is invisible to it.
- [GL context lifecycle](gl-context-lifecycle.md) — a restore re-packs from this grid on the
  assumption that the loss did not touch it.
- [widget lifecycle](widget-lifecycle.md) — the blink loop's phase re-issue assumes the backend
  still holds the grid of the last frame.

## Known holes / open

- **The grid's memory is bounded only by `u32`.** `FrameGrid::try_new` refuses `cols × rows` past
  `u32::MAX` and nothing smaller, and allocates six per-cell vectors before any data is read; a JS
  header naming e.g. 60000 × 60000 asks wasm32 for more than its address space. By reading (not
  measured), that is an allocation failure — a trap — rather than the thrown refusal #355 made the
  rule. `resolve_frame` bounds `apply_frame` by the data it carries (`FrameShorterThanGrid`);
  `apply_damage` has no equivalent before the allocation.
- **Nothing states who owns re-synchronisation**, or how a consumer detects that this grid and the
  engine's screen disagree. A consumer can request a Full frame with `Engine::mark_fully_damaged`
  (its doc names reattach), but nothing in the family says when to. PenTerm drops an undecodable
  frame on the assumption that "the next full or partial frame repaints over it"
  (`penterm 7dadc3c95 src/blocks/terminal/lib/nativeFrameSource.ts — createNativeFrameSource`); a
  dropped `Partial`'s cells are not in the next `Partial`, so the grid stays stale until a Full frame.
- No record governs a seam whose failure mode is a plausible-looking wrong picture.
