import { describe, expect, it } from "vitest";
import { FakeSearchEngine } from "../demo/fake-search";
import type { SearchOptions } from "../src/search";
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

// #429 lens: an in-flight backend round-trip must not RESURRECT state that a
// clear()/newer search already replaced — the continuation would restore a
// non-zero total for an empty query (result() lies, next() navigates a cleared
// backend) and designate AFTER port.clear() ran.
describe("SearchController — a superseded search never resurrects state", () => {
  // A port whose search() resolution the test controls, so clear() can run
  // while the round-trip is still in flight.
  function deferredPort() {
    const resolvers: ((n: number) => void)[] = [];
    const designated: number[] = [];
    const shown: number[] = [];
    let cleared = 0;
    return {
      port: {
        search: () => new Promise<number>((r) => resolvers.push(r)),
        showMatch: (i: number) => {
          shown.push(i);
          return Promise.resolve();
        },
        designateMatch: (i: number) => {
          designated.push(i);
          return Promise.resolve();
        },
        clear: () => {
          cleared++;
        },
      },
      resolve: (n: number) => resolvers.shift()?.(n),
      designated,
      shown,
      clearedCount: () => cleared,
    };
  }
  const settle = () => new Promise((r) => setTimeout(r, 0));

  it("discards a re-search that resolves after clear()", async () => {
    const d = deferredPort();
    const sched = new ManualScheduler();
    const ctrl = new SearchController(d.port, {
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    const first = ctrl.search("foo");
    d.resolve(3);
    await first; // live search: 3 matches, shown [0]

    ctrl.onFrame();
    await sched.flush(); // reSearch now awaiting the port
    ctrl.clear(); // …and the user closes the search meanwhile
    d.resolve(7); // the stale round-trip lands late
    await settle();

    expect(ctrl.result()).toEqual({ current: 0, total: 0 }); // not resurrected
    expect(d.designated).toEqual([]); // no designation after clear
    await ctrl.next(); // inert — nothing to navigate
    expect(d.shown).toEqual([0]);
  });

  it("discards an initial search that resolves after clear()", async () => {
    const d = deferredPort();
    const ctrl = new SearchController(d.port);
    const inFlight = ctrl.search("foo");
    ctrl.clear();
    d.resolve(5);
    await inFlight;

    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
    expect(d.shown).toEqual([]); // no showMatch(0) after clear
  });

  it("lets a newer query win over a slower older round-trip", async () => {
    const d = deferredPort();
    const ctrl = new SearchController(d.port);
    const oldQuery = ctrl.search("foo");
    const newQuery = ctrl.search("bar");
    d.resolve(9); // resolves the FIRST (stale) search
    d.resolve(2); // resolves the second (live) one
    await oldQuery;
    await newQuery;

    expect(ctrl.result()).toEqual({ current: 1, total: 2 }); // "bar" won
  });
});

// #429 lens: turning the query INVALID must drop the previous query's painted
// highlights + active designation — otherwise the box says "invalid" while the
// screen keeps emphasizing matches of a query that no longer exists.
describe("SearchController — invalid regex clears the stale paint", () => {
  const validatorRejecting = (bad: string) => (p: string) => p !== bad;

  it("drops the paint without ending the session when the live query turns invalid", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port, { isValidRegex: validatorRejecting("foo(") });
    await ctrl.search("foo", { regex: true }); // live highlights + designation

    await ctrl.search("foo(", { regex: true }); // now invalid

    expect(ctrl.isInvalidRegex()).toBe(true);
    expect(port.clearedHighlights).toBe(1); // stale highlights + designation dropped
    expect(port.cleared).toBe(0); // …but the session, and its anchor, survive (#687)
    expect(port.searched).toEqual(["foo"]); // the invalid query never searched
  });

  // `clearHighlights` is OPTIONAL (additive): a backend that predates #687 still
  // has the paint dropped — through `clear()`, at the old cost of the anchor.
  // Nothing downstream is obliged, and the screen never keeps painting a
  // rejected query (which is the #316 D2 defect this path exists for).
  it("falls back to clear() on a port without clearHighlights", async () => {
    let cleared = 0;
    const port = {
      search: () => Promise.resolve(3),
      showMatch: () => Promise.resolve(),
      clear: () => {
        cleared++;
      },
    };
    const ctrl = new SearchController(port, { isValidRegex: validatorRejecting("foo(") });
    await ctrl.search("foo", { regex: true });

    await ctrl.search("foo(", { regex: true }); // now invalid

    expect(cleared).toBe(1);
    expect(ctrl.isInvalidRegex()).toBe(true);
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

  // The engine RESETS the active designation on every `set_search_highlights`
  // hand-over (#428 — stale indices are structurally precluded), so a re-search
  // must re-designate the current match through the scroll-free channel or the
  // active emphasis vanishes on every burst of output. xterm keeps the emphasis
  // across its incremental re-find the same way (`noScroll: true`).
  it("re-designates the active match after a re-search, without scrolling", async () => {
    const port = new StubSearchPort();
    port.count = 5;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo"); // shown [0]
    await ctrl.next(); // index 1, shown [0, 1]

    ctrl.onFrame();
    await sched.flush();

    expect(port.designated).toEqual([1]); // active restored at the same index
    expect(port.shown).toEqual([0, 1]); // …but never via showMatch (no scroll)
  });

  // The re-designated index is the CLAMPED one when the backend cannot anchor —
  // the emphasis lands on the last match, in step with the count label. A
  // backend that can anchor keeps the occurrence instead (#437, below).
  it("re-designates at the clamped index when matches shrink", async () => {
    const port = new StubSearchPort();
    port.count = 5;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");
    await ctrl.next();
    await ctrl.next();
    await ctrl.next(); // index 3

    port.count = 2; // matches shrank under the active index
    ctrl.onFrame();
    await sched.flush();

    expect(port.designated).toEqual([1]); // min(3, 2-1) — matches the 2/2 label
  });

  // No matches → nothing to designate. The empty hand-over already cleared the
  // active designation engine-side; a designate call would be a stale index.
  it("designates nothing when the re-search finds no matches", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");

    port.count = 0;
    ctrl.onFrame();
    await sched.flush();

    expect(port.designated).toEqual([]);
    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
  });

  // `designateMatch` is OPTIONAL (additive) — a backend that predates #429 still
  // works; it just loses the active emphasis across output (never crashes).
  it("tolerates a port without designateMatch", async () => {
    const searched: string[] = [];
    const port = {
      search: (q: string) => {
        searched.push(q);
        return Promise.resolve(2);
      },
      showMatch: () => Promise.resolve(),
      clear: () => {},
    };
    const sched = new ManualScheduler();
    const ctrl = new SearchController(port, {
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    await ctrl.search("foo");

    ctrl.onFrame();
    await sched.flush(); // must not throw

    expect(searched).toEqual(["foo", "foo"]);
    expect(ctrl.result().total).toBe(2);
  });

  // If output removes matches so the buffer now has fewer than the active index,
  // the index clamps to the last match instead of pointing past the end. This is
  // the FALLBACK, not the contract: a backend that can anchor keeps the same
  // occurrence instead (#437, below) — the clamp is what is left when it cannot.
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

// An index is not a stable name for a match. All three references keep the
// emphasis on the same TEXT OCCURRENCE across a re-search, by three different
// mechanisms — xterm re-finds at the previous selection's position
// (`SearchEngine.ts:191`) and derives "n of m" from it
// (`SearchResultTracker.ts:85`); alacritty searches from a stored origin
// `Point`, which next/prev parks at the focused match so that later edits do not
// move it (`event.rs:1152`); ghostty keeps a tracked pin beside the index and
// shifts the index whenever the result list mutates. The backend owns the
// positions, so it answers where the emphasis went; the controller only asks.
describe("SearchController — the emphasis follows the occurrence, not the ordinal (#437)", () => {
  function debounced(port: StubSearchPort, sched: ManualScheduler) {
    return new SearchController(port, { setTimer: sched.setTimer, clearTimer: sched.clearTimer });
  }

  // Output added matches ABOVE the active one (a cursor-addressed write, or the
  // reverse: eviction removed some). The old ordinal now names different text;
  // the backend reports where the same occurrence landed and the emphasis — and
  // the count label with it — follows it there.
  it("re-designates where the backend anchored the emphasis, not at the old ordinal", async () => {
    const port = new StubSearchPort();
    port.count = 5;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");
    await ctrl.next(); // index 1

    port.count = 8; // three matches appeared above the active one
    port.anchored = 4; // …so the same occurrence is now #4
    ctrl.onFrame();
    await sched.flush();

    expect(port.designated).toEqual([4]);
    expect(ctrl.result()).toEqual({ current: 5, total: 8 }); // label reads from the anchor
    expect(port.shown).toEqual([0, 1]); // still no scroll on output
  });

  // The anchored index wins over the clamp even when it points the other way:
  // matches shrank, so the clamp would say "last match", while the occurrence
  // the user was on is still there, earlier in the set.
  it("prefers the anchor over the clamp when the set shrinks", async () => {
    const port = new StubSearchPort();
    port.count = 9;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");
    await ctrl.next();
    await ctrl.next(); // index 2

    port.count = 3; // clamp would give min(2, 2) = 2 — the last match
    port.anchored = 0; // but the occurrence itself moved to the front
    ctrl.onFrame();
    await sched.flush();

    expect(port.designated).toEqual([0]);
    expect(ctrl.result()).toEqual({ current: 1, total: 3 });
  });

  // No matches left → nothing to anchor to and nothing to designate; the
  // backend is not asked where the emphasis went, because there is no set.
  it("does not ask the backend for an anchor when the re-search finds nothing", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const sched = new ManualScheduler();
    const ctrl = debounced(port, sched);
    await ctrl.search("foo");
    const asksBefore = port.anchorCalls;

    port.count = 0;
    port.anchored = 7; // a stale answer the controller must never reach for
    ctrl.onFrame();
    await sched.flush();

    expect(port.designated).toEqual([]);
    expect(port.anchorCalls).toBe(asksBefore); // never asked — not asked-and-ignored
    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
  });

  // The anchor round-trip is subject to the same epoch guard as the search
  // itself (#429): a re-search superseded by clear() must not designate.
  it("discards an anchored re-search that resolves after clear()", async () => {
    let release: (n: number) => void = () => {};
    const designated: number[] = [];
    const port = {
      search: () => new Promise<number>((r) => (release = r)),
      showMatch: () => Promise.resolve(),
      designateMatch: (i: number) => {
        designated.push(i);
        return Promise.resolve();
      },
      anchoredIndex: () => Promise.resolve(2),
      clear: () => {},
    };
    const sched = new ManualScheduler();
    const ctrl = new SearchController(port, {
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    const first = ctrl.search("foo");
    release(3);
    await first;

    ctrl.onFrame();
    const flushed = sched.flush();
    ctrl.clear(); // supersedes the in-flight re-search
    release(3);
    await flushed;

    expect(designated).toEqual([]);
    expect(ctrl.result()).toEqual({ current: 0, total: 0 });
  });
});

// A real backend answers across a macrotask (Tauri `invoke`, `postMessage`) — the
// stub resolves on a microtask, so an interleaving that needs a genuine gap
// between the hand-over and its answer is invisible to it. This port opens that
// gap on demand: `search` parks until `release()` is called.
// …driving the REAL demo backend, so what is under test is the port CONTRACT as
// something implements it, not a second hand-written model of it.
class DeferredBackend {
  readonly engine = new FakeSearchEngine();
  readonly shown: number[] = [];
  readonly designated: number[] = [];
  private pending: (() => void)[] = [];
  constructor(private readonly lines: string[]) {}
  search(q: string, options?: SearchOptions): Promise<number> {
    // The hand-over runs when the backend is ASKED — the demo's own port shape
    // (`demo/main.ts`: run the engine, then return) — and only the answer is
    // deferred. That is the ordering that makes the anchor's lifetime visible:
    // anything the user does next happens after the set was already replaced.
    const n = this.engine.search(q, this.lines, options);
    return new Promise((r) => this.pending.push(() => r(n)));
  }
  showMatch(i: number): Promise<void> {
    this.shown.push(i);
    this.engine.setActive(i);
    return Promise.resolve();
  }
  designateMatch(i: number): Promise<void> {
    this.designated.push(i);
    this.engine.setActive(i);
    return Promise.resolve();
  }
  anchoredIndex(): Promise<number | undefined> {
    return Promise.resolve(this.engine.anchoredIndex());
  }
  cleared = 0;
  clearedHighlights = 0;
  clear(): void {
    this.cleared++;
    this.engine.clear();
  }
  clearHighlights(): void {
    this.clearedHighlights++;
    this.engine.clearHighlights();
  }
  /** Let the oldest parked hand-over answer, then drain the microtask queue. */
  async release(): Promise<void> {
    this.pending.shift()?.();
    await new Promise((r) => setTimeout(r, 0));
  }
}

/** Six rows, one "foo" each — so match `i` sits on line `i` and an ordinal and a
 * position are trivially distinguishable in an assertion. */
const SIX_ROWS = ["foo a", "foo b", "foo c", "foo d", "foo e", "foo f"];

describe("SearchController — a live search is not undone by a background one (#437 lens)", () => {
  // The user presses Enter WHILE the debounced re-search is in flight. The
  // backend answered the hand-over that preceded the press, so its anchor
  // predates the navigation — and a background timer must never roll a user's
  // keypress back. All three references settle this by construction (ghostty
  // mutates the selection under the screen lock, alacritty is one event loop,
  // xterm's addon is synchronous); justerm's port is async, so it needs the
  // anchor to be maintained at designation time rather than sampled per search.
  it("does not roll back a next() pressed while a re-search is in flight", async () => {
    const port = new DeferredBackend(SIX_ROWS);
    const sched = new ManualScheduler();
    const ctrl = new SearchController(port, {
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    const first = ctrl.search("foo");
    await port.release();
    await first; // index 0
    await ctrl.next(); // index 1

    ctrl.onFrame();
    const flushed = sched.flush(); // re-search dispatched, hand-over parked
    await ctrl.next(); // the user advances to 2 mid-flight
    await port.release(); // …and only now does the hand-over answer
    await flushed;

    expect(ctrl.result()).toEqual({ current: 3, total: 6 }); // the press stands
    expect(port.designated).toEqual([2]); // …and the paint agrees with the label
  });

  // A re-search the epoch guard discards has ALREADY handed over. If the anchor
  // were sampled at hand-over time it would now read "nothing designated", and
  // the typing search that superseded it would fall back to match 0 — #441
  // unfixed precisely while output is streaming, which is when it matters.
  it("keeps the anchor when a superseded re-search hands over first", async () => {
    const port = new DeferredBackend(SIX_ROWS);
    const sched = new ManualScheduler();
    const ctrl = new SearchController(port, {
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
    });
    const first = ctrl.search("foo");
    await port.release();
    await first;
    await ctrl.next();
    await ctrl.next(); // index 2 — line 2

    ctrl.onFrame();
    const flushed = sched.flush(); // re-search hand-over parked
    const typed = ctrl.search("foo "); // the user types; supersedes it
    await port.release(); // the re-search's hand-over answers (discarded)
    await flushed;
    await port.release(); // the typing search's hand-over answers
    await typed;

    expect(ctrl.result()).toEqual({ current: 3, total: 6 }); // still on the occurrence
    expect(port.shown).toEqual([0, 1, 2, 2]); // no yank back to the first match
  });

  // A query that transiently matches nothing designates nothing — but the
  // emphasis the user had is not thereby forgotten. Under a streaming terminal
  // an over-typed query passes through 0 matches constantly, and losing the
  // anchor there brings the #441 viewport yank straight back on the next
  // keystroke. alacritty keeps its origin across exactly this (`event.rs:1540`
  // clears the focused match, not the origin); xterm loses it with its selection.
  it("survives a query that transiently matches nothing", async () => {
    const port = new DeferredBackend(SIX_ROWS);
    const ctrl = new SearchController(port);
    const first = ctrl.search("foo");
    await port.release();
    await first;
    await ctrl.next();
    await ctrl.next(); // index 2

    const dead = ctrl.search("fooz"); // nothing matches; nothing designated
    await port.release();
    await dead;
    const back = ctrl.search("foo"); // …backspace
    await port.release();
    await back;

    expect(ctrl.result()).toEqual({ current: 3, total: 6 });
    expect(port.shown).toEqual([0, 1, 2, 2]);
  });

  // …and an invalid regex is the same event wearing a different hat (#687). In
  // regex mode every group, class or escape passes through an invalid
  // intermediate state (`(`, `[`, `\`), so this fires on ordinary typing, not on
  // a mistake. The #316 D2 path drops the engine paint so the screen stops
  // showing a query the box has already rejected — but that is a *new-search*
  // paint drop, not the end of the session, which is why it goes through
  // `clearHighlights` and the anchor outlives it. xterm draws the same line:
  // its `clearDecorations(retainCachedSearchTerm)` retains on exactly the
  // new-search path (`SearchAddon.ts:133`).
  it("keeps the anchor through an invalid regex, and returns to the same occurrence", async () => {
    const port = new DeferredBackend(SIX_ROWS);
    const ctrl = new SearchController(port, { isValidRegex: (p) => !p.endsWith("(") });
    const first = ctrl.search("foo", { regex: true });
    await port.release();
    await first;
    await ctrl.next();
    await ctrl.next(); // index 2

    await ctrl.search("fo(", { regex: true }); // rejected — never reaches the port
    const back = ctrl.search("fo(o)", { regex: true }); // …completed: matches again
    await port.release();
    await back;

    expect(ctrl.result()).toEqual({ current: 3, total: 6 }); // the anchor survived
    expect(port.shown).toEqual([0, 1, 2, 2]);
    expect(port.cleared).toBe(0); // the session never ended
  });

  // The side condition that keeps the two verbs distinct: Escape DOES end the
  // session, so the anchor dies with it and the next query lands on its first
  // match. Without this, "keep the anchor" could be satisfied by never dropping
  // it at all — which would make a fresh search resume near an abandoned one.
  it("still forgets the occurrence when the session is ended", async () => {
    const port = new DeferredBackend(SIX_ROWS);
    const ctrl = new SearchController(port);
    const first = ctrl.search("foo");
    await port.release();
    await first;
    await ctrl.next();
    await ctrl.next(); // index 2

    ctrl.clear();
    const fresh = ctrl.search("foo");
    await port.release();
    await fresh;

    expect(ctrl.result()).toEqual({ current: 1, total: 6 });
    expect(port.shown).toEqual([0, 1, 2, 0]);
    expect(port.cleared).toBe(1);
  });

  // The re-search now moves the CURRENT index, not just the total — so a UI
  // that refreshes its label only on user input would show the pre-output
  // ordinal beside an overlay painting a different match. Nothing returns to the
  // caller on this path, so the controller has to say so (xterm fires
  // `onDidChangeResults` on exactly this path).
  it("reports a background re-search that moved the current index", async () => {
    const port = new StubSearchPort();
    port.count = 5;
    const seen: { current: number; total: number }[] = [];
    const sched = new ManualScheduler();
    const ctrl = new SearchController(port, {
      setTimer: sched.setTimer,
      clearTimer: sched.clearTimer,
      onResults: (r) => seen.push({ ...r }),
    });
    await ctrl.search("foo");
    await ctrl.next(); // index 1

    expect(seen).toEqual([]); // the foreground paths return to their caller

    port.count = 8;
    port.anchored = 4; // output pushed the occurrence down the set
    ctrl.onFrame();
    await sched.flush();

    expect(seen).toEqual([{ current: 5, total: 8 }]);
  });
});

// As-you-type, the query changes under a live emphasis. The references anchor
// it: xterm's incremental find starts at the current selection's start and
// expands there (`SearchEngine.ts:108-116`), alacritty re-runs from the stored
// origin (`event.rs:1523` via `1565`), and ghostty designates nothing at all
// while typing. None of them jumps to the first match and scrolls to it.
describe("SearchController — typing does not re-jump to the first match (#441)", () => {
  // Typing extends the query while a match is active: the emphasis stays on the
  // occurrence the user is looking at (or the first one after it), reached
  // through showMatch so an off-screen result is still scrolled into view.
  it("lands on the anchored occurrence when the query is extended", async () => {
    const port = new StubSearchPort();
    port.count = 6;
    const ctrl = new SearchController(port);
    await ctrl.search("se"); // lands on 0
    await ctrl.next(); // index 1

    port.count = 4;
    port.anchored = 2;
    await ctrl.search("sel");

    expect(port.shown).toEqual([0, 1, 2]);
    expect(ctrl.result()).toEqual({ current: 3, total: 4 });
    expect(port.designated).toEqual([]); // typing scrolls; it is user-driven
  });

  // A first search has nothing to anchor to — the backend says so, and the
  // controller lands on the first match exactly as it always has. This is the
  // control for the test above: the fix must not move the initial landing.
  it("still lands on the first match when there is no anchor", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);

    await ctrl.search("foo"); // port.anchored is undefined — no prior emphasis

    expect(port.shown).toEqual([0]);
    expect(ctrl.result()).toEqual({ current: 1, total: 3 });
  });

  // `anchoredIndex` is OPTIONAL (additive): a backend that predates this keeps
  // the old as-you-type behaviour — first match, every keystroke — and never
  // crashes on the missing method.
  it("tolerates a port without anchoredIndex", async () => {
    const shown: number[] = [];
    const port = {
      search: () => Promise.resolve(4),
      showMatch: (i: number) => {
        shown.push(i);
        return Promise.resolve();
      },
      clear: () => {},
    };
    const ctrl = new SearchController(port);

    await ctrl.search("se");
    await ctrl.next();
    await ctrl.search("sel"); // must not throw

    expect(shown).toEqual([0, 1, 0]);
  });

  // An anchor out of the new set's range is the backend contradicting itself.
  // The controller is total anyway rather than designating past the end — the
  // count label and the emphasis stay consistent whatever the backend says.
  it("ignores an anchor outside the new result set", async () => {
    const port = new StubSearchPort();
    port.count = 3;
    const ctrl = new SearchController(port);

    port.anchored = 9;
    await ctrl.search("foo");

    expect(port.shown).toEqual([0]);
    expect(ctrl.result()).toEqual({ current: 1, total: 3 });
  });
});
