import { describe, expect, it } from "vitest";
import type { MouseEventLike } from "../src/input";
import { hoverSpans, LinkTracker } from "../src/link-tracker";
import type { Link, LinkOptions, LogicalLine } from "../src/links";
import type { DecodedFrame, FlagBits } from "../src/types";

const F: FlagBits = {
  bold: 0x01,
  italic: 0x02,
  underline: 0x04,
  strikethrough: 0x08,
  wide_char_spacer: 0x100,
  inverse: 0x200,
  dim: 0x400,
  hidden: 0x800,
  blink: 0x10,
  wide_char: 0x1000,
  wrapline: 0x2000,
};

const COLS = 20;
const ROWS = 3;

/** A frame writing `text` at the start of each listed row (padded to the width), `link` per row. */
function frame(kind: number, rows: Record<number, { text: string; link?: number }>, linkTable: string[] = []): DecodedFrame {
  const codepoints: number[] = [];
  const link: number[] = [];
  const spans: number[] = [];
  for (const [line, r] of Object.entries(rows)) {
    const chars = [...r.text.padEnd(COLS)];
    spans.push(Number(line), 0, COLS - 1, codepoints.length, COLS);
    for (const c of chars) {
      codepoints.push(c.codePointAt(0)!);
      link.push(c === " " ? 0 : (r.link ?? 0));
    }
  }
  const n = codepoints.length;
  return {
    cols: COLS,
    rows: ROWS,
    kind,
    codepoints,
    fg: new Array(n).fill(0),
    bg: new Array(n).fill(0),
    flags: new Array(n).fill(0),
    extra: new Array(n).fill(0),
    spans,
    sideTable: [],
    link,
    linkTable,
  } as DecodedFrame;
}

/** The logical line core would report for a single unwrapped row holding `text`. */
function lineOf(row: number, text: string): LogicalLine {
  return { text, cells: [...text].map((_, c) => [row, c] as [number, number]) };
}

/** A port whose answers the test releases by hand. */
class HeldPort {
  readonly asked: number[] = [];
  private pending: Array<(line: LogicalLine | undefined) => void> = [];
  lineAt(row: number): Promise<LogicalLine | undefined> {
    this.asked.push(row);
    return new Promise((resolve) => this.pending.push(resolve));
  }
  async answer(line: LogicalLine | undefined): Promise<void> {
    this.pending.shift()!(line);
    await Promise.resolve();
    await Promise.resolve();
  }
}

const ev = (over: Partial<MouseEventLike> = {}): MouseEventLike =>
  ({ clientX: 0, clientY: 0, button: 0, buttons: 0, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...over }) as MouseEventLike;

function tracker(options: Partial<LinkOptions> = {}) {
  const hovers: Array<Link | "leave"> = [];
  const opened: Array<[string, MouseEventLike]> = [];
  const t = new LinkTracker({
    flagBits: F,
    options: { onActivate: (uri, e) => opened.push([uri, e]), ...options },
    onHover: (l) => hovers.push(l),
    onLeave: () => hovers.push("leave"),
  });
  return { t, hovers, opened };
}

describe("hoverSpans (#934)", () => {
  it("runs each row's consecutive columns into one span and drops rows off the viewport", () => {
    const cells: Array<[number, number]> = [
      [-1, 7], [-1, 8], // wrapped in from above the top
      [0, 3], [0, 4], [0, 5], [0, 9], // a gap splits the row
      [1, 0], [1, 1],
      [3, 0], // past the bottom of a 3-row viewport
    ];

    expect([...hoverSpans(cells, 3)]).toEqual([0, 3, 5, 0, 9, 9, 1, 0, 1]);
  });
});

