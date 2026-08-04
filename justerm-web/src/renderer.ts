import type { DecodedFrame } from "./types";

/**
 * The renderer port — the small interface the widget drives.
 *
 * The renderer (WASM + WebGL2) sits behind this so the widget's wiring is testable
 * with a fake (no GL context). The real adapter ({@link import("./justerm-renderer").JustermRenderer})
 * wraps the first-party `justerm-renderer` and pushes each frame's cells + overlay + cursor +
 * decorations, letting the renderer composite them in wasm (ADR-0018, #273).
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
  /**
   * An IME composition started (`true`) or ended (`false`) (#592). The caret stays put for the
   * duration — two of the three references do this, and for the same reason: the caret is the
   * anchor a composing user is reading against.
   *
   * Driven from the composition events on the hidden textarea, not from a frame: composition is a
   * browser-side fact the engine never sees. Optional: a renderer with no cursor may omit it.
   */
  setComposing?(composing: boolean): void;
  /**
   * Draw the in-progress composition at `(col, row)`, or clear it with an empty run (#249,
   * ADR-0028). `codepoints` is the preedit **as the OS reports it** — `compositionupdate.data`,
   * never the textarea's value, which lags it by one event (measured).
   *
   * Returns the column the caret and the IME anchor belong at: one past the run's last cell. The
   * widget cannot compute that itself — it has no `wcwidth`, and the run shifts left at the right
   * edge rather than clipping — so this return is the only source for it.
   *
   * Optional, and a renderer that omits it is not broken, only preedit-blind: the composition still
   * commits, the user just cannot see it until it does, which is the state every consumer was in
   * before this existed.
   */
  setPreedit?(col: number, row: number, codepoints: Uint32Array): number;
  /**
   * Release what the renderer runs on its own behalf — its animation loop and any listener it
   * registered (#606). Called by {@link import("./terminal").Terminal.dispose}, **once**.
   *
   * **Why the widget calls this on an object it did not construct.** The consumer builds the
   * renderer and hands it over; the widget then drives it, and the renderer starts clocks *because*
   * it was driven — a rAF blink loop, a `prefers-reduced-motion` listener that redraws. Unsubscribing
   * from the frame source stops frames, not those. So without this the widget can be disposed and
   * its canvas still repaint. Both references for "who ends a handed-over component" answer the same
   * way: xterm.js disposes each consumer-constructed addon from `Terminal.dispose()`
   * (`common/public/AddonManager.ts` `dispose()` → `instance.dispose()`), and this port's sibling,
   * `FrameSource`, already gets the same treatment through the `Unsubscribe` it returns — it simply
   * handed the widget a means, and this one did not.
   *
   * **`Terminal.dispose()` is end of life, not unmount.** After it, the widget refuses to mount
   * again, so this is called once per widget and a renderer is not expected to come back. Reusing
   * one renderer across two Terminals is therefore not supported: the second `dispose()` would end a
   * renderer the first widget still holds.
   *
   * **Implementations must be idempotent** — a consumer that also calls `dispose()` on the renderer
   * it built must not be punished for it. (xterm.js buys this by wrapping the addon's own method
   * behind an `isDisposed` guard; this port asks for it instead of imposing a wrapper.)
   *
   * **What this does NOT promise.** It stops *work*, not *memory*: the concrete
   * {@link import("./justerm-renderer").JustermRenderer} keeps its wasm instance, GL context, glyph
   * atlas and the canvas context-loss listeners its Rust side owns — those are released by the wasm
   * binding's `free()`, which cannot be called while the consumer still holds the object. A renderer
   * that needs to release GPU resources should say so in its own docs.
   *
   * Optional: a renderer with nothing of its own to end (the test fake) may omit it.
   */
  dispose?(): void;
}
