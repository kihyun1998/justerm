import { describe, expect, it } from "vitest";
import type { MouseEventLike } from "../src/input";
import { hoverSpans, LinkTracker } from "../src/link-tracker";
import type { Link, LinkOptions, LinkPort, LogicalLine } from "../src/links";
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

/** One row of a {@link frame}: `text` from column 0, padded to the width. A wide char takes two
 * columns (its spacer is written as codepoint 0); `wraps` sets the row's soft-wrap flag. */
interface Row {
  text: string;
  link?: number;
  wraps?: boolean;
}

const WIDE = new Set(["한"]);

/** A frame writing each listed row whole, with an optional scroll op applied before it. */
function frame(
  kind: number,
  rows: Record<number, Row>,
  linkTable: string[] = [],
  scroll?: { top: number; bottom: number; count: number },
): DecodedFrame {
  const codepoints: number[] = [];
  const flags: number[] = [];
  const link: number[] = [];
  const spans: number[] = [];
  for (const [line, r] of Object.entries(rows)) {
    spans.push(Number(line), 0, COLS - 1, codepoints.length, COLS);
    const start = codepoints.length;
    for (const c of r.text) {
      const l = c === " " ? 0 : (r.link ?? 0);
      codepoints.push(c.codePointAt(0)!);
      flags.push(WIDE.has(c) ? F.wide_char : 0);
      link.push(l);
      if (WIDE.has(c)) {
        codepoints.push(0);
        flags.push(F.wide_char_spacer);
        link.push(l);
      }
    }
    while (codepoints.length - start < COLS) {
      codepoints.push(0x20);
      flags.push(0);
      link.push(0);
    }
    if (r.wraps) flags[start + COLS - 1]! |= F.wrapline;
  }
  const n = codepoints.length;
  return {
    cols: COLS,
    rows: ROWS,
    kind,
    codepoints,
    fg: new Array(n).fill(0),
    bg: new Array(n).fill(0),
    flags,
    extra: new Array(n).fill(0),
    spans,
    sideTable: [],
    link,
    linkTable,
    ...(scroll ? { hasScroll: true, scrollTop: scroll.top, scrollBottom: scroll.bottom, scrollCount: scroll.count } : {}),
  } as DecodedFrame;
}

/** The logical line core would report for `texts` written from column 0 of `row` on, one row each,
 * each but the last soft-wrapped into the next — spacers skipped, as core skips them. */
function wrapped(row: number, ...texts: string[]): LogicalLine {
  const cells: Array<[number, number]> = [];
  let text = "";
  texts.forEach((t, i) => {
    let col = 0;
    for (const c of t) {
      text += c;
      cells.push([row + i, col]);
      col += WIDE.has(c) ? 2 : 1;
    }
  });
  return { text: text.replace(/ +$/, ""), cells: cells.slice(0, [...text.replace(/ +$/, "")].length) };
}

/** The logical line core would report for a single unwrapped row holding `text`. */
function lineOf(row: number, text: string): LogicalLine {
  return wrapped(row, text);
}

/** A port whose answers the test releases by hand. */
class HeldPort implements LinkPort {
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

  it("discards an answer the screen has moved past, and asks again when the pointer moves", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);

    t.applyFrame(frame(1, { 1: { text: "rewritten" } })); // lands while the question is out
    await port.answer(lineOf(1, url));
    expect(hovers).toEqual([]);
    expect(port.asked).toEqual([1]);
    t.pointer([1, 7]);

