import type { FrameSource, Unsubscribe, DecodedFrame } from "./types";
import type { Renderer } from "./renderer";
import {
  captureInput,
  isUserInput,
  wheelMouseFromDom,
  type CellGeometry,
  type InputScrollSignal,
  type InputSink,
  type NamedKey,
} from "./input";
import { PointerRouter, type LocalPointer } from "./pointer";
import { SCROLLBAR_ATTRIBUTE } from "./scrollbar";
import { routeWheel, scrollsToBottomOnInput, WheelScroller, type ScrollOptions } from "./scroll-control";
import { CompositionController, preeditIntent, preeditLatch, textareaMove, type TextareaAnchor } from "./composition";
import { suggestionCell, suggestionColorRef, type SuggestionOptions } from "./suggestion";
import { ClipboardController, type ClipboardOptions } from "./clipboard";
import { dispatchTermEvent, type EventHandlers } from "./events";
import { hoverSpans, LinkTracker } from "./link-tracker";
import type { Link, LinkOptions } from "./links";

/** The clearing call's payload — a run of no cells. Named because `new Uint32Array(0)` at a call
 * site reads as an accident rather than as "stop drawing this". */
const EMPTY_PREEDIT = new Uint32Array(0);

/**
 * Wrap an {@link InputSink} so the renderer tracks input before each intent forwards: a key or
 * committed IME text restarts the cursor blink, and a focus intent sets the renderer's focus
 * state. Both renderer hooks are optional; other intents pass through.
 */
