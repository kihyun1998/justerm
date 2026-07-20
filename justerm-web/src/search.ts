/**
 * The control seam from the web search box to the engine. In frame mode the
 * consumer wires this to the backend, which runs core's `search` (literal,
 * smart-case, across scrollback), caps the highlight set, and drives
 * `set_search_highlights` / `set_active_search_highlight` / `scroll_to_match`.
 * Sibling to {@link SelectionPort}.
 *
 * The backend owns the `Vec<Match>` (matches never cross the wire — only their
 * viewport `matchSpans` / `activeMatchSpans` do), so navigation is by **index**:
 * the controller asks for match `i`, the backend designates it active + scrolls
 * to it (it does NOT select it — the selection channel stays the user's, #429).
 */
/**
 * Search modes on top of the literal + smart-case default — the TS mirror of core's
 * `SearchOptions` (#314/#316), matching xterm.js's `ISearchOptions`. Every field is
 * optional; an omitted / empty object is exactly the literal, smart-case {@link
 * SearchPort.search}. The backend runs core's `search_with` with these.
 *
 * In `regex` mode a consumer validates the query as-you-type with the wasm
 * `isValidRegex` (core's dialect, not JS `RegExp`) before searching, since an
 * invalid pattern otherwise yields a silent empty result (#316 D2).
 */
export interface SearchOptions {
  /** Treat the query as a regular expression (core's `regex` crate dialect: no
   * lookaround/backreferences, Unicode-aware `\w \d \b`) instead of a literal. */
  regex?: boolean;
  /** Match only where the run is bounded by non-word characters (`\bword\b`). */
  wholeWord?: boolean;
  /** Override smart-case: `true` = case-sensitive, `false` = force insensitive,
   * omitted = smart-case (insensitive iff the query has no uppercase). */
  caseSensitive?: boolean;
}

export interface SearchPort {
  /** Run the query (with optional {@link SearchOptions}); highlight up to the
   * backend's cap and return the *full* match count (the cap limits highlights,
   * not the count). Every hand-over RESETS the engine's active designation
   * (#428), so the controller re-designates after an incremental re-search. */
  search(query: string, options?: SearchOptions): Promise<number>;
  /** Make match `index` the active one — designate it on the engine's *active*
   * channel (`set_active_search_highlight`, its own overlay colour above
   * selection and matches, #429) and scroll it into view (off-screen →
   * centered; on-screen → left alone), backend-side. It does NOT select the
   * match: the selection channel stays the user's (#424), so a manual text
   * selection coexists with search navigation. Past the backend's highlight
   * cap, an INDEX designation paints nothing — a capping backend designates by
   * absolute span instead (core `set_active_search_match`, #436), which paints
   * the active emphasis alone (honestly no plain highlight underneath). */
  showMatch(index: number): Promise<void>;
  /** Designate match `index` as active WITHOUT scrolling — the incremental
   * re-search path (#429): a new hand-over reset the engine's designation, and
   * re-navigating on every burst of output would yank the viewport (xterm's
   * `noScroll` re-find). Optional (additive): a backend without it merely loses
   * the active emphasis across output. */
  designateMatch?(index: number): Promise<void>;
  /** Drop the search: clear highlights and the active designation. */
  clear(): void;
}

/** A recording {@link SearchPort} for tests/demos. `count` is what the next
 * {@link search} resolves to. */
export class StubSearchPort implements SearchPort {
  count = 0;
  readonly searched: string[] = [];
  /** The options passed alongside each {@link search} query (parallel to
   * {@link searched}); `undefined` for a plain literal search. */
  readonly searchedOptions: (SearchOptions | undefined)[] = [];
  readonly shown: number[] = [];
  /** The indices passed to {@link designateMatch} (the scroll-free re-designation
   * channel, #429) — separate from {@link shown} so a test can tell navigation
   * from re-designation. */
  readonly designated: number[] = [];
  cleared = 0;
  search(query: string, options?: SearchOptions): Promise<number> {
    this.searched.push(query);
    this.searchedOptions.push(options);
    return Promise.resolve(this.count);
  }
  showMatch(index: number): Promise<void> {
    this.shown.push(index);
    return Promise.resolve();
  }
  designateMatch(index: number): Promise<void> {
    this.designated.push(index);
    return Promise.resolve();
  }
  clear(): void {
    this.cleared++;
  }
}

/** Debounce (ms) for re-running the query on terminal output (xterm parity). */
const DEBOUNCE_MS = 200;

/** The current/total the search box shows: `current` is 1-based, `0` when there
 * are no matches.
 *
 * This is also the consumer's ANNOUNCE seam (#439) — the parity twin of xterm's
 * `onDidChangeResults`, which exists precisely so hosts (VS Code) speak find
 * results. Announce policy is the consumer's (ADR-0017): mirror VS Code's
 * SimpleFindWidget — a dedicated `aria-live=polite` region speaking
 * `"{current} of {total} found for '{query}'"` / `"No results found for
 * '{query}'"` on user-driven updates (typing, next/prev), gated by an SR-active
 * check (#161) and silent when the search UI is closed. The demo wires the
 * reference implementation. */
export interface SearchResult {
  current: number;
  total: number;
}

/**
 * Drives a search box against a {@link SearchPort}. Pure logic — no DOM, no
 * timers of its own: the widget feeds it the query and navigation, it tracks the
 * result count + active index. Highlights come back via frame `matchSpans`
 * (rendered by {@link highlightRects} from S8); this only drives the model.
 */
