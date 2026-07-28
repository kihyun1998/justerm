import { describe, expect, it } from "vitest";
import { TextBlink } from "../src/text-blink";

describe("TextBlink", () => {
  it("is disabled by default, so SGR 5 text is always shown", () => {
    const b = new TextBlink();
    expect(b.enabled).toBe(false);
    // Every phase of the clock reads visible — there is no off phase to land in.
    for (const t of [0, 250, 500, 1_000, 123_456]) expect(b.isVisible(t)).toBe(true);
  });

  it("alternates on the consumer's interval once enabled", () => {
    const b = new TextBlink();
    b.setIntervalMs(500);
    expect(b.enabled).toBe(true);
    expect(b.isVisible(0)).toBe(true);
    expect(b.isVisible(499)).toBe(true);
    expect(b.isVisible(500)).toBe(false);
    expect(b.isVisible(999)).toBe(false);
    expect(b.isVisible(1_000)).toBe(true);
  });

  it("has a phase independent of the interval's magnitude", () => {
    const b = new TextBlink();
    b.setIntervalMs(1_000);
    expect(b.isVisible(999)).toBe(true);
    expect(b.isVisible(1_001)).toBe(false);
    expect(b.isVisible(2_001)).toBe(true);
  });

  it("goes back to always-visible when the interval is cleared", () => {
    const b = new TextBlink();
    b.setIntervalMs(500);
    expect(b.isVisible(500)).toBe(false);
    b.setIntervalMs(0);
    // The off phase must not survive the disable, or text stays hidden forever.
    expect(b.enabled).toBe(false);
    expect(b.isVisible(500)).toBe(true);
  });

  it("treats a negative / non-finite interval as disabled rather than throwing", () => {
    // A renderer that panics across a policy setter is a worse contract than one that reports
    // what it adopted (same rule as CursorBlink.setIdleTimeout).
    for (const bad of [-1, -0.5, Number.NaN, Number.POSITIVE_INFINITY]) {
      const b = new TextBlink();
      b.setIntervalMs(bad);
      expect(b.enabled).toBe(false);
      expect(b.isVisible(500)).toBe(true);
    }
  });

  it("floors a fractional interval, and a sub-1ms one disables", () => {
    const b = new TextBlink();
    b.setIntervalMs(500.9);
    expect(b.isVisible(500)).toBe(false); // floored to 500, not 501
    b.setIntervalMs(0.5);
    expect(b.enabled).toBe(false);
  });

  it("pins text visible under prefers-reduced-motion, whatever the interval says", () => {
    const b = new TextBlink();
    b.setIntervalMs(500);
    b.setReducedMotion(true);
    expect(b.enabled).toBe(false);
    expect(b.isVisible(500)).toBe(true);
    expect(b.isVisible(1_500)).toBe(true);
    // …and releasing it restores the blink without the interval being re-set.
    b.setReducedMotion(false);
    expect(b.enabled).toBe(true);
    expect(b.isVisible(500)).toBe(false);
  });

  it("is a pure function of the clock — no restart affordance, unlike the cursor", () => {
    // CursorBlink resets its phase on a keystroke (restartFromInput) so the caret stays solid
    // while typing; text blink has no such affordance in any reference. Asserted as purity: the
    // answer for a given `now` cannot depend on what was asked before it, so nothing on the input
    // path can shift the phase.
    const b = new TextBlink();
    b.setIntervalMs(500);
    const walked = [0, 500, 1_000, 1_500, 500, 0].map((t) => b.isVisible(t));
    expect(walked).toEqual([true, false, true, false, false, true]);
  });
});
