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

import { type Marker, readMarkers } from "./markers";

/** Which layer a decoration paints on, mirroring xterm's `IDecorationOptions.layer`:
 * `bottom` overrides the cell background *under* the glyph, `top` paints *over* it. */
export type DecorationLayer = "bottom" | "top";

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
  /** First column of the decoration (default 0). */
  readonly x?: number;
  /** Column span width (default 1 — a single cell). */
  readonly width?: number;
  /** Paint layer (default `bottom`). */
  readonly layer?: DecorationLayer;
  /** Background colour override ref (opaque; resolved by the renderer). */
  readonly bg?: number;
  /** Foreground colour override ref (opaque; resolved by the renderer). */
  readonly fg?: number;
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

/** The frame fields the registry reads. A `DecodedFrame` satisfies it structurally. */
interface DecorationFrame {
  readonly markerPositions?: ArrayLike<number>;
}

/** Internal decoration record — also the public {@link Decoration} handle. Its
 * `dispose` closes over the registry so the handle removes itself. */
interface StoredDecoration extends Decoration {
  readonly markerId: number;
  readonly x: number;
  readonly width: number;
  readonly layer: DecorationLayer;
  readonly bg?: number;
  readonly fg?: number;
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
      layer: options.layer ?? "bottom",
      bg: options.bg,
      fg: options.fg,
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
    for (const m of readMarkers(frame.markerPositions)) {
      const set = this.byMarker.get(m.id);
      if (!set) continue;
      for (const d of set) rects.push(this.rect(d, m));
    }
    return rects;
  }

  private rect(d: StoredDecoration, m: Marker): DecorationRect {
    return {
      row: m.row,
      left: d.x,
      right: d.x + d.width - 1,
      layer: d.layer,
      bg: d.bg,
      fg: d.fg,
    };
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
