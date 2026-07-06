import type { DecodedFrame } from "./types";

/**
 * The renderer port — the small interface the widget drives.
 *
 * beamterm (WASM + WebGL) sits behind this so the widget's wiring is testable
 * with a fake (no GL context). The real adapter wraps `@beamterm/renderer`;
 * S2 (#105) fills `applyFrame` with the cell-by-cell mapping (resolveRgb,
 * flags, wide-char). For S1 it need only accept a frame and present it.
 */
export interface Renderer {
  /** Apply a decoded frame's content to the renderer's back buffer. */
  applyFrame(frame: DecodedFrame): void;
  /** Present the back buffer to the screen. */
  render(): void;
  /**
   * Show the cursor now and reset its blink phase (#107). The widget calls it on
   * a key intent so the caret stays solid while typing instead of blinking off
   * right after a keystroke — the frame-driven cursor MOVE already restarts the
   * blink inside the renderer, but a keystroke arrives before its echo frame.
   * Optional: a renderer with no cursor (the test fake) may omit it.
   */
  restartCursorBlink?(): void;
  /**
   * Set the terminal's focus state (#115). A blurred terminal stops blinking and
   * shows the inactive selection tint (xterm's two selection colours). The widget
   * drives it from focus/blur intents so the caret + selection reflect focus —
   * without this the blink and active tint persist after the page loses focus.
   * Optional: a renderer with no cursor/selection may omit it.
   */
  setFocused?(focused: boolean): void;
}