export class SearchController {
  private total = 0;
  private index = 0;
  /** The active query (empty = no search), so output frames can re-run it. */
  private query = "";
  /** The active query's modes, so an incremental re-search reuses them (#316). */
  private options: SearchOptions | undefined;
  /** The active regex-mode query failed validation — the box shows "invalid" and
   * no search ran (#316 D2). Only ever true in regex mode with a validator. */
  private invalid = false;
  /** Bumped by every {@link search}/{@link clear} — an in-flight backend
   * round-trip captures it and discards its own result if superseded, so a slow
   * response can never resurrect a cleared/replaced search (#429 lens: the
   * stale continuation would restore a non-zero total for an empty query and
   * designate AFTER `port.clear()` ran). */
  private epoch = 0;
  private pending: number | undefined;
  private readonly setTimer: (fn: () => void, ms: number) => number;
  private readonly clearTimer: (handle: number) => void;
  /** Validate a regex-mode query against core's dialect (the wasm `isValidRegex`)
   * before searching — a JS `RegExp` check would misjudge (#316 D2). Absent =
   * best-effort skipped (a consumer without the wasm helper still searches). */
  private readonly validateRegex?: (pattern: string) => boolean;

  constructor(
    private readonly port: SearchPort,
    opts: {
      setTimer?: (fn: () => void, ms: number) => number;
      clearTimer?: (handle: number) => void;
      isValidRegex?: (pattern: string) => boolean;
    } = {},
  ) {
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms) as unknown as number);
    this.clearTimer = opts.clearTimer ?? ((h) => clearTimeout(h));
    this.validateRegex = opts.isValidRegex;
  }

  /** Whether the active regex-mode query is invalid (#316 D2) — the box red-flags
   * it and no search ran. Always `false` for literal queries or when no validator
   * is injected. */
  isInvalidRegex(): boolean {
    return this.invalid;
  }

  /** Run a new query (with optional {@link SearchOptions}) and track its match
   * count, landing on the first match. The options stick to the query so an
   * incremental re-search on output reuses them (#316). */
  async search(query: string, options?: SearchOptions): Promise<void> {
    const epoch = ++this.epoch;
    this.query = query;
    this.options = options;
    // Regex mode: reject an invalid pattern up front (core's dialect) so a bad
    // pattern shows as "invalid", not a silent 0 matches (#316 D2). Drop the
    // previous query's engine paint too — otherwise the box says "invalid"
    // while the screen keeps highlighting matches of a query that no longer
    // exists (with its active emphasis, post-#429).
    if (options?.regex && this.validateRegex && !this.validateRegex(query)) {
      this.invalid = true;
      this.total = 0;
      this.index = 0;
      this.port.clear();
      return;
    }
    this.invalid = false;
    const total = await this.port.search(query, options);
    if (epoch !== this.epoch) return; // superseded by clear()/a newer query
    this.total = total;
    this.index = 0;
    if (this.total > 0) await this.port.showMatch(0);
  }

  /** The buffer changed under the query — feed this EVERY frame that mutates it:
   * terminal output *and* resize/reflow (xterm hooks `onResize` into the same
   * debounced re-find; core invalidates highlights on reflow, so a consumer that
   * only wires output frames shows a stale count + no highlights after a resize
   * until the next output burst). Re-runs the active query after a debounce so
   * highlights track the buffer — count refreshes, the active match is
   * re-designated at its (clamped) index, but *not* re-navigated (no scroll).
   * Inert with no active query. */
  onFrame(): void {
    if (!this.query) return;
    if (this.pending !== undefined) this.clearTimer(this.pending);
    this.pending = this.setTimer(() => void this.reSearch(), DEBOUNCE_MS);
  }

  private async reSearch(): Promise<void> {
    this.pending = undefined;
    if (this.invalid) return; // an invalid regex never became a live search
    const epoch = this.epoch;
    const total = await this.port.search(this.query, this.options);
    if (epoch !== this.epoch) return; // superseded by clear()/a newer query
    this.total = total;
    // Keep the active match where it was, clamped to the new result set.
    this.index = this.total === 0 ? 0 : Math.min(this.index, this.total - 1);
    // The hand-over reset the engine's active designation (#428), so restore it
    // at the clamped index — scroll-free, or every burst of output would yank
    // the viewport (xterm's `noScroll` re-find keeps the emphasis the same way).
    // With no matches there is nothing to designate (the empty hand-over already
    // cleared it); without the optional port method the emphasis is just lost.
    if (this.total > 0) await this.port.designateMatch?.(this.index);
  }

  /** Drop the search: clear engine highlights + the active designation and
   * reset all state (a user selection is not the search's to clear, #429). */
  clear(): void {
    this.epoch++; // invalidate any in-flight round-trip (see `epoch`)
    if (this.pending !== undefined) {
      this.clearTimer(this.pending);
      this.pending = undefined;
    }
    this.port.clear();
    this.total = 0;
    this.index = 0;
    this.query = "";
    this.options = undefined;
    this.invalid = false;
  }

  /** Advance to the next match, wrapping past the last back to the first. */
  async next(): Promise<void> {
    if (this.total === 0) return;
    this.index = (this.index + 1) % this.total;
    await this.port.showMatch(this.index);
  }

  /** Step back to the previous match, wrapping past the first to the last. */
  async prev(): Promise<void> {
    if (this.total === 0) return;
    this.index = (this.index - 1 + this.total) % this.total;
    await this.port.showMatch(this.index);
  }

  /** The count for the UI: 1-based current, `0` total when nothing matches. */
  result(): SearchResult {
    return this.total === 0 ? { current: 0, total: 0 } : { current: this.index + 1, total: this.total };
  }
}
