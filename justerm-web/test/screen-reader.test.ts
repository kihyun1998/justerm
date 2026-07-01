import { describe, expect, it } from "vitest";
import { ScreenReaderState } from "../src/screen-reader";
import type { LiveRegionSink } from "../src/accessibility";

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

  // The command success/fail signal is NOT gated here (no `gateSignal`): #160's
  // controller reads `isActive()` through #167's `auto` policy state instead, so
  // an `on` modality can override an SR-off state — a blanket wrapper couldn't.
});
