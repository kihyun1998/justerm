// DEMO-ONLY fake search engine — the search counterpart to fake-select.ts.
// Mimics core's `search` (literal, smart-case) over the demo log so `pnpm demo`
// can drive the real SearchController + matchSpans rendering without a backend.
// The real engine is justerm-core; this is a throwaway stand-in.

/** One match, inclusive, in absolute log coordinates (single line in the demo). */
export interface Match {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

export class FakeSearchEngine {
  private matches: Match[] = [];

  /** Find every literal occurrence of `query` in `lines` (smart-case: a query
   * with no uppercase matches case-insensitively). Returns the match count. */
  search(query: string, lines: string[]): number {
    this.matches = [];
    if (!query) return 0;
    const ci = !/[A-Z]/.test(query);
    const needle = ci ? query.toLowerCase() : query;
    lines.forEach((text, line) => {
      const hay = ci ? text.toLowerCase() : text;
      let i = hay.indexOf(needle);
      while (i !== -1) {
        this.matches.push({ startLine: line, startCol: i, endLine: line, endCol: i + needle.length - 1 });
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

  match(index: number): Match | undefined {
    return this.matches[index];
  }

  clear(): void {
    this.matches = [];
  }
}
