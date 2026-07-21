/**
 * Marker-anchored decorations (#120 S1), the frame-mode analog of xterm's
 * `DecorationService`. A consumer registers a decoration against a **marker id**
 * (markers originate in core and ride the wire as `markerPositions` per frame —
 * justerm has no local `registerMarker`), and each frame the registry joins its
 * decorations with the frame's markers to project on-viewport {@link
 * DecorationRect}s: positions + **absolute** `0xRRGGBB` colours (the consumer resolves
 * its theme before pushing; the renderer uses them verbatim, #393/#408 — unlike a *cell*
 * colour, which ships as a ref). The paint is the renderer's (ADR-0017, #115) — this is
 * the model + lifecycle, no DOM.
 *
 * Rendering the rects (2-layer cell override, overview-ruler) is S2 (#198) / S3
 * (#199); this slice ships the registry, the per-frame projection, and marker
 * auto-dispose.
 */

import { readMarkers } from "./markers";

/** Which layer a decoration paints on, mirroring xterm's `IDecorationOptions.layer`:
 * `bottom` overrides the cell background *under* the glyph, `top` paints *over* it. */
export type DecorationLayer = "bottom" | "top";

/** Where on the overview-ruler track a mark sits across its width (#120 S3),
 * mirroring xterm's `IDecorationOverviewRulerOptions.position`. `full` spans the
 * whole width; the others are gutter columns. */
export type RulerPosition = "left" | "center" | "right" | "full";

/** Overview-ruler options for a decoration (#120 S3): a mark on the scrollbar at
 * the marker's buffer-relative position, so off-viewport anchors are visible.
 * `color` is an absolute packed `0xRRGGBB` (consumer-resolved, like `bg`/`fg`). */
export interface OverviewRulerOptions {
  /** Mark colour (opaque ref; the consumer/renderer resolves it). */
  readonly color: number;
  /** Where across the ruler width the mark sits (default `full`). */
  readonly position?: RulerPosition;
}

/** One overview-ruler mark projected for a frame (#120 S3): its position down the
 * track as a `0..1` ratio (the marker's absolute line ÷ total content lines), its
 * colour, and its across-width placement. The scrollbar renders it. */
export interface RulerMark {
  readonly topRatio: number;
  readonly color: number;
  readonly position: RulerPosition;
}

/** Options for {@link DecorationRegistry.register}, the subset of xterm's
 * `IDecorationOptions` this slice models. `bg`/`fg` are **absolute** packed `0xRRGGBB`
 * (the consumer resolves its theme; the renderer uses them verbatim, not re-resolved — #393/#408).
 *
 * Deferred (tracked, not silent — the 2-lens pass surfaced these): xterm's
 * `overviewRulerOptions` → S3 (#199); `height` (multi-row span) and `anchor`
 * ('left'/'right') → S2 (#198). Adding them is additive (optional fields), and
 * multi-row will project as N single-row {@link DecorationRect}s (so the rect
 * shape stays single-row and a renderer's per-cell test stays `highlightAt`-like)
 * — no breaking change to this type, so modelling them before a renderer uses
 * them would be speculative. */
