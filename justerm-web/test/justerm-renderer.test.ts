import { describe, expect, it } from "vitest";
import type { DecorationRect } from "../src/decorations";
import {
  asU32,
  cursorCommand,
  damageHeader,
  decorationWire,
  gridForBox,
} from "../src/justerm-renderer";
import type { DecodedFrame } from "../src/types";

/** Minimal decoded frame: the `apply_damage`/cursor fields the adapter reads, defaulted so a test
 * overrides only what it exercises. */
const frame = (over: Partial<DecodedFrame> = {}): DecodedFrame =>
  ({
    kind: 0,
    cols: 4,
    rows: 1,
    codepoints: new Uint32Array(4),
    fg: new Uint32Array(4),
    bg: new Uint32Array(4),
    flags: new Uint16Array(4),
    extra: new Uint16Array(4),
    spans: new Uint32Array(0),
    sideTable: [],
    ...over,
  }) as DecodedFrame;

const NO_REF = 0xffffffff >>> 0;

describe("damageHeader", () => {
  it("packs [cols, rows, kind, hasScroll, top, bottom, count, blinkOn]", () => {
    const h = damageHeader(frame({ cols: 80, rows: 24, kind: 1 }));
    expect(Array.from(h)).toEqual([80, 24, 1, 0, 0, 0, 0, 1]);
    expect(h).toBeInstanceOf(Uint32Array);
    expect(h.length).toBe(8);
  });

  it("sets the scroll flag + fields only when all three scroll fields are present and non-zero", () => {
    const withScroll = damageHeader(frame({ scrollTop: 2, scrollBottom: 10, scrollCount: 3 }));
    expect(withScroll[3]).toBe(1);
    expect(Array.from(withScroll.slice(4, 7))).toEqual([2, 10, 3]);

    // A zero count is "no scroll" — the flag must stay 0 so the renderer skips the shift.
    expect(damageHeader(frame({ scrollTop: 2, scrollBottom: 10, scrollCount: 0 }))[3]).toBe(0);
    // A partial scroll triple (count present, bounds absent) is not a scroll.
    expect(damageHeader(frame({ scrollCount: 3 }))[3]).toBe(0);
  });

  it("carries a negative scrollCount as a u32 the renderer reads back as i32", () => {
    // A downward shift is negative; the Uint32Array slot holds its two's-complement, and the
    // renderer's `header[6] as i32 as i16` recovers -1. Guards the wire, not just the sign.
    const h = damageHeader(frame({ scrollTop: 0, scrollBottom: 5, scrollCount: -1 }));
    const slot = h[6] ?? 0;
    expect(slot).toBe(0xffffffff);
    expect(slot | 0).toBe(-1);
  });

  it("passes blinkOn=false through as 0", () => {
    expect(damageHeader(frame(), false)[7]).toBe(0);
  });
});

// #467 (the #457 class, one seam over): `asU32`'s plain-array fallback (`Uint32Array.from`)
// silently REINTERPRETS an out-of-range value rather than rejecting it — the same invisibility
// #457 pinned for the decoration wire. Documenting it here means a future span source (a second
// search backend, a host hand-building frames) cannot reintroduce the wrap while believing this
// coercion would catch it. Production is unaffected: selection/match/activeMatch spans arrive from
// the decoder as a real `Uint32Array` and take the identity fast path (asserted below). The
// deliberate choice NOT to reject here (a per-frame coercion knows nothing of a value's meaning or
// geometry — the producer owns validity) is recorded on #467; this test would break the day someone
// changes the fallback to reject/clip, forcing that decision to be conscious.
describe("asU32 span coercion (#467)", () => {
  it("passes a real Uint32Array through by reference — the decoder fast path, no copy", () => {
    const real = new Uint32Array([1, 2, 3]);
    expect(asU32(real)).toBe(real); // same reference; a copy would defeat the per-frame fast path
  });

  it("silently reinterprets an out-of-range value in the plain-array fallback", () => {
    expect(asU32([-5])[0], "a negative span value wraps to two's-complement").toBe(0xfffffffb);
    expect(asU32([Number.NaN])[0], "NaN lands as a plausible 0, not an error").toBe(0);
    expect(asU32([Number.POSITIVE_INFINITY])[0], "Infinity lands as 0, not an error").toBe(0);
    expect(asU32([2 ** 32 + 3])[0], "a value >= 2**32 wraps mod 2**32").toBe(3);
  });
});

