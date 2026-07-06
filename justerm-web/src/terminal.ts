import type { FrameSource, Unsubscribe, DecodedFrame } from "./types";
import type { Renderer } from "./renderer";
import {
  captureInput,
  MouseEvents,
  wheelMouseFromDom,
  type CellGeometry,
  type InputSink,
  type NamedKey,
} from "./input";
import { WheelScroller, type ScrollOptions } from "./scroll-control";

/**
 * Whether a wheel notch reports to the app rather than scrolling scrollback
 * locally — true only when the app tracks the wheel (the WHEEL bit of the frame's
 * `mouseWantedEvents` mask, #129). `undefined` (frame omitted the field) → local.
 * Per-category (not "any mouse mode"): an X10 app (`?9`, DOWN only) keeps the
 * wheel local, matching xterm's per-protocol wheel gate.
 */
export function wheelGoesToApp(mouseWantedEvents: number | undefined): boolean {
  return ((mouseWantedEvents ?? 0) & MouseEvents.Wheel) !== 0;
}

/**
 * The display offset a local wheel scroll requests, or `null` when the notch
 * moved no whole line. `lines` is the {@link WheelScroller} result (positive =
 * down/newer); `displayOffset` is lines UP from the bottom (0 = following), so
 * scrolling newer LOWERS it. Clamped to `[0, scrollbackLen]` — can't scroll past
 * the live edge or before the oldest history line. The backend scrolls to it.
 */
export function wheelScrollTarget(
  lines: number,
  displayOffset: number,
  scrollbackLen: number,
): number | null {
  if (lines === 0) return null;
  const target = displayOffset - lines;
  return Math.max(0, Math.min(scrollbackLen, target));
}

/**
 * What a wheel notch does, once {@link WheelScroller} has turned it into whole
 * `lines`. Three destinations, mirroring xterm: `app` (the app tracks the wheel —
 * a wheel-button report), `altKeys` (the alt buffer has no scrollback, so a
 * non-tracking app gets cursor keys — xterm's `_handlePassiveWheel`), and `scroll`
 * (normal-buffer local scrollback). `none` = a sub-line/zero notch (nothing yet).
 */
export type WheelAction =
  | { kind: "app"; direction: "up" | "down" }
  | { kind: "altKeys"; direction: "up" | "down" }
  | { kind: "scroll"; displayOffset: number }
  | { kind: "none" };

/**
 * Decide where a wheel notch goes. Gate on the accumulated `lines` first (a
 * sub-line trackpad notch or a shift/zero wheel is `none` — the {@link
 * WheelScroller} already returned 0), so the app never gets hyper-sensitive
 * per-pixel reports (xterm routes its wheel report through the SAME accumulator).
 * Precedence: a wheel-tracking app wins even on the alt screen; else the alt
 * buffer (no scrollback) takes cursor keys; else local scrollback.
 */
export function routeWheel(
  mouseWantedEvents: number | undefined,
  lines: number,
  altScreen: boolean,
  displayOffset: number,
  scrollbackLen: number,
): WheelAction {
  if (lines === 0) return { kind: "none" };
  const direction = lines < 0 ? "up" : "down";
  if (wheelGoesToApp(mouseWantedEvents)) return { kind: "app", direction };
  if (altScreen) return { kind: "altKeys", direction };
  return { kind: "scroll", displayOffset: wheelScrollTarget(lines, displayOffset, scrollbackLen)! };
}

/**
 * Wrap an {@link InputSink} so the renderer's local cursor/selection state tracks
 * input before each intent forwards: a KEY intent restarts the cursor blink (the
 * S5 #107 deferral — the caret must show at once on a keystroke, before the echo
 * frame), a FOCUS intent drives the renderer's focus state (a blurred terminal
 * stops blinking + shows the inactive selection tint). Both renderer hooks are
 * optional; a cursorless renderer is left untouched. Other intents pass through.
 */
export function rendererNotifyingSink(sink: InputSink, renderer: Renderer): InputSink {
  return {
    send(intent) {
      if (intent.kind === "key") renderer.restartCursorBlink?.();
      else if (intent.kind === "focus") renderer.setFocused?.(intent.focused);
      sink.send(intent);
    },
  };
}

/**
 * Wiring the {@link Terminal} needs to be a complete widget, not just a frame
 * pump. Omit it and the widget is the pure source→renderer pump (headless-
 * testable, no DOM); supply it and `mount` also captures input, restarts the
 * cursor blink on typing, tracks focus, and routes the wheel (S16 #133).
 */
export interface TerminalOptions {
  /** The element input listeners attach to (the canvas or a wrapper). The widget
   * makes it focusable and focuses it on pointer-down so keystrokes are captured
   * (a canvas is not focusable by default). */
  element: HTMLElement;
  /** Where normalised input intents go — keys/paste/focus, a wheel notch when the
   * app tracks the wheel, and cursor keys from a wheel on the alt screen. The
   * backend feeds them to core's encoders. */
  input: InputSink;
  /** Canvas origin + cell size, read per event (it changes on resize) — maps a
   * wheel notch to cell coords for the app-reporting path. */
  getGeometry(): CellGeometry;
  /** A local scroll request: scroll the viewport to this display offset (lines up
   * from the bottom). Wheel (normal buffer, no app tracking) funnels here; the
   * consumer's scrollbar drag funnels to the SAME callback for one coherent
   * request. Omit to disable local scrolling. The backend applies it → a frame. */
  onScroll?(displayOffset: number): void;
  /** Wheel scroll tuning (xterm `scrollSensitivity`). */
  scroll?: ScrollOptions;
}

