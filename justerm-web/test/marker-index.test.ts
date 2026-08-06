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
    cache.onMarkerCreated(9, 640, 1, 120);

    cache.sync(frame(140, 0));
    expect(cache.lineOf(9)).toBe(620);
  });

  // #737 — the batch above hands the birth the basis of the frame it was just synced
  // against, which is the one arrangement where the carried basis and the frame's agree.
  // These two are the arrangement where they do not, and their numbers are core's, not
  // invented: `a_birth_carries_the_basis_its_line_is_absolute_at` runs
  // `Engine::with_scrollback(20, 5, 4)` over two single-`feed` batches that differ only in
  // whether the OSC-133 mark precedes or follows three evictions. Every value below except
  // the carried basis is identical across them, and the engine's own answer differs by 3.
  describe.each([
    { order: "created before the batch's evictions", carried: 22, truth: 5 },
    { order: "created after them", carried: 25, truth: 8 },
  ])("a marker $order (#737)", ({ carried, truth }) => {
    const BORN = { id: 42, line: 8 };

    it("is placed on the engine's own answer when the events are drained first", async () => {
      const port = deferredPort();
      const cache = new MarkerIndexCache(port);
      cache.sync({ evictedTotal: 22, markerEpoch: 0, markerCount: 0 });
      await port.resolveWith(snap([], 22, 0));

      cache.onMarkerCreated(BORN.id, BORN.line, 1, carried);
      cache.sync({ evictedTotal: 25, markerEpoch: 0, markerCount: 1 });

      expect(cache.lineOf(BORN.id)).toBe(truth);
      // And it stayed `O(1)`: the drift check sees a population that already includes the
      // birth, so nothing re-pulls. This is the ordering the contract asks for.
      expect(port.pulls).toBe(1);
    });

    it("is placed on the engine's own answer when the frame is synced first", async () => {
      const port = deferredPort();
      const cache = new MarkerIndexCache(port);
      cache.sync({ evictedTotal: 22, markerEpoch: 0, markerCount: 0 });
      await port.resolveWith(snap([], 22, 0));

      cache.sync({ evictedTotal: 25, markerEpoch: 0, markerCount: 1 });
      cache.onMarkerCreated(BORN.id, BORN.line, 1, carried);

      // The cost this ordering pays, asserted rather than hidden: the frame's count ran
      // one ahead of an index that had not been told yet, so the drift check spent a
      // re-pull on a fact the event was already carrying. Correctness does not depend on
      // the order; this does.
      expect(port.pulls).toBe(2);
      expect(cache.lineOf(BORN.id)).toBeUndefined();

      // The replay must land on the engine's answer too — it overwrites the snapshot's
      // own entry for this id, so a birth stamped with the wrong basis would clobber a
      // correct pulled line with an incorrect one.
      await port.resolveWith(snap([{ id: BORN.id, line: truth }], 25, 0));
      cache.sync({ evictedTotal: 25, markerEpoch: 0, markerCount: 1 });
      expect(cache.lineOf(BORN.id)).toBe(truth);
    });
  });

  // #737 — the new fourth parameter is required, so this is reachable only from an
  // untyped host; what makes it worth a test rather than a type is where a stored `NaN`
  // ends up. It counts toward `size`, so `markerCount === lines.size` holds and the drift
  // check stops watching — the one mechanism that would otherwise notice and heal.
  it("refuses a non-finite basis instead of storing an entry that blinds the drift check", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 0 });
    await port.resolveWith(snap([], 0, 0));

    // What an untyped host calling the pre-#737 three-argument form produces.
    (cache as unknown as { onMarkerCreated(...a: unknown[]): void }).onMarkerCreated(7, 40, 1);

    expect(cache.size).toBe(0);
    expect(cache.lineOf(7)).toBeUndefined();
    // And the count now disagrees, which is the condition that re-pulls: the marker
    // arrives from the engine instead of from the event that could not be understood.
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 1 });
    expect(port.pulls).toBe(2);
    await port.resolveWith(snap([{ id: 7, line: 40 }], 0, 0));
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 1 });
    expect(cache.lineOf(7)).toBe(40);
  });

  // #737 — a KNOWN LIMITATION, pinned so it cannot be un-noticed. The carried basis makes
  // the two drain orders equivalent on the *eviction* axis only. A reflow between the
  // birth and the drain moves markers non-uniformly, which is what the epoch exists for —
  // and a birth still queued when the epoch moves describes the buffer as it was before
  // it, so syncing the post-reflow frame first replays a stale line over the correct
  // pulled one. Numbers are core's: a mark at absolute 3, `resize(10, 5)` reflowing it to
  // 5, basis unmoved, epoch 0 -> 1, no disposal.
  //
  // This is why the contract says drain-then-sync rather than "either order". A fix would
  // have to make the birth self-describing on the epoch axis too — it must FLIP this test.
  it("replays a pre-reflow birth over a post-reflow pull when the frame is synced first", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 0 });
    await port.resolveWith(snap([], 0, 0));

    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 }); // post-reflow frame
    cache.onMarkerCreated(1, 3, 1, 0); // the birth queued before it
    await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 1));
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });

    expect(cache.lineOf(1)).toBe(3); // the engine's answer is 5
  });

  it("adopts the same birth correctly when the events are drained first", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 0 });
    await port.resolveWith(snap([], 0, 0));

    cache.onMarkerCreated(1, 3, 1, 0); // drained on the generation it was born in
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 }); // then the reflow
    await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 1));
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });

    expect(cache.lineOf(1)).toBe(5);
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

