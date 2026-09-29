import { MouseEvents, type Intent, type Key } from "./input";

/** The wheel-event fields the scroller reads (a DOM `WheelEvent` satisfies it). */
export interface WheelLike {
  deltaY: number;
  /** `0` = DOM_DELTA_PIXEL, `1` = DOM_DELTA_LINE, `2` = DOM_DELTA_PAGE. */
  deltaMode: number;
  shiftKey?: boolean;
  altKey?: boolean;
  ctrlKey?: boolean;
}

/** Dynamic context for a wheel event: cell metrics + current viewport rows. */
export interface WheelContext {
  cellHeight: number;
  dpr: number;
  rows: number;
}

export interface ScrollOptions {
  /** Lines per wheel notch multiplier (xterm `scrollSensitivity`, default 1). May be fractional: the
   * scroller still emits whole lines, carrying the remainder into the next notch. */
  scrollSensitivity?: number;
  /** Extra multiplier when a modifier is held (xterm default 5). */
  fastScrollSensitivity?: number;
}

/**
 * Turns wheel events into a scrollback line delta, after xterm.js's
 * `MouseService._consumeWheelEvent`. Stateful: every mode accumulates sub-line
 * remainders across calls and emits whole lines only — where xterm.js returns
 * LINE and PAGE amounts unrounded (#908).
 */
/** `WheelEvent.deltaMode` values. */
const DOM_DELTA_PIXEL = 0;
const DOM_DELTA_PAGE = 2;

export class WheelScroller {
  private scrollSensitivity: number;
  private fastScrollSensitivity: number;
  /** Sub-line remainder carried between wheel events, whatever their `deltaMode`. */
  private wheelPartialScroll = 0;
  /** The `deltaMode` the remainder was accumulated in. */
  private lastDeltaMode: number | undefined;

  constructor(opts: ScrollOptions = {}) {
    this.scrollSensitivity = opts.scrollSensitivity ?? 1;
    this.fastScrollSensitivity = opts.fastScrollSensitivity ?? 5;
  }

  /** Change the sensitivities from the next event on. A field `opts` leaves out keeps its
   * current value, and the carried sub-line remainder is kept. */
  setOptions(opts: ScrollOptions): void {
    if (opts.scrollSensitivity !== undefined) this.scrollSensitivity = opts.scrollSensitivity;
    if (opts.fastScrollSensitivity !== undefined) {
      this.fastScrollSensitivity = opts.fastScrollSensitivity;
    }
  }

  /** Whole lines to scroll (sign = direction, positive = down/newer); `0` = none. */
  consumeWheelEvent(ev: WheelLike, ctx: WheelContext): number {
    // Horizontal (shift) and zero scrolls do nothing — xterm bails first.
    if (ev.deltaY === 0 || ev.shiftKey) {
      return 0;
    }
    // A held Alt/Ctrl fast-scrolls (xterm `_applyScrollModifier`). Shift is in xterm's
    // condition too, but it already bailed above — so the reachable trigger is Alt/Ctrl.
    const fast = ev.altKey || ev.ctrlKey;
    let amount = ev.deltaY * this.scrollSensitivity * (fast ? this.fastScrollSensitivity : 1);

    if (ev.deltaMode === DOM_DELTA_PIXEL) {
      amount /= ctx.cellHeight / ctx.dpr;
      // A small delta is a trackpad swipe — damp it so it doesn't fly.
      if (Math.abs(ev.deltaY) < 50) {
        amount *= 0.3;
      }
    } else if (ev.deltaMode === DOM_DELTA_PAGE) {
      amount *= ctx.rows;
    }
    // An unmeasured cell (`cellHeight` 0), a non-finite `deltaY` or a `rows` that is
    // not a number makes this non-finite, and the accumulator below would *keep* it:
    // `Infinity % 1` is `NaN`, so every later notch is `NaN` too — including after the
    // geometry recovers, since nothing but `reset()` clears it. Measured in a real
    // browser, that killed the wheel outright rather than mis-scrolling it (#675).
    // Bail before the accumulator, so the instance stays usable. Deliberately NOT
    // hoisted into one check on `ctx` at entry: LINE mode never divides by the cell,
    // so refusing the whole context because `cellHeight` is 0 would break a scroll
    // that works today (pinned in the tests).
    if (!Number.isFinite(amount)) return 0;
    // Every mode emits only whole lines and carries the fraction to the next event,
    // because the count becomes a display offset for the consumer's scroll (#908).
    // Toward zero, so a scroll the other way first cancels what is pending; `+ 0`
    // turns a `-0` into `0`. A change of `deltaMode` is a change of device (a trackpad and
    // a mouse wheel) and starts from zero, so neither shortens the other's notch.
    if (ev.deltaMode !== this.lastDeltaMode) {
      this.lastDeltaMode = ev.deltaMode;
      this.wheelPartialScroll = 0;
    }
    this.wheelPartialScroll += amount;
    const lines = Math.trunc(this.wheelPartialScroll) + 0;
    this.wheelPartialScroll -= lines;
    return lines;
  }

