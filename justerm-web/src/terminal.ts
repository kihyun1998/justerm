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
import { CompositionController } from "./composition";
import { dispatchTermEvent, type EventHandlers } from "./events";

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
      // Typed text — a key OR committed IME text — keeps the caret solid (#116).
      if (intent.kind === "key" || intent.kind === "text") renderer.restartCursorBlink?.();
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
   * (a canvas is not focusable by default). Provide it WITH `input` + `getGeometry`
   * to wire keyboard/IME/wheel; omit the group for an output-only widget (e.g. one
   * that only wants {@link events}). */
  element?: HTMLElement;
  /** Where normalised input intents go — keys/paste/focus, a wheel notch when the
   * app tracks the wheel, and cursor keys from a wheel on the alt screen. The
   * backend feeds them to core's encoders. Required with `element`. */
  input?: InputSink;
  /** Canvas origin + cell size, read per event (it changes on resize) — maps a
   * wheel notch to cell coords for the app-reporting path. Required with `element`. */
  getGeometry?(): CellGeometry;
  /** A local scroll request: scroll the viewport to this display offset (lines up
   * from the bottom). Wheel (normal buffer, no app tracking) funnels here; the
   * consumer's scrollbar drag funnels to the SAME callback for one coherent
   * request. Omit to disable local scrolling. The backend applies it → a frame. */
  onScroll?(displayOffset: number): void;
  /** Wheel scroll tuning (xterm `scrollSensitivity`). */
  scroll?: ScrollOptions;
  /** Fire-and-forget consumer notifications (#117) — title/bell/cwd. The widget
   * subscribes the source's {@link import("./types").FrameSource.subscribeEvents}
   * channel and routes each event to these callbacks. Independent of the DOM group
   * above (works on an output-only widget). onLinkActivate stays with the link
   * controller (#113), not this stream. */
  events?: EventHandlers;
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
  /** Unsubscribe from the source's event channel (#117), if subscribed. */
  private eventUnsub: Unsubscribe | undefined;
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
  /** The hidden `<textarea>` that is the real keyboard/IME/clipboard target (a
   * canvas can't receive composition events); created on mount w/ options. */
  private textarea: HTMLTextAreaElement | undefined;
  private composition: CompositionController | undefined;
  /** Last cursor cell the textarea was moved to, so it repositions only on a move
   * (not every frame — that would force a layout read+write per output flush). */
  private textareaCell = "";

  constructor(
    private readonly source: FrameSource,
    private readonly renderer: Renderer,
    private readonly options?: TerminalOptions,
  ) {}

  /** Focus the keyboard/IME input target (the hidden textarea, #116). Consumers
   * that move focus away — an accessible-view overlay, a control button — call this
   * to return it, since the real input target is the textarea, not the canvas. */
  focus(): void {
    this.textarea?.focus();
  }

  /** Begin consuming frames from the source; wire the DOM if options were given. */
  mount(): void {
    this.unsubscribe = this.source.subscribe((frame) => {
      this.renderer.applyFrame(frame);
      this.renderer.render();
      this.track(frame);
      this.positionTextarea(frame);
    });
    if (this.options?.element) this.attach(this.options);
    // Consumer events (#117) — independent of the DOM group; wire whenever the
    // source has an event channel and the consumer supplied handlers.
    const events = this.options?.events;
    if (events && this.source.subscribeEvents) {
      this.eventUnsub = this.source.subscribeEvents((e) => dispatchTermEvent(e, events));
    }
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

  /** Attach the DOM listeners (browser-only glue). A hidden `<textarea>` over the
   * cursor is the real keyboard/IME/clipboard target (a canvas can't receive
   * composition events, #116); keys/paste/focus flow through it via {@link
   * captureInput}, gated by the {@link CompositionController} so an IME owns its
   * keys. The element (a container over the canvas) keeps the wheel + a pointer-down
   * that focuses the textarea. */
  private attach(o: TerminalOptions): void {
    // The DOM group is all-or-nothing: element requires input + getGeometry.
    const element = o.element;
    const input = o.input;
    const getGeometry = o.getGeometry;
    if (!element || !input || !getGeometry) return;
    const sink = rendererNotifyingSink(input, this.renderer);
    const ta = makeHiddenTextarea();
    element.appendChild(ta);
    this.textarea = ta;
    const composition = new CompositionController(ta, sink);
    this.composition = composition;

    // Keys flow through the textarea; the IME gate vetoes composition keys. A key
    // that finalizes a composition (Enter) still reports — the commit went first.
    this.detach.push(
      captureInput(ta, sink, {
        getGeometry,
        mouseReporting: () => false,
        beforeKey: (e) => {
          const proceed = composition.keydown(e.keyCode);
          // Clear once idle whether the key was swallowed (229 diff) or finalized a
          // composition (Enter, proceed=true) — both leave committed text behind.
          this.clearTextareaWhenIdle();
          return proceed;
        },
      }),
    );
    // Composition events only fire on the focused textarea; route them to the
    // controller, then clear the textarea once its deferred read has run.
    const onStart = (): void => composition.compositionStart();
    const onUpdate = (e: CompositionEvent): void => composition.compositionUpdate(e.data);
    const onEnd = (): void => {
      composition.compositionEnd();
      this.clearTextareaWhenIdle();
    };
    ta.addEventListener("compositionstart", onStart);
    ta.addEventListener("compositionupdate", onUpdate);
    ta.addEventListener("compositionend", onEnd);
    this.detach.push(() => {
      ta.removeEventListener("compositionstart", onStart);
      ta.removeEventListener("compositionupdate", onUpdate);
      ta.removeEventListener("compositionend", onEnd);
    });

    // Pointer-down focuses the textarea (it's pointer-events:none so the canvas
    // still gets the click for selection) and resets the blink phase.
    const onDown = (): void => {
      ta.focus();
      this.renderer.restartCursorBlink?.();
    };
    element.addEventListener("mousedown", onDown);
    this.detach.push(() => element.removeEventListener("mousedown", onDown));

    this.scroller = new WheelScroller(o.scroll);
    const onWheel = (e: WheelEvent): void => this.onWheel(e, o);
    element.addEventListener("wheel", onWheel, { passive: false });
    this.detach.push(() => element.removeEventListener("wheel", onWheel));
  }

  /** Clear the textarea once the controller's deferred read has run (same macro-task
   * queue → FIFO), but only if no composition is still in flight — so it doesn't
   * grow unbounded as IME text accumulates, without truncating a live composition. */
  private clearTextareaWhenIdle(): void {
    setTimeout(() => {
      if (this.textarea && this.composition && !this.composition.active) this.textarea.value = "";
    }, 0);
  }

  /** Move the hidden textarea over the cursor cell so the IME candidate window
   * appears there (xterm's updateCompositionElements). Geometry from the same
   * source the input uses; skipped when the cursor is absent/hidden. */
  private positionTextarea(frame: DecodedFrame): void {
    const ta = this.textarea;
    const getGeometry = this.options?.getGeometry;
    if (!ta || !getGeometry || frame.cursorRow === undefined || frame.cursorVisible === false) return;
    const col = frame.cursorCol ?? 0;
    const row = frame.cursorRow;
    // Only touch the DOM (a layout read via getGeometry + two style writes) when the
    // cursor actually moved, not on every output frame.
    const cell = `${col},${row}`;
    if (cell === this.textareaCell) return;
    this.textareaCell = cell;
    const g = getGeometry();
    ta.style.left = `${col * g.cellWidth}px`;
    ta.style.top = `${row * g.cellHeight}px`;
  }

  /** Route a wheel notch through the shared accumulator, then dispatch: a
   * wheel-button report to the app, cursor keys on the alt screen, or a local
   * scroll request. `none` (sub-line/zero) leaves the event for native scroll. */
  private onWheel(e: WheelEvent, o: TerminalOptions): void {
    // Attached only with the DOM group, so these are present; narrow for the types.
    const getGeometry = o.getGeometry;
    const input = o.input;
    if (!getGeometry || !input) return;
    // getGeometry's cellHeight is CSS px, matching pixel-mode deltaY; dpr 1 keeps
    // the scroller's `cellHeight / dpr` at CSS-px-per-cell.
    const lines = this.scroller!.consumeWheelEvent(e, {
      cellHeight: getGeometry().cellHeight,
      dpr: 1,
      rows: this.rows,
    });
    const action = routeWheel(this.mask, lines, this.altScreen, this.displayOffset, this.scrollbackLen);
    if (action.kind === "none") return;
    e.preventDefault();
    switch (action.kind) {
      case "app":
        // Direction from the accumulated lines (not raw deltaY) — coords from the event.
        input.send({ kind: "mouse", event: wheelMouseFromDom(e, lines, getGeometry()) });
        return;
      case "altKeys": {
        // The alt buffer has no scrollback; a wheel becomes a cursor key (xterm).
        // DECCKM (application-cursor-keys) is core's encode_key job — the web only
        // picks the direction.
        const key: NamedKey = action.direction === "up" ? "up" : "down";
        input.send({ kind: "key", event: { key: { type: key }, mods: 0, action: "press" } });
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
    this.eventUnsub?.();
    this.eventUnsub = undefined;
    for (const off of this.detach) off();
    this.detach = [];
    this.scroller = undefined;
    this.textarea?.remove();
    this.textarea = undefined;
    this.composition = undefined;
  }
}

/** The hidden `<textarea>` input proxy (#116). Positioned over the cursor so the
 * IME candidate window appears there, but visually invisible and click-through
 * (`pointer-events: none`) so the canvas owns the pointer; focus is programmatic.
 * `aria-hidden` because the screen-reader surface is the a11y mirror (#119), not
 * this element. */
function makeHiddenTextarea(): HTMLTextAreaElement {
  const ta = document.createElement("textarea");
  ta.setAttribute("aria-hidden", "true");
  ta.autocapitalize = "off";
  ta.autocomplete = "off";
  ta.spellcheck = false;
  Object.assign(ta.style, {
    position: "absolute",
    left: "0",
    top: "0",
    width: "1px",
    height: "1em",
    padding: "0",
    border: "0",
    margin: "0",
    outline: "none",
    resize: "none",
    opacity: "0",
    background: "transparent",
    color: "transparent",
    caretColor: "transparent",
    overflow: "hidden",
    whiteSpace: "nowrap",
    pointerEvents: "none",
    zIndex: "1",
  } satisfies Partial<CSSStyleDeclaration>);
  return ta;
}
