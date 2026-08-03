// DEMO-ONLY fake search engine — the search counterpart to fake-select.ts.
// Mimics core's `search_with` (literal/regex, whole-word, smart-case) over the
// demo log so `pnpm demo` can drive the real SearchController + matchSpans
// rendering without a backend. The real engine is justerm-core; this is a
// throwaway stand-in. NOTE: regex here uses JS `RegExp`, whose dialect differs
// from core's `regex` crate — the demo gates invalid patterns with the *real*
// wasm `isValidRegex` (core's dialect) before ever reaching this fake.

/** One match, inclusive, in absolute log coordinates (single line in the demo). */
export interface Match {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

/** Demo mirror of the library `SearchOptions` (see src/search.ts). */
export interface FakeSearchOptions {
  regex?: boolean;
  wholeWord?: boolean;
  caseSensitive?: boolean;
}

const WORD = /\w/;
// Out of range means "no character there", and no character is not a word character - so `?? ""` says
// exactly what a bound check means. It never actually fires: `start` is always a match index and `end`
// is `start + len - 1` with `len >= 1`, so the `start === 0` / `end === text.length - 1`
// short-circuits already cover precisely the two indices that would fall outside.
//
// It is not a pure formality either. `WORD.test(undefined)` coerces to `"undefined"` and returns TRUE,
// so had the guard ever failed, the old code would have called the missing character word-y. `?? ""`
// gives the opposite - and correct - answer.
const isWordBounded = (text: string, start: number, end: number): boolean =>
  (start === 0 || !WORD.test(text[start - 1] ?? "")) &&
  (end === text.length - 1 || !WORD.test(text[end + 1] ?? ""));

export class FakeSearchEngine {
  private matches: Match[] = [];
  /** Index of the ACTIVE (current) match (#429) — the engine-side designation
   * `set_active_search_highlight` mirrors. `undefined` = nothing designated. */
  private activeIndex: number | undefined;
  /** The POSITION the emphasis was last designated at (#437/#441) — set by
   * {@link setActive}, and deliberately NOT re-sampled per {@link search}: it
   * has to outlive the set it came from, because a hand-over resets the
   * designation and the next thing that reads the anchor is a *later* search.
   * Sampling it per hand-over loses it whenever a superseded re-search lands
   * first, or the query passes through zero matches — the two orderings that a
   * streaming terminal produces constantly. This is alacritty's shape (`origin`
   * is a `Point` in `SearchState`, cleared by neither `search_reset_state` nor
   * the match set) rather than xterm's (whose anchor IS the selection, so it
   * dies with it). Dropped by {@link clear}. */
  private anchor: Match | undefined;

  /** Find every occurrence of `query` in `lines`, honouring `options` (regex,
   * whole-word, case). Smart-case unless `caseSensitive` is set: a query with no
   * uppercase matches case-insensitively. Returns the match count.
   *
   * Every hand-over RESETS the active designation, mirroring the real engine's
   * `set_search_highlights` contract (#428) — the consumer re-designates. */
  search(query: string, lines: string[], options?: FakeSearchOptions): number {
    this.matches = [];
    this.activeIndex = undefined;
    if (!query) return 0;
    const ci = options?.caseSensitive === undefined ? !/[A-Z]/.test(query) : !options.caseSensitive;
    const wholeWord = options?.wholeWord ?? false;

    if (options?.regex) {
      // A real backend gates invalid patterns with the wasm `isValidRegex` before
      // ever searching (core returns empty, never throws). Mirror that here so the
      // demo stays robust even if the validator is unavailable — an invalid pattern
      // is 0 matches, not an exception.
      let re: RegExp;
      try {
        re = new RegExp(query, ci ? "gi" : "g");
      } catch {
        return 0;
      }
      lines.forEach((text, line) => {
        for (let m = re.exec(text); m !== null; m = re.exec(text)) {
          if (m[0].length === 0) {
            re.lastIndex++; // avoid an infinite loop on an empty match
            continue;
          }
          const start = m.index;
          const end = start + m[0].length - 1;
          if (!wholeWord || isWordBounded(text, start, end)) {
            this.matches.push({ startLine: line, startCol: start, endLine: line, endCol: end });
          }
        }
      });
      return this.matches.length;
    }

    const needle = ci ? query.toLowerCase() : query;
    lines.forEach((text, line) => {
      const hay = ci ? text.toLowerCase() : text;
      let i = hay.indexOf(needle);
      while (i !== -1) {
        const end = i + needle.length - 1;
        if (!wholeWord || isWordBounded(text, i, end)) {
          this.matches.push({ startLine: line, startCol: i, endLine: line, endCol: end });
        }
        i = hay.indexOf(needle, i + needle.length);
      }
    });
    return this.matches.length;
  }

