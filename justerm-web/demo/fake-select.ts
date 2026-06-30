// DEMO-ONLY fake selection engine. Replicates just enough of justerm-core's
// selection model (char/word/line/block range + text extraction, across the
// log) so `pnpm demo` can exercise the real SelectionController + overlay
// rendering without a backend. The real engine is penterm/justerm-core; this is
// a throwaway stand-in, exactly like the demo's hand-rolled scrollback.
import type { SelType, Side } from "../src/index";

interface Pt {
  line: number; // absolute log line
  col: number;
  side: Side;
}

export class FakeSelectionEngine {
  private anchor: Pt | undefined;
  private focus: Pt | undefined;
  private ty: SelType = "char";

  constructor(
    private readonly lines: () => string[],
    /** Absolute log line shown at viewport row 0, for the current scroll. */
    private readonly viewTop: () => number,
    private readonly rows: () => number,
  ) {}

  begin(vrow: number, vcol: number, side: Side, ty: SelType): void {
    const p = { line: this.viewTop() + vrow, col: vcol, side };
    this.anchor = p;
    this.focus = p;
    this.ty = ty;
  }

  extend(vrow: number, vcol: number, side: Side): void {
    if (this.anchor) this.focus = { line: this.viewTop() + vrow, col: vcol, side };
  }

  clear(): void {
    this.anchor = undefined;
    this.focus = undefined;
  }

  /** Selection projected onto the current viewport as flat `(row, left, right)`
   * triples — the `selectionSpans` the overlay renderer paints. */
  range(): number[] {
    const ord = this.ordered();
    if (!ord) return [];
    const [s, e] = ord;
    const top = this.viewTop();
    const out: number[] = [];
    for (let line = s.line; line <= e.line; line++) {
      const [from, to] = this.colRange(line, s, e);
      const row = line - top;
      if (row >= 0 && row < this.rows() && to > from) out.push(row, from, to - 1);
    }
    return out;
  }

  /** The selected text across the whole log (not just the viewport). */
  text(): string | null {
    const ord = this.ordered();
    if (!ord) return null;
    const [s, e] = ord;
    const lines = this.lines();
    const out: string[] = [];
    for (let line = s.line; line <= e.line; line++) {
      const [from, to] = this.colRange(line, s, e);
      out.push((lines[line] ?? "").slice(from, to).replace(/\s+$/, ""));
    }
    const text = out.join("\n");
    return text.length ? text : null;
  }

  /** Half-open `[from, to)` columns for `line`, per selection type. */
  private colRange(line: number, s: Pt, e: Pt): [number, number] {
    const len = (this.lines()[line] ?? "").length;
    if (this.ty === "line") return [0, len];
    if (this.ty === "block") {
      const lo = Math.min(this.anchor!.col, this.focus!.col);
      const hi = Math.max(this.anchor!.col, this.focus!.col);
      return [lo, Math.min(hi + 1, len)];
    }
    if (this.ty === "word") {
      const from = line === s.line ? this.wordStart(s.line, s.col) : 0;
      const to = line === e.line ? this.wordEnd(e.line, e.col) + 1 : len;
      return [from, Math.min(to, len)];
    }
    // char — each endpoint's side decides whether its own cell is included.
    const from = line === s.line ? (s.side === "right" ? s.col + 1 : s.col) : 0;
    const to = line === e.line ? (e.side === "right" ? e.col + 1 : e.col) : len;
    return [Math.max(0, from), Math.min(to, len)];
  }

  private wordStart(line: number, col: number): number {
    const s = this.lines()[line] ?? "";
    let i = Math.min(col, s.length - 1);
    if (i < 0 || /\s/.test(s[i]!)) return col;
    while (i > 0 && !/\s/.test(s[i - 1]!)) i--;
    return i;
  }

  private wordEnd(line: number, col: number): number {
    const s = this.lines()[line] ?? "";
    let i = Math.min(col, s.length - 1);
    if (i < 0 || /\s/.test(s[i]!)) return col;
    while (i < s.length - 1 && !/\s/.test(s[i + 1]!)) i++;
    return i;
  }

  private ordered(): [Pt, Pt] | null {
    const a = this.anchor;
    const f = this.focus;
    if (!a || !f) return null;
    return a.line < f.line || (a.line === f.line && a.col <= f.col) ? [a, f] : [f, a];
  }
}
