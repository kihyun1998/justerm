import { computeLinks, LinkController, osc8Links } from "../src/links";
import type { LogicalLine } from "../src/links";
import type { DecodedFrame } from "../src/types";

// A frame stripped to the fields osc8Links reads (span directory + per-cell link
// index + the URI table).
function linkFrame(over: Partial<DecodedFrame>): DecodedFrame {
  return {
    cols: 80,
    rows: 24,
    kind: 0,
    codepoints: [],
    fg: [],
    bg: [],
    flags: [],
    extra: [],
    spans: [],
    sideTable: [],
    ...over,
  };
}

// A single-row logical line: each char maps to (row 0, its column).
function row(text: string): LogicalLine {
  return { text, cells: [...text].map((_, i) => [0, i] as [number, number]) };
}

describe("computeLinks — URL regex over a logical line", () => {
  // The ADR-0017 (ii) payoff: core hands web the assembled line text + a cell
  // map; web runs the URL regex and maps the match back through the cells. The
  // trailing "!" is excluded by the regex's final-char rule.
  it("finds a URL and maps it to its covering cells", () => {
    const line = row("go http://a.co!");

    const links = computeLinks(line);

    expect(links.length).toBe(1);
    expect(links[0]!.uri).toBe("http://a.co");
    expect(links[0]!.cells).toEqual([
      [0, 3], [0, 4], [0, 5], [0, 6], [0, 7], [0, 8],
      [0, 9], [0, 10], [0, 11], [0, 12], [0, 13],
    ]);
  });

  // Several URLs on one line each become a link, in order.
  it("finds multiple URLs in one line", () => {
    const links = computeLinks(row("a http://x.io b https://y.io c"));

    expect(links.map((l) => l.uri)).toEqual(["http://x.io", "https://y.io"]);
  });

  // A URL that wrapped across rows: the cell map carries each char's real row,
  // so the link's cells span both rows (the consumer highlights both). This is
  // the buffer-wide payoff — web could not assemble this itself.
  it("maps a URL wrapping across rows to cells on both rows", () => {
    const text = "http://ab.io";
    // first 6 chars on row 0, the rest on row 1
    const cells = [...text].map((_, i) => (i < 6 ? [0, i] : [1, i - 6]) as [number, number]);

    const [link] = computeLinks({ text, cells });

    expect(link!.uri).toBe("http://ab.io");
    expect(link!.cells).toEqual([
      [0, 0], [0, 1], [0, 2], [0, 3], [0, 4], [0, 5], // row 0
      [1, 0], [1, 1], [1, 2], [1, 3], [1, 4], [1, 5], // row 1
    ]);
  });

  // Plain prose with no URL yields nothing.
  it("returns no links when there is no URL", () => {
    expect(computeLinks(row("just some words here"))).toEqual([]);
  });
});

describe("osc8Links — explicit OSC 8 hyperlinks from the frame", () => {
  // Cells carrying the same link index are one hyperlink; the URI comes from
  // linkTable[index - 1]. (a) of the two link sources — explicit, from the VT
  // stream, already on the wire.
  it("groups same-index linked cells into a link with its URI", () => {
    const frame = linkFrame({
      // one span: row 0, cols 2..4 (3 cells), all link index 1
      spans: [0, 2, 4, 0, 3],
      link: [1, 1, 1],
      linkTable: ["https://a.co"],
    });

    expect(osc8Links(frame)).toEqual([
      { uri: "https://a.co", cells: [[0, 2], [0, 3], [0, 4]] },
    ]);
  });

  // Unlinked cells (index 0) contribute nothing; a frame with no link table is
  // empty.
  it("ignores unlinked cells and link-less frames", () => {
    expect(osc8Links(linkFrame({ spans: [0, 0, 1, 0, 2], link: [0, 0] }))).toEqual([]);
    expect(osc8Links(linkFrame({ spans: [0, 0, 0, 0, 1] }))).toEqual([]);
  });
});

describe("LinkController — hover / leave / activate", () => {
  type Ev = [string, string?];
  function controller(events: Ev[]) {
    return new LinkController({
      onHover: (l) => events.push(["hover", l.uri]),
      onLeave: () => events.push(["leave"]),
      onActivate: (uri) => events.push(["activate", uri]),
    });
  }

  it("hovers the link under the pointer and leaves when it moves off", () => {
    const events: Ev[] = [];
    const ctrl = controller(events);
    ctrl.setLinks([], [{ uri: "http://x.io", cells: [[0, 2], [0, 3]] }]);

    ctrl.pointerMove(0, 2); // onto the link
    ctrl.pointerMove(0, 3); // still on it — no new event
    ctrl.pointerMove(0, 5); // off it

    expect(events).toEqual([["hover", "http://x.io"], ["leave"]]);
  });
});

