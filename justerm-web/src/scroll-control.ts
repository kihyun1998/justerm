import { isUserInput, MouseEvents, type InputScrollSignal } from "./input";

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
 * Whether a wheel notch reports to the app rather than scrolling scrollback locally: true only
 * when the frame's `mouseWantedEvents` mask has the WHEEL bit (#129). `undefined` (the frame
 * omitted the field) is local. Per category, not "any mouse mode": an X10 app (`?9`, presses
 * only) keeps the wheel local.
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
  // Checked on the inputs, not the result (#675) — docs/map/invariant/pointer-coordinates-are-bounded-by-their-producer.md.
  if (!Number.isFinite(lines) || !Number.isFinite(displayOffset) || !Number.isFinite(scrollbackLen)) {
    return null;
  }
  return Math.max(0, Math.min(scrollbackLen, displayOffset - lines));
}

/**
 * What a wheel notch does, once {@link WheelScroller} has turned it into whole `lines`: `app` (a
 * wheel-button report to an app tracking the wheel), `altKeys` (cursor keys, for the alt buffer,
 * which has no scrollback), `scroll` (local scrollback), or `none` (a sub-line or zero notch).
 */
export type WheelAction =
  | { kind: "app"; direction: "up" | "down" }
  | { kind: "altKeys"; direction: "up" | "down" }
  | { kind: "scroll"; displayOffset: number }
  | { kind: "none" };

/**
 * Decide where a wheel notch goes. A sub-line or zero notch is `none` before any destination is
 * asked. Otherwise a wheel-tracking app wins, even on the alt screen; else the alt buffer takes
 * cursor keys; else local scrollback. A non-finite `lines`, or an offset
 * {@link wheelScrollTarget} refuses, is `none`. Why this order, and why the guards:
 * [`docs/map/territory/viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/viewport.md).
 */
export function routeWheel(
  mouseWantedEvents: number | undefined,
  lines: number,
  altScreen: boolean,
  displayOffset: number,
  scrollbackLen: number,
): WheelAction {
  if (lines === 0 || !Number.isFinite(lines)) return { kind: "none" };
  const direction = lines < 0 ? "up" : "down";
  if (wheelGoesToApp(mouseWantedEvents)) return { kind: "app", direction };
  if (altScreen) return { kind: "altKeys", direction };
  const target = wheelScrollTarget(lines, displayOffset, scrollbackLen);
  if (target === null) return { kind: "none" };
  return { kind: "scroll", displayOffset: target };
}

/**
 * Whether user input should bring the viewport back to the bottom.
 *
 * Counts: a key, committed IME text, a paste, and a keydown the IME gate swallowed — a bare
 * modifier on either of the two key paths excepted. Does not: focus and mouse intents. A key
 * vetoed by `TerminalOptions.beforeKey` never becomes an {@link Intent}, so it cannot reach
 * here as one.
 *
 * `displayOffset` is the only state this reads; a non-finite one snaps. Why:
 * [`docs/map/territory/viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/viewport.md).
 */
export function scrollsToBottomOnInput(signal: InputScrollSignal, displayOffset: number): boolean {
  return isUserInput(signal) && displayOffset !== 0;
}
