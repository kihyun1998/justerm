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
 * A drag at `clientY` as a ratio down `track` (0 = top, 1 = bottom), or `undefined` when
 * the track has **no box to be a ratio of**.
 *
 * An element that is `display: none`, detached, or not yet laid out reports every
 * `getBoundingClientRect()` field as `0` — and `0` is finite, so nothing downstream can tell
 * "there is no track" from "the pointer is at the track's top". The division then answers
 * `±Infinity` (or `NaN` at exactly `top`), and the clamp on the next line turns that into a
 * perfectly plausible **end of the track**. Measured before the fix against a hidden pane's
 * all-zero rect, viewport scrolled 40 lines up: every mouse move produced ratio `1` →
 * display offset `0`, i.e. the terminal slammed to the live bottom and stayed there.
 *
 * `Scrollbar` binds its move/up listeners to `window` and {@link Scrollbar.update} hides the
 * track without clearing `dragging`, so a drag genuinely outlives its box. **Two routes reach
 * it, and only one of them involves the host:**
 *
 * - a host hides the pane — `display: none` on an **ancestor**, documented since #801. The
 *   scrollback is untouched, so the ratio is `1` and the offset is driven to `0`: the measured
 *   slam to the live bottom, on top of the `NaN`;
 * - the widget hides its **own** track, because {@link scrollbarMetrics} makes `visible` mean
 *   `scrollbackLen > 0` and core's `full_reset` (RIS, `ESC c` — a program's `rs1`, `tput reset`,
 *   a crashed TUI) replaces the whole `Term`, taking `scrollback_len()` to `0`. No host action at
 *   all: thumb down, the application resets, the next frame hides the track under the drag. Here
 *   every legal offset is `0`, so only the `NaN` half of the harm exists.
 *
 * **Why the predicate is on the box and not on the ratio — and this is about *which* ratio.**
 * A finiteness test on the value this function *returns* would **accept the headline input**:
 * `Math.max(0, Math.min(1, Infinity))` is `1`, and `1` is finite, so an all-zero rect sails
 * through and `dragToDisplayOffset(1, …)` is that slam to the live bottom. Only a finiteness test
 * on the **un-clamped quotient** is equivalent to this guard, and then only up to a negative
 * height. `wheelScrollTarget` had already recorded the general form (`terminal.ts`): *"a result
 * check is not a substitute … the clamp rescues an infinite request into a finite, wrong one …
 * guarding there would fix half the cases and read as if it had fixed all of them."*
 *
 * Given that, `<= 0` is preferred over the quotient form because it says what is true — the box
 * was never measured — rather than that the arithmetic went odd, and because it matches
 * `proposeDimensions` (#810) and the renderer's grant check (#639) instead of inventing a third
 * predicate. The invariant this belongs to is that **zero is finite**
 * (`docs/map/invariant/an-absent-box-measures-as-zero.md`), which is why a finiteness test is the
 * wrong shape to reach for even where one placement of it would work.
 *
 * Contrast `WheelScroller.consumeWheelEvent`, which discharges the same obligation — *a
 * producer owes its consumer a value the consumer’s type can mean* (#675) — by returning `0`.
 * That works there because a line count of `0` **is** the no-op; for a ratio `0` means *scroll
 * to the top*, which is one of the two symptoms above. Hence a union, and a caller that skips
 * the request entirely.
 */
export function dragTrackRatio(clientY: number, track: { top: number; height: number }): number | undefined {
  if (track.height <= 0) return undefined;
  return Math.max(0, Math.min(1, (clientY - track.top) / track.height));
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
  /** Overview-ruler mark elements (#120 S3) — a POOL, reused across {@link setMarks} calls and
   * trimmed to the current mark count. Re-creating them per call cost ~18 microseconds per mark
   * (measured, Chromium): 18 ms for 1000 marks, i.e. a whole 60 Hz frame for one call, and 101 ms
   * for 5000. Marks are per matching LINE, so a search over a deep scrollback reaches those counts
   * routinely (#440). */
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
      // #500 §3: marks are CENTRED on their line, so the first and last ones extend past the
      // track by half a mark. Upstream has the same overhang and never notices it, because its
      // ruler is a canvas and `ctx.fillRect` is clipped by the backing store
      // (`OverviewRulerRenderer.ts:198-212` @ 699f553) — a containment a DOM element does not
      // get. Without this, a mark at ratio 0 paints ON the terminal canvas above the track and
      // one at ratio 1 paints below it. The clip is therefore part of the centring change, not
      // a tidy-up: shipping the offset alone makes the top edge reachable where it was not.
      // The thumb is unaffected — `thumbTopRatio + thumbHeightRatio` is
      // `(scrollbackLen - displayOffset + rows) / total <= 1` for any `displayOffset >= 0`.
      overflow: "hidden",
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
   * analog. Each mark is a coloured bar **centred** on its `topRatio` down the track, so
   * off-viewport anchors are visible on the full-buffer scrollbar. Marks live on
   * the track (they show with it) and don't intercept drags (`pointer-events:
   * none`). Drive it with `registry.rulerMarksForFrame(frame)` each frame.
   *
   * **Geometry is class-dependent since #500 §2/§3**: the height comes from
   * {@link rulerMarkHeightPx} (a `full` mark is thin, a gutter mark fat — the precondition
   * ADR-0024 R3's layering needs), and the box is centred rather than hung below its line. The
   * track clips, because centring puts half of the first and last marks outside it and a DOM
   * element gets none of the containment upstream's canvas backing store provides.
   *
   * **Elements are reused, not re-created (#440).** The pool is updated in place and trimmed, which
   * changes nothing observable — array order is still DOM order — but makes the call cost a style
   * write per mark instead of a node allocation. Two traps it introduces, both handled below and
   * neither reachable before: a reused element carries the previous mark's horizontal properties
   * (the three classes set different ones), and a shorter list leaves the tail attached.
   *
   * **Array order IS paint order.** Marks are appended in the order given and carry no `z-index`,
   * so a later mark paints over an earlier one — which is how `rulerMarksForFrame` expresses both
   * its ordering rules (registration order within a position class, #458; `full` above the gutter
   * classes, #498). Do NOT sort, reverse, or stack them here: either would silently void those
   * rules, and no unit test can catch it (vitest runs in a `node` environment) — only the
   * `__rulerLayerProbe` e2e does.
   */
  setMarks(marks: RulerMark[]): void {
    for (let i = 0; i < marks.length; i++) {
      const m = marks[i]!;
      let el = this.markEls[i];
      if (el === undefined) {
        el = document.createElement("div");
        el.dataset.rulerMark = ""; // stable hook for the #498 e2e probe (not a style)
        this.track.appendChild(el);
        this.markEls.push(el);
      }
      Object.assign(el.style, {
        position: "absolute",
        top: `${m.topRatio * 100}%`,
        // #500 §3: `top` places the mark's LINE, and the box is centred on it — xterm's
        // `- drawHeight / 2` (`OverviewRulerRenderer.ts:204` @ 699f553). Top-aligning put every
        // mark half a mark low, and put the LAST line's mark partly outside the track whenever
        // `trackHeightPx / totalLines < markHeightPx` — about any buffer over ~300 lines on a
        // 600px track, i.e. the ordinary case rather than an edge one. It is also what makes
        // #463's clamp mean what its comment claims: a mark clamped to `topRatio === 1` used to
        // be drawn ENTIRELY below the track (never visible, the outcome the clamp exists to
        // prevent), and is now half-visible at the edge.
        //
        // `translateY(-50%)` rather than a negative margin, deliberately: the offset must track
        // whatever `rulerMarkHeightPx` returns for this class, and a percentage transform reads
        // the element's own used height. A hard-coded `-1px` would be correct for `full` and
        // wrong for every gutter mark the line below now makes taller.
        transform: "translateY(-50%)",
        height: `${rulerMarkHeightPx(m.position)}px`,
        background: `#${(m.color & 0xffffff).toString(16).padStart(6, "0")}`,
        pointerEvents: "none",
        // RESET all three horizontal properties before applying this mark's class, because a
        // reused element carries the previous mark's. `rulerMarkX` returns a DIFFERENT key set per
        // class (`{left,width}` / `{right,width}` / `{left,right}`), so a `full` element reused as
        // a `left` gutter mark would keep `right: 0` and span the track. Re-creating the element
        // hid this; reusing it does not.
        left: "",
        right: "",
        width: "",
        ...rulerMarkX(m.position),
      } satisfies Partial<CSSStyleDeclaration>);
    }
    // Trim the surplus: a shorter mark list leaves the tail of the pool attached, which would paint
    // the PREVIOUS frame's marks under this one's.
    for (let i = marks.length; i < this.markEls.length; i++) this.markEls[i]!.remove();
    this.markEls.length = marks.length;
  }

  private dragTo(clientY: number): void {
    if (!this.dragging) return;
    const ratio = dragTrackRatio(clientY, this.track.getBoundingClientRect());
    // No box, no request. Deliberately NOT ending the drag: hiding a pane is reversible since
    // #801, the button is still down, and `mouseup` on `window` still ends it — so a pane shown
    // again mid-drag resumes following the pointer instead of silently going dead. While it is
    // hidden the drag is *inert*, not continuing: VS Code's scrollbar keeps scrolling through a
    // hide because it clones its state at pointerdown and never re-reads a box, and that is the
    // one behavioural difference. Refusing to scroll a pane the user cannot see is the harm #801
    // is about, so inert is the answer here rather than an omission.
    if (ratio === undefined) return;
    this.opts.onScroll(dragToDisplayOffset(ratio, this.pos));
  }

  dispose(): void {
    this.onUp();
    this.track.remove();
  }
}

