import { CellMirror } from "./cell-mirror";
import type { MouseEventLike } from "./input";
import { computeLinks, LinkController, URL_REGEX, type Link, type LinkOptions, type LogicalLine } from "./links";
import type { DecodedFrame, FlagBits } from "./types";

/** What a {@link LinkTracker} reads and drives. */
export interface LinkTrackerDeps {
  flagBits: FlagBits;
  options: LinkOptions;
  /** The pointer is over this link. */
  onHover(link: Link): void;
  /** The pointer is over no link any more. */
  onLeave(): void;
}

/** A port answer, held with the row it was asked for. */
interface CachedLine {
  row: number;
  line: LogicalLine;
}

const NO_LINE: LogicalLine = { text: "", cells: [] };

/** A link's cells as the renderer's stride-3 `(row, left, right)` spans: one per run of consecutive
 * columns on a row, rows outside `0..rows` dropped. */
export function hoverSpans(cells: ReadonlyArray<readonly [number, number]>, rows: number): Uint32Array {
  const out: number[] = [];
  for (const [row, col] of cells) {
    if (row < 0 || row >= rows) continue;
    const n = out.length;
    if (n > 0 && out[n - 3] === row && out[n - 1] === col - 1) out[n - 1] = col;
    else out.push(row, col, col);
  }
  return new Uint32Array(out);
}

/**
 * The widget's link state (#934): a viewport mirror of the frame stream for OSC 8 links, the
 * logical lines the {@link import("./links").LinkPort} answered for plain-text URLs, and the
 * {@link LinkController} both feed.
 *
 * A port answer is kept while the mirror still shows it cell for cell — every character in the cell
 * it names, every other cell of its rows blank — and while the rows it spans soft-wrap into each
 * other and the rows around it do not. A scroll op carries the answers with the rows. Answers are
 * checked when the pointer is over the grid, and on its return otherwise. Only pointer motion asks
 * the port — a frame drops or carries answers but never asks — so the questions follow the rows the
 * pointer crosses, whatever the output rate. One question is out at a time, a row is asked about at
 * most once per frame, and a newer answer replaces the ones sharing a row with it.
 */
export class LinkTracker {
  private mirror: CellMirror | undefined;
  private readonly controller: LinkController;
  private readonly regex: RegExp;
  private lines: CachedLine[] = [];
  private cell: readonly [number, number] | undefined;
  /** The cell of the press being followed, until it moves off it or is released. */
  private pressed: readonly [number, number] | undefined;
  /** Counts applied frames; a question is tagged with the count it was asked at. */
  private generation = 0;
  private asked: { row: number; generation: number } | undefined;
  private inflight = false;
  /** Whether the pointer moved since the last question — the one thing that asks the port. */
  private moved = false;
  /** Whether the cache has not been checked against the mirror since the last frame. */
  private unchecked = false;
  /** Whether the link sets are older than the mirror or the cache. */
  private stale = true;
  /** The event of the release being handled, for `onActivate`. */
  private releaseEv: MouseEventLike | undefined;
  private disposed = false;

  constructor(private readonly deps: LinkTrackerDeps) {
    this.regex = deps.options.regex ?? URL_REGEX;
    this.controller = new LinkController({
      onHover: (link) => deps.onHover(link),
      onLeave: () => deps.onLeave(),
      onActivate: (uri) => {
        if (this.releaseEv) deps.options.onActivate(uri, this.releaseEv);
      },
    });
  }

  /** The viewport height of the last applied frame. */
  get rows(): number {
    return this.mirror?.rows ?? 0;
  }

  /** Fold a frame into the mirror, carry the cached lines through its scroll op, and check them
   * against it when the pointer is over the grid. */
  applyFrame(frame: DecodedFrame): void {
    if (!this.mirror || frame.cols !== this.mirror.cols || frame.rows !== this.mirror.rows) {
      this.mirror = new CellMirror(frame.cols, frame.rows, this.deps.flagBits);
      this.lines = [];
    }
    this.mirror.applyFrame(frame);
    if (frame.hasScroll) this.scrollLines(frame.scrollTop!, frame.scrollBottom!, frame.scrollCount!);
    this.generation++;
    this.unchecked = true;
    this.stale = true;
    if (this.cell) {
      this.check();
      this.refresh();
    }
  }

  /** The pointer is over viewport cell `cell`, or over no cell whose press would act locally.
   * `ev` carries the modifiers the consumer's {@link LinkOptions.activates} gate reads. */
  pointer(cell: readonly [number, number] | undefined, ev?: MouseEventLike): void {
    const gated = cell !== undefined && ev !== undefined && this.deps.options.activates?.(ev) === false;
    this.cell = cell && !gated ? this.lead(cell) : undefined;
    if (!this.cell) {
      this.controller.pointerLeave();
      return;
    }
    this.check();
    if (this.stale) this.refresh();
    this.controller.pointerMove(this.cell[0], this.cell[1]);
    this.moved = true;
    this.ask();
  }

  /** A primary press that stays local, at `cell`. */
  press(cell: readonly [number, number] | undefined, ev: MouseEventLike): void {
    this.check();
    if (this.stale) this.refresh();
    const counts = cell && (this.deps.options.activates?.(ev) ?? true);
    this.pressed = cell && counts ? this.lead(cell) : undefined;
    // A press off the grid or refused by the gate still ends the previous one.
    if (this.pressed) this.controller.press(this.pressed[0], this.pressed[1]);
    else this.controller.press(-1, -1);
  }

