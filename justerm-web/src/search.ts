/**
 * The control seam from the web search box to the engine. In frame mode the
 * consumer wires this to the backend, which runs core's `search` (literal,
 * smart-case, across scrollback), caps the highlight set, and drives
 * `set_search_highlights` / `scroll_to_match`. Sibling to {@link SelectionPort}.
 *
 * The backend owns the `Vec<Match>` (matches never cross the wire — only their
 * viewport `matchSpans` do), so navigation is by **index**: the controller asks
 * for match `i`, the backend selects + scrolls to it.
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
   * not the count). */
  search(query: string, options?: SearchOptions): Promise<number>;
  /** Make match `index` the active one — select it and scroll it into view
   * (off-screen → centered; on-screen → just selected), backend-side. */
  showMatch(index: number): Promise<void>;
  /** Drop the search: clear highlights and the active selection. */
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
  clear(): void {
    this.cleared++;
  }
}

/** Debounce (ms) for re-running the query on terminal output (xterm parity). */
const DEBOUNCE_MS = 200;

/** The current/total the search box shows: `current` is 1-based, `0` when there
 * are no matches. */
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
    this.query = query;
    this.options = options;
    // Regex mode: reject an invalid pattern up front (core's dialect) so a bad
    // pattern shows as "invalid", not a silent 0 matches (#316 D2).
    if (options?.regex && this.validateRegex && !this.validateRegex(query)) {
      this.invalid = true;
      this.total = 0;
      this.index = 0;
      return;
    }
    this.invalid = false;
    this.total = await this.port.search(query, options);
    this.index = 0;
    if (this.total > 0) await this.port.showMatch(0);
  }

  /** A new frame arrived (terminal output). Re-run the active query after a
   * debounce so highlights track the changed buffer — count refreshes, but the
   * active match is *not* re-navigated (no scroll). Inert with no active query. */
  onFrame(): void {
    if (!this.query) return;
    if (this.pending !== undefined) this.clearTimer(this.pending);
    this.pending = this.setTimer(() => void this.reSearch(), DEBOUNCE_MS);
  }

  private async reSearch(): Promise<void> {
    this.pending = undefined;
    if (this.invalid) return; // an invalid regex never became a live search
    this.total = await this.port.search(this.query, this.options);
    // Keep the active match where it was, clamped to the new result set.
    this.index = this.total === 0 ? 0 : Math.min(this.index, this.total - 1);
  }

  /** Drop the search: clear engine highlights/selection and reset all state. */
  clear(): void {
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