describe("LinkTracker — OSC 8 links from the frame stream (#934)", () => {
  it("hovers a link whole after a Partial frame repainted part of it", () => {
    const { t, hovers } = tracker();
    t.applyFrame(frame(0, { 1: { text: "docs", link: 1 } }, ["http://a.io"]));
    t.applyFrame(frame(1, { 0: { text: "prompt" } }));

    t.pointer([1, 2]);

    expect(hovers).toEqual([{ uri: "http://a.io", cells: [[1, 0], [1, 1], [1, 2], [1, 3]] }]);
  });

  it("opens the link on a press and release on it, passing the release's event", () => {
    const { t, opened } = tracker();
    t.applyFrame(frame(0, { 0: { text: "docs", link: 1 } }, ["http://a.io"]));
    const up = ev({ ctrlKey: true });

    t.press([0, 0], ev());
    t.release([0, 3], up);

    expect(opened).toEqual([["http://a.io", up]]);
  });

  it("a press the consumer's gate refuses opens nothing", () => {
    const { t, opened } = tracker({ activates: (e) => e.ctrlKey });
    t.applyFrame(frame(0, { 0: { text: "docs", link: 1 } }, ["http://a.io"]));

    t.press([0, 0], ev());
    t.release([0, 0], ev());

    expect(opened).toEqual([]);
  });

  it("a pointer that stops counting drops the hover", () => {
    const { t, hovers } = tracker();
    t.applyFrame(frame(0, { 0: { text: "docs", link: 1 } }, ["http://a.io"]));
    t.pointer([0, 0]);

    t.pointer(undefined);

    expect(hovers.at(-1)).toBe("leave");
  });
});

describe("LinkTracker — plain-text URLs through the port (#934)", () => {
  const url = "see http://b.io";

  it("asks for the hovered row's line once and hovers the URL in it", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));

    t.pointer([1, 6]);
    t.pointer([1, 7]);
    await port.answer(lineOf(1, url));
    t.applyFrame(frame(1, { 0: { text: "unrelated" } }));

    expect(port.asked).toEqual([1]);
    expect(hovers).toHaveLength(1);
    expect((hovers[0] as Link).uri).toBe("http://b.io");
  });

  it("discards an answer the screen has moved past, and asks again", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);

    t.applyFrame(frame(1, { 1: { text: "rewritten" } })); // lands while the question is out
    await port.answer(lineOf(1, url));

    expect(hovers).toEqual([]);
    expect(port.asked).toEqual([1, 1]);
  });

  it("keeps one question out at a time", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 0: { text: "a" }, 1: { text: "b" }, 2: { text: "c" } }));

    t.pointer([0, 0]);
    t.pointer([1, 0]);
    t.pointer([2, 0]);
    expect(port.asked).toEqual([0]);

    await port.answer(lineOf(0, "a"));
    expect(port.asked).toEqual([0, 2]);
  });

  it("does not re-ask a blank row on every frame", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 0: { text: "x" } }));
    t.pointer([2, 3]);
    await port.answer(undefined);

    t.applyFrame(frame(1, { 0: { text: "y" } }));
    t.applyFrame(frame(1, { 0: { text: "z" } }));

    expect(port.asked).toEqual([2]);
  });

  it("asks again once the cached line's row changes", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);
    await port.answer(lineOf(1, url));

    t.applyFrame(frame(1, { 1: { text: "gone" } }));

    expect(port.asked).toEqual([1, 1]);
  });

  it("asks again once the cached line's row gains text past its end", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);
    await port.answer(lineOf(1, url));

    t.applyFrame(frame(1, { 1: { text: `${url}/x` } })); // the URL grew

    expect(port.asked).toEqual([1, 1]);
  });

  it("an answer the screen never showed is not asked again until the next frame", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);

    await port.answer(lineOf(1, "something else"));
    expect(port.asked).toEqual([1]);

    t.applyFrame(frame(1, { 0: { text: "tick" } }));
    expect(port.asked).toEqual([1, 1]);
  });

  it("runs the consumer's pattern", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port, regex: /https?:\/\/[a-z.]+/ });
    t.applyFrame(frame(0, { 0: { text: "go http://c.io/x" } }));
    t.pointer([0, 4]);

    await port.answer(lineOf(0, "go http://c.io/x"));

    expect((hovers[0] as Link).uri).toBe("http://c.io");
  });

  it("a resize forgets every cached line", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);
    await port.answer(lineOf(1, url));

    t.applyFrame({ ...frame(0, { 1: { text: url } }), cols: COLS } as DecodedFrame); // same size
    expect(port.asked).toEqual([1]);
    // A taller grid still showing the same row: only the resize can make it ask again.
    t.applyFrame({ ...frame(0, { 1: { text: url } }), rows: ROWS + 1 } as DecodedFrame);

    expect(port.asked).toEqual([1, 1]);
  });

  it("ignores an answer that lands after dispose", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);

    t.dispose();
    await port.answer(lineOf(1, url));

    expect(hovers).toEqual([]);
  });
});
