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
 * `bottom` overrides the cell background *under* the glyph, `top` paints *over* it.
 *
 * One consequence to know when picking `top` (#494): on a cell whose glyph *tiles*
 * with the background — Powerline separators, box-drawing, block elements — a `top`
 * decoration that sets ONLY `bg` paints the whole cell, glyph included, because such
 * a glyph is background-shaped ink rather than text. That is what makes a line
 * highlight over TUI output solid instead of holed at every box-drawing cell. Set
 * `fg` as well to keep the art, in your own colour. A `bottom` decoration is
 * unaffected: it sits *under* the glyph, so an opaque tile hides it by design. */
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
  /** Where the mark's **line** sits down the track, `0..1`.
   *
   * The name predates #500 §3 and now under-describes it: the scrollbar **centres** the mark's box
   * on this ratio rather than starting the box at it, so this is the line's position and not the
   * box's top edge. Kept as-is because `RulerMark` is a published type and the *value* did not
   * change — only how `scrollbar.ts` reads it. Nothing here is projection-side. */
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

/**
 * What this registry needs in order to place an anchor: one absolute buffer line per marker id.
 *
 * Declared as a capability rather than naming `MarkerIndexCache`, which is what a consumer
 * actually hands over. The registry calls exactly one method on it, and the cache's own
 * behaviour — the pull, the epoch invalidation, the basis rebase — is tested where it lives.
 * Widening the parameter also lets a consumer that already maintains marker lines its own way
 * feed this projection without adopting the cache, which is the boundary rule for this layer:
 * the mechanism is ours, the source of the data is theirs (ADR-0017).
 *
 * `undefined` for an id means **do not project it** — a decoration missing for a frame beats one
 * painted on a line it no longer owns.
 */
export interface MarkerLineSource {
  lineOf(id: number): number | undefined;
}

/** The frame fields the registry reads. A `DecodedFrame` satisfies it structurally.
 * `cols` sizes right-anchored spans; `rows` clips a multi-row `height` (#202). */
interface DecorationFrame {
  readonly markerPositions?: ArrayLike<number>;
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

/** Whether the "no marker index on a v16 frame" warning has already been emitted.
 * Module-scoped and deduped for the reason `checkGeometry` states: this runs once per
 * frame, so an undeduped warn would bury its own first line. */
let warnedNoMarkerIndex = false;

/** Forget the warning. Test-only — deliberately absent from the package entry point, like
 * `resetGeometryWarnings`. */
export function resetMarkerIndexWarning(): void {
  warnedNoMarkerIndex = false;
}

export class DecorationRegistry {
  /** Decorations grouped by anchor marker id, so `onMarkerDisposed` and the per-frame
   * marker-id filter (#482) are both O(decorations-on-that-marker). This is the *index*;
   * it does not decide precedence. */
  private readonly byMarker = new Map<number, Set<StoredDecoration>>();
  /** Every live decoration in **registration order** (a `Set` preserves insertion order) —
   * ADR-0024 R2, where the reasoning below is generalised: the model is "a decoration is colours plus a
   * mark, not an object", and the projection rules (precedence, ruler layering, `anchor`, above-viewport
   * anchors, guards) are its consequences. Read it before changing any of them. —
   * the *cell* projection order, and therefore the cell precedence order (#458): the renderer resolves
   * per-property last-in-wire-order (#452), so the last registered decoration wins a cell.
   * Kept alongside `byMarker` rather than derived from it, because a per-marker grouping can
   * only ever express order *within* a marker; across markers it would leak core's marker
   * emission order into consumer policy. The RULER projection partitions this order by position
   * class (#498) and is stable within each class, so it is not simply this order. */
  private readonly inRegistrationOrder = new Set<StoredDecoration>();

  /** The marker-line source (#490), when the consumer wired one. Optional on purpose — but
   * since v16 an absent one means no ruler marks and no above-top anchors, which is why
   * {@link rulerMarksForFrame} warns once rather than degrading in silence. */
  private markerIndex: MarkerLineSource | undefined;

  /**
   * Wire the pulled marker index (#490). Since wire v16 this is the **only** source of a
   * marker's absolute buffer line: the frame carries `markerPositions` (viewport rows for
   * on-screen markers) and nothing else, so an anchor above the viewport top, and every
   * overview-ruler mark, comes from here or not at all.
   *
   * It also **wins** over `markerPositions` where both answer, which is #461's rule carried
   * over from when the absolute line rode the frame: only an absolute line can express an
   * anchor above the top, so a derived viewport row must not mask it. `undefined` from the
   * index means exactly what it says: do not project — a decoration missing for a frame beats
   * one painted on a line it no longer owns.
   *
   * Without an index wired the ruler is simply blank; {@link rulerMarksForFrame} says so
   * once, because nothing else can (a missing anchor throws nothing and reddens nothing).
   */
  setMarkerIndex(source: MarkerLineSource | undefined): void {
    this.markerIndex = source;
  }

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
   * The join reads two sources **per marker**: the frame's viewport-relative `markerPositions`
   * answers for a marker it carries, and the pulled index (#490) answers for the rest.
   * That second source is not an extra: `markerPositions` omits a marker scrolled ABOVE the
   * viewport top (core drops it, `m.line.checked_sub(top)?`), so joining on it alone made a
   * multi-row decoration whose anchor had scrolled off vanish **entirely** instead of showing
   * the rows of it that are still on screen (#461). xterm has no such gap — it keys colour
   * lookup to the absolute buffer line and buckets every line the height covers.
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
    // This map's ITERATION order does not matter (#458): it is a lookup table, and the
    // projection walks the decorations in registration order, resolving each anchor from here.
    // Precedence therefore does not depend on which source carried a given anchor.
    //
    // **#482's O(M) scan is gone.** It read the frame's absolute-line group, which carried
    // every live marker (M, unbounded with scrollback — core caps nothing), so correlating it
    // to the D registered decorations cost a stride walk of the whole group every frame. v16
    // deleted that group (#490) and the index below answers each decoration directly, so both
    // reads here are now sized by decorations: `markerPositions` is viewport-bounded and the
    // index lookup is O(1) per decoration. This is the frame-mode ceiling that
    // docs/research/terminal-engine-renderer-architectures.md said could only be escaped by
    // taking the anchors out of the snapshot — which is what #490 did.
    const anchors = new Map<number, number>();
    // The absolute line WINS where the index has one — #461's rule, inherited unchanged from
    // when that line rode the frame. It is the only source that can express an anchor above the
    // viewport top, so a derived viewport row must not mask it. An index that has fallen behind
    // does not compete here: `lineOf` returns `undefined` while `adopted !== seen`, so a stale
    // line is never served, only a missing one.
    if (hasScroll && this.markerIndex) {
      for (const id of this.byMarker.keys()) {
        const line = this.markerIndex.lineOf(id);
        if (line !== undefined) anchors.set(id, line - top);
      }
    }
    // …and a marker the index does not hold still resolves from its viewport row. The two are
    // merged PER MARKER, not switched between: a consumer whose index holds only its decoration
    // markers while the frame also carries command marks must not see decorations silently
    // vanish (#461, and the demo does exactly this).
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
   * carrying `overviewRulerOptions`, look its marker id up in the pulled index (#490)
   * for an absolute buffer line and place a mark at `line / (scrollbackLen + rows)`
   * down the track. Off-viewport anchors show here even though they're absent from
   * {@link decorationsForFrame} — that is the whole point of a ruler, and since v16
   * the index is the only thing that knows them. A ruler decoration whose marker the
   * index does not hold yields no mark (inner join).
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
  /** Whether any live decoration asks for a ruler mark — the only ones this warning is
   * about, since a cell-only decoration still projects from the viewport group. */
  private hasRulerDecoration(): boolean {
    for (const d of this.inRegistrationOrder) {
      if (d.overviewRulerOptions) return true;
    }
    return false;
  }

  rulerMarksForFrame(frame: {
    scrollbackLen?: number;
    rows?: number;
    altScreen?: boolean;
    markerCount?: number;
  }): RulerMark[] {
    // The overview ruler is a scrollback navigator, so it's hidden on the alt
    // screen (vim/htop) — which has no user scrollback and whose markers are alt-
    // scoped decorations, not primary anchors. Mirrors xterm hiding its ruler
    // canvas (`display:none`) on buffer-activate to the alt buffer.
    if (frame.altScreen) return [];
    // A v16 frame carries `markerCount` and no absolute-line group (#490), so the ruler's
    // anchors can only come from a pulled index. Without one this method returns `[]` for
    // every frame and the overview ruler is simply blank — a failure with no exception, no
    // red test and no gate able to see it (`published-seam.types.ts` is one-directional, so
    // a removed getter only shrinks a union). Say so, once.
    //
    // Keyed on `markerCount`, which every v16 frame carries, rather than on "this frame
    // produced no marks" — that is true of any frame with no live markers, and warning there
    // would be false. The count is the one field that says "this frame came off a wire that
    // no longer ships anchors", which is the only case a host can be wrong about.
    if (
      frame.markerCount !== undefined &&
      this.markerIndex === undefined &&
      !warnedNoMarkerIndex &&
      this.hasRulerDecoration()
    ) {
      warnedNoMarkerIndex = true;
      console.warn(
        "justerm-web: this frame carries no marker anchors (wire v16) and no marker index " +
          "is wired, so the overview ruler will stay empty and decorations anchored above " +
          "the viewport will not project. Call DecorationRegistry.setMarkerIndex() with a " +
          "MarkerIndexCache driven by your MarkerPort and the marker events.",
      );
    }
    const total = (frame.scrollbackLen ?? 0) + (frame.rows ?? 0);
    // #463: reject a non-finite total, not just `<= 0`. `total <= 0` is a size comparison and
    // `NaN <= 0` is FALSE, so a NaN `scrollbackLen` (consumer-built frame) used to slip through
    // to `topRatio = line / NaN = NaN`, which `scrollbar.ts` writes as `top: NaN%` — invalid CSS
    // the browser drops, stacking the mark at the track default. `Number.isFinite` is the check
    // the `NaN <= 0` slip needs; it also rejects ±Infinity.
    if (!Number.isFinite(total) || total <= 0) return [];
    // One source since v16: the pulled index. The frame carries no absolute lines, so there is
    // nothing here to merge it with — unlike the cell projection, which still has
    // `markerPositions` for the on-screen half. The lookup is O(1) per ruler decoration.
    const idx = this.markerIndex;
    const lineOf = {
      get: (id: number): number | undefined => idx?.lineOf(id),
    };
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
    // This rule's payoff was gated on mark geometry, and the gate opened at #500 §2: `scrollbar.ts`
    // sizes a `full` mark at 2 CSS px and a gutter mark at 6 (`rulerMarkHeightPx`), so full-on-top is
    // now what keeps the thin one visible rather than merely deciding a colour on an exact overlap.
    // ADR-0024 carries the same amendment. (This paragraph said "flat 2px … becomes load-bearing the
    // moment heights become position-dependent" until #440; #500 made it false and missed it here.)
    // It matters more since #440: search marks default to `center`, the FAT class, sitting under
    // thin `full` decoration marks.
    const gutter: RulerMark[] = [];
    for (const d of this.inRegistrationOrder) {
      if (!d.overviewRulerOptions) continue;
      const line = lineOf.get(d.markerId);
      if (line === undefined) continue;
      // Both rules live in `trackRatio` now, shared with the search source (#440) so the two
      // cannot drift apart on them. They stay two rules and not one:
      //
      // #463: a non-finite marker line (NaN/±Infinity reaching the index from a consumer's port, or
      // from a hand-built snapshot) has no placeable position — skipped rather than emitted as
      // `top: NaN%` / `top: Infinity%`. A clamp cannot rescue this: `Math.max(0, NaN)` is `NaN`.
      // (`total` is finite > 0 above, so a non-finite ratio can only come from a non-finite line.)
      //
      // Clamped to the track: a line past the content end (`scrollbackLen + rows`) — a frame
      // lag/mismatch between the index's absolute lines and the scroll geometry — gives ratio > 1
      // (mark below the track, invisible); a negative line gives < 0. Pinned to [0, 1].
      const topRatio = trackRatio(line, total);
      if (topRatio === undefined) continue;
      const position = d.overviewRulerOptions.position ?? "full";
      const mark = { topRatio, color: d.overviewRulerOptions.color, position };
      // The gutter classes accumulate in `gutter`, everything else in `marks`; the concat below puts
      // the gutter marks first, so the full-width ones paint over them (#498). The test is written as
      // "is it one of the gutter positions?" rather than `=== "full"` so it agrees with
      // `scrollbar.ts` `rulerMarkX`, whose `switch` also renders anything unrecognised full-width —
      // unreachable from typed code (the union is closed), but the two must not disagree about which
      // marks are geometrically full-width.
      (isGutterMark(mark) ? gutter : marks).push(mark);
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


/**
 * Where a buffer line sits down the overview-ruler track, or `undefined` when it has no placeable
 * position (ADR-0024 R6). Shared by both mark sources so the two cannot drift apart on the rule.
 *
 * **The two branches are different rules and only look alike.** A **non-finite** ratio has no value
 * at all — a `NaN` reaches `scrollbar.ts` as `top: NaN%`, which the browser silently drops, stacking
 * every such mark at the track default; a clamp cannot rescue it (`Math.max(0, NaN)` is `NaN`). An
 * **out-of-range** ratio has a perfectly good value that is merely off the track (a lag between a
 * held line and the frame's scroll geometry), so it is pinned to `[0, 1]` rather than dropped —
 * #463's clamp, and ADR-0024 carries the amendment that separated the two cases.
 */
function trackRatio(line: number, total: number): number | undefined {
  const raw = line / total;
  if (!Number.isFinite(raw)) return undefined;
  return Math.min(1, Math.max(0, raw));
}

/** Whether a mark's position class is one of the gutter columns (as opposed to the full-width
 * default). Written as "is it a gutter position?" rather than `=== "full"` so it agrees with
 * `scrollbar.ts`'s `rulerMarkX`, whose `switch` also lays out anything unrecognised full-width. */
function isGutterMark(m: RulerMark): boolean {
  return m.position === "left" || m.position === "center" || m.position === "right";
}

/** The colours a search-match ruler mark is painted in (#440), plus where it sits across the
 * track's width. Absolute packed `0xRRGGBB`, consumer-resolved like every other decoration colour —
 * this layer stays theme-agnostic.
 *
 * The two colours mirror xterm's two **required** search decoration options, `matchOverviewRuler`
 * and `activeMatchColorOverviewRuler` (`addons/addon-search/typings/addon-search.d.ts`), which is
 * the evidence that ruler marks are core to the feature upstream rather than polish.
 */
export interface SearchRulerOptions {
  /** A line carrying at least one match. */
  readonly matchColor: number;
  /** The line the active (current) match sits on — it outranks {@link matchColor} for that line. */
  readonly activeMatchColor: number;
  /** Default `center`, matching xterm, whose search marks are `position: 'center'` for the active
   * and non-active alike. A gutter class, so ADR-0024 R3 paints any `full` mark above these. */
  readonly position?: RulerPosition;
}

/**
 * Project a search result set onto the overview ruler (#440) — the second mark source, beside
 * {@link DecorationRegistry.rulerMarksForFrame}. Compose the two with {@link composeRulerMarks};
 * do not concatenate them (see that function for why).
 *
 * `lines` is one **absolute buffer line per match, in the hand-over's order** — index-aligned with
 * the set the last search counted, so `activeIndex` is the controller's own navigation index and
 * needs no separate lookup. Core produces matches in buffer order, so the hand-over order is line
 * order; nothing here re-sorts, because paint order within a class is the source's order
 * (ADR-0024 R3's second key) and re-sorting would silently redefine it.
 *
 * **A match spanning a soft wrap is ONE mark, at its start line** — a second declared divergence,
 * smaller than the one below. Upstream registers a marker + decoration per covered row
 * (`_createResultDecorations`), so a wrapped match feeds a mark on every row it touches. Ours
 * follows this repo's own model instead: `rulerMarksForFrame` emits one mark per decoration at its
 * anchor even for `height > 1`, so a multi-row thing is one mark here whatever produced it.
 *
 * **One mark per LINE, not per match (ADR-0024 R1).** A line carrying ten matches is one mark. That
 * is also upstream's rule — it suppresses the mark when the line already carries one — and it is
 * what keeps a mark-every-match policy bounded by the buffer's height rather than by the query's
 * hit count.
 *
 * **The active match's line takes the active colour and is emitted last**, so it paints above a
 * neighbour close enough to overlap (array order is paint order — `scrollbar.ts` appends one div
 * per mark with no `z-index`). This is a **deliberate divergence**: upstream decorates every result
 * plain *first* and creates the active decoration *after*, so the active mark is suppressed by the
 * plain mark already on its line and the required `activeMatchColorOverviewRuler` never paints in
 * the normal flow. Shipping that verbatim would ship a dead option — the reasoning ADR-0024 R4
 * already records for `anchor`.
 *
 * **Staleness, measured rather than assumed (#440).** These lines are held from search time while
 * the geometry comes from the current frame, and core drops its held highlights on any
 * coordinate-shifting mutation *without* moving either dating scalar the frame carries
 * (`evictedTotal` / `markerEpoch`) — so a shift is not observable here. Measured: only matches **on
 * screen** move (scrollback matches keep their absolute index exactly), the error is bounded by the
 * screen height for one debounce window, and every mark stays on the track (#463). Keeping the
 * marks through that window therefore shows strictly more truth than dropping them would — the
 * engine shows none at all. Do not add a heuristic that guesses at the invalidation: an empty
 * `matchSpans` cannot distinguish "the set was dropped" from "no match is on screen".
 */
export function searchRulerMarks(
  lines: readonly number[],
  activeIndex: number | undefined,
  frame: { scrollbackLen?: number; rows?: number; altScreen?: boolean },
  opts: SearchRulerOptions,
): RulerMark[] {
  // The ruler is a scrollback navigator, so it is hidden on the alt screen — the same rule
  // `rulerMarksForFrame` holds. It has to be the same rule: two sources disagreeing about whether
  // the ruler exists would paint search marks onto a track the decoration source considers absent.
  if (frame.altScreen) return [];
  const total = (frame.scrollbackLen ?? 0) + (frame.rows ?? 0);
  if (!Number.isFinite(total) || total <= 0) return [];

  const position = opts.position ?? "center";
  const activeLine = activeIndex === undefined ? undefined : lines[activeIndex];
  const marks: RulerMark[] = [];
  const seen = new Set<number>();
  for (const line of lines) {
    // The active line is emitted below, in its own colour. Skipping it here rather than letting the
    // dedupe decide is what makes the active colour win its whole line: otherwise a plain match
    // sharing the line would claim it first and the active designation would be invisible.
    if (line === activeLine) continue;
    if (seen.has(line)) continue;
    seen.add(line);
    const topRatio = trackRatio(line, total);
    if (topRatio === undefined) continue;
    marks.push({ topRatio, color: opts.matchColor, position });
  }
  if (activeLine !== undefined) {
    const topRatio = trackRatio(activeLine, total);
    if (topRatio !== undefined) marks.push({ topRatio, color: opts.activeMatchColor, position });
  }
  return marks;
}

/**
 * Merge the two ruler mark sources into the single paint order `scrollbar.setMarks` consumes
 * (#440) — decorations from {@link DecorationRegistry.rulerMarksForFrame}, search matches from
 * {@link searchRulerMarks}.
 *
 * **This lives in the library on purpose, and concatenating the two arrays in a host is the thing
 * it exists to prevent.** ADR-0024 R3's total order was expressed entirely by
 * `rulerMarksForFrame`'s emission order while there was one source; with two, an order composed in
 * a host is re-derived per host and can disagree between them. No unit test can observe a violation
 * — vitest runs in a `node` environment, so nothing here has a layout — and the one gate that can
 * (`__rulerLayerProbe`) drives the demo, which would leave every other host's composition unproven.
 *
 * **R3 is re-applied across the join, not after it.** Every gutter mark first, then every `full`
 * one, so a decoration's gutter mark cannot end up over a `full` mark from the other source.
 * Within a class the order is decoration marks, then search marks: R3's second key is registration
 * order, which has no meaning for a match, so the search source takes one fixed rank derived from
 * the same rule — the search set is the most recent statement the consumer made.
 */
export function composeRulerMarks(
  decorationMarks: readonly RulerMark[],
  searchMarks: readonly RulerMark[],
): RulerMark[] {
  const gutter: RulerMark[] = [];
  const full: RulerMark[] = [];
  for (const m of decorationMarks) (isGutterMark(m) ? gutter : full).push(m);
  for (const m of searchMarks) (isGutterMark(m) ? gutter : full).push(m);
  return gutter.concat(full);
}
