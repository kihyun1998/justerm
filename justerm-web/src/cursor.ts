/** The cursor blink period, in ms (xterm `BLINK_INTERVAL`). */
export const BLINK_INTERVAL = 600;

/**
 * Cursor blink state — a web-side policy (the frame only reports the blink
 * *mode*). Time is injected via `isVisible(now)` so it's testable without real
 * timers; the integration drives it from a `setInterval`/rAF loop.
 */
export class CursorBlink {
  private lastRestart = 0;
  private focused = true;
  private reducedMotion = false;

  /** Whether the cursor is shown at time `now` (ms). */
  isVisible(now: number): boolean {
    // Reduced motion or unfocused = solid (no blink).
    if (this.reducedMotion || !this.focused) {
      return true;
    }
    return Math.floor((now - this.lastRestart) / BLINK_INTERVAL) % 2 === 0;
  }

  /** Show the cursor now and reset the blink phase (call on typing/cursor move). */
  restart(now: number): void {
    this.lastRestart = now;
  }

  /** Focus gates blinking — unfocused terminals show a solid cursor. */
  setFocused(focused: boolean): void {
    this.focused = focused;
  }

  /** Honour `prefers-reduced-motion` (#119): when set, the cursor never blinks.
   * The integration reads the media query and forwards changes here. */
  setReducedMotion(reduced: boolean): void {
    this.reducedMotion = reduced;
  }
}