    expect(port.asked).toEqual([1, 1]);
  });

  it("a frame never asks, however much output streams under a resting pointer", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);
    await port.answer(lineOf(1, url));

    for (let i = 0; i < 50; i++) {
      t.applyFrame(frame(1, { 2: { text: `line ${i} http://c${i}.io` } }, [], { top: 0, bottom: ROWS - 1, count: 1 }));
    }

    expect(port.asked).toEqual([1]);
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
    t.pointer([1, 7]);

    expect(port.asked).toEqual([1, 1]);
  });

  it("asks again once the cached line's row gains text past its end", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);
    await port.answer(lineOf(1, url));

    t.applyFrame(frame(1, { 1: { text: `${url}/x` } })); // the URL grew
    t.pointer([1, 7]);

    expect(port.asked).toEqual([1, 1]);
  });

  it("an answer the screen never showed is asked again only after a frame and a motion", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6]);

    await port.answer(lineOf(1, "something else"));
    t.pointer([1, 7]);
    expect(port.asked).toEqual([1]); // same row, same frame

    t.applyFrame(frame(1, { 0: { text: "tick" } }));
    expect(port.asked).toEqual([1]);
    t.pointer([1, 8]);
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
    t.pointer([1, 7]);
    expect(port.asked).toEqual([1]);
    // A taller grid still showing the same row: only the resize can make it ask again.
    t.applyFrame({ ...frame(0, { 1: { text: url } }), rows: ROWS + 1 } as DecodedFrame);
    t.pointer([1, 8]);

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

describe("LinkTracker — what keeps a port answer current (#934 check pass)", () => {
  const FULL = "https://ex.com/aaaaa"; // exactly COLS wide

  it("a cached line whose row starts to wrap is asked again, and the longer answer is the link", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: FULL } }));
    t.pointer([1, 10], ev());
    await port.answer(lineOf(1, FULL));

    // The next keystroke goes to row 2; row 1's text is unchanged and it now soft-wraps.
    t.applyFrame(frame(1, { 1: { text: FULL, wraps: true }, 2: { text: "bbb" } }));
    t.pointer([1, 11], ev());
    expect(port.asked).toEqual([1, 1]);
    await port.answer(wrapped(1, FULL, "bbb"));

    expect((hovers.at(-1) as Link).uri).toBe(`${FULL}bbb`);
  });

  it("a cached line whose next row stopped continuing it is asked again", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: FULL, wraps: true }, 2: { text: "bbb" } }));
    t.pointer([1, 10], ev());
    await port.answer(wrapped(1, FULL, "bbb"));

    t.applyFrame(frame(1, { 1: { text: FULL }, 2: { text: "bbb" } })); // same text, no wrap any more
    t.pointer([1, 11], ev());

    expect(port.asked).toEqual([1, 1]);
  });

  it("a cached line that the row above now wraps into is asked again", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: "http://b.io" } }));
    t.pointer([1, 4], ev());
    await port.answer(lineOf(1, "http://b.io"));

    // Row 0 fills and soft-wraps into row 1, whose text is unchanged: row 1 is no longer a line's start.
    t.applyFrame(frame(1, { 0: { text: "x".repeat(COLS), wraps: true } }));
    t.pointer([1, 5], ev());

    expect(port.asked).toEqual([1, 1]);
  });

  it("an answer whose cells do not sit where its text is shown is dropped", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 0: { text: "  see http://b.io" } }));
    t.pointer([0, 8], ev());

    // Right text, but mapped one column left of where the screen shows it.
    const line = lineOf(0, "  see http://b.io");
    await port.answer({ text: line.text, cells: line.cells.map(([r, c]) => [r, c - 1] as [number, number]) });

    expect(hovers).toEqual([]);
  });

  it("a short answer that misses its row's wrap is not kept, and the whole line is", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    t.applyFrame(frame(0, { 1: { text: FULL, wraps: true }, 2: { text: "bbb" } }));
    t.pointer([1, 10], ev());
    await port.answer(lineOf(1, FULL)); // a stale, short answer that still matches the text…
    expect(hovers).toEqual([]); // …but not the wrap, so it is not kept
    t.applyFrame(frame(1, { 0: { text: "tick" } }));
    t.pointer([1, 11], ev());
    await port.answer(wrapped(1, FULL, "bbb"));

    expect((hovers.at(-1) as Link).uri).toBe(`${FULL}bbb`);
  });

  it("a scroll carries the cached line with it, without asking again", async () => {
    const port = new HeldPort();
    const { t, hovers } = tracker({ port });
    const url = "see http://b.io";
    t.applyFrame(frame(0, { 2: { text: url } }));
    t.pointer([2, 6], ev());
    await port.answer(lineOf(2, url));

    // Output scrolls the whole screen up a row: the line is now on row 1, and so is the pointer.
    t.applyFrame(frame(1, { 2: { text: "" } }, [], { top: 0, bottom: ROWS - 1, count: 1 }));
    t.pointer([1, 6], ev());

    expect(port.asked).toEqual([2]);
    expect((hovers.at(-1) as Link).cells[0]).toEqual([1, 4]);
  });

  it("a line that went stale while the pointer was away is asked again when it returns", async () => {
    const port = new HeldPort();
    const { t } = tracker({ port });
    const url = "see http://b.io";
    t.applyFrame(frame(0, { 1: { text: url } }));
    t.pointer([1, 6], ev());
    await port.answer(lineOf(1, url));
    t.pointer(undefined);

    t.applyFrame(frame(1, { 1: { text: "gone" } }));
    expect(port.asked).toEqual([1]);
    t.pointer([1, 2], ev());

    expect(port.asked).toEqual([1, 1]);
  });

  it("the trailing half of a wide char inside a URL is part of the link", async () => {
    const port = new HeldPort();
    const { t, hovers, opened } = tracker({ port });
    const url = "https://a.org/한x"; // 한 on columns 14-15
    t.applyFrame(frame(0, { 0: { text: url } }));
    t.pointer([0, 15], ev());
    await port.answer(wrapped(0, url));

    expect((hovers.at(-1) as Link).uri).toBe(url);
    t.press([0, 15], ev());
    t.release([0, 15], ev());
    expect(opened.map(([u]) => u)).toEqual([url]);
  });

  it("a port that throws is an answer of no line, not a stuck question", () => {
    let calls = 0;
    const { t } = tracker({
      port: {
        lineAt: () => {
          calls++;
          throw new Error("ipc down");
        },
      },
    });
    t.applyFrame(frame(0, { 0: { text: "x" } }));

    expect(() => t.pointer([0, 0], ev())).not.toThrow();
    t.applyFrame(frame(1, { 0: { text: "y" } }));
    t.pointer([0, 1], ev());

    expect(calls).toBe(2);
  });
});

