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
  // `Math.max(0, Math.min(len, NaN))` is `NaN` — the same propagation `clampTo`
  // had at the pointer seam (#672), here at the scroll seam. A non-finite result
  // is "no request", not a request for a nonsense offset: this function is
  // exported, so it owes its own totality rather than trusting its one in-repo
  // caller, and any of the three arguments can arrive poisoned (#675).
  //
  // Checked on the **inputs**, and a result check is not a substitute for it —
  // the clamp *rescues* an infinite request into a finite, wrong one:
  // `Math.max(0, Math.min(100, 10 - Infinity))` is `0`, a silent jump to the
  // live edge. Only `NaN` survives to the output, so guarding there would fix
  // half the cases and read as if it had fixed all of them.
  if (!Number.isFinite(lines) || !Number.isFinite(displayOffset) || !Number.isFinite(scrollbackLen)) {
    return null;
  }
  return Math.max(0, Math.min(scrollbackLen, displayOffset - lines));
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
  // `NaN === 0` is false, so a non-finite count reaches every branch below. The
  // app branch is the one that fails *quietly*: `direction` comes from
  // `lines < 0`, which is false for `NaN`, so a poisoned scroller would report a
  // fabricated `down` to the application instead of reporting nothing (#675).
  if (lines === 0 || !Number.isFinite(lines)) return { kind: "none" };
  const direction = lines < 0 ? "up" : "down";
  if (wheelGoesToApp(mouseWantedEvents)) return { kind: "app", direction };
  if (altScreen) return { kind: "altKeys", direction };
  // No `!`: the target really can be null now (a poisoned `displayOffset` coming
  // back from a frame), and asserting it away is how a non-finite offset reached
  // the consumer's `onScroll` in the first place.
  const target = wheelScrollTarget(lines, displayOffset, scrollbackLen);
  if (target === null) return { kind: "none" };
  return { kind: "scroll", displayOffset: target };
}

/** Where the hidden textarea is anchored, in cells — the cursor cell a frame reported. */
export interface TextareaAnchor {
  col: number;
  row: number;
}

/**
 * Whether the hidden textarea must move, and the cache key to remember (#631).
 *
 * The key is the cursor's **cell coordinate**, but the anchor is computed from the **geometry** —
 * so a cell-size change (`setFontSize`, `setFontFamily`, `setLetterSpacing`, `setLineHeight`, and
 * `setDevicePixelRatio` once it is wired) moves what the anchor should be while leaving the
 * coordinate identical. A coordinate-keyed cache structurally cannot express "the geometry moved",
 * which is why `force` exists: the callers that sit at a moment something actually *reads* the
 * anchor override the cache instead of trying to keep it fresh at all times.
 *
 * Why not simply drop the cache and re-read every frame, which is what all three references do
 * (xterm.js `_syncTextArea`, ghostty `imePoint`, alacritty `update_ime_position` — none of them
 * caches)? Because their cell is a **stored field** they push to (`dimensions.css.cell`,
 * `size.cell`, `size_info`), while ours arrives through a consumer-supplied
 * {@link TerminalOptions.getGeometry} callback whose cost we do not control — the demo's and the
 * README's both do a `getBoundingClientRect()`. Per-frame is cheap for them and a forced layout
 * read per output flush for us. Valid only as long as `getGeometry` stays a pull-based consumer
 * callback; if the widget ever holds a pushed cell, prefer the reference's no-cache shape.
 *
 * `composing` suppresses **every** path, forced or not (#637 established the rule, #649 closed the
 * `force` exemption). While a composition is open the OS re-reads the anchor to keep its candidate
 * window placed — measured with a real Korean IME: the Hanja window follows the anchor down as
 * unsolicited output moves the cursor, walking away from the text being composed.
 *
 * `force` originally won over `composing` so that #631's `compositionstart` re-sync could not be
 * gated by its own guard. It never needed that: `onStart` re-anchors *before* telling the controller,
 * so it sees `composing === false` either way. What the exemption actually bought was a second
 * entrance to the same harm — `element` mousedown → `onDown` → `Terminal.focus()` → a forced re-sync
 * onto the superseded cursor cell, reachable through the public `focus()` from any consumer, not only
 * a pointer (#649). So `force` now means one thing and one thing only: *override the coordinate
 * cache*. It says nothing about the composition rule.
 *
 * **`composing` is `isComposing`, never `active`.** `active` stays true through the deferred commit
 * read, and a continuous-CJK `compositionstart` lands inside exactly that window — so keying this on
 * `active` would swallow #631's re-sync in the ordinary Korean/Japanese typing pattern, while looking
 * equivalent at the call site. `composition.test.ts` pins the two diverging there.
 *
 * Suppression returns `undefined`, which the caller treats as a decided no-op that leaves the cache
 * alone — so the move is not recorded as applied and the anchor catches up on the first frame after
 * the composition ends, rather than waiting for the cursor to move again.
 *
 * **Freezing is this codebase's form of the rule, not the rule itself.** All three references converge
 * on *the anchor tracks where the user's composition is, never where the output cursor went* — and two
 * of the three do that by **actively re-aiming during the composition**, not by suppressing:
 *
 * - ghostty folds the preedit's width into the IME rect it pushes on every key event
 *   (`src/Surface.zig:2108`, used at `:2151`) — no gate at all
 * - alacritty picks the point *from* the preedit when there is one and from the cursor when there is
 *   not (`alacritty/src/display/mod.rs:1136-1142`, `:1215`) — also no gate
 * - xterm.js is the only one that suppresses, and only the **involuntary** writer: `_syncTextArea`
 *   bails while composing (`browser/CoreBrowserTerminal.ts:338`, gating all three of its callers) while
 *   `CompositionHelper.updateCompositionElements` deliberately rewrites `left`/`top` every render
 *   (`browser/input/CompositionHelper.ts:273-274`)
 *
 * **justerm-web now re-aims too, and this guard is unchanged — which is the part worth reading.**
 * Until #249 there was no preedit view here, so *"where the composition is"* collapsed to *"where it
 * started"* and freezing was the only available expression of the shared rule. #249 supplied the
 * missing representation, and the prediction recorded here was that this guard *"is what has to
 * give"*. It did not, and the reason is ADR-0028 D4: the writer that knows where the composition is
 * does not pass through the guard that exists for the writers that do not. The preedit's re-aim goes
 * straight to {@link Terminal.writeTextareaAnchor}, exactly as xterm's `updateCompositionElements`
 * never goes through `_syncTextArea`. So this stays a rule about the **involuntary** writers — the
 * frame stream (#637) and the focus path (#649) — which is what both of them actually measured.
 */
