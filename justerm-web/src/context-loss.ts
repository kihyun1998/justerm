/**
 * The consumer's context-loss handler, held on this side of the renderer boundary (#579).
 *
 * `justerm-renderer` notifies a **single** JS function when a lost WebGL context has not come back
 * within the restore deadline (`setOnContextLoss`, #327). This object is that function. The
 * consumer's own handler is swapped behind it, and the channel is closed for good when the widget
 * ends.
 *
 * Deliberately a relay rather than the consumer's function handed straight to the renderer: the
 * renderer's `setOnContextLoss` has no unset, and `dispose()` cannot reach the slot the renderer
 * clears only in `Drop` (#606). Pure and host-tested, like `FrameLoop`. Why, and what the browser
 * proves instead: `docs/map/territory/widget-lifecycle.md`.
 */
export class ContextLossRelay {
  private handler: (() => void) | undefined;
  /** Latched, never cleared: `Terminal.dispose()` is end of life, not unmount (#606). */
  private ended = false;

  /**
   * What the renderer holds. Registered **once**, at `create`, whether or not the consumer opted
   * in — the same reason `setBgAlpha` is pushed unconditionally: the value the renderer
   * holds is then the one this package states, rather than one nobody wrote down.
   *
   * An arrow property, so its identity survives every {@link set}. A method would have to be
   * re-bound per swap, and re-registering is precisely what the renderer's missing unset makes
   * unsafe to rely on.
   */
  readonly notify = (): void => {
    this.handler?.();
  };

  /**
   * Install the consumer's handler, or `undefined` to stop delivering to one. **A no-op once
   * {@link end} has run** — deliberately gated here and not in {@link notify}
   * (`docs/map/territory/widget-lifecycle.md`).
   */
  set(handler: (() => void) | undefined): void {
    if (this.ended) return;
    this.handler = handler;
  }

  /**
   * Close the channel for good, and drop the consumer's closure with it (the widget has no business
   * retaining it past its own life). Idempotent, as every teardown on this port must be.
   *
   * Both statements carry weight, and neither is redundant — see {@link set}: the latch is what
   * makes this *closed* rather than *cleared*, and the clear is what stops the handler installed
   * before it from being delivered to.
   */
  end(): void {
    this.ended = true;
    this.handler = undefined;
  }
}
