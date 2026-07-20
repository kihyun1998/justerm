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
   * cells in from the right edge, the span extending leftward by `width`. */
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
  /** Decorations grouped by anchor marker id, so a per-frame join and an
   * `onMarkerDisposed` are both O(decorations-on-that-marker). Insertion order
   * within a marker is preserved (a `Set`), so projection is deterministic. */
  private readonly byMarker = new Map<number, Set<StoredDecoration>>();

  /**
   * Register a decoration anchored to `options.markerId`. Returns a handle whose
   * `dispose()` removes it. Registering against a marker id that never appears in
   * a frame is a harmless no-op — the handle simply never projects. (Unlike xterm
   * there is no marker object to guard on `isDisposed`, and marker ids are reused
   * by a full reset, so there is no permanent reject-set — disposal is purely
   * event-driven via {@link onMarkerDisposed}.)
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
   * Iterating `markerLines` rather than the registry also keeps the *ordering* source
   * unchanged: both marker groups come from core's `self.markers()`, so cross-marker
   * precedence is exactly what it was (its own open question, #458).
   */
  decorationsForFrame(frame: DecorationFrame): DecorationRect[] {
    const rects: DecorationRect[] = [];
    // Nothing registered → nothing to join against, and no reason to walk the frame's markers
    // (both reads below are O(markers) and run per frame).
    if (this.byMarker.size === 0) return rects;
    const cols = frame.cols ?? 0;
    // Clip a multi-row `height` to the viewport bottom. No `rows` → don't clip.
    const maxRow = frame.rows !== undefined ? frame.rows - 1 : Number.POSITIVE_INFINITY;
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
    // Seeding from `markerLines` first also keeps core's marker order as the precedence order
    // (#458), with any `markerPositions`-only marker appended.
    const anchors = new Map<number, number>();
    if (hasScroll) {
      for (const [id, line] of readMarkerLines(frame.markerLines)) anchors.set(id, line - top);
    }
    for (const m of readMarkers(frame.markerPositions)) {
      if (!anchors.has(m.id)) anchors.set(m.id, m.row);
    }
    for (const [markerId, startRow] of anchors) {
      const set = this.byMarker.get(markerId);
      if (!set) continue;
      for (const d of set) {
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
        // #461: clamp the START row to the viewport top — the vertical mirror of the column
        // clip above, and for the same reason: rows cross as u32 too, so a negative row would
        // wrap rather than arrive as "above the screen". An anchor above the top yields the
        // visible tail of its span; one whose span ends above the top yields nothing.
        const firstRow = Math.max(0, startRow);
        const lastRow = Math.min(startRow + d.height - 1, maxRow);
        // No `lastRow < firstRow` guard here, deliberately: the loop bound already yields
        // nothing for an empty span, and for NaN / ±Infinity too (every comparison is false).
        // A first draft had one; a mutation test showed removing it changed no result, which
        // makes it dead code rather than an uncovered guard. The columns above DO need their
        // explicit check, because they are emitted rather than iterated.
        for (let row = firstRow; row <= lastRow; row++) {
          rects.push({ row, left, right, layer: d.layer, bg: d.bg, fg: d.fg });
        }
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
    if (total <= 0) return [];
    const lineOf = readMarkerLines(frame.markerLines);
    const marks: RulerMark[] = [];
    for (const [markerId, set] of this.byMarker) {
      const line = lineOf.get(markerId);
      if (line === undefined) continue;
      for (const d of set) {
        if (!d.overviewRulerOptions) continue;
        marks.push({
          topRatio: line / total,
          color: d.overviewRulerOptions.color,
          position: d.overviewRulerOptions.position ?? "full",
        });
      }
    }
    return marks;
  }


  private remove(d: StoredDecoration): void {
    if (d.disposed) return;
    d.disposed = true;
    const set = this.byMarker.get(d.markerId);
    if (!set) return;
    set.delete(d);
    if (set.size === 0) this.byMarker.delete(d.markerId);
  }
}

/** A decoration's inclusive `[left, right]` viewport columns for a frame of `cols`
 * width (#202). `left` anchor: `x`-based from the left; `right` anchor: `x` cells
 * in from the right edge, extending leftward by `width` (xterm's `style.right`). */
function columns(d: StoredDecoration, cols: number): [number, number] {
  if (d.anchor === "right") return [cols - d.x - d.width, cols - 1 - d.x];
  return [d.x, d.x + d.width - 1];
}

/** u32 lanes per record in a frame's `markerLines` (#120 S3, wire v11): `id`,
 * absolute `line`. See the wasm `MARKER_LINE_STRIDE`. */
const MARKER_LINE_STRIDE = 2;

/** Decode `markerLines` (flat stride-2) into a `markerId → absolute line` map. */
function readMarkerLines(flat?: ArrayLike<number>): Map<number, number> {
  const out = new Map<number, number>();
  if (!flat) return out;
  for (let i = 0; i + MARKER_LINE_STRIDE <= flat.length; i += MARKER_LINE_STRIDE) {
    out.set(flat[i]!, flat[i + 1]!);
  }
  return out;
}