export interface DecorationOptions {
  /** The marker this decoration anchors to (its row is read per frame). */
  readonly markerId: number;
  /** Column offset relative to the anchor (default 0). */
  readonly x?: number;
  /** Column span width (default 1 — a single cell). */
  readonly width?: number;
  /** Row span (default 1); a decoration `height` rows tall extends DOWN from the
   * marker's row (#202, xterm `top = marker.line`). Projected as one single-row
   * {@link DecorationRect} per covered row, clipped to the viewport bottom. */
  readonly height?: number;
  /** Which edge `x` is measured from (#202, default `left`). `right` counts `x`
   * cells in from the right edge, the span extending leftward by `width`.
   *
   * **Deliberate divergence from xterm (#459).** xterm's *colour* hit test ignores `anchor`
   * entirely (`DecorationService.forEachDecorationAtCell` computes `xmin = x`, `xmax = xmin +
   * width`, with no anchor term), so there a right-anchored decoration's background still paints
   * from the LEFT edge; `anchor` moves only its DOM element (`BufferDecorationRenderer` sets
   * `style.left`/`style.right` from it — the one place xterm reads the option). justerm has no
   * decoration element **in the grid**: a decoration here is its cell colours plus an optional
   * scrollbar ruler mark, and that mark's across-track placement comes from
   * {@link OverviewRulerOptions.position}, never from `anchor`. So ignoring `anchor` in the colour
   * span would leave the option with nothing at all to affect — a dead field. Honouring it is the
   * only reading under which it means anything, and it is pixel-proven end to end by the #457 e2e,
   * which drives a right-anchored decoration wider than the viewport and reads both edges.
   *
   * xterm's own typings also read our way: `anchor` is "Where the decoration will be anchored —
   * defaults to the left edge", and `x` is documented as "The x position offset **relative to the
   * anchor**" — which its colour path then contradicts by measuring `x` from the left regardless.
   * So this is the doc-conformant behaviour and upstream's colour path is the outlier, not a
   * contract justerm is departing from. */
  readonly anchor?: "left" | "right";
  /** Paint layer (default `bottom`). */
  readonly layer?: DecorationLayer;
  /** Background colour override ref (opaque; resolved by the renderer). */
  readonly bg?: number;
  /** Foreground colour override ref (opaque; resolved by the renderer). */
  readonly fg?: number;
  /** Overview-ruler mark options (#120 S3). Absent → no ruler mark (a cell-only
   * decoration). Independent of `bg`/`fg`: a decoration can do either or both. */
  readonly overviewRulerOptions?: OverviewRulerOptions;
}

/** A live decoration handle. Disposing it (or a {@link
 * DecorationRegistry.onMarkerDisposed} for its marker) stops it projecting. */
export interface Decoration {
  /** Remove the decoration; idempotent. */
  dispose(): void;
  /** Whether this decoration has been disposed. */
  readonly disposed: boolean;
}

/** A decoration projected onto the viewport for one frame: the marker's current
 * `row`, the inclusive column span `left..=right` (matching `overlay.ts`
 * `HighlightSpan`, so a renderer's per-cell test is `col >= left && col <=
 * right`), the layer, and the absolute `0xRRGGBB` colours (used verbatim). */
export interface DecorationRect {
  readonly row: number;
  readonly left: number;
  readonly right: number;
  readonly layer: DecorationLayer;
  readonly bg?: number;
  readonly fg?: number;
}

/** The frame fields the registry reads. A `DecodedFrame` satisfies it structurally.
 * `cols` sizes right-anchored spans; `rows` clips a multi-row `height` (#202). */
interface DecorationFrame {
  readonly markerPositions?: ArrayLike<number>;
  /** Every live marker's ABSOLUTE buffer line (the v11 group), including markers scrolled off
   * the viewport — which `markerPositions` omits. Needed to place a multi-row decoration whose
   * anchor is above the top (#461). With `scrollbackLen`/`displayOffset` it wins per marker;
   * a marker carried only by `markerPositions` still resolves from its viewport row. */
  readonly markerLines?: ArrayLike<number>;
  readonly displayOffset?: number;
  readonly scrollbackLen?: number;
  readonly cols?: number;
  readonly rows?: number;
}

/** Internal decoration record — also the public {@link Decoration} handle. Its
 * `dispose` closes over the registry so the handle removes itself. */
interface StoredDecoration extends Decoration {
  readonly markerId: number;
  readonly x: number;
  readonly width: number;
  readonly height: number;
  readonly anchor: "left" | "right";
  readonly layer: DecorationLayer;
  readonly bg?: number;
  readonly fg?: number;
  readonly overviewRulerOptions?: OverviewRulerOptions;
  disposed: boolean;
}

