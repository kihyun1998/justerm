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
 * A port answer is kept while the mirror still shows the text it describes, on every viewport row
 * it covers and on the row it was asked for. One question is out at a time, and a row is asked
 * about at most once per frame.
 */
export class LinkTracker {
  private mirror: CellMirror | undefined;
  private readonly controller: LinkController;
  private readonly regex: RegExp;
  private lines: CachedLine[] = [];
  private cell: readonly [number, number] | undefined;
  /** Counts applied frames; a question is tagged with the count it was asked at. */
  private generation = 0;
  private asked: { row: number; generation: number } | undefined;
  private inflight = false;
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

  /** Fold a frame into the mirror, and drop every cached line it contradicts. */
  applyFrame(frame: DecodedFrame): void {
    if (!this.mirror || frame.cols !== this.mirror.cols || frame.rows !== this.mirror.rows) {
      this.mirror = new CellMirror(frame.cols, frame.rows, this.deps.flagBits);
      this.lines = [];
    }
    this.mirror.applyFrame(frame);
    this.generation++;
    this.lines = this.lines.filter((c) => this.isCurrent(c));
    this.stale = true;
    if (this.cell) this.refresh();
    this.ask();
  }

  /** The pointer is over viewport cell `cell`, or over no cell whose press would act locally. */
  pointer(cell: readonly [number, number] | undefined): void {
    this.cell = cell;
    if (!cell) {
      this.controller.pointerLeave();
      return;
    }
    if (this.stale) this.refresh();
    this.controller.pointerMove(cell[0], cell[1]);
    this.ask();
  }

  /** A primary press that stays local, at `cell`. */
  press(cell: readonly [number, number] | undefined, ev: MouseEventLike): void {
    if (this.stale) this.refresh();
    const counts = cell && (this.deps.options.activates?.(ev) ?? true);
    // A press off the grid or refused by the gate still ends the previous one.
    if (counts) this.controller.press(cell[0], cell[1]);
    else this.controller.press(-1, -1);
  }

  /** The release of that press, at `cell`. */
  release(cell: readonly [number, number] | undefined, ev: MouseEventLike): void {
    if (this.stale) this.refresh();
    this.releaseEv = ev;
    try {
      if (cell) this.controller.release(cell[0], cell[1]);
      else this.controller.release(-1, -1);
    } finally {
      this.releaseEv = undefined;
    }
  }

  /** Stop: an answer that lands later is dropped. */
  dispose(): void {
    this.disposed = true;
  }

  private refresh(): void {
    this.stale = false;
    const osc8 = this.mirror?.osc8Links() ?? [];
    const regex = this.lines.flatMap((c) => computeLinks(c.line, this.regex));
    this.controller.setLinks(osc8, regex);
  }

  /** Ask the port for the pointer row's line, unless it is cached, out, or asked this frame. */
  private ask(): void {
    const port = this.deps.options.port;
    const cell = this.cell;
    if (!port || !cell || this.inflight || this.disposed) return;
    const row = cell[0];
    if (this.lines.some((c) => c.row === row || c.line.cells.some(([r]) => r === row))) return;
    if (this.asked?.row === row && this.asked.generation === this.generation) return;
    this.asked = { row, generation: this.generation };
    this.inflight = true;
    const settle = (line: LogicalLine | undefined): void => {
      this.inflight = false;
      if (this.disposed) return;
      const cached = { row, line: line ?? NO_LINE };
      if (this.isCurrent(cached)) {
        this.lines.push(cached);
        this.stale = true;
        if (this.cell) this.refresh(); // re-resolves the hover at the pointer
      }
      this.ask();
    };
    port.lineAt(row).then(settle, () => settle(undefined));
  }

  /** Whether the mirror still shows `c.line` on every viewport row it covers, and `c.row`. */
  private isCurrent(c: CachedLine): boolean {
    const mirror = this.mirror;
    if (!mirror) return false;
    const expected = new Map<number, string>([[c.row, ""]]);
    const chars = [...c.line.text];
    c.line.cells.forEach(([r], i) => {
      if (r < 0 || r >= mirror.rows) return; // off-screen context, not the mirror's to check
      expected.set(r, (expected.get(r) ?? "") + chars[i]!);
    });
    for (const [r, text] of expected) {
      if (r < 0 || r >= mirror.rows) return false;
      let shown = "";
      for (let col = 0; col < mirror.cols; col++) {
        if (!mirror.isSpacer(r, col)) shown += mirror.symbolAt(r, col);
      }
      // Core trims only U+0020 off a logical line's end, so only U+0020 may follow the text.
      if (!shown.startsWith(text) || !/^ *$/.test(shown.slice(text.length))) return false;
    }
    return true;
  }
}
