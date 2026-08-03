/**
 * A self-perpetuating `requestAnimationFrame` loop that can be started, stopped, and — the part
 * this module exists for — **restarted after its body throws** (#696).
 *
 * The widget's blink loop was written inline as `tick(){ …work…; this.rafId = raf(tick) }`, with a
 * re-entry guard reading `if (this.rafId !== undefined) return`. A throw from the work skipped the
 * re-arm, and `rafId` still held the id of the frame that was *currently running* — so the guard
 * refused every later restart and the loop was off for the life of the widget. Nothing logged;
 * frames driven by PTY output kept painting, and only the blinking was gone.
 *
 * **The fix is the order of two assignments, not a `try`/`catch`**, and it is the reference's own
 * shape: xterm.js's `RenderDebouncer._innerRefresh` clears `_animationFrame` as its *first*
 * statement, before invoking the render callback (`src/browser/RenderDebouncer.ts:54-55`), so a
 * throw leaves nothing behind to block the next schedule. Catching instead would force a choice
 * between swallowing the error and re-raising it sixty times a second; clearing first needs
 * neither, and the error propagates to the browser exactly as it does today.
 *
 * After a throw the loop is **stopped but restartable**, which for this widget means "restarted by
 * the next frame": `updateCursor` runs per decoded frame and calls `start()` unconditionally, and
 * `applyFrame` calls it whenever text blink is enabled.
 *
 * `raf`/`caf` are injected because this module is host-tested and the test environment is `node`
 * (`vitest.config.ts`) — there is no DOM to take them from, which is also why the widget that owns
 * this loop has no instantiation seam of its own.
 */
export class FrameLoop {
  /** The pending frame's id, or `undefined` when no frame is scheduled. */
  private id: number | undefined;

  constructor(
    private readonly raf: (cb: () => void) => number,
    private readonly caf: (id: number) => void,
    private readonly body: () => void,
  ) {}

  /** Whether a frame is currently scheduled. Exposed for assertions, not for control flow. */
  get running(): boolean {
    return this.id !== undefined;
  }

  /** Schedule the loop if it is not already running. Idempotent, and safe after a throw. */
  start(): void {
    if (this.id !== undefined) {
      return;
    }
    this.id = this.raf(this.run);
  }

  /** Cancel the pending frame, if any. Idempotent. */
  stop(): void {
    if (this.id !== undefined) {
      this.caf(this.id);
      this.id = undefined;
    }
  }

  /**
   * One iteration. The clear comes **first** — see the class doc: at this point the id it holds
   * belongs to the frame that is already running, so it is not a handle to anything cancellable,
   * and leaving it in place is what let a throw latch the loop off.
   *
   * A corollary worth stating because it is invisible from the call site: `body` must not call
   * [`start`](FrameLoop#start) on this loop, because during the body no frame is scheduled and the
   * guard would let a *second* loop begin. Today's body does not (it only calls into the renderer
   * backend and the blink state); a future one that does would double the blink rate.
   */
  private run = (): void => {
    this.id = undefined;
    this.body();
    this.id = this.raf(this.run);
  };
}
