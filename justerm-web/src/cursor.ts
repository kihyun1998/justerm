import type { DrawOp } from "./render-core";

/** The cursor blink period, in ms (xterm `BLINK_INTERVAL`). */
export const BLINK_INTERVAL = 600;

/** Cursor shapes, matching the decoder's `cursorShape`. */
export const CursorShape = { Block: 0, Underline: 1, Bar: 2 } as const;

/**
 * Style a cell's {@link DrawOp} as the cursor. beamterm has no cursor primitive
 * and is cell-level, so each shape is a cell transform:
 * - **Block**: invert — cell → cursor colour, glyph → the cell's original
 *   background so it stays legible (penterm's applyCursor).
 * - **Underline**: keep the background, draw in the cursor colour with an
 *   underline (the underline is the cursor; the glyph takes the colour too).
 * - **Bar**: no sub-cell bar exists in beamterm, so it falls back to Block.
 */
export function cursorOp(base: DrawOp, shape: number, cursorColor: number): DrawOp {
  if (shape === CursorShape.Underline) {
    return { ...base, fg: cursorColor, underline: true };
  }
  return { ...base, fg: base.bg, bg: cursorColor };
}

/**
 * Cursor blink state — a web-side policy (the frame only reports the blink
 * *mode*). Time is injected via `isVisible(now)` so it's testable without real
 * timers; the integration drives it from a `setInterval`/rAF loop.
 */
export class CursorBlink {
  private lastRestart = 0;
  private focused = true;

  /** Whether the cursor is shown at time `now` (ms). */
  isVisible(now: number): boolean {
    // Unfocused = solid (no blink).
    if (!this.focused) {
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
}