export class DecorationRegistry {
  /** Decorations grouped by anchor marker id, so `onMarkerDisposed` and the per-frame
   * marker-id filter (#482) are both O(decorations-on-that-marker). This is the *index*;
   * it does not decide precedence. */
  private readonly byMarker = new Map<number, Set<StoredDecoration>>();
  /** Every live decoration in **registration order** (a `Set` preserves insertion order) —
   * the *cell* projection order, and therefore the cell precedence order (#458): the renderer resolves
   * per-property last-in-wire-order (#452), so the last registered decoration wins a cell.
   * Kept alongside `byMarker` rather than derived from it, because a per-marker grouping can
   * only ever express order *within* a marker; across markers it would leak core's marker
   * emission order into consumer policy. The RULER projection partitions this order by position
   * class (#498) and is stable within each class, so it is not simply this order. */
  private readonly inRegistrationOrder = new Set<StoredDecoration>();

  /**
   * Register a decoration anchored to `options.markerId`. Returns a handle whose
   * `dispose()` removes it. Registering against a marker id that never appears in
   * a frame is a harmless no-op — the handle simply never projects. (Unlike xterm
   * there is no marker object to guard on `isDisposed`, and marker ids are reused
   * by a full reset, so there is no permanent reject-set — disposal is purely
   * event-driven via {@link onMarkerDisposed}.)
   *
   * Registration order is **precedence** order (#458): where two decorations set the same
   * property on the same cell, the one registered later wins, whichever markers they anchor to.
   * To raise an existing decoration above its peers **on a cell**, `dispose()` its handle and
   * register again — note this does not apply to ruler marks, where a gutter mark can never rise
   * above a `full` one whatever the registration order (#498) —
   * calling `register` alone mints a *second* decoration, leaving the first live (still projecting,
   * still ruler-marking, and it takes over again if the new one is disposed).
   */
  register(options: DecorationOptions): Decoration {
    const d: StoredDecoration = {
      markerId: options.markerId,
      x: options.x ?? 0,
      width: options.width ?? 1,
      height: options.height ?? 1,
      anchor: options.anchor ?? "left",
      layer: options.layer ?? "bottom",
      bg: options.bg,
      fg: options.fg,
      overviewRulerOptions: options.overviewRulerOptions,
      disposed: false,
      dispose: () => this.remove(d),
    };
    let set = this.byMarker.get(d.markerId);
    if (!set) {
      set = new Set();
      this.byMarker.set(d.markerId, set);
    }
    set.add(d);
    // Load-bearing: `d` is always a FRESH record, so this appends. `Set.add` of a member already
    // present is a no-op that does NOT move it to the end — an "update these options in place"
    // convenience would therefore silently stop a re-registration from taking precedence (#458).
    this.inRegistrationOrder.add(d);
    return d;
  }

  /**
   * Dispose every decoration anchored to `markerId` — the backend's
   * `MarkerDisposed` event (out-of-band from frames, like #160's), which the
   * consumer forwards here. Mirrors xterm's `marker.onDispose(() =>
   * decoration.dispose())`: a trimmed/reset marker takes its decorations with it,
   * so a reissued id never inherits a stale decoration.
   */
  onMarkerDisposed(markerId: number): void {
    const set = this.byMarker.get(markerId);
    if (!set) return;
    // dispose() mutates `set` via remove(); iterate a snapshot.
    for (const d of [...set]) d.dispose();
  }