export function textareaMove(
  cursor: TextareaAnchor | undefined,
  lastKey: string,
  force: boolean,
  composing: boolean,
): { col: number; row: number; key: string } | undefined {
  if (!cursor) return undefined;
  const key = `${cursor.col},${cursor.row}`;
  if (composing || (!force && key === lastKey)) return undefined;
  return { col: cursor.col, row: cursor.row, key };
}

/**
 * Wrap an {@link InputSink} so the renderer's local cursor/selection state tracks
 * input before each intent forwards: a KEY intent restarts the cursor blink (the
 * S5 #107 deferral — the caret must show at once on a keystroke, before the echo
 * frame), a FOCUS intent drives the renderer's focus state (a blurred terminal
 * stops blinking + shows the inactive selection tint). Both renderer hooks are
 * optional; a cursorless renderer is left untouched. Other intents pass through.
 */
/**
 * What a `compositionupdate` should do to the drawn run: nothing, or push these codepoints (#249).
 *
 * Pure so the two decisions are testable at all — the widget half that acts on them needs a DOM and
 * the unit suite runs in `environment: "node"`, which is the blind spot #649 measured.
 *
 * - **Unchanged text is dropped.** A real IME emits one settling `compositionupdate` per syllable
 *   carrying the data it already sent (measured on a Windows Korean IME), and each one would
 *   otherwise cost a full re-pack.
 * - **No origin, no push.** The origin is latched at `compositionstart`; a composition that somehow
 *   runs before any frame has reported a cursor has nowhere to draw, and guessing a cell is worse
 *   than drawing nothing.
 *
 * The text itself is split by **code point**, not by UTF-16 unit: a preedit can carry astral
 * scalars, and `Array.from`'s iterator is what makes `"\u{1F600}"` one cell rather than two halves
 * of a surrogate pair.
 */
