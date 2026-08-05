/**
 * #490 S2 — the consumer half: a marker index pulled once and kept.
 *
 * Core stopped being able to hand every live marker to every frame (measured at
 * 37–70% of an 80×24 frame), so the widget asks once and maintains the answer:
 * rebase by the frame's `evictedTotal` delta, append/drop on the marker events, and
 * re-ask when `markerEpoch` moves.
 *
 * Two rules here are *policy*, not mechanics, and both fall out of the same
 * principle this repo applies to the wire — a silently wrong answer is worse than a
 * loudly absent one:
 *
 * - while a re-pull is in flight the index reports **unknown**, so a decoration
 *   disappears for a frame or two rather than painting on a line it no longer owns;
 * - a changed epoch issues **one** pull, not one per frame, which is the
 *   once-per-frame obligation the wire contract states (a marker below a bottom
 *   margin bumps every output line).
 */
import { describe, expect, it } from "vitest";
import { MarkerIndexCache, type MarkerIndexSnapshot, type MarkerPort } from "../src/marker-index";

/** A port whose pulls resolve when the test says so, so the in-flight window is testable. */
function deferredPort(): MarkerPort & {
  pulls: number;
  resolveWith(snap: MarkerIndexSnapshot): Promise<void>;
} {
  let pending: ((s: MarkerIndexSnapshot) => void) | undefined;
  return {
    pulls: 0,
    index() {
      this.pulls++;
      return new Promise<MarkerIndexSnapshot>((res) => {
        pending = res;
      });
    },
    async resolveWith(snap) {
      pending?.(snap);
      pending = undefined;
      await Promise.resolve();
      await Promise.resolve();
    },
  };
}

const snap = (
  markers: Array<{ id: number; line: number }>,
  evictedTotal: number,
  epoch: number,
): MarkerIndexSnapshot => ({
  markers: markers.map((m) => ({ ...m, kind: 1 })),
  evictedTotal,
  epoch,
});

const frame = (evictedTotal: number, markerEpoch: number) => ({ evictedTotal, markerEpoch });

describe("MarkerIndexCache", () => {
  it("rebases a pulled line by the frame's eviction delta", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(100, 0));
    await port.resolveWith(snap([{ id: 7, line: 500 }], 100, 0));

    cache.sync(frame(100, 0));
    expect(cache.lineOf(7)).toBe(500);

    // 30 more lines evicted, no epoch change: the consumer does the arithmetic itself.
    cache.sync(frame(130, 0));
    expect(cache.lineOf(7)).toBe(470);
    expect(port.pulls).toBe(1);
  });

  it("appends a marker born after the pull, on the basis it was born at", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(100, 0));
    await port.resolveWith(snap([], 100, 0));

    // The stream creates one at absolute 640, when 120 lines had been evicted.
    cache.sync(frame(120, 0));
    cache.onMarkerCreated(9, 640, 1);

    cache.sync(frame(140, 0));
    expect(cache.lineOf(9)).toBe(620);
  });

  it("drops a disposed marker without re-pulling", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 0));
    await port.resolveWith(snap([{ id: 1, line: 10 }], 0, 0));

    cache.onMarkerDisposed(1);
    cache.sync(frame(0, 0));

    expect(cache.lineOf(1)).toBeUndefined();
    expect(port.pulls).toBe(1);
  });

  it("reports unknown while a re-pull is in flight, rather than a stale line", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 0));
    await port.resolveWith(snap([{ id: 3, line: 50 }], 0, 0));
    cache.sync(frame(0, 0));
    expect(cache.lineOf(3)).toBe(50);

    // A reflow: every held line is wrong in a way no delta repairs.
    cache.sync(frame(0, 1));
    expect(cache.lineOf(3)).toBeUndefined();

    await port.resolveWith(snap([{ id: 3, line: 12 }], 0, 1));
    cache.sync(frame(0, 1));
    expect(cache.lineOf(3)).toBe(12);
  });

  it("issues one pull for a changed epoch, not one per frame", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 0));
    await port.resolveWith(snap([{ id: 1, line: 1 }], 0, 0));

    // A marker below a bottom margin bumps the epoch on EVERY output line. Without a
    // cap this is a pull per frame, which is the cost the whole design removes.
    for (let i = 1; i <= 60; i++) cache.sync(frame(0, i));

    expect(port.pulls).toBe(2);
  });

  it("survives a frame that carries no basis at all", () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);

    // A hand-built or older frame: the getters are optional in `DecodedFrame`.
    expect(() => cache.sync({})).not.toThrow();
    expect(cache.lineOf(1)).toBeUndefined();
  });
});
