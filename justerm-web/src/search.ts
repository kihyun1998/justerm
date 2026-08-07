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
 *
 * An index is a fine *request* and a poor *memory*: the set is re-derived on
 * every re-search, so the same ordinal can name different text. Carrying the
 * emphasis across a re-search therefore goes back through the backend, which is
 * the only side holding positions — {@link SearchPort.anchoredIndex} (#437/#441).
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
  /** Where the emphasis belongs in the result set the last {@link search}
   * produced, so it stays on the same **occurrence** rather than on the same
   * ordinal (#437/#441).
   *
   * The controller navigates by index, but an index is not a stable name for a
   * match: scrollback eviction removes matches above the active one and a
   * cursor-addressed write adds some, so the same ordinal silently becomes a
   * different piece of text. All three references keep the occurrence instead,
   * by three different mechanisms — xterm re-finds at the previous selection's
   * exact position and derives "n of m" from it, alacritty searches from a
   * stored origin `Point`, ghostty holds a tracked pin beside the index and
   * shifts the index whenever the result list mutates. Only the backend can
   * answer this: the `Vec<Match>` and its buffer coordinates are its, and a
   * frame-mode consumer holds no positions at all (ADR-0017 — mechanism
   * backend, policy here).
   *
   * **The anchor is written when the emphasis is placed, not sampled when a
   * search hands over.** A backend implementing this records the position of
   * every match it designates ({@link showMatch} / {@link designateMatch}) and
   * keeps it until {@link clear} — through hand-overs, through a query that
   * matches nothing, through a designation that never happened. Sampling it per
   * hand-over instead is wrong in two orderings a streaming terminal produces
   * constantly: a debounced re-search that hands over and is then superseded has
   * already reset the designation, so the surviving search reads "nothing"; and a
   * `next()` pressed during a round trip would be rolled back by an anchor older
   * than the keypress. Both were measured. alacritty keeps its `origin` `Point`
   * this way; xterm's anchor is the selection, so it does die with it.
   *
   * Given that anchor, this reports
   * - the index of that same occurrence in the new set, or
   * - the first occurrence at or after it (wrapping to 0) when it is gone, or
   * - `undefined` when there is no anchor — no search has been navigated since
   *   the last {@link clear}, so land on 0.
   *
   * **A remembered position is not stable, and the engine renumbers it in your
   * absence.** A `Match` is in absolute buffer coordinates, and the engine moves
   * that space at four separate sites: evicting the oldest line past the
   * scrollback cap shifts every index **down** by one, a top-anchored sub-region
   * scroll shifts the lines below the margin **up** by one (#449), an in-screen
   * region scroll moves only what is inside the region, and a reflow rewrites all
   * of them. A reflow and an alt-screen switch additionally end the anchor's
   * *meaning*, since a remembered position then names different text.
   *
   * Only the first of those was measured, and its effect is the reason this
   * paragraph is not merely advisory: at the cap the emphasis walks forward one
   * occurrence per evicted line, with the count label unchanged and no user input
   * at all (#691).
   *
   * **A backend that runs `justerm-core` should not hold the coordinate itself.**
   * `Engine::track_point` returns a stable id whose position the engine maintains
   * across all four movers and answers `None` once its **line** has left the
   * buffer — the same treatment the selection and decoration markers get. Note
   * the scope: erasing or overwriting the cells under the point leaves it valid,
   * because it is a positional reference and `anchoredIndex` resolves by nearest
   * position (justerm#750 measured this and sharpened the core's wording).
   * Register the point when you designate a match, resolve it here, release it on
   * `clear()`. A backend that keeps a raw `Match` instead is the case this
   * paragraph warns about, and nothing upstream re-clamps it for you.
   *
   * Optional (additive): a backend without it degrades to the previous
   * behaviour — the clamped ordinal on output, the first match while typing.
   * Call it only after the {@link search} whose set it refers to has resolved. */
  anchoredIndex?(): Promise<number | undefined>;
  /** Drop the engine paint — highlights **and** the active designation — while
   * the search session continues: the anchor ({@link anchoredIndex}) survives.
   * The narrower sibling of {@link clear}, and the split is one core already
   * draws on its own side of the wire: highlights are *query-derived* state,
   * which is invalidated, while the anchor is *user-authored* — where the user
   * navigated to — and is carried instead. `clear()` ends the session and takes
   * both.
   *
   * Its caller is the #316 D2 path: a regex-mode query that fails validation
   * must stop the screen painting the previous query's matches, but that is a
   * *new search* dropping its predecessor's paint, not a user leaving the
   * search. The distinction is not academic — in regex mode every group, class
   * or escape passes through an invalid intermediate state (`(`, `[`, `\`), so
   * ending the session there means the character that *completes* the pattern
   * re-lands the emphasis on match 0 and scrolls to it (#687).
   *
   * **alacritty is the only reference that has this situation, and it keeps the
   * anchor through it.** A non-empty pattern that fails to compile leaves
   * `dfas = None`, so `goto_match` returns before its body and `origin` is never
   * touched — the `search_reset_state` call lives in the *empty*-regex branch
   * that arm never reaches (`event.rs:1520`, `:1557`). xterm has no
   * invalid-pattern state at all (its `isValidSearchTerm` is `length > 0`), but
   * its paint drop is anchor-neutral anyway: `clearDecorations` never touches
   * the selection, which is what carries its emphasis. Neither ends a session
   * because the query changed. justerm needs a port *method* where they need
   * neither, because the side holding the positions is across a process
   * boundary — the seam falls where xterm has a private call.
   *
   * One deliberate difference: alacritty's marker survives an invalid pattern,
   * while this drops the designation with the highlights. #316 D2 requires the
   * screen to stop showing a rejected query, and core voids the designation on
   * every hand-over anyway (#428) — core, the backend and this port agree.
   *
   * Optional (additive): a backend without it falls back to {@link clear}, which
   * is exactly the pre-#687 behaviour — the paint still goes, at the cost of the
   * anchor. */
  clearHighlights?(): void;
  /** Drop the search: clear highlights, the active designation **and the anchor**
   * ({@link anchoredIndex}) — this ends the search session, so the next query
   * lands on its first match rather than near the last one. The user leaving the
   * search box (Escape) is what this is for; a query merely superseding another
   * one wants {@link clearHighlights}. */
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
  /** What the next {@link anchoredIndex} resolves to — `undefined` (the default)
   * models a backend that cannot anchor, which is exactly the fallback path. */
  anchored: number | undefined = undefined;
  cleared = 0;
  /** Counted separately from {@link cleared} so a test can tell "dropped the
   * paint" from "ended the session" — the whole of #687. */
  clearedHighlights = 0;
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
  /** How many times {@link anchoredIndex} was asked — a test can tell "the
   * controller took the fallback" from "the controller never asked". */
  anchorCalls = 0;
  anchoredIndex(): Promise<number | undefined> {
    this.anchorCalls++;
    return Promise.resolve(this.anchored);
  }
  clearHighlights(): void {
    this.clearedHighlights++;
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

  /** Told when the DEBOUNCED re-search changed the result — the one transition
   * no caller can observe, because nothing returns to it. Every other path
   * ({@link search} / {@link next} / {@link prev} / {@link clear}) resolves to
   * the caller, which reads {@link result} itself. xterm's parity twin is
   * `onDidChangeResults`, fired on this exact path (`SearchResultTracker`).
   *
   * It exists because the re-search now moves the *current index*, not only the
   * total (#437): the emphasis follows its occurrence, so a UI that only
   * refreshes its label on user input starts showing a number the paint
   * disagrees with. Announce policy stays the consumer's (ADR-0017) and #439's
   * cadence is deliberately user-driven — the reference implementation refreshes
   * the visible label here and does **not** speak. */
  private readonly onResults?: (r: SearchResult) => void;

  constructor(
    private readonly port: SearchPort,
    opts: {
      setTimer?: (fn: () => void, ms: number) => number;
      clearTimer?: (handle: number) => void;
      isValidRegex?: (pattern: string) => boolean;
      onResults?: (r: SearchResult) => void;
    } = {},
  ) {
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms) as unknown as number);
    this.clearTimer = opts.clearTimer ?? ((h) => clearTimeout(h));
    this.validateRegex = opts.isValidRegex;
    this.onResults = opts.onResults;
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
    //
    // The paint, not the session: this is a new search superseding its
    // predecessor's highlights, and a user typing `(` has not left the search
    // box. Ending the session here would take the anchor with it, so the
    // character that completes the pattern would re-land the emphasis on match 0
    // — #441's symptom returning through a side door, on every group, class and
    // escape a regex contains (#687). A backend that predates the narrower verb
    // falls back to the session-ending one: the paint still goes, which is what
    // #316 D2 is about, and only the anchor is lost.
    if (options?.regex && this.validateRegex && !this.validateRegex(query)) {
      this.invalid = true;
      this.total = 0;
      this.index = 0;
      if (this.port.clearHighlights) this.port.clearHighlights();
      else this.port.clear();
      return;
    }
    this.invalid = false;
    const total = await this.port.search(query, options);
    if (epoch !== this.epoch) return; // superseded by clear()/a newer query
    // Typing extends a query under a live emphasis, so landing on match 0 would
    // yank the viewport back to the top of the buffer on every keystroke. Both
    // references that designate while typing anchor instead — xterm re-finds at
    // the current selection's start, alacritty from a stored origin — and the
    // third (ghostty) designates nothing at all while typing. Nobody re-lands on
    // the first match. So keep the occurrence the backend anchored and fall back
    // to the first match only when there is no anchor (#441).
    //
    // Where this lands is xterm's, and it carries xterm's consequence: because
    // each keystroke designates, the anchor moves with it, so the emphasis
    // RATCHETS forward through the buffer and backspacing does not walk it back.
    // alacritty does not ratchet — its origin is written at search start and by
    // next/prev only (`event.rs:970`, `:1143`), never by `update_search` — so it
    // returns to where the search began. 2-1, and the majority is the one whose
    // anchor is a by-product of designating, which is also justerm's shape.
    const anchored = await this.anchoredIndexWithin(total);
    if (epoch !== this.epoch) return;
    this.total = total;
    this.index = anchored ?? 0;
    if (this.total > 0) await this.port.showMatch(this.index);
  }

  /** The backend's answer to "where did the emphasis go", validated against the
   * set it is an index into. `undefined` = no anchor, an unanchored backend, or
   * an answer outside the new set — all three mean *take the fallback*, so the
   * controller stays total whatever a backend reports (#437/#441). */
  private async anchoredIndexWithin(total: number): Promise<number | undefined> {
    if (total === 0) return undefined; // nothing to anchor to; nothing to ask
    const i = await this.port.anchoredIndex?.();
    return i !== undefined && Number.isInteger(i) && i >= 0 && i < total ? i : undefined;
  }

  /** The buffer changed under the query — feed this EVERY frame that mutates it:
   * terminal output *and* resize/reflow (xterm hooks `onResize` into the same
   * debounced re-find; core invalidates highlights on reflow, so a consumer that
   * only wires output frames shows a stale count + no highlights after a resize
   * until the next output burst). Re-runs the active query after a debounce so
   * highlights track the buffer — count refreshes, the active match is
   * re-designated on the same **occurrence** (see {@link SearchPort.anchoredIndex};
   * its clamped ordinal when the backend cannot anchor), but *not* re-navigated
   * (no scroll). Inert with no active query. */
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
    // Keep the emphasis on the same OCCURRENCE. An index is not a stable name
    // for a match — eviction removes matches above the active one and a
    // cursor-addressed write adds some, so the old ordinal silently names
    // different text with no user input at all (#437). Only the backend holds
    // the positions, so it reports where the occurrence went; the clamp below
    // is what is left when it cannot answer.
    const anchored = await this.anchoredIndexWithin(total);
    if (epoch !== this.epoch) return;
    this.total = total;
    this.index = anchored ?? (this.total === 0 ? 0 : Math.min(this.index, this.total - 1));
    // The hand-over reset the engine's active designation (#428), so restore it
    // where the occurrence now sits — scroll-free, or every burst of output would
    // yank the viewport (xterm's `noScroll` re-find keeps the emphasis the same way).
    // With no matches there is nothing to designate (the empty hand-over already
    // cleared it); without the optional port method the emphasis is just lost.
    if (this.total > 0) await this.port.designateMatch?.(this.index);
    this.onResults?.(this.result());
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
