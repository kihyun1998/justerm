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
  /** Lines per wheel notch multiplier (xterm `scrollSensitivity`, default 1). */
  scrollSensitivity?: number;
  /** Extra multiplier when a modifier is held (xterm default 5). */
  fastScrollSensitivity?: number;
}

/**
 * Turns wheel events into a scrollback line delta, mirroring xterm v6's
 * `CoreMouseService.consumeWheelEvent`. Stateful: trackpad pixel scrolls
 * accumulate sub-line remainders across calls.
 */
/** `WheelEvent.deltaMode` values. */
const DOM_DELTA_PIXEL = 0;
const DOM_DELTA_PAGE = 2;

export class WheelScroller {
  private readonly scrollSensitivity: number;
  /** Sub-line remainder carried between pixel (trackpad) wheel events. */
  private wheelPartialScroll = 0;

  constructor(opts: ScrollOptions = {}) {
    this.scrollSensitivity = opts.scrollSensitivity ?? 1;
  }

  /** Lines to scroll (sign = direction, positive = down/newer); `0` = none. */
  consumeWheelEvent(ev: WheelLike, ctx: WheelContext): number {
    // Horizontal (shift) and zero scrolls do nothing — xterm bails first.
    if (ev.deltaY === 0 || ev.shiftKey) {
      return 0;
    }
    let amount = ev.deltaY * this.scrollSensitivity;

    if (ev.deltaMode === DOM_DELTA_PIXEL) {
      amount /= ctx.cellHeight / ctx.dpr;
      // A small delta is a trackpad swipe — damp it so it doesn't fly.
      if (Math.abs(ev.deltaY) < 50) {
        amount *= 0.3;
      }
      this.wheelPartialScroll += amount;
      // Emit only whole lines; keep the fractional part for the next event.
      amount = Math.floor(Math.abs(this.wheelPartialScroll)) * (this.wheelPartialScroll > 0 ? 1 : -1);
      this.wheelPartialScroll %= 1;
    } else if (ev.deltaMode === DOM_DELTA_PAGE) {
      amount *= ctx.rows;
    }
    return amount;
  }

  /** Drop the carried remainder — call on a buffer switch (alt-screen). */
  reset(): void {
    this.wheelPartialScroll = 0;
  }
}