describe("decorationWire", () => {
  it("is empty for no rects", () => {
    expect(decorationWire([]).length).toBe(0);
  });

  it("flattens stride-6 with layer bottom=0 / top=1 and absolute colours verbatim", () => {
    const rects: DecorationRect[] = [
      { row: 1, left: 2, right: 4, layer: "bottom", bg: 0x804000, fg: 0x00ff88 },
      { row: 0, left: 0, right: 0, layer: "top", bg: 0x006080, fg: 0x112233 },
    ];
    expect(Array.from(decorationWire(rects))).toEqual([
      1, 2, 4, 0, 0x804000, 0x00ff88, //
      0, 0, 0, 1, 0x006080, 0x112233,
    ]);
  });

  // #457 (2-lens GAP 4): pin WHY `decorationsForFrame` clips, at the seam that forces
  // it. This lane is a `Uint32Array`, so it cannot represent an out-of-range column —
  // it silently reinterprets one. Documenting that here means a future rect source
  // (a second decoration source, or the row lane) cannot reintroduce the wrap while
  // believing the wire would catch it.
  it("silently reinterprets an out-of-range column — the reason spans are clipped upstream", () => {
    const bad = decorationWire([
      { row: 0, left: -5, right: 19, layer: "bottom", bg: 0x008f00 },
    ]);
    expect(bad[1], "a negative column wraps, so `col >= left` matches nothing").toBe(0xfffffffb);

    const nan = decorationWire([
      { row: 0, left: Number.NaN, right: Number.NaN, layer: "bottom", bg: 0x008f00 },
    ]);
    expect(nan[1], "NaN lands as a plausible column 0, not as an error").toBe(0);

    const wrapRight = decorationWire([
      { row: 0, left: 0, right: -1, layer: "bottom", bg: 0x008f00 },
    ]);
    expect(wrapRight[2], "a negative right matches EVERY column").toBe(0xffffffff);
  });

  it("encodes an absent bg/fg override as NO_REF (not 0, which is a valid black)", () => {
    const bgOnly = decorationWire([{ row: 0, left: 0, right: 0, layer: "bottom", bg: 0x000000 }]);
    expect(bgOnly[4]).toBe(0x000000); // black bg is a real colour, kept
    expect(bgOnly[5]).toBe(NO_REF); // absent fg → sentinel

    const fgOnly = decorationWire([{ row: 0, left: 0, right: 0, layer: "top", fg: 0x000000 }]);
    expect(fgOnly[4]).toBe(NO_REF); // absent bg → sentinel
    expect(fgOnly[5]).toBe(0x000000);
  });
});

describe("gridForBox", () => {
  it("floors box ÷ cell to whole cells", () => {
    expect(gridForBox(800, 240, 8, 16)).toEqual({ cols: 100, rows: 15 });
    // A partial trailing cell is dropped (floor), never clipped.
    expect(gridForBox(805, 249, 8, 16)).toEqual({ cols: 100, rows: 15 });
  });

  it("floors a fractional-DPR cell size the same way", () => {
    // 16.5 CSS px/cell (33 device px at dpr 2) — 100 cols fit in 1650, 99 in a 1648 box.
    expect(gridForBox(1650, 100, 16.5, 33).cols).toBe(100);
    expect(gridForBox(1648, 100, 16.5, 33).cols).toBe(99);
  });

  it("never returns a zero-cell grid (a grid must have a cell)", () => {
    expect(gridForBox(3, 3, 8, 16)).toEqual({ cols: 1, rows: 1 });
    expect(gridForBox(0, 0, 8, 16)).toEqual({ cols: 1, rows: 1 });
  });
});

describe("cursorCommand", () => {
  it("is 'none' when the frame carries no cursor info (leave the cursor as-is)", () => {
    expect(cursorCommand(frame())).toEqual({ kind: "none" });
  });

  it("is 'clear' when the cursor is reported hidden", () => {
    expect(cursorCommand(frame({ cursorVisible: false }))).toEqual({ kind: "clear" });
    // Row present but invisible is still a clear (DECTCEM off while positioned).
    expect(cursorCommand(frame({ cursorRow: 3, cursorVisible: false }))).toEqual({ kind: "clear" });
  });

  it("is 'set' with the reported position + shape when visible", () => {
    expect(
      cursorCommand(frame({ cursorVisible: true, cursorCol: 5, cursorRow: 2, cursorShape: 2 })),
    ).toEqual({ kind: "set", col: 5, row: 2, shape: 2 });
  });

  it("defaults col/row/shape to 0 when visible but unspecified", () => {
    expect(cursorCommand(frame({ cursorVisible: true }))).toEqual({
      kind: "set",
      col: 0,
      row: 0,
      shape: 0,
    });
  });
});
