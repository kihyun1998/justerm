import { describe, expect, it } from "vitest";
import { FrameLoop } from "../src/frame-loop";

/**
 * A controllable `requestAnimationFrame` pair. Frames are not run until `tick()` asks for one, so a
 * test drives the loop a step at a time rather than waiting on a real clock — and `cancelled`
 * records what `stop()` actually cancelled, which is the half a "did it stop" assertion misses.
 */
function fakeRaf() {
  let next = 1;
  const pending = new Map<number, () => void>();
  const cancelled: number[] = [];
  return {
    raf: (cb: () => void): number => {
      const id = next++;
      pending.set(id, cb);
      return id;
    },
    caf: (id: number): void => {
      cancelled.push(id);
      pending.delete(id);
    },
    /** Run every frame scheduled so far. Returns how many ran. */
    tick(): number {
      const due = [...pending.entries()];
      pending.clear();
      for (const [, cb] of due) cb();
      return due.length;
    },
    get scheduled(): number {
      return pending.size;
    },
    cancelled,
  };
}

describe("FrameLoop", () => {
  it("keeps rescheduling itself once started", () => {
    const f = fakeRaf();
    let runs = 0;
    const loop = new FrameLoop(f.raf, f.caf, () => runs++);

    loop.start();
    expect(f.scheduled).toBe(1);
    f.tick();
    f.tick();
    f.tick();
    expect(runs).toBe(3);
    // Exactly one frame outstanding after each iteration — a loop that re-armed without clearing
    // would accumulate, and `runs` alone cannot see that.
    expect(f.scheduled).toBe(1);
  });

  it("does not start a second loop while one is running", () => {
    const f = fakeRaf();
    let runs = 0;
    const loop = new FrameLoop(f.raf, f.caf, () => runs++);

    loop.start();
    loop.start();
    loop.start();
    expect(f.scheduled).toBe(1);
    f.tick();
    expect(runs).toBe(1);
  });

  it("stops, cancelling the frame it actually scheduled", () => {
    const f = fakeRaf();
    const loop = new FrameLoop(f.raf, f.caf, () => {});

    loop.start();
    f.tick(); // now the second frame is the outstanding one, id 2
    loop.stop();
    expect(f.cancelled).toEqual([2]);
    expect(f.scheduled).toBe(0);
    expect(loop.running).toBe(false);
    // Idempotent: a second stop cancels nothing rather than cancelling a stale id.
    loop.stop();
    expect(f.cancelled).toEqual([2]);
  });

  it("survives a throw from its body: the loop stops, and a later start restarts it (#696)", () => {
    const f = fakeRaf();
    let runs = 0;
    let explode = false;
    const loop = new FrameLoop(f.raf, f.caf, () => {
      runs++;
      if (explode) throw new Error("render failed");
    });

    loop.start();
    f.tick();
    expect(runs).toBe(1);

    // The frame that throws. The throw escapes — deliberately not caught here, because the loop
    // must not swallow it either; the browser reports it exactly as it does today.
    explode = true;
    expect(() => f.tick()).toThrow("render failed");
    expect(runs).toBe(2);

    // THE assertion. Before the fix the id of the frame that just ran was still held, so this
    // reported `true` and every later `start()` returned early — blink was off for the life of the
    // widget, silently.
    expect(loop.running).toBe(false);
    expect(f.scheduled).toBe(0);

    // ...and the loop is restartable, which is what the widget's per-frame `start()` relies on.
    explode = false;
    loop.start();
    expect(f.scheduled).toBe(1);
    f.tick();
    expect(runs).toBe(3);
    // Still exactly one loop, not two — a fix that re-armed *and* left the id would double it.
    expect(f.scheduled).toBe(1);
  });

  it("a throw does not leave a stale id for stop() to cancel", () => {
    const f = fakeRaf();
    const loop = new FrameLoop(f.raf, f.caf, () => {
      throw new Error("boom");
    });

    loop.start();
    expect(() => f.tick()).toThrow("boom");
    loop.stop();
    // Nothing to cancel: the frame that threw had already run. Cancelling its id would be a stale
    // handle, harmless today but a lie about what is scheduled.
    expect(f.cancelled).toEqual([]);
  });
});