/**
 * A ruler mark's height **in CSS px**, by position class (#500 §2).
 *
 * A `full` mark is thin and a gutter mark is fat, which is the geometry **ADR-0024 R3's
 * layering needs to mean anything**: that rule paints `full` above the gutter classes, and the
 * ADR records its own validity condition — the payoff "is gated on mark geometry", because with
 * one flat height class-last-wins only decides which colour shows on an exact overlap. So this
 * is not decoration: it is the precondition of a rule that already shipped.
 *
 * **CSS px, not device px** — and the issue's own acceptance item said device px, which was
 * wrong. Upstream's `drawHeight` is in device px because its ruler is a canvas whose backing
 * store is device-sized (`OverviewRulerRenderer.ts:124`, `:128`, `:153` @ 699f553); a DOM element
 * has no backing store, so that step has no analogue here. xterm's *public* option for the same
 * surface is documented "in CSS pixels" (`typings/xterm.d.ts:753`), and ADR-0023 governs
 * anyway: a length in the same space as an existing one (`ScrollbarOptions.width`) uses that
 * space. Porting the formula verbatim would also import a defect — its `[6, 12]` clamp is
 * applied to an already-device-px quantity and the result multiplied by `dpr` **again**, so the
 * same CSS layout yields a 10px gutter mark at dpr 1 and a 12px one at dpr 2.
 *
 * **Why `full` is unchanged at 2.** `round(2 * dpr)` device px *is* 2 CSS px, which is what this
 * file already wrote. `full` is also the default position (`decorations.ts`), so the divergence
 * this fixes was only ever the explicitly-gutter-positioned marks.
 *
 * **Why the gutter constant is 6, and when that stops being right.** Upstream's gutter height is
 * `clamp(trackHeightPx / totalLines, 6, 12)`. That expression equals its lower bound whenever
 * `totalLines > trackHeightPx / 6` — roughly 100 lines on a 600px track — so 6 is not the cheap
 * end of a range, it is *the adaptive formula's value on the domain a terminal actually runs in*
 * (scrollback is conventionally 1000+). **Validity condition:** below ~100 total lines upstream
 * grows the mark toward 12px and this stays at 6, so a shallow-buffer ruler is thinner here.
 * Making it adaptive is not blocked by the substrate — `height: clamp(6px, calc(100% /
 * totalLines), 12px)` would let the browser resolve it against the track with no measurement —
 * but by data: {@link Scrollbar.setMarks} is not given `totalLines`, and `pos` is written only by
 * {@link Scrollbar.update}, whose ordering relative to `setMarks` this class does not enforce.
 * Revisit if a shallow-scrollback consumer ever cares.
 *
 * Anything unrecognised is `full`, matching {@link rulerMarkX}'s own default branch — the two
 * must not disagree about which marks are the full-width ones, or a mark would be laid out as
 * `full` horizontally and as a gutter mark vertically.
 */
export function rulerMarkHeightPx(position: RulerMark["position"]): number {
  switch (position) {
    case "left":
    case "center":
    case "right":
      return 6;
    default:
      return 2;
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
