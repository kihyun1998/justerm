/**
 * A self-perpetuating `requestAnimationFrame` loop that can be started, stopped, and — the part
 * this module exists for — **restarted after its body throws** (#696).
 *
 * It clears its handle **before** running the body, so a throw leaves nothing to block the next
 * schedule — ordering, not a `try`/`catch` (`docs/map/territory/widget-lifecycle.md`). After a
 * throw the loop is stopped but restartable: `updateCursor` calls `start()` per decoded frame.
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
   * `body` must not call [`start`](FrameLoop#start) on this loop: during the body no frame is
   * scheduled, so the guard would let a *second* loop begin.
   */
  private run = (): void => {
    this.id = undefined;
    this.body();
    this.id = this.raf(this.run);
  };
}
