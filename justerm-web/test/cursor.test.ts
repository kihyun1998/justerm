import { describe, expect, it } from "vitest";
import { CursorBlink } from "../src/cursor";

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

  // prefers-reduced-motion (#119): the blink is motion the user asked to avoid,
  // so the cursor stays solid regardless of phase. The media query is read at the
  // integration layer and injected via setReducedMotion.
  it("stays solid when reduced motion is requested", () => {
    const blink = new CursorBlink();
    blink.setReducedMotion(true);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
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
