import { describe, expect, it } from "vitest";
import { CursorBlink, cursorOp } from "../src/cursor";
import type { DrawOp } from "../src/render-core";

// Cursor shapes (decoder cursorShape): 0 = Block, 1 = Underline, 2 = Bar.
const BLOCK = 0;
const UNDERLINE = 1;

const baseOp = (over: Partial<DrawOp> = {}): DrawOp => ({
  x: 3,
  y: 2,
  symbol: "a",
  fg: 0xffffff,
  bg: 0x000000,
  bold: false,
  italic: false,
  underline: false,
  strikethrough: false,
  ...over,
});

describe("CursorBlink", () => {
  // xterm BLINK_INTERVAL = 600ms: the cursor shows for the first interval, hides
  // for the next, and so on. Time is injected so the state is testable without
  // real timers.
  it("toggles visibility every 600ms", () => {
    const blink = new CursorBlink();

    expect([blink.isVisible(0), blink.isVisible(599), blink.isVisible(600), blink.isVisible(1200)]).toEqual(
      [true, true, false, true],
    );
  });

  // Typing or moving the cursor restarts the animation: the cursor shows at once
  // and the interval resets from that moment, so it never blinks off right after
  // input (xterm restartBlinkAnimation).
  it("shows immediately and resets the phase on restart", () => {
    const blink = new CursorBlink();
    expect(blink.isVisible(600)).toBe(false); // would be hidden mid-blink

    blink.restart(600);

    expect(blink.isVisible(600)).toBe(true); // shown at once
    expect(blink.isVisible(1199)).toBe(true); // still the first interval after restart
    expect(blink.isVisible(1200)).toBe(false); // hides 600ms later
  });

  // Unfocused terminals stop blinking — the cursor stays solid (xterm pause sets
  // isCursorVisible = true and clears the interval).
  it("stays solid while unfocused", () => {
    const blink = new CursorBlink();

    blink.setFocused(false);
    expect(blink.isVisible(600)).toBe(true); // would blink off if focused
    expect(blink.isVisible(1800)).toBe(true);

    blink.setFocused(true);
    expect(blink.isVisible(600)).toBe(false); // focused again → blinks
  });
});

describe("cursorOp", () => {
  // beamterm has no cursor primitive, so a block cursor is a cell-invert: the
  // cell is filled with the theme cursor colour and the glyph is drawn in the
  // cell's original background so it stays legible (penterm applyCursor).
  it("inverts the cell for a block cursor", () => {
    const op = cursorOp(baseOp({ fg: 0xffffff, bg: 0x222222 }), BLOCK, 0xff8800);

    expect({ symbol: op.symbol, fg: op.fg, bg: op.bg }).toEqual({
      symbol: "a",
      fg: 0x222222, // original bg → glyph
      bg: 0xff8800, // cursor colour → cell
    });
  });

  // An underline cursor keeps the cell background but draws in the cursor colour
  // with an underline. beamterm is cell-level (no sub-cell bar), so the glyph
  // takes the cursor colour too — the underline is the cursor signal.
  it("underlines in the cursor colour for an underline cursor", () => {
    const op = cursorOp(baseOp({ fg: 0xffffff, bg: 0x222222 }), UNDERLINE, 0xff8800);

    expect({ fg: op.fg, bg: op.bg, underline: op.underline }).toEqual({
      fg: 0xff8800, // cursor colour
      bg: 0x222222, // original bg kept
      underline: true,
    });
  });
});