  /**
   * Project the registry onto one frame: emit a {@link DecorationRect} per decoration per
   * covered viewport row, joining each decoration's marker id against the frame.
   *
   * The join merges the two marker groups **per marker**: the absolute `markerLines` line (plus
   * the frame's scroll position) wins where a marker has one, and a marker carried only by the
   * viewport-relative `markerPositions` still resolves from its row.
   * That distinction matters: `markerPositions` omits a marker scrolled ABOVE the viewport top
   * (core drops it, `m.line.checked_sub(top)?`), so joining on it alone made a multi-row
   * decoration whose anchor had scrolled off vanish **entirely** instead of showing the rows
   * of it that are still on screen (#461). xterm has no such gap — it keys colour lookup to
   * the absolute buffer line and buckets every line the height covers.
   *
   * Emission order is **registration order** (#458), so where two decorations cover the same
   * cell the LAST registered one wins — the renderer resolves per-property last-in-wire-order
   * (#452). Precedence is consumer policy (ADR-0017) and therefore follows the consumer's own
   * input, never core's marker emission order, which is decided by where the anchors sit in the
   * buffer and cannot be influenced from here. It matches xterm's documented contract
   * (`typings/xterm.d.ts`: "the last registered decoration will be used") — and is in fact
   * *stronger*: xterm's ordering is per buffer LINE (a cell only ever consults that line's bucket,
   * `DecorationService.getDecorationsAtCell`), and buffer motion re-appends a decoration that spans
   * an insert/delete point (`_reindexDecoration`, and the insert path's `spanCrossers`), promoting
   * it to "last" — so upstream a `height > 1` decoration's precedence can change when the buffer
   * moves. Here the order is the consumer's registration sequence and nothing on the wire can
   * perturb it. (Not, as an earlier draft of this comment claimed, `_mergeLineBucket`'s concat
   * branch: every line-key remap upstream is injective, so that branch cannot fire.)
   */
  decorationsForFrame(frame: DecorationFrame): DecorationRect[] {
    const rects: DecorationRect[] = [];
    // Nothing registered → nothing to join against, and no reason to walk the frame's markers
    // (both reads below are O(markers) and run per frame).
    if (this.byMarker.size === 0) return rects;
    const cols = frame.cols ?? 0;
    // Absolute line of viewport row 0. Both halves are needed, so a frame missing either keeps
    // to `markerPositions` rather than silently assuming 0.
    const hasScroll = frame.scrollbackLen !== undefined && frame.displayOffset !== undefined;
    const top = (frame.scrollbackLen ?? 0) - (frame.displayOffset ?? 0);
    // markerId → the decoration's FIRST row, viewport-relative and possibly NEGATIVE (that is
    // the point; it is clamped per-row below and never sent).
    //
    // The two groups are merged PER MARKER, not switched between: the absolute line wins where
    // a marker has one (only it can express an anchor above the top), and a marker carried only
    // by `markerPositions` still resolves. Core makes `markerLines` a superset — both are
    // `self.markers()`, one filtered — but this code cannot enforce that, and a consumer that
    // sends the groups from different sources would otherwise see decorations silently vanish.
    // This map's ITERATION order no longer matters (#458): it is a lookup table, and the
    // projection walks the decorations in registration order, resolving each anchor from here.
    // Precedence therefore does not depend on which marker group carried a given anchor.
    //
    // #482: BOTH reads keep only ids with a registered decoration (`this.byMarker`), so `anchors`
    // and the projection loop below are sized by decorations (D), not by the wire's live-marker
    // count (M, unbounded with scrollback — core caps nothing). The markerLines stride scan is
    // still O(M): correlating a flat per-frame snapshot to decorations cannot go below that in
    // frame-mode without a persistent out-of-band marker index (see #482 and
    // docs/research/terminal-engine-renderer-architectures.md). But no per-marker Map entry lands
    // for a marker nothing is registered against, which is the allocation the regression added.
    const anchors = new Map<number, number>();
    if (hasScroll) {
      for (const [id, line] of readMarkerLines(frame.markerLines, this.byMarker)) anchors.set(id, line - top);
    }
    for (const m of readMarkers(frame.markerPositions)) {
      if (this.byMarker.has(m.id) && !anchors.has(m.id)) anchors.set(m.id, m.row);
    }
    // Walk the decorations in REGISTRATION order (#458), resolving each one's anchor row, rather
    // than walking the markers and emitting whatever hangs off each. Same work — the anchor lookup
    // is O(1) and the loop is O(D), so #482's "sized by decorations, not by the wire's marker
    // count" holds — but the emission order is now the consumer's own, so precedence cannot be
    // decided by where core happens to place the anchors. A decoration whose marker is not in this
    // frame simply has no anchor and is skipped, exactly as before.
    for (const d of this.inRegistrationOrder) {
      const startRow = anchors.get(d.markerId);
      if (startRow === undefined) continue;
      const [rawLeft, rawRight] = columns(d, cols);
      // #457: clip to the viewport HERE, because the wire cannot carry the alternative.
      // Columns cross as u32 (`decorationWire`), so an out-of-range column does not
      // arrive as "out of range" — it arrives as a plausible one. A negative `left`
      // wraps to ~4.29e9 and the renderer's `col >= left` matches nothing (the
      // decoration vanishes); a negative `right` makes `col <= right` true for EVERY
      // column (it paints the whole row); NaN, ±Infinity and anything >= 2**32 all
      // land as 0 (a spurious paint on column 0).
      //
      // xterm needs no equivalent: it stores no span, testing `x >= xmin && x < xmax`
      // per visible cell (`DecorationService.forEachDecorationAtCell`), so an
      // out-of-range span simply never matches. Clipping reproduces that result for a
      // LEFT-anchored decoration exactly. For a RIGHT-anchored one there is nothing to
      // reproduce — xterm's colour path ignores `anchor` entirely (only its DOM element
      // honours it), so justerm's right-anchored span is first-party design (#459).
      const left = Math.max(0, rawLeft);
      // Clip the high end to the last visible column when the frame carries geometry.
      // Absent geometry we cannot, so the guarantee below is: every emitted column is a
      // finite, non-negative integer — and additionally <= cols-1 whenever `cols` is
      // known, which the real frame path always is (`DecodedFrame.cols` is required).
      const right = frame.cols !== undefined ? Math.min(frame.cols - 1, rawRight) : rawRight;
      // Drop anything with no visible cell: off-screen either side, degenerate
      // (zero-width), or non-finite. Emitting nothing is correct AND is what keeps an
      // unrepresentable column from reaching the u32 lane at all.
      if (!Number.isFinite(left) || !Number.isFinite(right) || right < left) continue;
      // #461/#462: clamp the START row to the viewport top and DROP a non-finite anchor.
      // Rows cross the wire as u32 (`decorationWire`) exactly like columns, so an out-of-range
      // row must not reach it. A NEGATIVE anchor is clamped to the top (its span's visible tail
      // shows; a span ending above the top shows nothing). A NON-FINITE anchor is the residue
      // the clamp does NOT cover: `Math.max(0, +Infinity)` is `+Infinity`, and `+Infinity <=
      // +Infinity` is TRUE while `+Infinity + 1` stays `+Infinity`, so without this guard the
      // row loop below never terminates (#462 — it OOMs rather than emitting a wrapped row).
      const firstRow = Math.max(0, startRow);
      if (!Number.isFinite(firstRow)) continue;
      // Bottom clip: to the last viewport row when the frame carries geometry — which the real
      // path always does (`DecodedFrame.rows` is required). WITHOUT `rows` there is no viewport
      // to clip to and no row below the anchor can be shown, so the span caps to the anchor row.
      // That also BOUNDS the loop: the old `+Infinity` fallback let a large `height` walk up to
      // ~1e9 rows (hang / OOM) and write rows that wrap the u32 wire (#462). `firstRow` is finite
      // (guarded above) and `bottom` is finite, so `lastRow` is finite — a degenerate or
      // above-top span simply does not iterate (no explicit `lastRow < firstRow` guard: a first
      // draft's was shown dead by a mutation test; the columns above still need theirs, being
      // emitted rather than iterated).
      const bottom = frame.rows !== undefined ? frame.rows - 1 : firstRow;
      const lastRow = Math.min(startRow + d.height - 1, bottom);
      for (let row = firstRow; row <= lastRow; row++) {
        rects.push({ row, left, right, layer: d.layer, bg: d.bg, fg: d.fg });
      }
    }
    return rects;
  }

