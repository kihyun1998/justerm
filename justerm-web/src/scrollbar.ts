import type { RulerMark } from "./decorations";

/** The viewport scroll position the scrollbar reads (from the decoded frame). */
export interface ScrollPosition {
  /** Lines scrolled up from the bottom (0 = following the live screen). */
  displayOffset: number;
  /** History lines; total content = `scrollbackLen + rows`. */
  scrollbackLen: number;
  /** Visible viewport rows. */
  rows: number;
}

/** Thumb geometry as ratios of the track (0..1). */
export interface ScrollbarMetrics {
  /** Whether the bar shows at all — only when content overflows (Auto). */
  visible: boolean;
  /** Thumb height ÷ track height = viewport ÷ total content. */
  thumbHeightRatio: number;
  /** Thumb top ÷ track height = top viewport line ÷ total content. */
  thumbTopRatio: number;
}

/**
 * Thumb geometry from the frame's scroll position, mirroring xterm `Viewport`
 * (`scrollHeight = cell × lines.length`, `scrollTop = ydisp × cell`). `total`
 * is all lines (scrollback + screen); the thumb is the viewport's share of it,
 * positioned at the viewport's top line.
 */
export function scrollbarMetrics(pos: ScrollPosition): ScrollbarMetrics {
  const total = pos.scrollbackLen + pos.rows;
  return {
    visible: total > pos.rows,
    thumbHeightRatio: pos.rows / total,
    thumbTopRatio: (pos.scrollbackLen - pos.displayOffset) / total,
  };
}

/**
 * The display offset a drag to `topRatio` (0 = track top, 1 = bottom) requests.
 * Inverse of {@link scrollbarMetrics}'s `thumbTopRatio`: the dragged-to viewport
 * top line maps back to an offset, clamped to `[0, scrollbackLen]`. The backend
 * scrolls to it.
 */
export function dragToDisplayOffset(topRatio: number, pos: ScrollPosition): number {
  const total = pos.scrollbackLen + pos.rows;
  const topLine = topRatio * total;
  const offset = Math.round(pos.scrollbackLen - topLine);
  return Math.max(0, Math.min(pos.scrollbackLen, offset));
}

export interface ScrollbarOptions {
  /** Bar width in px (xterm `overviewRuler.width`, default 14). */
  width?: number;
  /** A drag requests this display offset; the consumer scrolls the backend there. */
  onScroll(displayOffset: number): void;
}

/**
 * A custom DOM scrollbar over the canvas — the GPU renderer has no native overflow bar,
 * so (like xterm's VS Code `SmoothScrollableElement`) the bar is a DOM overlay.
 * `update(pos)` sizes/positions the thumb from {@link scrollbarMetrics}; dragging
 * maps to a display offset via {@link dragToDisplayOffset} and calls `onScroll`.
 *
 * Browser-only glue — not unit-tested; the geometry it calls is.
 */
export class Scrollbar {
  private readonly track: HTMLDivElement;
  private readonly thumb: HTMLDivElement;
  private pos: ScrollPosition = { displayOffset: 0, scrollbackLen: 0, rows: 0 };
  /** Overview-ruler mark elements (#120 S3), re-created each {@link setMarks}. */
  private readonly markEls: HTMLDivElement[] = [];
  private dragging = false;
  private readonly onMove: (e: globalThis.MouseEvent) => void;
  private readonly onUp: () => void;

  constructor(
    parent: HTMLElement,
    private readonly opts: ScrollbarOptions,
  ) {
    const width = opts.width ?? 14;
    this.track = document.createElement("div");
    Object.assign(this.track.style, {
      position: "absolute",
      top: "0",
      right: "0",
      width: `${width}px`,
      height: "100%",
      display: "none",
    } satisfies Partial<CSSStyleDeclaration>);
    this.thumb = document.createElement("div");
    Object.assign(this.thumb.style, {
      position: "absolute",
      left: "2px",
      right: "2px",
      borderRadius: "4px",
      background: "rgba(255,255,255,0.25)",
    } satisfies Partial<CSSStyleDeclaration>);
    this.track.appendChild(this.thumb);
    parent.appendChild(this.track);

    this.onMove = (e) => this.dragTo(e.clientY);
    this.onUp = () => {
      this.dragging = false;
      window.removeEventListener("mousemove", this.onMove);
      window.removeEventListener("mouseup", this.onUp);
    };
    this.thumb.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.dragging = true;
      window.addEventListener("mousemove", this.onMove);
      window.addEventListener("mouseup", this.onUp);
    });
  }

  /** Re-size/position the thumb from the frame's scroll position. */
  update(pos: ScrollPosition): void {
    this.pos = pos;
    const m = scrollbarMetrics(pos);
    this.track.style.display = m.visible ? "block" : "none";
    this.thumb.style.height = `${m.thumbHeightRatio * 100}%`;
    this.thumb.style.top = `${m.thumbTopRatio * 100}%`;
  }

  /**
   * Render the overview-ruler marks (#120 S3) — xterm's `OverviewRulerRenderer`
   * analog. Each mark is a thin coloured bar at its `topRatio` down the track, so
   * off-viewport anchors are visible on the full-buffer scrollbar. Marks live on
   * the track (they show with it) and don't intercept drags (`pointer-events:
   * none`). Drive it with `registry.rulerMarksForFrame(frame)` each frame.
   *
   * **Array order IS paint order.** Marks are appended in the order given and carry no `z-index`,
   * so a later mark paints over an earlier one — which is how `rulerMarksForFrame` expresses both
   * its ordering rules (registration order within a position class, #458; `full` above the gutter
   * classes, #498). Do NOT sort, reverse, or stack them here: either would silently void those
   * rules, and no unit test can catch it (vitest runs in a `node` environment) — only the
   * `__rulerLayerProbe` e2e does.
   */
  setMarks(marks: RulerMark[]): void {
    for (const el of this.markEls) el.remove();
    this.markEls.length = 0;
    for (const m of marks) {
      const el = document.createElement("div");
      Object.assign(el.style, {
        position: "absolute",
        top: `${m.topRatio * 100}%`,
        height: "2px",
        background: `#${(m.color & 0xffffff).toString(16).padStart(6, "0")}`,
        pointerEvents: "none",
        ...rulerMarkX(m.position),
      } satisfies Partial<CSSStyleDeclaration>);
      el.dataset.rulerMark = ""; // stable hook for the #498 e2e probe (not a style)
      this.track.appendChild(el);
      this.markEls.push(el);
    }
  }

  private dragTo(clientY: number): void {
    if (!this.dragging) return;
    const r = this.track.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientY - r.top) / r.height));
    this.opts.onScroll(dragToDisplayOffset(ratio, this.pos));
  }

  dispose(): void {
    this.onUp();
    this.track.remove();
  }
}

/** Horizontal placement CSS for a ruler mark's `position` — `full` spans the
 * track; `left`/`center`/`right` are thirds (gutter columns). */
function rulerMarkX(position: RulerMark["position"]): Partial<CSSStyleDeclaration> {
  switch (position) {
    case "left":
      return { left: "0", width: "33%" };
    case "center":
      return { left: "33%", width: "34%" };
    case "right":
      return { right: "0", width: "33%" };
    default:
      return { left: "0", right: "0" };
  }
}
