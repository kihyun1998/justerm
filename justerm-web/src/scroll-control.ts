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
 * Turns wheel events into a scrollback line delta, mirroring xterm v6's
 * `CoreMouseService.consumeWheelEvent`. Stateful: every mode accumulates sub-line
 * remainders across calls and emits whole lines only.
 */
/** `WheelEvent.deltaMode` values. */
const DOM_DELTA_PIXEL = 0;
const DOM_DELTA_PAGE = 2;

export class WheelScroller {
  private readonly scrollSensitivity: number;
  private readonly fastScrollSensitivity: number;
  /** Sub-line remainder carried between wheel events, whatever their `deltaMode`. */
  private wheelPartialScroll = 0;

  constructor(opts: ScrollOptions = {}) {
    this.scrollSensitivity = opts.scrollSensitivity ?? 1;
    this.fastScrollSensitivity = opts.fastScrollSensitivity ?? 5;
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
    // turns a `-0` into `0`.
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