  /**
   * Project the overview-ruler marks for one frame (#120 S3): for each decoration
   * carrying `overviewRulerOptions`, join its marker id with the frame's
   * `markerLines` (EVERY live marker's absolute buffer line, on-screen or not — the
   * v11 group) and place a mark at `line / (scrollbackLen + rows)` down the track.
   * Off-viewport anchors show here even though they're absent from
   * {@link decorationsForFrame} — that is the whole point of a ruler. A ruler
   * decoration whose marker isn't currently live yields no mark (inner join).
   *
   * The mark is one point per marker line, independent of the decoration's
   * `height` — matching xterm, whose `ColorZoneStore` builds a single-line zone
   * (`startBufferLine === endBufferLine`) for a decoration regardless of height.
   *
   * The ratio is clamped to the track `[0, 1]` — a marker line past `scrollbackLen + rows`
   * (a frame lag between the absolute lines and the scroll geometry) would otherwise fall off
   * the bottom — and a non-finite `scrollbackLen` or marker line yields no mark rather than the
   * `top: NaN%` invalid CSS it used to (#463). xterm needs no clamp: its zones come from
   * in-buffer lines that are always in range.
   */
  rulerMarksForFrame(frame: {
    markerLines?: ArrayLike<number>;
    scrollbackLen?: number;
    rows?: number;
    altScreen?: boolean;
  }): RulerMark[] {
    // The overview ruler is a scrollback navigator, so it's hidden on the alt
    // screen (vim/htop) — which has no user scrollback and whose markers are alt-
    // scoped decorations, not primary anchors. Mirrors xterm hiding its ruler
    // canvas (`display:none`) on buffer-activate to the alt buffer.
    if (frame.altScreen) return [];
    const total = (frame.scrollbackLen ?? 0) + (frame.rows ?? 0);
    // #463: reject a non-finite total, not just `<= 0`. `total <= 0` is a size comparison and
    // `NaN <= 0` is FALSE, so a NaN `scrollbackLen` (consumer-built frame) used to slip through
    // to `topRatio = line / NaN = NaN`, which `scrollbar.ts` writes as `top: NaN%` — invalid CSS
    // the browser drops, stacking the mark at the track default. `Number.isFinite` is the check
    // the `NaN <= 0` slip needs; it also rejects ±Infinity.
    if (!Number.isFinite(total) || total <= 0) return [];
    const lineOf = readMarkerLines(frame.markerLines, this.byMarker); // #482: sized D, not M
    const marks: RulerMark[] = [];
    // Two rules compose here, and they answer different questions.
    //
    // WITHIN a position class, registration order (#458), same as the cell projection: marks overlap
    // on the track when their lines are close, and `scrollbar.ts` appends one div per mark with no
    // z-index, so emission order is paint order. That part is a deliberate divergence: xterm's
    // intra-class order is BUFFER-LINE order (its ruler walks a `marker.line`-keyed `SortedList`),
    // with only the same-line ties left to insertion batching. Registration order is chosen instead
    // because it is the consumer's own input, exactly as for the cell projection — one rule for the
    // file rather than two.
    //
    // ACROSS classes, a `full`-width mark paints above the gutter ones (#498) — this IS xterm's
    // rule (`OverviewRulerRenderer.ts:173-181` renders every non-`full` zone, then every `full`
    // one), and it is deliberate rather than an artifact: xterm's own search marks are `position:
    // 'center'` (`addon-search/DecorationManager.ts:142`), so a full-width mark is the whole-line
    // statement that outranks the narrow ones. `full` is also the default position, so this is the
    // common overlap rather than an exotic one.
    //
    // The partition below is STABLE (one pass collecting each class in order, then concatenated), so
    // the two rules compose instead of one overriding the other.
    //
    // Validity condition worth knowing: upstream the payoff is bigger than here, because xterm sizes
    // a `full` mark at ~2 device px and a gutter mark 3-6x taller, so full-on-top is what keeps the
    // thin one visible at all. `scrollbar.ts` draws every mark at a flat 2px, so today this rule only
    // decides which COLOUR shows on an overlap. It becomes load-bearing the moment mark heights
    // become position-dependent.
    const gutter: RulerMark[] = [];
    for (const d of this.inRegistrationOrder) {
      if (!d.overviewRulerOptions) continue;
      const line = lineOf.get(d.markerId);
      if (line === undefined) continue;
      const rawRatio = line / total;
      // #463: a non-finite marker line (NaN/±Infinity from a consumer-built markerLines) has no
      // placeable position — skip it rather than emit `top: NaN%` / `top: Infinity%`. A clamp
      // cannot rescue this: `Math.max(0, NaN)` is `NaN`. (`total` is finite > 0 above, so a
      // non-finite ratio can only come from a non-finite line.)
      if (!Number.isFinite(rawRatio)) continue;
      // Clamp to the track: a line past the content end (`scrollbackLen + rows`) — a frame
      // lag/mismatch between the absolute markerLines and the scroll geometry — gives ratio > 1
      // (mark below the track, invisible); a negative line gives < 0. Pin to [0, 1].
      const topRatio = Math.min(1, Math.max(0, rawRatio));
      const position = d.overviewRulerOptions.position ?? "full";
      const mark = { topRatio, color: d.overviewRulerOptions.color, position };
      // The gutter classes accumulate in `gutter`, everything else in `marks`; the concat below puts
      // the gutter marks first, so the full-width ones paint over them (#498). The test is written as
      // "is it one of the gutter positions?" rather than `=== "full"` so it agrees with
      // `scrollbar.ts` `rulerMarkX`, whose `switch` also renders anything unrecognised full-width —
      // unreachable from typed code (the union is closed), but the two must not disagree about which
      // marks are geometrically full-width.
      const isGutter = position === "left" || position === "center" || position === "right";
      (isGutter ? gutter : marks).push(mark);
    }
    return gutter.concat(marks);
  }


