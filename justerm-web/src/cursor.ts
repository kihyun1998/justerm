/** The cursor blink period, in ms (xterm `BLINK_INTERVAL`). */
export const BLINK_INTERVAL = 600;

/**
 * Cursor blink state — the *phase* is a web-side policy (the engine reports only the mode), but
 * **whether** the cursor blinks at all is resolved here from two inputs (#575):
 *
 * 1. the **application's** intent, from the frame's `cursorBlink` (wire v4, #81) — core writes it
 *    from both DECSCUSR (`CSI Ps SP q`) and att610 (`CSI ?12 h/l`);
 * 2. the **consumer's** override, three-state: `undefined` follows the application, `true`/`false`
 *    force it either way.
 *
 * Both references resolve the same pair, and justerm follows **alacritty's placement** —
 * `cursor_style.blinking_override().unwrap_or(terminal_blinking)`
 * (`alacritty/src/event.rs:1631` @ `852e971`, with `Always`/`Never => Some` and `On`/`Off => None`
 * at `config/cursor.rs:125-131`). xterm.js resolves the same two values with the three-state on the
 * *application* side instead (`decPrivateModes.cursorBlink ?? rawOptions.cursorBlink`,
 * `browser/renderer/dom/DomRenderer.ts:531` @ `699f553`). alacritty's shape is the one ADR-0017
 * already implies — core reports the mechanism, the consumer holds the policy — and, unlike
 * xterm.js's, it needs no wire change: core's `cursor_blink` bool is the application's half.
 *
 * **The default is solid.** Both references default to no blink (xterm.js `OptionsService.ts:16`,
 * alacritty `config/cursor.rs:107`) and so does core's `Cursor::blink`; the widget's previous
 * unconditional blink was the outlier and ignored the mode entirely.
 *
 * Time is injected via `isVisible(now)` so the state is testable without real timers; the
 * integration drives it from a `setInterval`/rAF loop.
 */
export class CursorBlink {
  private lastRestart = 0;
  private focused = true;
  private reducedMotion = false;
  private appBlink = false;
  private override: boolean | undefined = undefined;

  /** Whether the cursor is shown at time `now` (ms). */
  isVisible(now: number): boolean {
    // Not blinking at all, reduced motion, or unfocused = solid (and solid means *shown*).
    if (!this.blinking() || this.reducedMotion || !this.focused) {
      return true;
    }
    return Math.floor((now - this.lastRestart) / BLINK_INTERVAL) % 2 === 0;
  }

  /** The consumer's override, else the application's mode (alacritty's `unwrap_or`). */
  private blinking(): boolean {
    return this.override ?? this.appBlink;
  }

  /** The application's blink mode for this frame — `DecodedFrame.cursorBlink`. */
  setAppBlink(blink: boolean): void {
    this.appBlink = blink;
  }

  /**
   * Force blinking on/off regardless of the application, or `undefined` to follow it.
   *
   * This exists because the application's half is not as authoritative as it looks. Measured on a
   * real PTY (RHEL 9.2, `TERM=xterm-256color`, 2026-07-28): of six real programs **none** emit
   * DECSCUSR, while vim, htop and top all emit `CSI ?12 l` — not as a preference, but because
   * terminfo's own "normal cursor" string carries one (`cnorm=\E[?12l\E[?25h`, with
   * `cvvis=\E[?12;25h` as its blinking counterpart). So an ncurses `curs_set()` turns the blink off
   * as a side effect, and merely quitting vim would otherwise pin the cursor steady for the rest of
   * the session with no way back. xterm.js reaches the same conclusion from the other side and
   * ignores `?12` altogether unless the `allowSetCursorBlink` quirk is set
   * (`common/InputHandler.ts:1959` @ `699f553`).
   */
  setBlinkOverride(blink: boolean | undefined): void {
    this.override = blink;
  }

  /** Show the cursor now and reset the blink phase (call on typing/cursor move). */
  restart(now: number): void {
    this.lastRestart = now;
  }

  /** Focus gates blinking — unfocused terminals show a solid cursor (alacritty gates on
   * `is_focused`, `alacritty/src/event.rs:1643`). */
  setFocused(focused: boolean): void {
    this.focused = focused;
  }

  /**
   * Honour `prefers-reduced-motion` (#119): when set, the cursor never blinks. The integration
   * reads the media query and forwards changes here.
   *
   * It outranks **both** other inputs — an application that asked to blink and a consumer that
   * forced it. Derived rather than ported: neither reference has this input at all (xterm.js's
   * `src` has zero `prefers-reduced-motion` hits; alacritty is native), and the asymmetry is what
   * makes the precedence safe in one direction only — reduced motion can only ever *subtract*
   * motion, so letting it win can never make a steady cursor blink.
   */
  setReducedMotion(reduced: boolean): void {
    this.reducedMotion = reduced;
  }
}
