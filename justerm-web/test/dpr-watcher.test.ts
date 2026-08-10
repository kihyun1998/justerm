import { describe, expect, it } from "vitest";
import { DprWatcher, type ResolutionQuery } from "../src/dpr-watcher";

/**
 * A stand-in for the browser's resolution media queries, and the one thing it has to model
 * faithfully is the reason this class exists: **a query is bound to the ratio it was created with.**
 *
 * So the fake keeps one query object per ratio and only delivers to the listeners registered on
 * *that* object. A watcher that fails to re-arm therefore stays attached to a query for a ratio the
 * environment has already left, and the second move reaches nobody — which is exactly how the real
 * defect presents, and it is not observable at all if the fake just keeps one global listener list.
 */
class FakeEnvironment {
  dpr = 1;
  /** Every query handed out, by the ratio it was built for. */
  readonly queries = new Map<number, FakeQuery>();
  /** Ratios queried, in order — so a test can assert re-arming happened at the right value. */
  readonly asked: number[] = [];

  readonly match = (dpr: number): ResolutionQuery => {
    this.asked.push(dpr);
    const q = new FakeQuery();
    this.queries.set(dpr, q);
    return q;
  };

  readonly current = (): number => this.dpr;

  /** Move the display, then notify the query bound to the ratio we just left. */
  moveTo(next: number): void {
    const left = this.dpr;
    this.dpr = next;
    this.queries.get(left)?.fire();
  }
}

class FakeQuery implements ResolutionQuery {
  private readonly listeners = new Set<() => void>();
  addEventListener(_type: "change", listener: () => void): void {
    this.listeners.add(listener);
  }
  removeEventListener(_type: "change", listener: () => void): void {
    this.listeners.delete(listener);
  }
  get listenerCount(): number {
    return this.listeners.size;
  }
  fire(): void {
    for (const l of [...this.listeners]) l();
  }
}

const watcherOn = (env: FakeEnvironment): { watcher: DprWatcher; seen: number[] } => {
  const seen: number[] = [];
  const watcher = new DprWatcher(env.match, env.current, (dpr) => seen.push(dpr));
  return { watcher, seen };
};

describe("DprWatcher", () => {
  it("arms a query at the ratio in force when it starts", () => {
    const env = new FakeEnvironment();
    env.dpr = 1.5;
    const { watcher } = watcherOn(env);

    watcher.start();

    expect(env.asked).toEqual([1.5]);
    expect(env.queries.get(1.5)!.listenerCount).toBe(1);
  });

  it("reports the ratio the display moved TO, not the one it was armed at", () => {
    const env = new FakeEnvironment();
    const { watcher, seen } = watcherOn(env);
    watcher.start();

    env.moveTo(2);

    // The event itself carries nothing useful — a resolution query only says "you left 1dppx" — so
    // the new value has to be read from the environment at delivery time.
    expect(seen).toEqual([2]);
  });

  it("re-arms at the new ratio, so a SECOND move is still seen", () => {
    const env = new FakeEnvironment();
    const { watcher, seen } = watcherOn(env);
    watcher.start();

    env.moveTo(2);
    env.moveTo(3);

    // The whole point. Without the re-arm the watcher is still listening to the 1dppx query, which
    // the environment never fires again, and every move after the first is silently lost.
    expect(seen).toEqual([2, 3]);
    expect(env.asked).toEqual([1, 2, 3]);
  });

  it("lets the superseded query go rather than accumulating listeners", () => {
    const env = new FakeEnvironment();
    const { watcher } = watcherOn(env);
    watcher.start();

    env.moveTo(2);

    // Not tidiness: a retained listener on a stale query would double-deliver the moment the display
    // returned to a ratio previously armed, which is the ordinary case of dragging a window back.
    expect(env.queries.get(1)!.listenerCount).toBe(0);
    expect(env.queries.get(2)!.listenerCount).toBe(1);
  });

  it("delivers nothing after stop(), and stop() is idempotent", () => {
    const env = new FakeEnvironment();
    const { watcher, seen } = watcherOn(env);
    watcher.start();

    watcher.stop();
    watcher.stop();
    env.moveTo(2);

    expect(seen).toEqual([]);
    expect(env.queries.get(1)!.listenerCount).toBe(0);
  });

  it("stop() detaches the CURRENT query, not the one it started with", () => {
    const env = new FakeEnvironment();
    const { watcher, seen } = watcherOn(env);
    watcher.start();
    env.moveTo(2); // now armed on the 2dppx query

    watcher.stop();
    env.moveTo(3);

    // A stop() that removed its listener from the original query would leave the re-armed one live —
    // the teardown and the re-arm have to agree about which query is current.
    expect(seen).toEqual([2]);
    expect(env.queries.get(2)!.listenerCount).toBe(0);
  });

  it("start() twice arms once", () => {
    const env = new FakeEnvironment();
    const { watcher, seen } = watcherOn(env);

    watcher.start();
    watcher.start();
    env.moveTo(2);

    expect(env.asked).toEqual([1, 2]);
    expect(seen).toEqual([2]);
  });

  it("stays stopped — a start() after stop() does not revive it", () => {
    const env = new FakeEnvironment();
    const { watcher, seen } = watcherOn(env);
    watcher.start();
    watcher.stop();

    watcher.start();
    env.moveTo(2);

    // `stop()` is the widget's end of life (`dispose()`), not an unmount — the same latch
    // `ContextLossRelay.end` carries, and for the same reason: nothing on this port restarts.
    expect(seen).toEqual([]);
  });
});
