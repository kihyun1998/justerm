/**
 * Marker-anchored decorations (#120 S1), the frame-mode analog of xterm's
 * `DecorationService`. A consumer registers a decoration against a **marker id**
 * (markers originate in core and ride the wire as `markerPositions` per frame —
 * justerm has no local `registerMarker`), and each frame the registry joins its
 * decorations with the frame's markers to project on-viewport {@link
 * DecorationRect}s: positions + opaque colour refs only. Colour *resolution* and
 * the actual paint are the renderer's/consumer's (ADR-0017, #115) — this is the
 * model + lifecycle, no DOM, no beamterm.
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
 * `color` is an opaque ref (consumer theme, theme-agnostic like `bg`/`fg`). */
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
 * `IDecorationOptions` this slice models. `bg`/`fg` are opaque colour refs (the
 * renderer/#115 resolves them).
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
 * right`), the layer, and the opaque colour refs. */
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
   * Project the registry onto one frame: for each marker visible in the frame
   * (`markerPositions` carries only on-viewport markers), emit a {@link
   * DecorationRect} per decoration anchored to it, at the marker's current row.
   * A marker scrolled off the viewport is absent here, so its decorations yield
   * nothing and reappear when it scrolls back.
   */
  decorationsForFrame(frame: DecorationFrame): DecorationRect[] {
    const rects: DecorationRect[] = [];
    const cols = frame.cols ?? 0;
    // Clip a multi-row `height` to the viewport bottom (the marker's row is already
    // on-viewport, so its span may run past it). No `rows` → don't clip.
    const maxRow = frame.rows !== undefined ? frame.rows - 1 : Number.POSITIVE_INFINITY;
    for (const m of readMarkers(frame.markerPositions)) {
      const set = this.byMarker.get(m.id);
      if (!set) continue;
      for (const d of set) {
        const [left, right] = columns(d, cols);
        const lastRow = Math.min(m.row + d.height - 1, maxRow);
        for (let row = m.row; row <= lastRow; row++) {
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