  /** The press's pointer moved to `cell`. Leaving the pressed cell makes the gesture a selection. */
  drag(cell: readonly [number, number] | undefined): void {
    if (!this.pressed) return;
    const at = cell && this.lead(cell);
    if (at && at[0] === this.pressed[0] && at[1] === this.pressed[1]) return;
    this.pressed = undefined;
    this.controller.press(-1, -1);
  }

  /** The release of that press, at `cell`. */
  release(cell: readonly [number, number] | undefined, ev: MouseEventLike): void {
    this.check();
    if (this.stale) this.refresh();
    const at = cell && this.lead(cell);
    this.pressed = undefined;
    this.releaseEv = ev;
    try {
      if (at) this.controller.release(at[0], at[1]);
      else this.controller.release(-1, -1);
    } finally {
      this.releaseEv = undefined;
    }
  }

  /** Stop: an answer that lands later is dropped. */
  dispose(): void {
    this.disposed = true;
  }

  /** The lead cell of a wide pair for its trailing half; any other cell as is. */
  private lead(cell: readonly [number, number]): readonly [number, number] {
    const [row, col] = cell;
    return col > 0 && this.mirror?.isSpacer(row, col) ? [row, col - 1] : cell;
  }

  /** Drop the cached lines the mirror no longer shows, if a frame came since the last check. */
  private check(): void {
    if (!this.unchecked) return;
    this.unchecked = false;
    const kept = this.lines.filter((c) => this.isCurrent(c));
    if (kept.length !== this.lines.length) this.stale = true;
    this.lines = kept;
  }

  /** Move the cached lines with a whole-screen scroll op (`count` > 0 = up), into scrollback included.
   * A region scroll moves nothing here: the check drops what it moved away. */
  private scrollLines(top: number, bottom: number, count: number): void {
    if (top !== 0 || bottom !== this.rows - 1) return;
    this.lines = this.lines.map((c) => ({
      row: c.row - count,
      line: { text: c.line.text, cells: c.line.cells.map(([r, col]) => [r - count, col] as [number, number]) },
    }));
  }

  private refresh(): void {
    this.stale = false;
    const osc8 = this.mirror?.osc8Links() ?? [];
    const regex = this.lines.flatMap((c) => computeLinks(c.line, this.regex));
    this.controller.setLinks(osc8, regex);
  }

  /** Ask the port for the pointer row's line, if the pointer moved since the last question and the
   * row is not cached, out, or asked this frame. */
  private ask(): void {
    const port = this.deps.options.port;
    const cell = this.cell;
    if (!port || !cell || !this.moved || this.inflight || this.disposed) return;
    const row = cell[0];
    if (this.lines.some((c) => c.row === row || c.line.cells.some(([r]) => r === row))) return;
    if (this.asked?.row === row && this.asked.generation === this.generation) return;
    this.asked = { row, generation: this.generation };
    this.moved = false;
    this.inflight = true;
    const settle = (line: LogicalLine | undefined): void => {
      this.inflight = false;
      if (this.disposed) return;
      const cached = { row, line: line ?? NO_LINE };
      if (this.isCurrent(cached)) {
        const mine = new Set([row, ...cached.line.cells.map(([r]) => r)]);
        const shares = (c: CachedLine): boolean => mine.has(c.row) || c.line.cells.some(([r]) => mine.has(r));
        this.lines = [...this.lines.filter((c) => !shares(c)), cached];
        this.stale = true;
        if (this.cell) this.refresh(); // re-resolves the hover at the pointer
      }
      this.ask();
    };
    let answer: Promise<LogicalLine | undefined>;
    try {
      answer = port.lineAt(row);
    } catch {
      settle(undefined);
      return;
    }
    answer.then(settle, () => settle(undefined));
  }

  /** Whether the mirror still shows `c.line` (see the class doc), and `c.row` is inside it or blank. */
  private isCurrent(c: CachedLine): boolean {
    const mirror = this.mirror;
    if (!mirror) return false;
    const inView = (r: number): boolean => r >= 0 && r < mirror.rows;
    if (!inView(c.row)) return false;
    // What each viewport cell must show: the characters the line maps to it, in order.
    const shows = new Map<number, string>();
    const chars = [...c.line.text];
    c.line.cells.forEach(([r, col], i) => {
      if (!inView(r)) return;
      const key = r * mirror.cols + col;
      shows.set(key, (shows.get(key) ?? "") + chars[i]!);
    });
    const rows = new Set([c.row, ...c.line.cells.map(([r]) => r).filter(inView)]);
    for (const r of rows) {
      for (let col = 0; col < mirror.cols; col++) {
        const want = shows.get(r * mirror.cols + col);
        const got = mirror.symbolAt(r, col);
        if (want !== undefined ? got !== want : got !== " " && !mirror.isSpacer(r, col)) return false;
      }
    }
    if (c.line.cells.length === 0) return true;
    const first = Math.min(...c.line.cells.map(([r]) => r));
    const last = Math.max(...c.line.cells.map(([r]) => r));
    for (let r = Math.max(first, 0); r < Math.min(last, mirror.rows); r++) if (!mirror.wraps(r)) return false;
    if (inView(last) && mirror.wraps(last)) return false;
    if (inView(first - 1) && mirror.wraps(first - 1)) return false;
    return true;
  }
}
