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
export interface SearchPort {
  /** Run the query; highlight up to the backend's cap and return the *full*
   * match count (the cap limits highlights, not the count). */
  search(query: string): Promise<number>;
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
  readonly shown: number[] = [];
  cleared = 0;
  search(query: string): Promise<number> {
    this.searched.push(query);
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
  private pending: number | undefined;
  private readonly setTimer: (fn: () => void, ms: number) => number;
  private readonly clearTimer: (handle: number) => void;

  constructor(
    private readonly port: SearchPort,
    opts: {
      setTimer?: (fn: () => void, ms: number) => number;
      clearTimer?: (handle: number) => void;
    } = {},
  ) {
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms) as unknown as number);
    this.clearTimer = opts.clearTimer ?? ((h) => clearTimeout(h));
  }

  /** Run a new query and track its match count, landing on the first match. */
  async search(query: string): Promise<void> {
    this.query = query;
    this.total = await this.port.search(query);
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
    this.total = await this.port.search(this.query);
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