/**
 * The browser terminal widget: wires a {@link FrameSource} to a {@link Renderer}
 * and, given {@link TerminalOptions}, to the DOM (input capture + wheel + cursor
 * blink + focus). It owns no transport and no GL — both are injected. Each frame
 * from the source is handed to the renderer and presented. This keeps the widget
 * source-agnostic (frame mode / in-wasm) and renderer-agnostic (real beamterm /
 * fake), which is what makes it testable without a backend or a canvas.
 *
 * The DOM attachment in {@link mount} is browser-only glue (not unit-tested, like
 * {@link captureInput}); the decisions it makes — wheel routing ({@link routeWheel})
 * and renderer notification ({@link rendererNotifyingSink}) — are pure and covered.
 */
export class Terminal {
  private unsubscribe: Unsubscribe | undefined;
  /** Detachers for the input capture + wheel + focus listeners (mount w/ options). */
  private detach: Array<() => void> = [];
  /** Wheel → line delta (stateful: carries trackpad sub-line remainders). Shared
   * by the app-report and local-scroll paths, like xterm's single accumulator. */
  private scroller: WheelScroller | undefined;
  /** Latest frame state the wheel router reads (a frame may omit any of them). */
  private mask = 0;
  private displayOffset = 0;
  private scrollbackLen = 0;
  private rows = 0;
  private altScreen = false;

  constructor(
    private readonly source: FrameSource,
    private readonly renderer: Renderer,
    private readonly options?: TerminalOptions,
  ) {}

  /** Begin consuming frames from the source; wire the DOM if options were given. */
  mount(): void {
    this.unsubscribe = this.source.subscribe((frame) => {
      this.renderer.applyFrame(frame);
      this.renderer.render();
      this.track(frame);
    });
    if (this.options) this.attach(this.options);
  }

  /** Retain the scroll/routing state each frame carries; drop the wheel remainder
   * on a buffer switch (alt-screen), so a fresh screen doesn't inherit a stale
   * trackpad fraction (xterm resets on buffer switch). */
  private track(frame: DecodedFrame): void {
    this.mask = frame.mouseWantedEvents ?? 0;
    this.displayOffset = frame.displayOffset ?? 0;
    this.scrollbackLen = frame.scrollbackLen ?? 0;
    this.rows = frame.rows;
    const alt = frame.altScreen ?? false;
    if (alt !== this.altScreen) {
      this.altScreen = alt;
      this.scroller?.reset();
    }
  }

  /** Attach the DOM listeners (browser-only glue). Make the element focusable and
   * focus it on pointer-down (a canvas is not focusable by default, and a
   * consumer's selection handler may `preventDefault` the native focus); keys/
   * paste/focus report via {@link captureInput}; mouse clicks stay local (out of
   * S16 scope), so its `mouseReporting` is off and the widget owns the wheel. */
  private attach(o: TerminalOptions): void {
    if (o.element.tabIndex < 0) o.element.tabIndex = 0;
    const sink = rendererNotifyingSink(o.input, this.renderer);
    this.detach.push(
      captureInput(o.element, sink, { getGeometry: o.getGeometry, mouseReporting: () => false }),
    );
    // Focus (and reset the blink phase, xterm restartBlinkAnimation) on pointer-down,
    // so a click into the terminal starts capturing keys and shows a solid caret.
    const onDown = (): void => {
      o.element.focus();
      this.renderer.restartCursorBlink?.();
    };
    o.element.addEventListener("mousedown", onDown);
    this.detach.push(() => o.element.removeEventListener("mousedown", onDown));

    this.scroller = new WheelScroller(o.scroll);
    const onWheel = (e: WheelEvent): void => this.onWheel(e, o);
    o.element.addEventListener("wheel", onWheel, { passive: false });
    this.detach.push(() => o.element.removeEventListener("wheel", onWheel));
  }

  /** Route a wheel notch through the shared accumulator, then dispatch: a
   * wheel-button report to the app, cursor keys on the alt screen, or a local
   * scroll request. `none` (sub-line/zero) leaves the event for native scroll. */
  private onWheel(e: WheelEvent, o: TerminalOptions): void {
    // getGeometry's cellHeight is CSS px, matching pixel-mode deltaY; dpr 1 keeps
    // the scroller's `cellHeight / dpr` at CSS-px-per-cell.
    const lines = this.scroller!.consumeWheelEvent(e, {
      cellHeight: o.getGeometry().cellHeight,
      dpr: 1,
      rows: this.rows,
    });
    const action = routeWheel(this.mask, lines, this.altScreen, this.displayOffset, this.scrollbackLen);
    if (action.kind === "none") return;
    e.preventDefault();
    switch (action.kind) {
      case "app":
        // Direction from the accumulated lines (not raw deltaY) — coords from the event.
        o.input.send({ kind: "mouse", event: wheelMouseFromDom(e, lines, o.getGeometry()) });
        return;
      case "altKeys": {
        // The alt buffer has no scrollback; a wheel becomes a cursor key (xterm).
        // DECCKM (application-cursor-keys) is core's encode_key job — the web only
        // picks the direction.
        const key: NamedKey = action.direction === "up" ? "up" : "down";
        o.input.send({ kind: "key", event: { key: { type: key }, mods: 0, action: "press" } });
        return;
      }
      case "scroll":
        if (!o.onScroll) return;
        // Optimistically advance the tracked offset so a fast burst of notches
        // composes (frame mode's echo is an async round-trip); track() reconciles.
        this.displayOffset = action.displayOffset;
        o.onScroll(action.displayOffset);
        return;
    }
  }

  /** Stop consuming frames and detach DOM listeners; safe to call more than once. */
  dispose(): void {
    this.unsubscribe?.();
    this.unsubscribe = undefined;
    for (const off of this.detach) off();
    this.detach = [];
    this.scroller = undefined;
  }
}
