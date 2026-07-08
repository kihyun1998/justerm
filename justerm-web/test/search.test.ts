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

describe("SearchController — search modes (#316)", () => {
  // The query box can run in regex / whole-word / case-sensitive modes. The
  // controller forwards the chosen SearchOptions to the port so the backend runs
  // core's `search_with` — mirroring xterm's ISearchOptions. Omitted = core's
  // default (literal, smart-case).
  it("forwards SearchOptions to the port", async () => {
    const port = new StubSearchPort();
    port.count = 1;
    const ctrl = new SearchController(port);

    await ctrl.search("Foo", { regex: true, wholeWord: true, caseSensitive: true });

    expect(port.searched).toEqual(["Foo"]);
    expect(port.searchedOptions).toEqual([{ regex: true, wholeWord: true, caseSensitive: true }]);
  });

  // A plain search (no options) stays the literal default — the options argument
  // is optional, so existing callers are unaffected.
  it("defaults to no options for a plain search", async () => {
    const port = new StubSearchPort();
    port.count = 1;
    const ctrl = new SearchController(port);

    await ctrl.search("foo");

    expect(port.searchedOptions).toEqual([undefined]);
  });
});

describe("SearchController — invalid regex (#316 D2)", () => {
  // A stand-in for the wasm `isValidRegex` (core's dialect) — rejects one pattern.
  const validatorRejecting = (bad: string) => (p: string) => p !== bad;

  // In regex mode an invalid pattern is caught *before* the backend runs — core's
  // `search_with` would silently return empty (indistinguishable from a real
  // no-match), so the controller flags it and skips the search entirely.
  it("flags an invalid regex and does not search", async () => {
    const port = new StubSearchPort();
    port.count = 3; // would report matches if it ran
    const ctrl = new SearchController(port, { isValidRegex: validatorRejecting("foo(") });

    await ctrl.search("foo(", { regex: true });

    expect(ctrl.isInvalidRegex()).toBe(true);
    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
    expect(port.searched).toEqual([]); // never hit the backend
    expect(port.shown).toEqual([]);
  });

  // Once the query becomes a valid pattern the flag clears and the search runs.
  it("clears the invalid flag once the regex becomes valid", async () => {
    const port = new StubSearchPort();
    port.count = 1;
    const ctrl = new SearchController(port, { isValidRegex: validatorRejecting("foo(") });
    await ctrl.search("foo(", { regex: true }); // invalid

    await ctrl.search("f.o", { regex: true }); // now valid

    expect(ctrl.isInvalidRegex()).toBe(false);
    expect(port.searched).toEqual(["f.o"]);
    expect(ctrl.result()).toEqual({ current: 1, total: 1 });
  });

  // Validation is regex-mode only: in literal mode "(" is just a character, so a
  // query that would be an invalid *regex* still searches.
  it("only validates in regex mode — a literal query is never invalid", async () => {
    const port = new StubSearchPort();
    port.count = 2;
    const ctrl = new SearchController(port, { isValidRegex: validatorRejecting("foo(") });

    await ctrl.search("foo(", {}); // literal mode

    expect(ctrl.isInvalidRegex()).toBe(false);
    expect(port.searched).toEqual(["foo("]);
  });

  // With no validator injected (a consumer without the wasm helper) regex mode
  // still runs — validation is a best-effort surface, not a hard gate.
  it("searches normally when no validator is injected", async () => {
    const port = new StubSearchPort();
    port.count = 1;
    const ctrl = new SearchController(port);

    await ctrl.search("foo(", { regex: true });

    expect(ctrl.isInvalidRegex()).toBe(false);
    expect(port.searched).toEqual(["foo("]);
  });

  // clear() drops the invalid flag along with the rest of the search state.
  it("clear() resets the invalid flag", async () => {
    const port = new StubSearchPort();
    const ctrl = new SearchController(port, { isValidRegex: validatorRejecting("foo(") });
    await ctrl.search("foo(", { regex: true }); // invalid

    ctrl.clear();

    expect(ctrl.isInvalidRegex()).toBe(false);
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
