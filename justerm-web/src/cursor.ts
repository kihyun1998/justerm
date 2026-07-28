/** The cursor blink period, in ms (xterm `BLINK_INTERVAL`). */
export const BLINK_INTERVAL = 600;

/**
 * How long the cursor keeps blinking with no user input before it parks solid, in ms (#593).
 *
 * **Five minutes, xterm.js's value** (`browser/renderer/shared/Constants.ts:12`,
 * `CURSOR_BLINK_IDLE_TIMEOUT = 5 * 60 * 1000` @ `699f553`) — *not* alacritty's five seconds
 * (`alacritty/src/config/cursor.rs:34` @ `852e971`). Both references implement the same rule and
 * disagree by 60x, so neither is inheritable on authority; the tie-breaker is that justerm-web's
 * reset set is already exactly xterm.js's (a key/text intent plus pointer-down), so matching the
 * reference we are structurally identical to is the least arbitrary default available. Five seconds
 * also stops the caret while a user is merely *reading* long output, which reads as a freeze rather
 * than as a feature. Overridable per consumer — this is policy (ADR-0017), not a constant of nature.
 */
export const BLINK_IDLE_TIMEOUT = 5 * 60 * 1000;

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
  /** When the user last interacted — the idle clock (#593). Distinct from `lastRestart`: see
   * {@link restart} vs {@link restartFromInput}. */
  private lastInput = 0;
  private idleTimeoutMs = BLINK_IDLE_TIMEOUT;
  private composing = false;

  /** Whether the cursor is shown at time `now` (ms). */
  isVisible(now: number): boolean {
    // Not blinking at all, reduced motion, unfocused, or mid-composition = solid (and solid means
    // *shown*).
    if (!this.blinking() || this.reducedMotion || !this.focused || this.composing) {
      return true;
    }
    // Idle too long → park solid (#593). A pure read of `(now, lastInput)`, so it composes with the
    // gates above instead of being a latch that needs un-latching — which is what made a timeout
    // fit this chain at all.
    if (this.idleTimeoutMs > 0 && now - this.lastInput > this.idleTimeoutMs) {
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

  /**
   * Reset the blink **phase** so the caret shows at once — for a cursor *move*, which is application
   * output (a TUI repainting moves the cursor every frame).
   *
   * Deliberately does **not** touch the idle clock. Both references reset theirs on *user input*
   * only — alacritty in `on_typing_start` (`event.rs:1201-1213`), xterm.js in
   * `restartBlinkAnimation` (keystroke / mousedown) — so feeding cursor movement into it would mean
   * a terminal running any live TUI never idles out, which is broader than either. Use
   * {@link restartFromInput} on the input path.
   */
  restart(now: number): void {
    this.lastRestart = now;
  }

  /**
   * An IME composition is in progress (#592) — the caret stays put until it ends.
   *
   * Two of the three references do this and for the same stated reason: alacritty suppresses the
   * blink in the same expression that resolves it (`alacritty/src/event.rs:1633`), and ghostty
   * forces a solid block during preedit *"because it shows an important editing state to the user"*
   * (`src/renderer/cursor.zig:47`). xterm.js has no rule at all.
   *
   * **Measured before building, in a real browser**: with the application silent — the default since
   * #575 — the caret is *already* solid during composition, and no content cell changes either. So
   * this is a no-op in the common case and bites only where an application explicitly asked to
   * blink. Narrow on purpose.
   *
   * It suppresses the blink and nothing else. It deliberately does not touch the idle clock: someone
   * mid-composition has not gone idle, but {@link restartFromInput} is the only thing that speaks
   * for input, and giving this gate a second job is how a chain of single-purpose gates rots.
   */
  setComposing(composing: boolean): void {
    this.composing = composing;
  }

  /** The user interacted: reset the blink phase **and** the idle clock (#593). */
  restartFromInput(now: number): void {
    this.lastRestart = now;
    this.lastInput = now;
  }

  /**
   * How long to keep blinking with no user input before parking solid, in ms.
   * `0` disables the timeout entirely — alacritty's `blink_timeout: 0` (`config/cursor.rs:64-70`).
   * A negative or non-finite value is treated as `0` rather than throwing: a renderer that panics
   * across a policy setter is a worse contract than one that reports what it adopted.
   */
  setIdleTimeout(ms: number): void {
    this.idleTimeoutMs = Number.isFinite(ms) && ms > 0 ? ms : 0;
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