  /** Drop the carried remainder — call on a buffer switch (alt-screen). */
  reset(): void {
    this.wheelPartialScroll = 0;
  }
}

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

/**
 * DOM `KeyboardEvent.key` values that name a modifier. `keyOf` maps every key it does
 * not recognise to a `char`, so a bare modifier press arrives as `Char("Shift")` and is
 * indistinguishable from typing without this list.
 */
const MODIFIER_KEYS: ReadonlySet<string> = new Set([
  "Alt",
  "AltGraph",
  "CapsLock",
  "Control",
  "Fn",
  "FnLock",
  "Hyper",
  "Meta",
  "NumLock",
  "ScrollLock",
  "Shift",
  "Super",
  "Symbol",
  "SymbolLock",
]);

/**
 * What the input path saw, for the scroll-on-user-input decision: an {@link Intent} on its
 * way to the application, or `imeKey` — a keydown the IME gate swallowed, which produces no
 * intent at all. `imeKey` carries its DOM `KeyboardEvent.key` because the gate swallows bare
 * modifiers too, and they are not input on either path.
 */
export type InputScrollSignal = Intent | { kind: "imeKey"; key: string };

/** A bare modifier press. `keyOf` maps it to a `char` carrying the DOM key name. */
function isBareModifier(key: Key): boolean {
  return key.type === "char" && MODIFIER_KEYS.has(key.char);
}

/**
 * Whether user input should bring the viewport back to the bottom.
 *
 * Counts: a key, committed IME text, a paste, and a keydown the IME gate swallowed — a bare
 * modifier on either of the two key paths excepted. Does not: focus and mouse intents. A key
 * vetoed by `TerminalOptions.beforeKey` never becomes an {@link Intent}, so it cannot reach
 * here as one.
 *
 * `displayOffset` is the only state this needs — `0` already means the live edge, on the
 * alt screen included, so there is no screen to ask about. A non-finite offset snaps:
 * unlike {@link wheelScrollTarget}, the requested offset is the constant `0` rather than
 * something computed from the argument.
 */
export function scrollsToBottomOnInput(signal: InputScrollSignal, displayOffset: number): boolean {
  return isUserInput(signal) && displayOffset !== 0;
}

/**
 * Whether this signal is the user providing input at all — the question the snap and the
 * selection drop share, and **all** they share. Only the snap asks where the view is; a selection
 * is dropped wherever it was, which is what both references do (xterm.js fires `onUserInput`
 * outside its `scrollOnUserInput` guard; alacritty's `on_terminal_input_start` clears before it
 * tests `display_offset`). Splitting them is not a refactor — bundling the offset guard would
 * leave a selection alive exactly when the user is already at the bottom, which is most of the time.
 */
export function isUserInput(signal: InputScrollSignal): boolean {
  switch (signal.kind) {
    case "key":
      return !isBareModifier(signal.event.key);
    case "imeKey":
      return !MODIFIER_KEYS.has(signal.key);
    case "text":
    case "paste":
      return true;
    default:
      return false;
  }
}
