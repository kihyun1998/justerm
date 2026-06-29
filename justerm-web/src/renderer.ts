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
}
