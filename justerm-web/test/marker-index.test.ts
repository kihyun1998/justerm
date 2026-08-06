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
    cache.onMarkerCreated(9, 640, 1, 120, 0);

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

      cache.onMarkerCreated(BORN.id, BORN.line, 1, carried, 0);
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
      cache.onMarkerCreated(BORN.id, BORN.line, 1, carried, 0);

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

  // #741 — this was pinned by #737 as a KNOWN LIMITATION answering **3**, and flipping it
  // is what the fix is for. The carried basis makes the two drain orders equivalent on the
  // *eviction* axis only: a reflow moves markers non-uniformly, which is what the epoch
  // exists for, and a birth still queued when the epoch moves describes a buffer that no
  // longer exists. Nothing in the arrival order tells the two apart — `invalidate()`
  // empties `pendingOps`, so a birth queued *before* the reflow and one queued *after* the
  // pull was issued reach the replay looking identical. The generation is the only thing
  // that separates them, and only core can say it.
  //
  // Numbers are core's, from `a_birth_carries_the_generation_its_line_belongs_to`: a mark
  // at absolute 3, `resize(10, 5)` reflowing it to 5, basis unmoved at 0, epoch 0 -> 1, no
  // disposal.
  it("drops a pre-reflow birth that the post-reflow pull has already repaired", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 0 });
    await port.resolveWith(snap([], 0, 0));

    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 }); // post-reflow frame
    cache.onMarkerCreated(1, 3, 1, 0, 0); // the birth queued before it — generation 0
    await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 1));
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });

    expect(cache.lineOf(1)).toBe(5); // the engine's own answer, not the queued line
  });

  it("adopts the same birth correctly when the events are drained first", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 0 });
    await port.resolveWith(snap([], 0, 0));

    cache.onMarkerCreated(1, 3, 1, 0, 0); // drained on the generation it was born in
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 }); // then the reflow
    await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 1));
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });

    expect(cache.lineOf(1)).toBe(5);
  });

  // #741 — the half a pending-op gate cannot see. This birth never enters `pendingOps` at
  // all: no pull is in flight when it arrives, so the store path takes it and overwrites a
  // line the pull had already repaired. Reachable whenever the event channel runs slower
  // than the frame channel — which frame mode allows by construction, since core has no
  // IPC and the consumer owns both transports (ADR-0017). The same generation check
  // answers it, which is why the rule is stated once and applied at both points.
  it("refuses a stale birth drained after the post-reflow pull has landed", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 0 });
    await port.resolveWith(snap([], 0, 0));

    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 }); // the reflow
    await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 1)); // repaired, and adopted
    cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });
    expect(cache.lineOf(1)).toBe(5);

    // The birth arrives late, still describing the buffer as it was before the reflow.
    cache.onMarkerCreated(1, 3, 1, 0, 0);

    expect(cache.lineOf(1)).toBe(5);
  });

  // #741 — the two above are blind to *which* comparison the guards make: in both, the
  // stale generation is the smaller number, so `!==` and `<` agree. Core's counter is
  // `wrapping_add` (`bump_marker_epoch`), and at the wrap the stale generation is the
  // LARGER one — the single window where the two candidate predicates differ. Without
  // these, the doc-comments claiming "equality, never order" are unasserted and a
  // reviewer's plausible tightening to `<` passes every other test here.
  describe("across an epoch wrap, where the stale generation is the larger number", () => {
    const LAST = 0xffffffff; // what `wrapping_add` returns just before it wraps to 0

    it("drops the queued birth at the replay", async () => {
      const port = deferredPort();
      const cache = new MarkerIndexCache(port);
      cache.sync({ evictedTotal: 0, markerEpoch: LAST, markerCount: 0 });
      await port.resolveWith(snap([], 0, LAST));

      cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 1 }); // the wrap
      cache.onMarkerCreated(1, 3, 1, 0, LAST); // queued in the generation before it
      await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 0));
      cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 1 });

      expect(cache.lineOf(1)).toBe(5);
    });

    it("refuses the late birth at the store", async () => {
      const port = deferredPort();
      const cache = new MarkerIndexCache(port);
      cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 1 }); // already wrapped
      await port.resolveWith(snap([{ id: 1, line: 5 }], 0, 0));
      cache.sync({ evictedTotal: 0, markerEpoch: 0, markerCount: 1 });

      cache.onMarkerCreated(1, 3, 1, 0, LAST);

      expect(cache.lineOf(1)).toBe(5);
    });
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

    cache.onMarkerCreated(11, 40, 1, 0, 0);
    // The very next frames repeat the same epoch — nothing invalidating happened.
    for (let i = 0; i < 5; i++) cache.sync(frame(0, 0));

    expect(cache.lineOf(11)).toBe(40);
  });

  it("does not lose a marker born while a pull was in flight", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync(frame(0, 1)); // pull out

    cache.onMarkerCreated(42, 77, 1, 0, 1); // born mid-flight, in the generation the pull is for
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

/**
 * #746 — the index could stop asking. Three defects, one class, and the first two share a
 * shape: a piece of bookkeeping that was correct when it was written and became wrong when
 * #741 changed what an op knows about itself.
 */
describe("MarkerIndexCache recovery", () => {
  // #746 F1 — `invalidate()` used to empty `pendingOps`. Whether the outstanding pull is
  // really stale is decided by the SNAPSHOT's epoch, and the query channel legitimately runs
  // ahead of the frame channel — so the pull can land carrying the very generation the wipe
  // was for, adopt, and answer from content that predates the erased ops. Nothing
  // re-delivers them: creation and disposal deliberately do not move the epoch (#490).
  //
  // Both directions are asserted because they fail in opposite ways and the one guard
  // watching this class sees neither: `lines.size` is unchanged, so `markerCount ===
  // lines.size` still holds. Count equality is not set equality.
  it("keeps a queued birth and a queued death when the pull adopts the generation the invalidation was for", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 7, markerCount: 2 });
    await port.resolveWith(snap([{ id: 1, line: 10 }, { id: 2, line: 22 }], 0, 7));
    expect(cache.lineOf(2)).toBe(22);

    // A reflow (7 -> 8): the frame invalidates and pulls. The engine then reflows again
    // before the backend evaluates the query, so the snapshot in flight carries epoch 9.
    cache.sync({ evictedTotal: 0, markerEpoch: 8, markerCount: 2 });

    // Only after that evaluation does the stream dispose 2 and create 3 — events, so no
    // epoch moves — and the host drains them before syncing, which is the order the
    // contract asks for.
    cache.onMarkerDisposed(2);
    cache.onMarkerCreated(3, 33, 1, 0, 9);

    // Now the frame carrying epoch 9 arrives. This is the invalidation that used to erase
    // both ops.
    cache.sync({ evictedTotal: 0, markerEpoch: 9, markerCount: 2 });

    // And the snapshot lands on that same generation, so it adopts and starts answering.
    await port.resolveWith(snap([{ id: 1, line: 10 }, { id: 2, line: 22 }], 0, 9));
    cache.sync({ evictedTotal: 0, markerEpoch: 9, markerCount: 2 });

    expect(cache.lineOf(3)).toBe(33); // the birth survives the invalidation
    expect(cache.lineOf(2)).toBeUndefined(); // and the death is not undone by the snapshot
  });

  // #746 Slice A — the pull trigger asked an EDGE ("the epoch just changed") and therefore
  // could not see the state "a request is needed and none is out". Reaching it takes only a
  // second bump while a pull is out: the cap swallows the re-pull, and the pull in flight
  // then answers with the generation it was evaluated in.
  it("asks again when a pull lands one generation behind the newest frame", async () => {
    const port = deferredPort();
    const cache = new MarkerIndexCache(port);
    cache.sync({ evictedTotal: 0, markerEpoch: 7, markerCount: 1 });
    await port.resolveWith(snap([{ id: 1, line: 10 }], 0, 7));
    expect(cache.lineOf(1)).toBe(10);

    cache.sync({ evictedTotal: 0, markerEpoch: 8, markerCount: 1 }); // invalidate + pull
    cache.sync({ evictedTotal: 0, markerEpoch: 9, markerCount: 1 }); // the cap swallows this one
    expect(port.pulls).toBe(2);

    // The answer arrives for generation 8 while the frames are already at 9.
    await port.resolveWith(snap([{ id: 1, line: 11 }], 0, 8));
    expect(cache.lineOf(1)).toBeUndefined(); // correctly refuses: it does not describe frame 9

    // Master stopped here, permanently, with the engine quiet. The level trigger asks again.
    cache.sync({ evictedTotal: 0, markerEpoch: 9, markerCount: 1 });
    expect(port.pulls).toBe(3);
    await port.resolveWith(snap([{ id: 1, line: 12 }], 0, 9));
    cache.sync({ evictedTotal: 0, markerEpoch: 9, markerCount: 1 });

    expect(cache.lineOf(1)).toBe(12);
  });

  // #746 Slice A, the other half of the same predicate: a REFUSED transport is not retried,
  // because this class cannot tell a blip from a dead session — it sees one report. The
  // pinned "one request per epoch change" contract above is that rule; this asserts the new
  // latch does not weaken it while the trigger is level rather than edge.
  it("does not retry a refused transport, but still spends one attempt per epoch change", async () => {
    let pulls = 0;
    const cache = new MarkerIndexCache({
      index: () => {
        pulls++;
        return Promise.reject(new Error("transport down"));
      },
    });

    for (let i = 0; i < 30; i++) {
      cache.sync({ evictedTotal: 0, markerEpoch: 1, markerCount: 1 });
      await Promise.resolve();
      await Promise.resolve();
    }
    expect(pulls).toBe(1);

    cache.sync({ evictedTotal: 0, markerEpoch: 2, markerCount: 1 });
    await Promise.resolve();
    await Promise.resolve();
    expect(pulls).toBe(2);
  });

  // #746 F4 — the resolve arm is guarded by the round-trip token and the rejection arm was
  // not. `reset()` orphans a pull without waiting for it, so an orphaned rejection landing
  // after the new session's pull went out cleared the flag that pull owns. With it wrongly
  // false, the events stop queueing and the landing snapshot erases them — F1's shape,
  // reached from the other side.
  it("ignores an orphaned pull's rejection while the pull that replaced it is still out", async () => {
    let rejectFirst: (() => void) | undefined;
    let resolveSecond: ((s: MarkerIndexSnapshot) => void) | undefined;
    let pulls = 0;
    const cache = new MarkerIndexCache({
      index() {
        pulls++;
        return pulls === 1
          ? new Promise<MarkerIndexSnapshot>((_r, j) => {
              rejectFirst = () => j(new Error("orphaned"));
            })
          : new Promise<MarkerIndexSnapshot>((r) => {
              resolveSecond = r;
            });
      },
    });

    cache.sync(frame(0, 1)); // session 1's pull goes out
    cache.reset(); // …and is orphaned, not awaited
    cache.sync(frame(0, 1)); // session 2 starts its own
    expect(pulls).toBe(2);

    rejectFirst?.();
    await Promise.resolve();
    await Promise.resolve();

    // A marker born now must be QUEUED, because a pull really is in flight.
    cache.onMarkerCreated(7, 40, 1, 0, 1);

    resolveSecond?.(snap([{ id: 1, line: 5 }], 0, 1));
    await Promise.resolve();
    await Promise.resolve();
    cache.sync(frame(0, 1));

    expect(cache.lineOf(7)).toBe(40); // survived the landing snapshot's clear
    expect(cache.lineOf(1)).toBe(5);
  });
});
