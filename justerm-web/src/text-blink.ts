/**
 * The SGR 5 (blink) phase — a *text* blink, not the caret's (#576).
 *
 * The mechanism was already paid for on both sides and died in the middle: core carries `BLINK` on
 * the wire (`cell.rs`, exported as `flags().blink`), the renderer conceals a blinking cell on the
 * off phase (`attrs.rs` `is_concealed`, `frame.rs` collapses it to `BLANK_SLOT`), and the phase
 * itself is deliberately the consumer's (`webgl.rs`: *"the renderer only draws what it is
 * handed"*). Nothing flipped it, so `ESC[5m` text rendered identically to plain text.
 *
 * **It is opt-in, and off by default — that is the reference position, not a hedge.** Of the three
 * references only xterm.js animates blinking text at all, and it ships the interval as an option
 * defaulting to **`0` = disabled** (`common/services/OptionsService.ts:17`
 * `blinkIntervalDuration: 0`, consumed by `browser/renderer/shared/TextBlinkStateManager.ts`
 * @ `699f553`). alacritty has no text blink in its tree at all; ghostty parses SGR 5 and stores
 * `style.flags.blink` (`src/terminal/style.zig:33` @ `e6e26e1`) but its renderer never reads it.
 * So there is **no inheritable default cadence** — the number is a product choice the consumer
 * makes, which is why this exposes an interval rather than a boolean.
 *
 * Deliberately **not** {@link CursorBlink}: the two phases are separate clocks. Sharing one would
 * couple SGR 5 text to `restart()` on every keystroke, which is a caret affordance (the caret stays
 * solid while you type) and has no counterpart for text in any reference. xterm.js keeps two
 * managers for the same reason.
 *
 * Time is injected via `isVisible(now)` so the phase is testable without real timers; the
 * integration drives it from the adapter's rAF loop.
 */
export class TextBlink {
  private intervalMs = 0;
  private reducedMotion = false;

  /** Whether text blink is actually running — an interval was set *and* motion is allowed. */
  get enabled(): boolean {
    return this.intervalMs > 0 && !this.reducedMotion;
  }

  /**
   * Whether SGR 5 text is shown at time `now` (ms). Disabled ⇒ always `true`: a blink that is
   * turned off must leave text **shown**, never stuck in the phase it happened to be in.
   */
  isVisible(now: number): boolean {
    if (!this.enabled) return true;
    return Math.floor(now / this.intervalMs) % 2 === 0;
  }

  /**
   * The half-period in ms — xterm.js's `blinkIntervalDuration`. `0` (the default) disables the
   * blink entirely.
   *
   * A negative, fractional-below-1 or non-finite value is treated as `0` rather than throwing:
   * xterm.js does throw on a negative (`OptionsService.ts:173-178`), but this widget's other
   * policy setters report what they adopted instead of panicking across the seam
   * ({@link CursorBlink.setIdleTimeout}), and internal coherence is the tie-breaker for a
   * consumer-facing API shape.
   */
  setIntervalMs(ms: number): void {
    this.intervalMs = Number.isFinite(ms) && ms >= 1 ? Math.floor(ms) : 0;
  }

  /**
   * Honour `prefers-reduced-motion` (#119): when set, SGR 5 text never blinks.
   *
   * It outranks the application's request and the consumer's interval alike — the precedence
   * settled on #575 and recorded on #583: reduced motion can only ever *subtract* motion, so
   * letting it win can never make steady text blink. Derived rather than ported (xterm.js's `src`
   * has zero `prefers-reduced-motion` hits; alacritty and ghostty are native), and it generalises
   * to any later motion knob on this widget rather than being a per-feature call.
   */
  setReducedMotion(reduced: boolean): void {
    this.reducedMotion = reduced;
  }
}
