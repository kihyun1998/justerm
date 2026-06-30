import { describe, expect, it } from "vitest";
import { SearchController, StubSearchPort } from "../src/search";

// A manual timer: setTimer stashes the callback, flush() fires the latest one.
// Lets debounce tests run without real time (mirrors S8's injected clock).
class ManualScheduler {
  private fn: (() => void) | null = null;
  readonly setTimer = (fn: () => void): number => {
    this.fn = fn;
    return 1;
  };
  readonly clearTimer = (): void => {
    this.fn = null;
  };
  /** Fire the pending timer (if any) and let its async work settle. */
  async flush(): Promise<void> {
    const fn = this.fn;
    this.fn = null;
    fn?.();
    await new Promise((r) => setTimeout(r, 0));
  }
}

describe("SearchController — query → results", () => {
  // A query runs through the engine (core `search`, on the backend across
  // scrollback) and the controller exposes the count for the UI. The backend
  // caps the *highlight* set at 1000; the count is the full total.
  it("searches the query and exposes the match count", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);

    await ctrl.search("foo");

    expect(port.searched).toEqual(["foo"]);
    expect(ctrl.result()).toEqual({ current: 1, total: 3 });
  });

  // A search lands on its first match — the backend selects it and scrolls it
  // into view (xterm `findNext` after highlighting).
  it("activates the first match after a search", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);

    await ctrl.search("foo");

    expect(port.shown).toEqual([0]);
  });

  // A query with no matches activates nothing and reports an empty count — the
  // box shows 0/0, the backend is not asked to show a match.
  it("activates nothing when the query has no matches", async () => {
    const port = new StubSearchPort();
    port.count = 0;
    const ctrl = new SearchController(port);

    await ctrl.search("zzz");

    expect(port.shown).toEqual([]);
    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
  });

  // next() advances the active match and wraps past the end back to the first —
  // the consumer-driven navigation core's search model expects.
  it("next() advances and wraps around", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);
    await ctrl.search("foo"); // index 0

    await ctrl.next(); // 1
    expect(ctrl.result()).toEqual({ current: 2, total: 3 });
    await ctrl.next(); // 2
    await ctrl.next(); // wrap → 0

    expect(ctrl.result()).toEqual({ current: 1, total: 3 });
    expect(port.shown).toEqual([0, 1, 2, 0]);
  });

  // prev() steps back and wraps from the first match to the last.
  it("prev() steps back and wraps to the last", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);
    await ctrl.search("foo"); // index 0

    await ctrl.prev(); // wrap → 2 (last)

    expect(ctrl.result()).toEqual({ current: 3, total: 3 });
    expect(port.shown).toEqual([0, 2]);
  });

  // clear() drops the search — highlights/selection gone (port.clear), count
  // reset, and navigation becomes inert until the next query.
  it("clear() resets the search and makes navigation inert", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);
    await ctrl.search("foo"); // shown [0]

    ctrl.clear();
    await ctrl.next(); // inert — nothing to navigate

    expect(port.cleared).toBe(1);
    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
    expect(port.shown).toEqual([0]); // no extra showMatch after clear
  });
});

describe("SearchController — incremental re-search on output", () => {
  function debounced(port: StubSearchPort, sched: ManualScheduler) {
    return new SearchController(port, { setTimer: sched.setTimer, clearTimer: sched.clearTimer });
  }

  // New terminal output (a frame) re-runs the active query so highlights track
  // the changed buffer (xterm onWriteParsed, 200ms debounce, incremental). It
  // updates the count but does NOT scroll/navigate — the active match stays put.
  it("re-searches after the debounce without navigating", async () => {
    const port = new StubSearchPort();
    port.count = 2;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo"); // searched ["foo"], shown [0]

    port.count = 5; // output produced more matches
    ctrl.onFrame();
    await sched.flush();

    expect(port.searched).toEqual(["foo", "foo"]); // re-ran the query
    expect(ctrl.result().total).toBe(5); // count refreshed
    expect(port.shown).toEqual([0]); // no extra showMatch — noScroll
  });
});

describe("SearchController — debounce contracts", () => {
  function debounced(port: StubSearchPort, sched: ManualScheduler) {
    return new SearchController(port, { setTimer: sched.setTimer, clearTimer: sched.clearTimer });
  }

  // Rapid output (many frames in the window) collapses to one re-search — the
  // whole point of the debounce.
  it("coalesces rapid frames into a single re-search", async () => {
    const port = new StubSearchPort();
    port.count = 1;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");

    ctrl.onFrame();
    ctrl.onFrame();
    ctrl.onFrame();
    await sched.flush();

    expect(port.searched).toEqual(["foo", "foo"]); // initial + exactly one re-search
  });

  // A frame with no active search does nothing — no timer, no query to run.
  it("ignores frames when no search is active", async () => {
    const port = new StubSearchPort();
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);

    ctrl.onFrame();
    await sched.flush();

    expect(port.searched).toEqual([]);
  });

  // If output removes matches so the buffer now has fewer than the active index,
  // the index clamps to the last match instead of pointing past the end.
  it("clamps the active index when matches shrink", async () => {
    const port = new StubSearchPort();
    port.count = 5;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");
    await ctrl.next();
    await ctrl.next();
    await ctrl.next(); // index 3 → current 4/5

    port.count = 2; // matches shrank
    ctrl.onFrame();
    await sched.flush();

    expect(ctrl.result()).toEqual({ current: 2, total: 2 }); // clamped to last
  });
});