describe("LinkController — activate + precedence", () => {
  type Ev = [string, string?];
  function controller(events: Ev[]) {
    return new LinkController({
      onHover: (l) => events.push(["hover", l.uri]),
      onLeave: () => events.push(["leave"]),
      onActivate: (uri) => events.push(["activate", uri]),
    });
  }

  it("activates the link's URI on click, nothing off a link", () => {
    const events: Ev[] = [];
    const ctrl = controller(events);
    ctrl.setLinks([], [{ uri: "http://x.io", cells: [[0, 2]] }]);

    ctrl.click(0, 2);
    ctrl.click(0, 9); // no link here

    expect(events).toEqual([["activate", "http://x.io"]]);
  });

  it("prefers an OSC 8 link over a regex link on the same cell", () => {
    const events: Ev[] = [];
    const ctrl = controller(events);
    ctrl.setLinks(
      [{ uri: "https://osc8.real", cells: [[0, 2]] }], // explicit
      [{ uri: "http://regex.io", cells: [[0, 2]] }], // detected, same cell
    );

    ctrl.pointerMove(0, 2);

    expect(events).toEqual([["hover", "https://osc8.real"]]);
  });

  it("leaves the old link and hovers the new on a direct move between links", () => {
    const events: Ev[] = [];
    const ctrl = controller(events);
    ctrl.setLinks([], [
      { uri: "http://a.io", cells: [[0, 0]] },
      { uri: "http://b.io", cells: [[0, 1]] },
    ]);

    ctrl.pointerMove(0, 0);
    ctrl.pointerMove(0, 1);

    expect(events).toEqual([["hover", "http://a.io"], ["leave"], ["hover", "http://b.io"]]);
  });
});

describe("computeLinks — Unicode alignment (adversarial)", () => {
  // cells are one-per-code-point (the engine pushes one per Rust char), but
  // regex m.index/length are UTF-16 code units. An astral char (emoji) before a
  // URL is 2 units / 1 cell — the slice must convert to code points or it shifts.
  it("aligns cells to code points when an emoji precedes the URL", () => {
    const text = "😀 http://a.co";
    const cells = [...text].map((_, i) => [0, i] as [number, number]); // codepoint-indexed, like core

    const [link] = computeLinks({ text, cells });

    expect(link!.uri).toBe("http://a.co");
    expect(link!.cells).toEqual([
      [0, 2], [0, 3], [0, 4], [0, 5], [0, 6], [0, 7],
      [0, 8], [0, 9], [0, 10], [0, 11], [0, 12],
    ]);
  });
});

describe("LinkController — setLinks lifecycle (adversarial)", () => {
  type Ev = [string, string?];
  const controller = (events: Ev[]) =>
    new LinkController({
      onHover: (l) => events.push(["hover", l.uri]),
      onLeave: () => events.push(["leave"]),
    });

  // A new frame that removes the link under a stationary pointer must fire leave
  // (else the consumer's underline/cursor sticks). Regression for setLinks
  // nulling `hovered` without the leave transition.
  it("fires leave when a frame removes the link under the pointer", () => {
    const events: Ev[] = [];
    const ctrl = controller(events);
    ctrl.setLinks([], [{ uri: "http://x.io", cells: [[0, 2]] }]);
    ctrl.pointerMove(0, 2); // hover

    ctrl.setLinks([], []); // link gone on the new frame

    expect(events).toEqual([["hover", "http://x.io"], ["leave"]]);
  });

  // A frame that keeps the same link under the pointer does not churn leave/hover.
  it("does not re-fire while the same link persists across frames", () => {
    const events: Ev[] = [];
    const ctrl = controller(events);
    ctrl.setLinks([], [{ uri: "http://x.io", cells: [[0, 2]] }]);
    ctrl.pointerMove(0, 2); // hover

    ctrl.setLinks([], [{ uri: "http://x.io", cells: [[0, 2]] }]); // same link, new objects

    expect(events).toEqual([["hover", "http://x.io"]]); // no extra leave/hover
  });
});

describe("computeLinks — homograph/spoof guard (adversarial, xterm parity)", () => {
  // new URL() normalizes a Cyrillic-homograph or octal-IP host to a *different*
  // punycode/decimal host than the glyphs shown — a spoof. xterm rejects these
  // by requiring the displayed text to start with the normalized origin; a bare
  // `new URL` would linkify them. (Cyrillic а/р in "раypal".)
  it("rejects a homograph host whose normalized form differs from the text", () => {
    expect(computeLinks(row("see http://раypal.com now"))).toEqual([]);
  });

  it("rejects an octal/hex IP that normalizes to a different host", () => {
    expect(computeLinks(row("http://0x7f.1/admin"))).toEqual([]);
  });

  // A normal URL (and one with userinfo) still passes the guard.
  it("still accepts a plain URL and a userinfo URL", () => {
    expect(computeLinks(row("http://ok.com/x")).map((l) => l.uri)).toEqual(["http://ok.com/x"]);
    expect(computeLinks(row("http://u@ok.com/x")).map((l) => l.uri)).toEqual(["http://u@ok.com/x"]);
  });
});
