import { describe, expect, it } from "vitest";
import { ScreenReaderState } from "../src/screen-reader";
import type { LiveRegionSink } from "../src/accessibility";
import type { SignalSink } from "../src/command-announce";

class RecLive implements LiveRegionSink {
  readonly said: string[] = [];
  cleared = 0;
  announce(text: string): void {
    this.said.push(text);
  }
  clear(): void {
    this.cleared++;
  }
}

class RecSignal implements SignalSink {
  readonly signals: string[] = [];
  commandSucceeded(): void {
    this.signals.push("ok");
  }
  commandFailed(): void {
    this.signals.push("fail");
  }
}

describe("ScreenReaderState (#161)", () => {
  // Default ACTIVE: a consumer that never wires the seam keeps announcing —
  // justerm can't auto-detect a screen reader (physical boundary), so defaulting
  // off would silently kill a11y. The host opts INTO suppression.
  it("defaults to active", () => {
    expect(new ScreenReaderState().isActive()).toBe(true);
  });

  // While active, the gated live sink forwards announces unchanged.
  it("forwards announces while active", () => {
    const sr = new ScreenReaderState();
    const live = new RecLive();
    const gated = sr.gateLive(live);

    gated.announce("hello");

    expect(live.said).toEqual(["hello"]);
  });

  // Turned off (host knows no screen reader is attached), announce is a no-op —
  // no wasted aria-live churn when nobody is listening (xterm disposes the whole
  // AccessibilityManager; justerm gates the announce).
  it("suppresses announces while inactive", () => {
    const sr = new ScreenReaderState();
    sr.setActive(false);
    const live = new RecLive();
    const gated = sr.gateLive(live);

    gated.announce("hello");

    expect(live.said).toEqual([]);
  });

  // Toggling back on resumes announcing — the gate is live, read per-call.
  it("resumes announcing when reactivated", () => {
    const sr = new ScreenReaderState();
    const live = new RecLive();
    const gated = sr.gateLive(live);

    sr.setActive(false);
    gated.announce("dropped");
    sr.setActive(true);
    gated.announce("heard");

    expect(live.said).toEqual(["heard"]);
  });

  // clear() still runs while inactive — it's cleanup, not output; harmless and
  // keeps the region tidy across a toggle.
  it("still forwards clear while inactive", () => {
    const sr = new ScreenReaderState();
    sr.setActive(false);
    const live = new RecLive();

    sr.gateLive(live).clear();

    expect(live.cleared).toBe(1);
  });

  // The signal sink is gated the same way: no success/fail earcon while inactive.
  it("gates the success/fail signal too", () => {
    const sr = new ScreenReaderState();
    const signal = new RecSignal();
    const gated = sr.gateSignal(signal);

    gated.commandSucceeded();
    expect(signal.signals).toEqual(["ok"]);

    sr.setActive(false);
    gated.commandFailed();
    expect(signal.signals).toEqual(["ok"]); // suppressed
  });
});