  private remove(d: StoredDecoration): void {
    if (d.disposed) return;
    d.disposed = true;
    this.inRegistrationOrder.delete(d);
    const set = this.byMarker.get(d.markerId);
    if (!set) return;
    set.delete(d);
    if (set.size === 0) this.byMarker.delete(d.markerId);
  }
}

/** A decoration's inclusive `[left, right]` viewport columns for a frame of `cols`
 * width (#202). `left` anchor: `x`-based from the left; `right` anchor: `x` cells
 * in from the right edge, extending leftward by `width` (xterm's `style.right`).
 *
 * The right-anchored branch is first-party (#459): xterm's colour path has no anchor term, because
 * there `anchor` positions the decoration's DOM element instead. See {@link
 * DecorationOptions.anchor} for why frame mode cannot inherit that split. */
function columns(d: StoredDecoration, cols: number): [number, number] {
  if (d.anchor === "right") return [cols - d.x - d.width, cols - 1 - d.x];
  return [d.x, d.x + d.width - 1];
}

/** u32 lanes per record in a frame's `markerLines` (#120 S3, wire v11): `id`,
 * absolute `line`. See the wasm `MARKER_LINE_STRIDE`. */
const MARKER_LINE_STRIDE = 2;

/** Decode `markerLines` (flat stride-2) into a `markerId → absolute line` map, keeping ONLY ids
 * present in `keep` (#482). The wire carries every live marker (M, unbounded with scrollback), but
 * only markers with a registered decoration can project or place a ruler mark, so the returned map
 * is sized by decorations (D), not the wire (M). The stride scan stays O(M) — a flat per-frame
 * snapshot cannot be correlated to decorations below that (docs/research/…-architectures.md) — but
 * no per-marker entry is allocated for a marker nothing is registered against. */
function readMarkerLines(
  flat: ArrayLike<number> | undefined,
  keep: ReadonlyMap<number, unknown>,
): Map<number, number> {
  const out = new Map<number, number>();
  if (!flat) return out;
  for (let i = 0; i + MARKER_LINE_STRIDE <= flat.length; i += MARKER_LINE_STRIDE) {
    const id = flat[i]!;
    if (keep.has(id)) out.set(id, flat[i + 1]!);
  }
  return out;
}