  /** Visible matches projected onto the viewport as flat `(row, left, right)`
   * triples — the `matchSpans` the overlay renderer paints. */
  matchSpans(viewTop: number, rows: number): number[] {
    const out: number[] = [];
    for (const m of this.matches) {
      const row = m.startLine - viewTop;
      if (row >= 0 && row < rows) out.push(row, m.startCol, m.endCol);
    }
    return out;
  }

  /** Designate match `index` as the active one (#429) — the fake's
   * `set_active_search_highlight`. Out of range → nothing designated (the real
   * engine takes an index into the highlight set it holds). */
  setActive(index: number): void {
    this.activeIndex = this.matches[index] ? index : undefined;
    // Designating is also what moves the anchor — the emphasis's position is
    // recorded the moment it is placed, so it is already correct for whatever
    // hand-over comes next (#437/#441). An out-of-range designation designates
    // nothing and therefore moves nothing.
    if (this.activeIndex !== undefined) this.anchor = this.matches[this.activeIndex];
  }

  /** The ACTIVE match projected onto the viewport as flat `(row, left, right)` —
   * the `activeMatchSpans` wire group (#428). Empty when nothing is designated
   * or the active match is off-screen. The active match is *also* present in
   * {@link matchSpans}; the renderer's ranking resolves the overlap. */
  activeMatchSpans(viewTop: number, rows: number): number[] {
    const m = this.activeIndex === undefined ? undefined : this.matches[this.activeIndex];
    if (!m) return [];
    const row = m.startLine - viewTop;
    return row >= 0 && row < rows ? [row, m.startCol, m.endCol] : [];
  }

  /** Where the emphasis belongs in the set the last {@link search} produced, so
   * it stays on the same OCCURRENCE rather than the same ordinal (#437/#441).
   *
   * The rule: the anchored occurrence itself if it survived, otherwise the first
   * one at or after it, wrapping to the top. That is xterm's `findNext` rule
   * (`SearchEngine.ts:139-148`) applied to BOTH paths, which is a deliberate
   * choice rather than a convergence — xterm's own two paths disagree, since its
   * debounced output re-find is `findPrevious`, whose fallback walks *backward*
   * from the anchor (`SearchEngine.ts:201-213`). alacritty answers a different
   * question (a fixed origin plus the user's last search direction) and ghostty
   * refuses to guess (exact pin equality, else back to the first or last match).
   * Nothing arbitrates the fallback direction; one rule on both paths is at
   * least self-consistent, which the reference is not.
   *
   * `undefined` = no anchor at all, so land on match 0. */
  anchoredIndex(): number | undefined {
    const a = this.anchor;
    if (!a || this.matches.length === 0) return undefined;
    const at = this.matches.findIndex(
      (m) => m.startLine > a.startLine || (m.startLine === a.startLine && m.startCol >= a.startCol),
    );
    return at === -1 ? 0 : at;
  }

  match(index: number): Match | undefined {
    return this.matches[index];
  }

  clear(): void {
    this.matches = [];
    this.activeIndex = undefined;
    this.anchor = undefined;
  }
}
