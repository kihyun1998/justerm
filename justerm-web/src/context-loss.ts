/**
 * The consumer's context-loss handler, held on this side of the renderer boundary (#579).
 *
 * `justerm-renderer` notifies a **single** JS function when a lost WebGL context has not come back
 * within the restore deadline (`setOnContextLoss`, #327). This object is that function. The
 * consumer's own handler is swapped behind it, and the channel is closed for good when the widget
 * ends.
 *
 * **Why the indirection rather than handing the consumer's function straight to the renderer.**
 * Two asymmetries in the published surface, neither of which this layer may reach across:
 *
 * - **There is no unset.** `setOnContextLoss(callback: Function)` takes a `Function`, so a consumer
 *   detaching (or replacing) its handler has nothing to pass. Registering `notify` once and
 *   swapping behind it is what makes {@link set} — including `set(undefined)` — expressible at all.
 * - **`dispose()` cannot reach the renderer's callback slot.** The renderer clears it in `Drop`
 *   (`webgl.rs`, `ContextLossHandler`), and `Drop` runs at the wasm binding's `free()`, which
 *   `Terminal.dispose()` deliberately never calls — it stops work, not memory (#606). So without
 *   {@link end} a deadline armed before disposal still delivers to a widget that has ended.
 *
 * That second point is **parity, not invention**: xterm.js's disposable clears its restore timeout
 * (`addons/addon-webgl/src/WebglRenderer.ts:161-163`), so a disposed renderer never fires
 * `onContextLoss` — and justerm-renderer's `Drop` comment names that very behaviour as the contract
 * it matches. `Drop` is simply the wrong end of the object's life for a widget teardown, so the
 * gate lives here.
 *
 * Pure and host-tested, for the reason `FrameLoop` is (#696): `vitest.config.ts` runs the `node`
 * environment and `JustermRenderer`'s constructor reads `window.matchMedia`, so the class that
 * composes this cannot be built in a unit test. Extracting the part that carries a rule is the
 * same trade #696 took, and it leaves the same hole recorded in
 * `docs/map/territory/widget-lifecycle.md` — the composition itself is proven in the browser, not
 * here.
 */
export class ContextLossRelay {
  private handler: (() => void) | undefined;
  /** Latched, never cleared: `Terminal.dispose()` is end of life, not unmount (#606). */
  private ended = false;

  /**
   * What the renderer holds. Registered **once**, at `create`, whether or not the consumer opted
   * in — the same reason `setBgAlpha` is pushed unconditionally (#577): the value the renderer
   * holds is then the one this package states, rather than one nobody wrote down.
   *
   * An arrow property, so its identity survives every {@link set}. A method would have to be
   * re-bound per swap, and re-registering is precisely what the renderer's missing unset makes
   * unsafe to rely on.
   */
  readonly notify = (): void => {
    if (this.ended) return;
    this.handler?.();
  };

  /**
   * Install the consumer's handler, or `undefined` to stop delivering to one.
   *
   * **Deliberately unguarded against {@link end}**, and that is measured rather than chosen for
   * brevity. A symmetric `if (this.ended) return;` was written here first; a mutation test showed
   * neither guard could be made to fail, because each masks the other — with the delivery gate in
   * place a late registration is already inert, and with `end` clearing the handler a missing gate
   * is already silent. Exactly one of the two carries the rule, so the redundant one is removed
   * rather than kept for symmetry (the same call `gridForBox` records in `justerm-renderer.ts`: a
   * branch that cannot change an outcome is untestable by construction).
   *
   * The gate kept is the one in {@link notify}, because the contract is about **delivery** — *a
   * widget that has ended never notifies* — and that holds however a handler came to be installed,
   * including through a path added later that does not think to ask.
   */
  set(handler: (() => void) | undefined): void {
    this.handler = handler;
  }

  /**
   * Close the channel for good, and drop the consumer's closure with it (the widget has no business
   * retaining it past its own life). Idempotent, as every teardown on this port must be.
   *
   * The `ended` latch is what makes this *closed* rather than *cleared*; the clear beside it
   * releases the closure and decides nothing — see {@link set}.
   */
  end(): void {
    this.ended = true;
    this.handler = undefined;
  }
}