export function rendererNotifyingSink(sink: InputSink, renderer: Renderer): InputSink {
  return {
    send(intent) {
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
 * cursor blink on typing, tracks focus, and routes the wheel and pointer presses.
 */
export interface TerminalOptions {
  /** The element input listeners attach to (the canvas or a wrapper). Provide it WITH `input` +
   * `getGeometry` to wire keyboard/IME/wheel; omit the group for an output-only widget (e.g. one
   * that only wants {@link events}).
   *
   * **It does not need to be focusable, and the widget does not make it so.** The real keyboard/IME
   * target is a hidden textarea the widget mounts inside it, and a pointer-down here focuses *that*
   * through {@link Terminal.focus}. A canvas being unfocusable is therefore not a problem to solve.
   *
   * **If you do make it (or a child) focusable, its pointer-down's default must be cancelled.** The
   * browser's focusing steps run after our `mousedown` handler, so an un-cancelled default moves
   * focus to your element and blurs the textarea — typing and IME both stop. The widget cancels a
   * press it reports to the application or hands to {@link selection}. Any other press keeps
   * its default and cancelling it is yours: with no `selection` and an application that tracks
   * nothing, a report it could not make (no measured box, the back/forward buttons), or a press on a
   * `Scrollbar` mounted inside the element. */
  element?: HTMLElement;
  /** Where normalised input intents go — keys/paste/focus, pointer and wheel reports
   * when the app tracks them, and cursor keys from a wheel on the alt screen. The
   * backend feeds them to core's encoders. Required with `element`. */
  input?: InputSink;
  /** Canvas origin + cell size, read per event (it changes on resize) — maps a
   * pointer event or wheel notch to cell coords. Required with `element`.
   *
   * Answer `undefined` when the box cannot be measured (`display: none`, detached, not yet laid
   * out): an absent box measures as all zeros and `0` is in range for everything derived from it,
   * so only the code that took the measurement can tell it from a real one. See
   * {@link CaptureOptions.getGeometry}, which states the contract. */
  getGeometry?(): CellGeometry | undefined;
  /**
   * The consumer's say over a keydown before the widget encodes it — xterm.js's
   * `attachCustomKeyEventHandler`. Return `false` to claim the key: no intent is sent and the widget
   * does not call `preventDefault`. Return `true` to let it through.
   *
   * **Asked after the IME gate, never before it.** A key an IME owns (a `keyCode` 229 composition
   * key, or a modifier while composing) never reaches this hook, so a claim cannot break composing.
   * A key that finalizes a composition has already committed its text when this is asked, as with
   * Enter.
   *
   * **A claimed key keeps its browser default, and cancelling it is yours.** An un-cancelled
   * `Ctrl+V` / `Ctrl+Shift+V` goes on to fire `paste` on the input textarea, which the widget sends
   * as a paste intent. Call `ev.preventDefault()` here for any chord whose default you replace.
   *
   * **It cannot stop all input.** An IME commit is a text intent, not a key, so returning `false` for
   * everything still lets composed text through; drop intents at {@link input} for that.
   */
  beforeKey?(ev: KeyboardEvent): boolean;
  /**
   * What a pointer press does when it stays local — normally the consumer's
   * {@link import("./selection").SelectionController}, which already has this shape.
   *
   * The widget owns the pointer: it listens on {@link element}, decides per press whether the
   * application gets it (the frame's `mouseWantedEvents` mask, Shift forcing it local), follows the
   * gesture on `window` to its release, and drives `tick()` while a local drag is held. So hand the
   * controller over here and **bind no pointer listeners of your own for it**, or every press is
   * handled twice. A press the widget acts on has its default cancelled, which is what keeps focus on
   * the input textarea (see {@link element}).
   *
   * Omit it and a press the application does not take does nothing locally.
   */
  selection?: LocalPointer;
  /**
   * Clickable links: OSC 8 links from the frame stream, and plain-text URLs through
   * {@link import("./links").LinkOptions.port}. The widget decides hover and click from the pointer
   * it already owns, so bind no pointer listeners for links either.
   *
   * A link is live where a press would stay local: over an application that tracks presses it is
   * inert unless Shift is held, the same rule the selection follows. It opens on a single primary
   * click whose press and release land on it. Hovering one sets the pointer cursor on
   * {@link element} — a descendant with its own `cursor` style hides it — and underlines it through
   * the renderer's `setLinkHover`.
   *
   * Needs {@link element} and a renderer that exposes `cellFlags`; without either, links are inert.
   */
  links?: LinkOptions;
  /** A local scroll request: scroll the viewport to this display offset (lines up
   * from the bottom). Three producers funnel to the SAME callback for one coherent
   * request: the wheel (normal buffer, no app tracking), the consumer's scrollbar
   * drag, and user input arriving while the view is scrolled up,
   * which always asks for `0`. Omit to disable local scrolling **and the input
   * snap with it**. The backend applies it → a frame. */
  onScroll?(displayOffset: number): void;
  /** Wheel scroll tuning (xterm `scrollSensitivity`). */
  scroll?: ScrollOptions;
  /** Fire-and-forget consumer notifications — title/bell/cwd and an
   * application's `OSC 9` / `OSC 777` notification. The widget
   * subscribes the source's {@link import("./types").FrameSource.subscribeEvents}
   * channel and routes each event to these callbacks. Independent of the DOM group
   * above (works on an output-only widget). Link activation is {@link links}, not
   * this stream: a link is per-cell state, not an event ([ADR-0020](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0020-what-qualifies-for-the-frame-snapshot.md)). */
  events?: EventHandlers;
  /** `OSC 52` clipboard requests — an application asking to write, or read,
   * the user's clipboard. Rides the same {@link events} subscription, but is not a
   * notification: the consumer *acts on* it and owes a query a reply.
   *
   * **Omit it and the widget does nothing in either direction** — no clipboard is
   * touched and a query goes unanswered, which is how the sequence is refused.
   * Supply a {@link import("./clipboard").ClipboardProvider} to honour writes, and
   * a `port` as well to answer reads. */
  clipboard?: ClipboardOptions;
}

/**
 * The browser terminal widget: wires a {@link FrameSource} to a {@link Renderer}
 * and, given {@link TerminalOptions}, to the DOM (input capture, wheel, cursor
 * blink, focus). It owns no transport and no GL — both are injected, so it runs
 * against any source (frame mode or in-wasm) and any renderer. Each frame from the
 * source is handed to the renderer and presented.
 *
 * The decisions {@link mount} wires — wheel routing ({@link routeWheel}), pointer
 * routing ({@link PointerRouter}), the input snap ({@link scrollsToBottomOnInput})
 * and renderer notification ({@link rendererNotifyingSink}) — are pure; the DOM
 * wiring is browser-only glue. How each half is tested:
 * [`docs/map/territory/browser-proof-harness.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/browser-proof-harness.md).
 */
export class Terminal {
  private unsubscribe: Unsubscribe | undefined;
  /** Unsubscribe from the source's event channel, if subscribed. */
  private eventUnsub: Unsubscribe | undefined;
  /** Detachers for the input capture + wheel + focus listeners (mount w/ options). */
  private detach: Array<() => void> = [];
  /** Wheel → line delta (stateful: carries trackpad sub-line remainders). Shared
   * by the app-report and local-scroll paths. */
  private readonly scroller: WheelScroller;
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
  /** The OSC 52 router, when the consumer wired one. Held so `dispose()` can
   * end it — an in-flight clipboard read outlives the event subscription. */
  private clipboardController: ClipboardController | undefined;
  /** Last cursor cell the textarea was moved to, so it repositions only on a move
   * rather than on every frame. */
  private textareaCell = "";
  /** The cursor cell the latest frame reported, retained so the anchor can be re-synced at a
   * point of use without waiting for a frame. Written by {@link Terminal.track} on every frame that
   * carries a cursor, and not cleared when the cursor hides. Why:
   * [`docs/map/invariant/composition-is-browser-owned-state.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/invariant/composition-is-browser-owned-state.md). */
  private cursorAnchor: TextareaAnchor | undefined;
  /** The `displayOffset` of the frame the renderer has actually been given — **not**
   * {@link Terminal.displayOffset}, which `onUserInput` advances to 0 optimistically ahead of the
   * echo. Anything mapping a grid coordinate onto what the user is looking at has to use
   * this one: the screen shows the last frame applied, not the scroll the widget has requested. */
  private frameOffset = 0;
  /** The viewport row the open composition's run is currently drawn at, or `undefined` when it is
   * not drawn — because nothing is composing, or because its cell is off the bottom of the
   * viewport. The re-assert below compares against it so a composition over a still view costs
   * nothing. */
  private preeditPaintedRow: number | undefined;
  /** The composition text currently drawn. Held to drop the settling `compositionupdate`
   * a real IME emits once per syllable with unchanged data — see {@link Terminal.showPreedit}. */
  private preeditText = "";
  /** Where the open composition started. Latched at `compositionstart` and held for the
   * composition's life, rather than re-read from {@link Terminal.cursorAnchor}, which the frame
   * stream keeps reassigning. */
  private preeditOrigin: TextareaAnchor | undefined;
  /** Where the open composition's run currently ends — the renderer's caret column. Set by every
   * non-empty update and consumed by the next `compositionstart`'s latch, so a composition that draws
   * nothing leaves it undefined. At the right margin it names a column inside the run, and after a
   * commit that wraps, the row above: [ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md). */
  private preeditEnd: TextareaAnchor | undefined;
  /** The consumer's suggestion text and how it draws ([justerm#972](https://github.com/kihyun1998/justerm/issues/972)). `""` = none. */
  private suggestionText = "";
  private suggestionColor = 0;
  private suggestionDim = true;
  /** The viewport cell the renderer was last given the suggestion at, or `undefined` when nothing
   * is drawn. */
  private suggestionPainted: TextareaAnchor | undefined;
  /** The link state, when {@link TerminalOptions.links} is wired. */
  private links: LinkTracker | undefined;
  /** The pointer router, once the DOM group is attached — a frame re-asks its hover question. */
  private router: PointerRouter | undefined;
  /** Whether a frame is being applied, so a hover change inside it rides that frame's present. */
  private applyingFrame = false;
  /** The element's inline `cursor` from before a link was hovered, restored on leave. */
  private cursorBeforeLink: string | undefined;
  /** Latched by {@link Terminal.dispose}: the widget's lifecycle is one-shot, so this both
   * keeps the renderer from being disposed twice and refuses a re-mount. */
  private disposed = false;

  constructor(
    private readonly source: FrameSource,
    private readonly renderer: Renderer,
    private readonly options?: TerminalOptions,
  ) {
    this.scroller = new WheelScroller(options?.scroll);
  }

  /** Change the wheel sensitivities ({@link TerminalOptions.scroll}) from the next wheel event
   * on, without rebuilding the widget. A field `opts` leaves out keeps its current value, and a
   * sub-line remainder already carried is kept. Callable before {@link Terminal.mount}; after
   * {@link Terminal.dispose} it has nothing left to affect. */
  setScrollOptions(opts: ScrollOptions): void {
    this.scroller.setOptions(opts);
  }

  /**
   * Draw `text` after the cursor, as an autosuggestion at a shell prompt; `""` clears it.
   *
   * It is drawn by the renderer and stored in no cell, so it never reaches copy or search. It
   * starts at the engine's cursor and moves with it on every frame. It stops at the first cell that
   * is not blank and at the right edge. A selection, search match, decoration or hovered link
   * keeps the engine's cell under it. It is hidden while an IME composition is open and when the
   * cursor's row is scrolled out of view.
   *
   * Returns `false`, drawing nothing, after {@link Terminal.dispose} and while the alternate screen is active. Entering the alternate
   * screen drops the suggestion, and leaving it does not bring it back. Needs a renderer with
   * {@link Renderer.setSuggestion}; with one that lacks it this returns `true` and draws nothing.
   */
  setSuggestion(text: string, options?: SuggestionOptions): boolean {
    if (this.disposed || (this.altScreen && text !== "")) return false;
    this.suggestionText = text;
    this.suggestionColor = suggestionColorRef(options?.color);
    this.suggestionDim = options?.dim ?? true;
    this.paintSuggestion(true);
    this.renderer.render();
    return true;
  }

  /** Focus the keyboard/IME input target (the hidden textarea). Consumers
   * that move focus away — an accessible-view overlay, a control button — call this
   * to return it, since the real input target is the textarea, not the canvas.
   *
   * Re-anchors first: focusing is a moment something reads the element's position, and the cell
   * may have moved since the cursor last did. */
  focus(): void {
    this.syncTextareaAnchor();
    this.textarea?.focus();
  }

  /**
   * Begin consuming frames from the source; wire the DOM if options were given.
   *
   * **Not callable after {@link Terminal.dispose}** — it throws. Build a new `Terminal` (and a new
   * renderer) instead. Why the lifecycle is one-shot:
   * [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md).
   */
  mount(): void {
    if (this.disposed) {
      throw new Error(
        "justerm-web: this Terminal was disposed — build a new one rather than re-mounting",
      );
    }
    const element = this.options?.element;
    const flagBits = this.renderer.cellFlags;
    if (this.options?.links && element && flagBits) {
      this.links = new LinkTracker({
        flagBits,
        options: this.options.links,
        onHover: (link) => this.showLink(link, element),
        onLeave: () => this.hideLink(element),
      });
    }
    this.unsubscribe = this.source.subscribe((frame) => {
      this.renderer.applyFrame(frame);
      // Before the links: they re-ask the hover's question against this frame's mask.
      this.track(frame);
      this.paintSuggestion(false);
      this.applyingFrame = true;
      try {
        this.links?.applyFrame(frame);
        this.router?.refresh();
      } finally {
        this.applyingFrame = false;
      }
      this.renderer.render();
      this.positionTextarea(frame);
      this.repaintPreedit();
    });
    if (this.options?.element) this.attach(this.options);
    // Consumer events — independent of the DOM group; wire whenever the source has an event
    // channel and the consumer wants something off it. One subscription serves notifications and
    // the clipboard pair (docs/map/territory/events-and-replies.md).
    const events = this.options?.events;
    const clipboard = this.options?.clipboard;
    const wantsClipboard = clipboard?.provider !== undefined || clipboard?.port !== undefined;
    if ((events || wantsClipboard) && this.source.subscribeEvents) {
      // Held on `this` so `dispose()` can end it.
      const controller = wantsClipboard ? new ClipboardController(clipboard) : undefined;
      this.clipboardController = controller;
      this.eventUnsub = this.source.subscribeEvents((e) => {
        // Floated, not awaited: `handle` never rejects.
        void controller?.handle(e);
        if (events) dispatchTermEvent(e, events);
      });
    }
  }

  /** Present a hovered link: the renderer underlines it and the element shows the pointer cursor. */
  private showLink(link: Link, element: HTMLElement): void {
    this.renderer.setLinkHover?.(hoverSpans(link.cells, this.links?.rows ?? 0));
    if (this.cursorBeforeLink === undefined) this.cursorBeforeLink = element.style.cursor;
    element.style.cursor = "pointer";
    if (!this.applyingFrame) this.renderer.render();
  }

  /** Undo {@link showLink}. */
  private hideLink(element: HTMLElement): void {
    this.renderer.setLinkHover?.(new Uint32Array(0));
    element.style.cursor = this.cursorBeforeLink ?? "";
    this.cursorBeforeLink = undefined;
    if (!this.applyingFrame) this.renderer.render();
  }

  /** Wraps the consumer's sink so input that counts returns the view to the bottom. */
  private scrollOnUserInput(inner: InputSink, o: TerminalOptions): InputSink {
    return {
      send: (intent) => {
        this.onUserInput(intent, o);
        inner.send(intent);
      },
    };
  }

  /** What user input does locally: drop the selection, and return the view to the bottom. */
  private onUserInput(signal: InputScrollSignal, o: TerminalOptions): void {
    if (!isUserInput(signal)) return;
    // Unconditional — see {@link isUserInput}. `clear` is optional on the port.
    o.selection?.clear?.();
    if (!o.onScroll || !scrollsToBottomOnInput(signal, this.displayOffset)) return;
    // Optimistic, ahead of the echo, as the wheel's scroll case is.
    this.displayOffset = 0;
    o.onScroll(0);
  }

  /** Retain the state each frame carries — scroll, routing, and the cursor cell the IME anchor is
   * placed from — and drop the wheel remainder on a buffer switch (alt-screen). Every write is
   * unconditional: what a frame says about *drawing* (`cursorVisible`) never decides whether the
   * widget keeps what that frame *said*. Why:
   * [`docs/map/invariant/composition-is-browser-owned-state.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/invariant/composition-is-browser-owned-state.md),
   * [`docs/map/territory/viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/viewport.md). */
  private track(frame: DecodedFrame): void {
    this.mask = frame.mouseWantedEvents ?? 0;
    // The IME anchor's cell, retained here with the rest of the frame state (#921).
    if (frame.cursorRow !== undefined) {
      this.cursorAnchor = { col: frame.cursorCol ?? 0, row: frame.cursorRow };
    }
    this.displayOffset = frame.displayOffset ?? 0;
    this.frameOffset = this.displayOffset;
    this.scrollbackLen = frame.scrollbackLen ?? 0;
    this.rows = frame.rows;
    const alt = frame.altScreen ?? false;
    if (alt !== this.altScreen) {
      this.altScreen = alt;
      this.scroller.reset();
      if (alt) this.suggestionText = "";
    }
  }

  /** Attach the DOM listeners (browser-only glue). A hidden `<textarea>` over the
   * cursor is the real keyboard/IME/clipboard target (a canvas can't receive
   * composition events); keys/paste/focus flow through it via {@link
   * captureInput}, gated by the {@link CompositionController} so an IME owns its
   * keys, then by the consumer's {@link TerminalOptions.beforeKey}. The element (a container over
   * the canvas) keeps the wheel and the pointer. */
  private attach(o: TerminalOptions): void {
    // The DOM group is all-or-nothing: element requires input + getGeometry.
    const element = o.element;
    const input = o.input;
    const getGeometry = o.getGeometry;
    if (!element || !input || !getGeometry) return;
    // The snap wraps the consumer's sink; `onWheel`'s reports go straight to `o.input`, past it
    // (docs/map/territory/viewport.md).
    const sink = rendererNotifyingSink(this.scrollOnUserInput(input, o), this.renderer);
    const ta = makeHiddenTextarea();
    element.appendChild(ta);
    this.textarea = ta;
    // Establish the renderer's focus state before any focus event (#912): unfocused, reported to the
    // renderer directly rather than through `sink` (docs/map/territory/widget-lifecycle.md).
    this.renderer.setFocused?.(false);
    const composition = new CompositionController(ta, sink);
    this.composition = composition;

    // Keys flow through the textarea; the IME gate vetoes composition keys. A key
    // that finalizes a composition (Enter) still reports — the commit went first.
    this.detach.push(
      captureInput(ta, sink, {
        getGeometry,
        beforeKey: (e) => {
          const proceed = composition.keydown(e.keyCode);
          // Clear once idle whether the key was swallowed (229 diff) or finalized a
          // composition (Enter, proceed=true) — both leave committed text behind.
          this.clearTextareaWhenIdle();
          // Swallowed by the IME: no intent will ever be sent for this key, so the sink
          // below cannot see it — and the user is typing (#913). The gate also swallows
          // bare Shift/Ctrl/Alt/CapsLock mid-composition, hence the key travels with it.
          if (!proceed) this.onUserInput({ kind: "imeKey", key: e.key }, o);
          return proceed && (o.beforeKey?.(e) ?? true);
        },
      }),
    );
    // Composition events only fire on the focused textarea; route them to the
    // controller, then clear the textarea once its deferred read has run.
    // The caret also stops blinking for the duration (#592).
    const onStart = (): void => {
      // Re-anchor BEFORE the controller is told a composition began (#631): the guard in
      // `textareaMove` keys on `composing`, which reads false here only because this runs before
      // `compositionStart()`. Swapping the two silently disables the re-sync.
      this.syncTextareaAnchor();
      // Latch the origin, after the re-sync above made the cell current. `active`, not `composing`:
      // read before `compositionStart()`, it means a commit is still queued behind its deferred read
      // (#911, ADR-0028).
      this.preeditOrigin = preeditLatch(this.cursorAnchor, this.preeditEnd, composition.active);
      this.preeditEnd = undefined;
      composition.compositionStart();
      this.renderer.setComposing?.(true);
      if (this.paintSuggestion(false)) this.renderer.render();
    };
    const onUpdate = (e: CompositionEvent): void => {
      composition.compositionUpdate(e.data);
      this.showPreedit(e.data);
    };
    const onEnd = (): void => {
      composition.compositionEnd();
      this.renderer.setComposing?.(false);
      if (this.paintSuggestion(false)) this.renderer.render();
      // Clear the drawn run BEFORE the commit reaches the grid (docs/map/territory/input-encoding.md).
      this.showPreedit("");
      this.preeditOrigin = undefined;
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

    // Pointer-down focuses the textarea (it's pointer-events:none, so the press lands on the
    // element) and resets the blink phase, then goes to the application or stays local (#902).
    let tickTimer: ReturnType<typeof setInterval> | undefined;
    const setTicking = (on: boolean): void => {
      clearInterval(tickTimer);
      tickTimer = on ? setInterval(() => o.selection?.tick(), SELECTION_TICK_MS) : undefined;
    };
    const router = (this.router = new PointerRouter({
      mask: () => this.mask,
      getGeometry,
      send: (event) => sink.send({ kind: "mouse", event }),
      local: o.selection,
      links: this.links,
      setTicking,
    }));
    const onMove = (e: MouseEvent): void => {
      router.move(e);
      if (!router.active) unfollow();
    };
    const onUp = (e: MouseEvent): void => {
      router.up(e);
      if (!router.active) unfollow();
    };
    const unfollow = (): void => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    const onDown = (e: MouseEvent): void => {
      this.focus(); // not `ta.focus()` — routes through the anchor re-sync (#631)
      this.renderer.restartCursorBlink?.();
      if (onScrollbar(e) || !router.down(e)) return;
      e.preventDefault();
      if (!router.active) return;
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    };
    const onHover = (e: MouseEvent): void => {
      if (onScrollbar(e)) router.leave();
      else router.hover(e);
    };
    const onLeave = (): void => router.leave();
    element.addEventListener("mousedown", onDown);
    element.addEventListener("mousemove", onHover);
    element.addEventListener("mouseleave", onLeave);
    this.detach.push(() => {
      element.removeEventListener("mousedown", onDown);
      element.removeEventListener("mousemove", onHover);
      element.removeEventListener("mouseleave", onLeave);
      unfollow();
      setTicking(false);
    });

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

  /** Move the hidden textarea over the cursor cell so the IME candidate window appears there.
   * **The DOM write** is skipped when the cursor is absent or hidden; the retained cell is
   * {@link Terminal.track}'s and is kept either way. Touches the DOM (a layout read via `getGeometry`
   * and two style writes) only when the cursor moved — a cache keyed on the coordinate, which cannot
   * see a cell-size change; {@link Terminal.syncTextareaAnchor} is the path that can. */
  private positionTextarea(frame: DecodedFrame): void {
    if (frame.cursorRow === undefined || frame.cursorVisible === false) return;
    // Built from the FRAME, not read back from `cursorAnchor`, so this has no ordering condition
    // against `track` (docs/map/invariant/composition-is-browser-owned-state.md).
    const cursor = { col: frame.cursorCol ?? 0, row: frame.cursorRow };
    this.applyTextareaAnchor(textareaMove(cursor, this.textareaCell, false, this.composition?.composing ?? false));
  }

  /**
   * Re-anchor the textarea at the retained cursor cell, re-reading the geometry. Runs at the moments
   * something reads the anchor — composition start and focus — because the coordinate cache cannot
   * see a cell-size change. `composing` outranks `force`: a composition freezes the anchor for this
   * caller too
   * ([`docs/map/invariant/composition-is-browser-owned-state.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/invariant/composition-is-browser-owned-state.md)).
   * Why these moments:
   * [`docs/map/invariant/cell-size-is-derived-state.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/invariant/cell-size-is-derived-state.md);
   * the measured cost of a stale anchor at focus:
   * [`docs/agents/reference-facts.md`](https://github.com/kihyun1998/justerm/blob/master/docs/agents/reference-facts.md#the-ime-anchor-nobody-caches-it-and-xterm-shares-our-staleness-631-verified-2026-07-30-637-adjudicated-2026-07-30-649-measured-2026-07-31).
   */
  private syncTextareaAnchor(): void {
    this.applyTextareaAnchor(textareaMove(this.cursorAnchor, this.textareaCell, true, this.composition?.composing ?? false));
  }

  /** Write a decided move to the DOM. Both callers funnel here so the cache and the two style
   * writes cannot drift apart; `undefined` is a decided no-op and leaves the cache alone. */
  private applyTextareaAnchor(move: ReturnType<typeof textareaMove>): void {
    if (!move) return;
    this.textareaCell = move.key;
    this.writeTextareaAnchor(move.col, move.row);
  }

  /** Draw the in-progress composition and re-aim the IME anchor at its end. `text` is
   * `compositionupdate.data` — the OS's own preedit, not the textarea's value (ADR-0028 D3).
   * Unchanged data is dropped. The re-aim is the voluntary writer, so it bypasses the freeze the
   * involuntary ones go through: [ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md) D4. */
  private showPreedit(text: string): void {
    const intent = preeditIntent(text, this.preeditText, this.preeditOrigin);
    this.preeditText = text;
    if (!intent) return;
    // The ORIGIN is latched at `compositionstart`, never re-read here
    // (docs/map/invariant/composition-is-browser-owned-state.md).
    this.paintPreedit(intent.codepoints, intent.origin);
  }

  /**
   * Draw the run at the viewport row its origin cell is **shown at**, or not at all. The origin is a
   * GRID row; at offset `d` a grid row `r` is on screen only while `r < rows - d`, and is shown at
   * viewport row `r + d`. Off the bottom it clears what is drawn and waits for
   * {@link Terminal.repaintPreedit}. Why:
   * [`docs/map/invariant/composition-is-browser-owned-state.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/invariant/composition-is-browser-owned-state.md).
   */
  private paintPreedit(codepoints: Uint32Array, at: TextareaAnchor): void {
    // `rows` is 0 until the first frame; unreachable, since `preeditIntent` declines without an
    // origin (docs/map/territory/input-encoding.md).
    const row = at.row + this.frameOffset;
    if (row >= this.rows) {
      // Clear what is drawn, if anything. `preeditEnd` is left as is.
      if (this.preeditPaintedRow !== undefined) {
        this.renderer.setPreedit?.(at.col, this.preeditPaintedRow, EMPTY_PREEDIT);
        this.preeditPaintedRow = undefined;
      }
      return;
    }
    const caretCol = this.renderer.setPreedit?.(at.col, row, codepoints);
    // A preedit-blind renderer: nothing drawn, nothing to aim at (docs/map/territory/input-encoding.md).
    if (caretCol === undefined) return;
    this.preeditPaintedRow = codepoints.length > 0 ? row : undefined;
    // Keep a non-empty run's end for the next composition to latch from (#911) — as a GRID row, like
    // the origin it will become. Skipped for the clearing call, whose caret column is the origin.
    if (codepoints.length > 0) this.preeditEnd = { col: caretCol, row: at.row };
    this.writeTextareaAnchor(caretCol, row);
  }

  /**
   * Give the renderer the suggestion at {@link suggestionCell}, or clear it. Sends only when the cell
   * changed or something drawn has to go, unless `force`; returns whether it sent.
   */
  private paintSuggestion(force: boolean): boolean {
    const want = suggestionCell(
      this.cursorAnchor,
      this.frameOffset,
      this.rows,
      this.suggestionText,
      this.composition?.composing ?? false,
    );
    const painted = this.suggestionPainted;
    if (!want) {
      this.suggestionPainted = undefined;
      if (!painted) return false;
      this.renderer.setSuggestion?.(painted.col, painted.row, EMPTY_PREEDIT, 0, false);
      return true;
    }
    if (!force && painted && painted.col === want.col && painted.row === want.row) return false;
    const codepoints = Uint32Array.from(this.suggestionText, (c) => c.codePointAt(0) ?? 0);
    this.renderer.setSuggestion?.(want.col, want.row, codepoints, this.suggestionColor, this.suggestionDim);
    this.suggestionPainted = want;
    return true;
  }

  /**
   * Re-assert the open composition's run against the frame just applied — [ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md) D5's
   * every-frame rule, applied to the run. Compares the row it would paint with the one it did, so a
   * composition over a still view does nothing.
   */
  private repaintPreedit(): void {
    const at = this.preeditOrigin;
    if (!at || this.preeditText.length === 0) return;
    const want = at.row + this.frameOffset;
    if ((want < this.rows ? want : undefined) === this.preeditPaintedRow) return;
    this.paintPreedit(Uint32Array.from(this.preeditText, (c) => c.codePointAt(0) ?? 0), at);
  }

  /** Put the textarea on a cell. The write itself, with no cache and no decision — see
   * {@link textareaMove} for the decision the *involuntary* writers go through. The preedit writer
   * calls this directly and leaves {@link Terminal.textareaCell} alone: [ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md) D4. */
  private writeTextareaAnchor(col: number, row: number): void {
    const ta = this.textarea;
    const getGeometry = this.options?.getGeometry;
    if (!ta || !getGeometry) return;
    // An unmeasured box has no anchor to write; the last one stays
    // (docs/map/invariant/an-absent-box-measures-as-zero.md, #819).
    const g = getGeometry();
    if (!g) return;
    ta.style.left = `${col * g.cellWidth}px`;
    ta.style.top = `${row * g.cellHeight}px`;
  }

  /** Route a wheel notch through the shared accumulator, then dispatch: a
   * wheel-button report to the app, cursor keys on the alt screen, or a local
   * scroll request. `none` (sub-line/zero) leaves the event for native scroll, except a
   * LINE/PAGE notch carried below a whole line, which is consumed. */
  private onWheel(e: WheelEvent, o: TerminalOptions): void {
    // Attached only with the DOM group, so these are present; narrow for the types.
    const getGeometry = o.getGeometry;
    const input = o.input;
    if (!getGeometry || !input) return;
    // Read once, and refuse an unmeasured box (#819, docs/map/invariant/an-absent-box-measures-as-zero.md).
    const geom = getGeometry();
    if (!geom) return;
    // getGeometry's cellHeight is CSS px, matching pixel-mode deltaY; dpr 1 keeps
    // the scroller's `cellHeight / dpr` at CSS-px-per-cell.
    const lines = this.scroller.consumeWheelEvent(e, {
      cellHeight: geom.cellHeight,
      dpr: 1,
      rows: this.rows,
    });
    const action = routeWheel(this.mask, lines, this.altScreen, this.displayOffset, this.scrollbackLen);
    if (action.kind === "none") {
      // A LINE or PAGE notch carried below a whole line is consumed; a sub-line PIXEL delta, shift
      // and a non-finite delta are left to native scroll (#908).
      const carried = e.deltaMode !== 0 /* DOM_DELTA_PIXEL */ && lines === 0 && e.deltaY !== 0 && !e.shiftKey;
      if (carried && Number.isFinite(e.deltaY)) e.preventDefault();
      return;
    }
    e.preventDefault();
    switch (action.kind) {
      case "app":
        // Direction from the accumulated lines (not raw deltaY) — coords from the event.
        input.send({ kind: "mouse", event: wheelMouseFromDom(e, lines, geom) });
        return;
      case "altKeys": {
        // A cursor key; DECCKM is core's `encode_key` job — the web only picks the direction.
        const key: NamedKey = action.direction === "up" ? "up" : "down";
        input.send({ kind: "key", event: { key: { type: key }, mods: 0, action: "press" } });
        return;
      }
      case "scroll":
        if (!o.onScroll) return;
        // Optimistic, so a burst of notches composes ahead of the echo; `track` reconciles.
        this.displayOffset = action.displayOffset;
        o.onScroll(action.displayOffset);
        return;
    }
  }

  /**
   * **End of life** for this widget: stop consuming frames, detach DOM listeners, and dispose the
   * renderer it was handed — last, after the widget has stopped feeding it. Safe to call more than
   * once; the renderer is disposed exactly once. After this the widget cannot be mounted again —
   * see {@link Terminal.mount} and
   * [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md).
   */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.unsubscribe?.();
    this.unsubscribe = undefined;
    this.eventUnsub?.();
    this.eventUnsub = undefined;
    // Unsubscribing is not enough: an in-flight clipboard read is already past the subscription
    // (docs/map/territory/events-and-replies.md).
    this.clipboardController?.dispose();
    this.clipboardController = undefined;
    for (const off of this.detach) off();
    this.detach = [];
    this.links?.dispose();
    this.links = undefined;
    this.router = undefined;
    const element = this.options?.element;
    if (element && this.cursorBeforeLink !== undefined) element.style.cursor = this.cursorBeforeLink;
    this.cursorBeforeLink = undefined;
    this.textarea?.remove();
    this.textarea = undefined;
    this.composition = undefined;
    // Optional on the port: a renderer with nothing of its own to stop omits it.
    this.renderer.dispose?.();
  }
}

/**
 * The attribute on a mounted {@link Terminal}'s input element — the hidden `<textarea>` that
 * receives keys, IME and paste. Its value is always empty; its presence is the identity.
 *
 * **A contract, so a host can depend on it.** A host asking *"is the keyboard in a text field?"*
 * reads a focused `<textarea>` as a form field; `el.hasAttribute(INPUT_ATTRIBUTE)` tells it this one
 * is a terminal's input, and `element.querySelector("[data-justerm-input]")` finds the one a widget
 * mounted inside the {@link TerminalOptions.element} it was given. The element exists only while a widget
 * mounted with that DOM group is alive: before `mount`, and after `dispose`, there is none.
 *
 * Do not identify it by `aria-label` (accessible text, not an identity) or by its position in the
 * DOM (not promised).
 */
export const INPUT_ATTRIBUTE = "data-justerm-input";

/** Whether a pointer event landed on a scrollbar inside the element rather than on the grid. */
function onScrollbar(e: MouseEvent): boolean {
  return e.target instanceof Element && e.target.closest(`[${SCROLLBAR_ATTRIBUTE}]`) !== null;
}

/** How often a held local drag is asked to auto-scroll past an edge, in ms. */
const SELECTION_TICK_MS = 50;

/** The hidden `<textarea>` input proxy (#116): positioned over the cursor so the IME candidate
 * window appears there, invisible and click-through (`pointer-events: none`) so the canvas owns the
 * pointer, and focused programmatically. A labelled accessible input, not `aria-hidden` (#248) —
 * why: `docs/map/territory/accessibility.md`. */
function makeHiddenTextarea(): HTMLTextAreaElement {
  const ta = document.createElement("textarea");
  ta.setAttribute(INPUT_ATTRIBUTE, "");
  ta.setAttribute("aria-label", "Terminal input");
  ta.setAttribute("aria-multiline", "false");
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