/**
 * The Step 5 pass reproduced three defects the first draft's tests could not see —
 * two of them because the fixtures called `sync` once, a state the real frame loop can
 * never be in (the host syncs every frame). These drive the loop.
 */
describe("MarkerIndexCache under a realistic frame loop", () => {
  it("keeps a create-event entry across the frames that follow it", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 0));
    await port.resolveWith(snap([], 0, 0));

    cache.onMarkerCreated(11, 40, 1, 0);
    // The very next frames repeat the same epoch — nothing invalidating happened.
    for (let i = 0; i < 5; i++) cache.sync(frame(0, 0));

    expect(cache.lineOf(11)).toBe(40);
  });

  it("does not lose a marker born while a pull was in flight", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 1)); // pull out

    cache.onMarkerCreated(42, 77, 1, 0); // born mid-flight; the snapshot predates it
    await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 1));
    cache.sync(frame(0, 1));

    expect(cache.lineOf(42)).toBe(77);
    expect(cache.lineOf(1)).toBe(5);
  });

  it("does not resurrect a marker disposed while a pull was in flight", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 1));

    cache.onMarkerDisposed(7); // the snapshot still contains it
    await port.resolveWith(snap([{ id: 7, line: 10 }], 0, 1));
    cache.sync(frame(0, 1));

    expect(cache.lineOf(7)).toBeUndefined();
  });

  it("costs one request per epoch change when the port keeps rejecting, not one per frame", async () => {
    let pulls = 0;
    const cache = new MarkerIndexCache({
      index: () => {
        pulls++;
        return Promise.reject(new Error("transport down"));
      },
    });

    for (let i = 0; i < 20; i++) {
      cache.sync(frame(0, 1));
      await Promise.resolve();
      await Promise.resolve();
    }
    expect(pulls).toBe(1);

    cache.sync(frame(0, 2)); // a genuine change earns one more attempt
    await Promise.resolve();
    await Promise.resolve();
    expect(pulls).toBe(2);
  });

  it("refuses to answer while the adopted epoch is not the one the newest frame reports", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 1));
    // The buffer moves on while the pull is out, so the answer that lands is already old.
    cache.sync(frame(0, 2));
    await port.resolveWith(snap([{ id: 3, line: 9 }], 0, 1));

    expect(cache.lineOf(3)).toBeUndefined();
  });

  it("reset() orphans an outstanding pull", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 1));

    cache.reset();
    await port.resolveWith(snap([{ id: 3, line: 9 }], 0, 1));

    expect(cache.size).toBe(0);
    expect(cache.lineOf(3)).toBeUndefined();
  });
});

/**
 * #490 — what makes `marker_count` worth its four bytes on the wire. Without this the
 * field is a header scalar nothing reads, which is what the v16 lens caught it being.
 */
describe("MarkerIndexCache drift detection", () => {
  it("re-pulls when the frame's count disagrees with what it holds", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 2 });
    await port.resolveWith(snap([{ id: 1, line: 10 }, { id: 2, line: 20 }], 0, 1));
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 2 });
    expect(port.pulls).toBe(1);
    expect(cache.lineOf(1)).toBe(10);

    // A marker was born and the host never forwarded the event: the epoch did not move
    // (creation deliberately does not bump it), so only the count can reveal the drift.
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 3 });

    expect(port.pulls).toBe(2);
    expect(cache.lineOf(1)).toBeUndefined();
  });

  it("does not re-pull while the count agrees", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });
    await port.resolveWith(snap([{ id: 1, line: 10 }], 0, 1));

    for (let i = 0; i < 50; i++) {
      cache.sync({ evictedTotal: i, markerEpoch: 1, markerCount: 1 });
    }
    expect(port.pulls).toBe(1);
  });

  it("ignores the count on a frame that does not carry one", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 1 });
    await port.resolveWith(snap([{ id: 1, line: 10 }], 0, 1));

    // A v15 decoder: no `markerCount`. An absent field must not read as "0 markers".
    cache.sync({ evictedTotal: 0, markerEpoch: 1 });

    expect(port.pulls).toBe(1);
    expect(cache.lineOf(1)).toBe(10);
  });
});

/**
 * #490 — the index fills asynchronously, so a host that draws on demand has to be told.
 * Without this a decoration registered now shows its ruler mark only at the next
 * unrelated redraw: measured as an empty ruler in the demo's synchronous probe, which
 * registers and reads in one turn.
 */
describe("MarkerIndexCache update notification", () => {
  it("notifies the host when a pull lands", async () => {
    const port = deferredPort();
    const seen: number[] = [];
    const cache = new MarkerIndexCache(port, () => seen.push(cache.size));

    cache.sync(frame(0, 1));
    expect(seen).toEqual([]); // nothing to redraw for yet

    await port.resolveWith(snap([{ id: 1, line: 10 }], 0, 1));

    expect(seen).toEqual([1]);
  });

  it("does not notify when there is no host callback", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 1));
    await expect(port.resolveWith(snap([], 0, 1))).resolves.toBeUndefined();
  });
});