export function preeditIntent(
  text: string,
  lastText: string,
  origin: TextareaAnchor | undefined,
): { codepoints: Uint32Array; origin: TextareaAnchor } | undefined {
  if (text === lastText || !origin) return undefined;
  return { codepoints: Uint32Array.from(text, (c) => c.codePointAt(0) ?? 0), origin };
}

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
  /** The element input listeners attach to (the canvas or a wrapper). Provide it WITH `input` +
   * `getGeometry` to wire keyboard/IME/wheel; omit the group for an output-only widget (e.g. one
   * that only wants {@link events}).
   *
   * **It does not need to be focusable, and the widget does not make it so** (#649 — this used to
   * claim otherwise, and no `tabIndex` was ever written). The real keyboard/IME target is a hidden
   * textarea the widget mounts inside it, and a pointer-down here focuses *that* through
   * {@link Terminal.focus}. A canvas being unfocusable is therefore not a problem to solve.
   *
   * **If you do make it (or a child) focusable, cancel the pointer-down's default action.** The
   * browser's focusing steps run after our `mousedown` handler, so an un-cancelled default moves
   * focus to your element and blurs the textarea — typing and IME both stop. `preventDefault()` on
   * `mousedown` is the fix, and it is what the demo and xterm.js both do
   * (`browser/services/MouseService.ts:224-226` — `preventDefault()` then focus). */
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
 * source-agnostic (frame mode / in-wasm) and renderer-agnostic (the real
 * justerm-renderer / a fake), which is what makes it testable without a backend or a canvas.
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
  /** The cursor cell the latest frame reported, retained so the anchor can be re-synced at a
   * point of use without waiting for a frame (#631). Not cleared when the cursor hides: an
   * application can hide the caret and the user can still open an IME, and re-anchoring at the
   * last known cell beats leaving the anchor wherever the geometry used to put it. */
  private cursorAnchor: TextareaAnchor | undefined;
  /** The composition text currently drawn (#249). Held to drop the settling `compositionupdate`
   * a real IME emits once per syllable with unchanged data — see {@link Terminal.showPreedit}. */
  private preeditText = "";
  /** Where the open composition started (#249). Latched at `compositionstart` and held for the
   * composition's life: the frame stream keeps reassigning {@link Terminal.cursorAnchor}, and a
   * composition that re-read it would walk to wherever the application's output went. */
  private preeditOrigin: TextareaAnchor | undefined;
  /** Latched by {@link Terminal.dispose} (#606): the widget's lifecycle is one-shot, so this both
   * keeps the renderer from being disposed twice and refuses a re-mount. */
  private disposed = false;

  constructor(
    private readonly source: FrameSource,
    private readonly renderer: Renderer,
    private readonly options?: TerminalOptions,
  ) {}

  /** Focus the keyboard/IME input target (the hidden textarea, #116). Consumers
   * that move focus away — an accessible-view overlay, a control button — call this
   * to return it, since the real input target is the textarea, not the canvas.
   *
   * Re-anchors first (#631): focusing is a read moment for the element's position — the browser's
   * focus steps scroll the nearest scrollable ancestor to it — and the cell may have moved since
   * the cursor last did. */
  focus(): void {
    this.syncTextareaAnchor();
    this.textarea?.focus();
  }

  /**
   * Begin consuming frames from the source; wire the DOM if options were given.
   *
   * **Not callable after {@link Terminal.dispose}** (#606). The alternative — a widget that can be
   * unmounted and mounted again — is a larger contract than this one currently keeps, and pretending
   * to keep it is worse than refusing: `textareaCell` and `cursorAnchor` survive disposal, so a
   * remounted widget would park the IME candidate window at the *previous* mount's anchor — since
   * #631 only until the next focus or composition start re-syncs it, which shortens the window
   * without closing it — and a re-mounted
   * renderer would have lost its `prefers-reduced-motion` listener for good (its only registration is
   * in a private constructor). Throwing names the contract at the moment it is broken; the reference
   * makes the same call structurally — xterm.js's disposable store latches on dispose and `open()`
   * has no re-entry path. Build a new `Terminal` (and a new renderer) instead.
   */
  mount(): void {
    if (this.disposed) {
      throw new Error(
        "justerm-web: this Terminal was disposed — build a new one rather than re-mounting",
      );
    }
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
    // The caret also stops blinking for the duration (#592) — composition is a browser fact the
    // engine never sees, so this is the only place that can tell the renderer about it.
    const onStart = (): void => {
      // Re-anchor BEFORE the controller is told a composition began (#631). The order is the
      // contract, not a coincidence: this is the one moment the OS reads the anchor to place its
      // candidate window, and any future "don't move the textarea mid-composition" guard (xterm
      // has one) would key on the controller's `active` flag — so flipping these two lines would
      // silently disable the re-sync. xterm.js orders its own `_syncTextArea()` before
      // `compositionHelper.compositionstart()` for exactly that reason.
      this.syncTextareaAnchor();
      // Latch the origin here, after the re-sync above has made the cell current (#631) and while
      // the guard still reads `composing === false`. Everything the composition draws hangs off it.
      this.preeditOrigin = this.cursorAnchor;
      composition.compositionStart();
      this.renderer.setComposing?.(true);
    };
    const onUpdate = (e: CompositionEvent): void => {
      composition.compositionUpdate(e.data);
      this.showPreedit(e.data);
    };
    const onEnd = (): void => {
      composition.compositionEnd();
      this.renderer.setComposing?.(false);
      // Clear the drawn run BEFORE the commit is anywhere near the grid. Measured (#249): the
      // committed text leaves as an intent one deferred read later, by which time the next
      // composition has already started — so there is no frame to hand the job over to, and
      // waiting for one would leave the last syllable drawn twice.
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

    // Pointer-down focuses the textarea (it's pointer-events:none so the canvas
    // still gets the click for selection) and resets the blink phase.
    const onDown = (): void => {
      this.focus(); // not `ta.focus()` — routes through the anchor re-sync (#631)
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
   * source the input uses; skipped when the cursor is absent/hidden.
   *
   * Only touches the DOM (a layout read via `getGeometry` + two style writes) when the cursor
   * actually moved, not on every output frame. That cache is keyed on the *coordinate*, so it
   * cannot see a cell-size change — {@link Terminal.syncTextareaAnchor} is the path that can. */
  private positionTextarea(frame: DecodedFrame): void {
    if (frame.cursorRow === undefined || frame.cursorVisible === false) return;
    const cursor = { col: frame.cursorCol ?? 0, row: frame.cursorRow };
    this.cursorAnchor = cursor;
    this.applyTextareaAnchor(textareaMove(cursor, this.textareaCell, false, this.composition?.composing ?? false));
  }

  /**
   * Re-anchor the textarea at the retained cursor cell, re-reading the geometry (#631).
   *
   * The cache above holds a *coordinate* while the anchor is derived from the *geometry*, and a
   * `setFontSize` / `setLetterSpacing` / `setLineHeight` with a stationary cursor moves the second
   * without touching the first — so the widget cannot keep the anchor fresh by watching frames.
   * Instead this runs at the moments something reads the anchor: composition start (the OS IME
   * candidate window) and focus (the browser's focus steps scroll the nearest scrollable ancestor
   * to the focused element). One geometry read per IME session and per focus, not per frame.
   *
   * **That focus read is measured, not assumed (#649).** In real Chrome, focusing the textarea from
   * `scrollY: 2000` scrolled the page back to it, and the destination tracked the anchor exactly —
   * a 1020px anchor difference moved the landing point by 1020px. So a stale anchor here does not
   * merely sit wrong, it scrolls the page to the wrong place, by an amount proportional to the
   * cursor's row (a `lineHeight` 1 → 1.6 change moved row 5 by 47px, i.e. ~9px per row).
   *
   * **`composing` outranks `force` (#649).** A composition freezes the anchor for *every* caller,
   * including this one: `Terminal.focus()` is public, so a pointer-down — or a consumer restoring
   * focus after a dialog — used to re-anchor to the superseded cursor cell mid-composition, which is
   * #637's harm through another entrance. #631's `compositionstart` re-sync is unaffected because it
   * runs *before* the controller is told, and therefore sees `composing === false`; that ordering in
   * {@link Terminal.attach}'s `onStart` is the contract this depends on. The predicate is
   * {@link CompositionController.composing}, never `active` — `active` outlives the candidate window
   * by one deferred read, which is exactly the window a continuous-CJK `compositionstart` lands in.
   *
   * xterm.js reaches the same conclusion for the same reason: an option change there never reaches
   * `_syncTextArea` either (`RenderService.handleResize` forwards to the renderer without touching
   * `BufferService`, so `onResize` never fires; and `Terminal.resize` early-returns on an unchanged
   * grid), and its whole mitigation is the same re-sync at `compositionstart`, whose comment names
   * this exact symptom. ghostty goes further and pushes the IME rect on every key event.
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

  /** Draw the in-progress composition and re-aim the IME anchor at its end (#249, ADR-0028).
   *
   * `text` is `compositionupdate.data` — **the OS's own preedit**, not the textarea's value, which
   * measurement showed lags it by exactly one event (`data="하"` while `value="ㅎ"`). That the two
   * disagree is not a defect to reconcile: `data` is what the IME is showing the user, and `value`
   * is what will be committed, and #116 reads the commit from `value` for its own good reasons.
   *
   * Unchanged data is dropped. A real IME emits one settling update per syllable where nothing
   * moved (measured), and each one would otherwise cost a full re-pack.
   *
   * Anchoring here is D4's voluntary writer: it re-aims *because* it knows where the composition
   * is, which is exactly what the frame stream and the focus path do not (#637/#649), so their
   * freeze stays and this bypasses it. Without the re-aim the candidate window would sit at the
   * composition's start while the text grows away from it. */
  private showPreedit(text: string): void {
    const intent = preeditIntent(text, this.preeditText, this.preeditOrigin);
    this.preeditText = text;
    if (!intent) return;
    // The ORIGIN is latched at `compositionstart`, never re-read here. `cursorAnchor` is reassigned
    // by every frame (`positionTextarea`, ahead of the guard), so reading it per update would let an
    // output frame relocate the whole run on the next keystroke — #637's harm through a new
    // entrance, measured: with unsolicited output running, the anchor held at row 5 while the
    // composition was open and then jumped to row 9 on the following keystroke.
    //
    // This is also what makes ADR-0028 D4 true rather than half-true. The preedit writer earns its
    // bypass of `textareaMove` by knowing where the composition is — and it only knows that if it
    // knows both halves: the EXTENT (live, from the run) and the ORIGIN (fixed, from where the user
    // started typing). Re-asking the frame stream for the origin is asking the one source #637
    // established cannot answer it.
    const at = intent.origin;
    const caretCol = this.renderer.setPreedit?.(at.col, at.row, intent.codepoints);
    if (caretCol === undefined) return; // a preedit-blind renderer: nothing drawn, nothing to aim at
    this.writeTextareaAnchor(caretCol, at.row);
  }

  /** Put the textarea on a cell. The write itself, with no cache and no decision — see
   * {@link textareaMove} for the decision the *involuntary* writers go through.
   *
   * The preedit writer (#249) calls this directly, which is ADR-0028 D4: a writer that knows where
   * the composition is does not pass through the guard that exists for writers that do not, and it
   * deliberately does not touch {@link Terminal.textareaCell} — that cache describes where the
   * frame stream last put the anchor, and a composition's own re-aim is not an answer to that
   * question. Same structure as xterm's, where `updateCompositionElements` writes `style.left/top`
   * itself and never goes through `_syncTextArea`. */
  private writeTextareaAnchor(col: number, row: number): void {
    const ta = this.textarea;
    const getGeometry = this.options?.getGeometry;
    if (!ta || !getGeometry) return;
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

  /**
   * **End of life** for this widget: stop consuming frames, detach DOM listeners, and dispose the
   * renderer it was handed (#606). Safe to call more than once; the renderer is disposed exactly
   * once. After this the widget cannot be mounted again — see {@link Terminal.mount}.
   *
   * The renderer is disposed **last**, after the widget has stopped feeding it, so nothing arrives
   * at an already-ended renderer. (xterm.js disposes its addons in reverse registration order for
   * the same reason.)
   */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
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
    // The consumer built this and handed it over; the widget drove it, so the widget ends it.
    // Optional on the port, so a renderer with nothing of its own to stop simply omits it.
    this.renderer.dispose?.();
  }
}

/** The hidden `<textarea>` input proxy (#116). Positioned over the cursor so the
 * IME candidate window appears there, but visually invisible and click-through
 * (`pointer-events: none`) so the canvas owns the pointer; focus is programmatic.
 *
 * It is a LABELED accessible input (xterm's helper textarea), NOT `aria-hidden`
 * (#248): it's programmatically focused to type, and focusing an aria-hidden element
 * is a WCAG 4.1.2 violation (a screen reader lands on it and announces "blank"). The
 * #119 row-tree is the separate review/announce surface — the two coexist as they do
 * in xterm (no `aria-owns`); typed-echo dedup (#119 onKey) keeps output from
 * double-reading what the AT already announced on input. */
function makeHiddenTextarea(): HTMLTextAreaElement {
  const ta = document.createElement("textarea");
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