describe("LinkTracker — press, drag and the consumer's gate (#934 check pass)", () => {
  it("a press that moves off its cell before the release opens nothing", () => {
    const { t, opened } = tracker();
    t.applyFrame(frame(0, { 0: { text: "docs", link: 1 } }, ["http://a.io"]));

    t.press([0, 0], ev());
    t.drag([0, 2]);
    t.release([0, 2], ev());

    expect(opened).toEqual([]);
  });

  it("a drag that stays on the pressed cell still opens", () => {
    const { t, opened } = tracker();
    t.applyFrame(frame(0, { 0: { text: "docs", link: 1 } }, ["http://a.io"]));

    t.press([0, 1], ev());
    t.drag([0, 1]);
    t.release([0, 1], ev());

    expect(opened.map(([u]) => u)).toEqual(["http://a.io"]);
  });

  it("hover asks the consumer's gate the press asks", () => {
    const { t, hovers } = tracker({ activates: (e) => e.ctrlKey });
    t.applyFrame(frame(0, { 0: { text: "docs", link: 1 } }, ["http://a.io"]));

    t.pointer([0, 1], ev());
    expect(hovers).toEqual([]);
    t.pointer([0, 1], ev({ ctrlKey: true }));

    expect((hovers.at(-1) as Link).uri).toBe("http://a.io");
  });
});
