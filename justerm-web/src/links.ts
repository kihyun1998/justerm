// Plain-text URL detection (#113 / ADR-0017 (ii)): the engine assembles the
// viewport's logical-line text + a per-char cell map (it has the whole buffer);
// the consumer — here — runs the URL regex and `new URL()` validation over that
// text and maps matches back through the cells. The policy (what a URL is) stays
// web-side; core has no regex dependency.

/** A viewport logical line from the engine: assembled text + per-char cell. */
export interface LogicalLine {
  text: string;
  /** Per `text` char, its viewport `[row, col]`. `row` outside `0..rows` is
   * off-screen wrapped context (the consumer highlights only in-range cells). */
  cells: ReadonlyArray<readonly [number, number]>;
}

/** A detected link: its URI and the viewport cells it covers. */
export interface Link {
  uri: string;
  cells: ReadonlyArray<readonly [number, number]>;
}

import type { DecodedFrame } from "./types";

/** `[line, left, right, cell_offset, count]` — the cell span directory stride. */
const SPAN_STRIDE = 5;

/** Explicit OSC 8 hyperlinks from the frame (source (a)): cells sharing a link
 * index are one link, its URI `linkTable[index - 1]`. Walks the span directory
 * the same way the renderer does, reading the per-cell `link` column.
 *
 * NB: a Partial frame ships only damaged spans, so a link's undamaged cells are
 * absent and its `cells` come out incomplete — the same partial-frame gap as the
 * highlight overlay (#140). Correct on Full frames; the consumer should run this
 * against a full-viewport frame (or the cell mirror once it carries links). */
export function osc8Links(frame: DecodedFrame): Link[] {
  const { spans, link, linkTable } = frame;
  if (!link || !linkTable) return [];
  // Group cells by their link index, preserving first-seen order.
  const byIndex = new Map<number, Array<readonly [number, number]>>();
  for (let s = 0; s < spans.length; s += SPAN_STRIDE) {
    const line = spans[s]!;
    const left = spans[s + 1]!;
    const cellOffset = spans[s + 3]!;
    const count = spans[s + 4]!;
    for (let k = 0; k < count; k++) {
      const index = link[cellOffset + k]!;
      if (index === 0) continue;
      let cells = byIndex.get(index);
      if (!cells) byIndex.set(index, (cells = []));
      cells.push([line, left + k]);
    }
  }
  const links: Link[] = [];
  for (const [index, cells] of byIndex) {
    links.push({ uri: linkTable[index - 1]!, cells });
  }
  return links;
}

/** xterm's strict http(s) URL regex: from `://` up to the first whitespace/quote
 * /unsafe char, with a trailing-punctuation guard (no final `,.!?` or brackets).
 * Source: `addon-web-links/src/WebLinksAddon.ts`. */
export const URL_REGEX = /(https?|HTTPS?):[/]{2}[^\s"'!*(){}|\\^<>`]*[^\s"':,.!?{}|\\^~[\]`()<>]/;

/** Whether `s` is a safe-to-linkify URL. Beyond `new URL()` parsing, the
 * displayed text must literally begin with the *normalized* origin (xterm's
 * guard) — this rejects homograph / punycode-IDN / octal-or-hex-IP hosts whose
 * normalized form differs from the glyphs shown, which would navigate somewhere
 * other than what the user reads. Source: xterm `WebLinkProvider.ts isUrl`. */
function isUrl(s: string): boolean {
  try {
    const url = new URL(s);
    const base =
      url.username && url.password
        ? `${url.protocol}//${url.username}:${url.password}@${url.host}`
        : url.username
          ? `${url.protocol}//${url.username}@${url.host}`
          : `${url.protocol}//${url.host}`;
    return s.toLowerCase().startsWith(base.toLowerCase());
  } catch {
    return false;
  }
}

/** Detect URLs in a logical line, mapping each back to its covering cells. The
 * regex is the consumer's policy; pass a custom one to override the default. */
export function computeLinks(line: LogicalLine, regex: RegExp = URL_REGEX): Link[] {
  // exec-loop needs the global flag to advance past each match.
  const rex = new RegExp(regex.source, regex.flags.includes("g") ? regex.flags : regex.flags + "g");
  const links: Link[] = [];
  let m: RegExpExecArray | null;
  while ((m = rex.exec(line.text)) !== null) {
    const uri = m[0];
    if (!isUrl(uri)) continue;
    // `cells` is one entry per code point (the engine pushes one per char), but
    // `m.index`/`uri.length` are UTF-16 code units — convert via code-point
    // counts so an astral char (emoji) before/in the URL doesn't shift the slice.
    const start = [...line.text.slice(0, m.index)].length;
    links.push({ uri, cells: line.cells.slice(start, start + [...uri].length) });
  }
  return links;
}

/**
 * Drives link hover/click against the current frame's links. Pure logic — no
 * DOM: the widget feeds it the pointer cell (mapped from pixels) and the link
 * sets; it fires hover/leave (underline + pointer cursor) and activate (open).
 * OSC 8 links take precedence over regex-detected ones on the same cell.
 */
export class LinkController {
  private links: Link[] = [];
  private hovered: Link | undefined;
  /** Last pointer cell, so a frame's `setLinks` can re-resolve the hover there. */
  private last: readonly [number, number] | undefined;
  private readonly onHover: (link: Link) => void;
  private readonly onLeave: () => void;
  private readonly onActivate: (uri: string) => void;

  constructor(opts: {
    onHover?: (link: Link) => void;
    onLeave?: () => void;
    onActivate?: (uri: string) => void;
  } = {}) {
    this.onHover = opts.onHover ?? (() => {});
    this.onLeave = opts.onLeave ?? (() => {});
    this.onActivate = opts.onActivate ?? (() => {});
  }

  /** Set the current links (e.g. each frame). OSC 8 first → precedence over regex
   * on a shared cell. Re-resolves the hover at the last pointer so a link that
   * appears/disappears under a stationary pointer fires hover/leave. */
  setLinks(osc8: Link[], regex: Link[]): void {
    this.links = [...osc8, ...regex];
    if (this.last) this.resolve(this.last[0], this.last[1]);
  }

  /** Pointer moved to viewport cell `(row, col)`. Fires hover/leave on change. */
  pointerMove(row: number, col: number): void {
    this.last = [row, col];
    this.resolve(row, col);
  }

  /** Transition the hover to the link (if any) at `(row, col)`. Compares by URI
   * so re-setting equal links across frames doesn't churn leave/hover. */
  private resolve(row: number, col: number): void {
    const link = this.linkAt(row, col);
    if (link?.uri === this.hovered?.uri) {
      this.hovered = link; // keep state fresh, no event
      return;
    }
    if (this.hovered) this.onLeave();
    this.hovered = link;
    if (link) this.onHover(link);
  }

  /** A click at viewport cell `(row, col)` — activates the link there, if any. */
  click(row: number, col: number): void {
    const link = this.linkAt(row, col);
    if (link) this.onActivate(link.uri);
  }

  private linkAt(row: number, col: number): Link | undefined {
    return this.links.find((l) => l.cells.some(([r, c]) => r === row && c === col));
  }
}
