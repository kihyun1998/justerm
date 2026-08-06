/**
 * A marker index the widget pulls once and maintains (#490 S2).
 *
 * Core used to hand every live marker's absolute line to every frame. That group was
 * measured at **37–70 % of an 80×24 frame** at ordinary OSC-133 densities and is
 * ADR-0020's R3 violation, so it is leaving the wire; the consumer asks once instead
 * and keeps the answer current from three much smaller signals:
 *
 * | what moved | signal | cost |
 * |---|---|---|
 * | scrollback evicted | `frame.evictedTotal` | one subtraction — eviction shifts every marker identically |
 * | a marker was born / died | `MarkerCreated` / `MarkerDisposed` events | `O(1)` each |
 * | anything else | `frame.markerEpoch` moved | a re-pull, because nothing smaller repairs it |
 *
 * **This class holds no lines of its own frame of reference.** Everything it stores is
 * absolute at the basis it arrived on, and {@link MarkerIndexCache.lineOf} rebases at
 * read time against the newest frame. That is why a create event carries a line: it is
 * absolute at the moment of creation, which is a different basis from the pull's.
 *
 * **And the create event must carry that basis too (#737).** This class read the sentence
 * above and then stamped a birth with the last *frame*'s basis, which is a third instant
 * again: one `feed` can create a marker and then evict, so the frame closing that batch
 * reports an origin the event's line predates. Measured in core — two batches with the
 * same mark and the same three evictions in opposite orders produce an identical event
 * line, identical bases before and after, an identical epoch and an identical
 * `markerCount`, and true lines three apart. Nothing the consumer could observe told them
 * apart, so the basis had to start travelling with the line it belongs to.
 *
 * **A basis dates only a uniform move, so the birth carries its generation too (#741).**
 * Eviction is the move one scalar can express; a reflow moves markers individually, which
 * is what `markerEpoch` says and what no delta repairs. A birth queued before that bump
 * describes a buffer that no longer exists — and arrival order cannot reveal it, because
 * `invalidate()` empties `pendingOps`, so a birth queued *before* the reflow and one born
 * *after* the pull went out reach the replay looking identical. The rule is therefore one
 * sentence applied wherever a line enters this class: **an entry is adopted only into the
 * generation it names**, and the re-pull the epoch already forces supplies it otherwise.
 * The two entry points are {@link MarkerIndexCache.onMarkerCreated}'s store and the
 * replay inside the pull — the second alone would leave a birth delivered *after* the
 * repairing pull landed, which is reachable whenever the host's event channel runs
 * slower than its frame channel.
 */

/** One live marker as the backend's `Engine::marker_index()` reports it. */
export interface MarkerIndexEntry {
  readonly id: number;
  /** Absolute `[scrollback ++ screen]` line, on the snapshot's own `evictedTotal`. */
  readonly line: number;
  readonly kind: number;
}

/** The backend's answer, with the basis that says how long it stays usable. */
export interface MarkerIndexSnapshot {
  readonly markers: readonly MarkerIndexEntry[];
  readonly evictedTotal: number;
  readonly epoch: number;
}

/**
 * The read-query seam to core's `Engine::marker_index()` — sibling of `CommandNavPort`
 * and `SearchPort`. Frame mode wires it to the backend over the consumer's own
 * transport (justerm has no IPC by identity, ADR-0017).
 */
export interface MarkerPort {
  index(): Promise<MarkerIndexSnapshot>;
}

/** The subset of a decoded frame this cache reads. Both fields are optional because
 * `DecodedFrame` mirrors the wasm getters by hand and a frame may omit them. */
export interface MarkerBasisFrame {
  readonly evictedTotal?: number;
  readonly markerEpoch?: number;
  /** How many markers the engine holds (wire v16). Optional: absent on an older decoder,
   * and absent must not read as "zero markers". */
  readonly markerCount?: number;
}

type Op =
  | {
      readonly add: true;
      readonly id: number;
      readonly line: number;
      readonly basis: number;
      /** The generation `line` belongs to. A death carries none deliberately: dropping an
       * id is the same act in every generation, so it is the one op nothing dates (#741). */
      readonly epoch: number;
    }
  | { readonly add: false; readonly id: number };

export class MarkerIndexCache {
  private readonly lines = new Map<number, { line: number; basis: number }>();
  /** The newest basis a frame reported, which `lineOf` rebases against. */
  private basis = 0;
  /** The epoch the current contents describe. `undefined` = the contents are unusable. */
  private adopted: number | undefined;
  /** The last epoch a *frame* reported — the invalidation key, deliberately distinct from
   * `adopted`. A frame repeating an epoch we have not caught up with yet is not a new
   * invalidation, and treating it as one wipes whatever the create events contributed. */
  private seen: number | undefined;
  /** Self-issued round-trip token (the `search.ts` pattern). A pull adopts only while it
   * is still the newest; comparing the backend's own epoch across two channels compares
   * two samples of one counter taken at different instants, which under a per-line bump
   * never match — the index then never refills and the retry never terminates. */
  private seq = 0;
  private inFlight = false;
  /** Whether the last pull *rejected*. It separates the two ways a pull fails to leave the
   * index usable, because only one of them is this class's to retry: a transport that
   * answered late or stale can be asked again with a guaranteed-progressing request, while
   * one that refused cannot be distinguished from a dead session by anything this object
   * can see (#746). Cleared by a landing pull and by a genuine epoch change. */
  private lastAttemptFailed = false;
  /** Events that landed while a pull was out, replayed onto the snapshot it returns. */
  private readonly pendingOps: Op[] = [];

  /**
   * @param port the query seam to the backend's `Engine::marker_index()`
   * @param onUpdated called after a pull lands, so a host that draws **on demand** can
   *   redraw with the answer. Without it a decoration registered now shows its ruler mark
   *   only at the next unrelated redraw — the index fills asynchronously, and nothing else
   *   tells the host that it did. A host with its own frame loop can omit this; one that
   *   renders in response to events cannot.
   */
  constructor(
    private readonly port: MarkerPort,
    private readonly onUpdated?: () => void,
  ) {}

  /**
   * Call once per frame, before projecting. Adopts the frame's basis and, if the epoch
   * moved, invalidates the index and asks for a fresh one.
   *
   * **At most one pull is in flight.** A marker sitting below a bottom margin bumps the
   * epoch on every output line (measured: 1 000 bumps over 1 000 region scrolls), so an
   * uncapped re-pull would cost `O(M)` per frame — exactly the cost this design removes.
   *
   * The cap bounds the **requests, not the outage** (#738). While the epoch moves every
   * frame, each pull is stale before it lands and {@link MarkerIndexCache.lineOf} reports
   * unknown for as long as the churn lasts — not for a round trip. The reach of that state
   * is narrow but the cost inside it is total (no ruler marks, no above-top anchors), and
   * ordinary scrolling is not in it at all: 0 bumps over 1 000 lines.
   *
   * **"Bounded by the churn" was itself too generous, and #746 is why the trigger below asks
   * a state rather than an edge.** With the pull started only when the epoch *changed*, a
   * pull landing one generation behind the newest frame ended the outage nowhere: the churn
   * stopped and the index stayed unusable, permanently, with `markerCount === lines.size`
   * holding so the drift check never fired. Measured on an ordinary interactive drag-resize
   * (9 bumps, our own 100 ms `FitController` cadence): 0 of 40 drags at a 50 ms round trip,
   * **8 of 40 at 100 ms**, 15 of 40 at 170 ms. Non-monotone, so "a slower transport is worse"
   * is false — a very slow pull evaluates *after* the final bump and is fine.
   */
  sync(frame: MarkerBasisFrame): void {
    if (frame.evictedTotal !== undefined) this.basis = frame.evictedTotal;
    const epoch = frame.markerEpoch;
    if (epoch === undefined) return;

    if (epoch !== this.seen) {
      this.seen = epoch;
      // A new generation is new information, so a transport that refused the last request
      // gets one more attempt — which is exactly the "one request per epoch change, not one
      // per frame" contract, expressed as a latch rather than as an early return.
      this.lastAttemptFailed = false;
      // What we hold describes a buffer that has moved non-uniformly. Drop it now rather
      // than serving it: a decoration that is missing is visible and self-correcting, one
      // painted on a line it no longer owns is neither. **How long "missing" lasts is set by
      // the churn, not by the round trip** (#738) — one frame for a single reflow, the whole
      // workload where the epoch moves per line. Still the right trade; not the cheap one an
      // earlier version of this comment claimed. **And not by the churn either, before #746**:
      // a single stale landing ended the outage nowhere at all.
      this.invalidate();
    } else if (epoch === this.seen) {
      // Same epoch, so nothing moved non-uniformly — but the population may still have
      // changed underneath us. Creation and disposal deliberately do not move the epoch
      // (they are `O(1)` events), so a host that wired the pull and *not* the events
      // drifts with nothing to notice. The frame's own count is what notices, and that is
      // the whole reason it rides the header (#490 v16).
      //
      // Guarded twice, and both matter. Only while the index is *usable*: mid-flight
      // `lines` is empty by design and would mismatch every frame. And only when the
      // frame carries a count at all: an older decoder omits it, and `undefined` must not
      // read as "zero markers" — that would re-pull forever against a v15 backend.
      if (
        this.adopted !== undefined &&
        frame.markerCount !== undefined &&
        frame.markerCount !== this.lines.size
      ) {
        this.invalidate();
      }
    }

    // **The one place a pull starts, and it asks a *state*, not an *edge* (#746).** Every
    // caller above only decides whether what we hold still describes the newest frame; this
    // decides whether to go and get one. Written as an edge — "the epoch just changed" — it
    // could not see the state where a request is needed and none is out, and that state is
    // ordinary: the cap swallows the re-pull for a second bump, and the pull already in
    // flight then answers with the generation it was evaluated in. Measured, an interactive
    // drag-resize reaches it once the query round trip approaches the resize cadence (our
    // own `FitController` debounce, 100 ms): at RTT ~100 ms, 8 drags in 40 ended with the
    // index permanently unusable while the engine was quiet — not "blank for the drag, then
    // correct" as #738 recorded.
    //
    // Three conditions, each load-bearing:
    // - `adopted !== seen` — what we hold does not describe the newest frame. In the
    //   settled state these are equal and nothing fires;
    // - `!inFlight` — the cap is unchanged: at most one pull is ever out. Measured at 1.00x
    //   master's request count through per-line churn, because while churn continues this
    //   asks exactly what the edge asked;
    // - `!lastAttemptFailed` — a refused transport is not this class's to retry (see the
    //   rejection arm). Only a transport that *answered* is retried, and that retry is
    //   guaranteed to progress, so it cannot loop.
    //
    // `invalidate()` first, always. `inFlight` implying `adopted === undefined` is what
    // `onMarkerCreated`'s generation gate leans on (#741): pulling without it leaves a
    // defined, stale `adopted` for an arriving birth to be compared against, which measured
    // as silently wrong coordinates on 8 of 60 fuzz seeds.
    if (this.adopted !== this.seen && !this.inFlight && !this.lastAttemptFailed) {
      this.invalidate();
      this.pull();
    }
  }

  /** Drop the contents and mark them unusable, leaving the flight and the frame-side
   * bookkeeping (`basis`, `seen`) alone.
   *
   * **It does not touch `pendingOps`, and that deletion is the point (#746).** It used to
   * empty them, because an op could not say which buffer it described and an invalidation
   * meant "everything I hold is about the wrong one". #741 dated every op, so the replay
   * now decides that per entry — precisely, where the wipe decided it bluntly.
   *
   * Bluntly was wrong, and it was silent. Whether the outstanding pull is *actually* stale
   * is decided by the **snapshot's** epoch, stamped when the backend evaluates it — and the
   * query channel legitimately runs ahead of the frame channel (see `pull`). So the
   * snapshot can arrive carrying the very generation this invalidation was for, adopt, and
   * start answering from content that predates the ops it just erased. Neither is
   * recoverable: creation and disposal deliberately do not move the epoch (#490), so
   * nothing re-delivers them. A wiped death resurrects a disposed marker; a wiped birth
   * hides a live one; and `lines.size` is unchanged either way, so the drift check below
   * stays silent forever — **count equality is not set equality**, which is the standing
   * limit of that guard and not something this change removes.
   *
   * **The concern this raises, cleared, with the conditions it is cleared under.** Ops now
   * outlive the flight that queued them, including one that *fails* — so could a queued
   * birth be replayed onto some much later snapshot and resurrect a marker disposed in
   * between? No, and it rests on two facts rather than on luck:
   * 1. after a failed flight the only route back to a pull is an **epoch change** — the
   *    drift check needs `adopted !== undefined` and every pull is preceded by an
   *    `invalidate()`, so it cannot fire while the index is unusable — and an epoch change
   *    makes every queued add's generation differ from the snapshot's, so the replay drops
   *    it (#741);
   * 2. a replayed **disposal** carries no generation and is idempotent, because marker ids
   *    are never reused: `next_marker_id` deliberately rides across RIS *"so a reissued id
   *    lets a stale `MarkerDisposed(7)` drop the live post-RIS marker 7"* (`term.rs`).
   *
   * If either stops holding — a pull issued without an epoch change while the index is
   * unusable, or ids that recycle — the queue needs an explicit owner again. The first
   * attempt to test this hazard passed **vacuously** (the failure latch stopped the second
   * pull ever happening), which is why the reasoning is written down instead of a test that
   * could not enter its own window. */
  private invalidate(): void {
    this.lines.clear();
    this.adopted = undefined;
  }

  private pull(): void {
    this.inFlight = true;
    const seq = ++this.seq;
    void this.port.index().then(
      (snap) => {
        if (seq !== this.seq) return; // superseded; not ours to adopt
        this.inFlight = false;
        this.lines.clear();
        for (const m of snap.markers) {
          this.lines.set(m.id, { line: m.line, basis: snap.evictedTotal });
        }
        // The snapshot is a view from *before* these landed, so replay them onto it —
        // otherwise a birth is lost and a death is resurrected, permanently, since
        // neither moves the epoch (deliberately, see core's `event.rs`).
        //
        // **Except a birth from another generation (#741).** This queue is what the pull
        // was invalidated *around*, so it holds two kinds of birth that arrival order
        // cannot separate — one queued before the reflow that started the pull, and one
        // born after the pull went out. Replaying the first over the snapshot puts back a
        // line the pull had just repaired, permanently, since nothing bumps again. The
        // carried generation is the only thing that tells them apart, and dropping is the
        // right repair rather than a fallback: a snapshot newer than the birth already
        // contains that marker, at the line the reflow moved it to.
        for (const op of this.pendingOps) {
          if (!op.add) {
            this.lines.delete(op.id);
            continue;
          }
          // Equality, never order: core's counter is `wrapping_add`, so `<` is meaningless
          // across a wrap. A birth *newer* than the snapshot is dropped too — the frame
          // carrying its generation invalidates and re-pulls, which is the same recovery
          // path, and until then absent beats a line dated to a buffer this index is not
          // describing.
          if (op.epoch !== snap.epoch) continue;
          this.lines.set(op.id, { line: op.line, basis: op.basis });
        }
        this.pendingOps.length = 0;
        this.adopted = snap.epoch;
        this.lastAttemptFailed = false;
        this.onUpdated?.();
        // **Only the frame stream starts a pull.** A snapshot answering with an epoch the
        // frames have not reached is normal — the query and the frames are two samples of
        // one counter taken at different instants — and re-pulling here to chase it is an
        // unbounded microtask chain: every answer mismatches for the same reason the last
        // one did. Measured: it starved the demo's page until the renderer stopped
        // responding to navigation at all.
        //
        // Nothing is needed instead. `lineOf` already refuses while `adopted !== seen`, and
        // the next frame carries the newer epoch straight into `sync`, which pulls through
        // the ordinary path. The recovery is driven by a bounded source — frames — rather
        // than by a promise chasing its own tail.
      },
      () => {
        // Guarded like the resolve arm above, and it was not (#746). A rejection belongs to
        // the pull that made it, so an *orphaned* one — `reset()` left it running while a
        // new session started its own pull — must not clear the flag the live pull owns.
        // With the flag wrongly false, `onMarkerCreated`/`onMarkerDisposed` stop queueing,
        // and the landing snapshot's `lines.clear()` erases them with nothing to replay.
        if (seq !== this.seq) return;
        // A failed transport leaves the index empty rather than stale — the same choice
        // the in-flight window makes.
        this.inFlight = false;
        // The transport did not answer, so `sync`'s level trigger must not treat this as
        // "the index is behind and a pull would fix it": it would retry once per frame
        // against a dead port. How hard to retry someone else's transport is the host's
        // policy, not this class's — it sees one report (a rejected promise) and cannot
        // tell a blip from a dead session. A genuine epoch change clears this, so the
        // contract stays "one request per epoch change", unchanged since #490.
        this.lastAttemptFailed = true;
      },
    );
  }

  /**
   * A marker was created (core's `TermEvent::MarkerCreated`). `line` is absolute on the
   * basis current at creation, which is why it is stored with one.
   *
   * @param evictedTotal the event's own `evicted_total` — **not** the newest frame's.
   *   Those are different instants whenever the batch that created the marker went on to
   *   evict, and the difference is the number of lines the mark is misplaced by (#737).
   *   Required rather than defaulted so a host that has not wired it fails to compile
   *   instead of silently placing markers on the last frame's basis, which is what this
   *   method did before.
   *
   * @param epoch the event's own `epoch` — the marker generation `line` belongs to
   *   (#741). A basis dates a *uniform* move; anything that moves markers individually —
   *   a reflow, a region rotate — moves the generation instead, and a line from another
   *   generation is not stale by a delta, it is an answer about a different buffer.
   *   Measured: a mark at absolute 3 reflowed to 5 with the basis unmoved at 0.
   *
   * **Drain the events, then sync the frame.** That is now a *cost* preference rather
   * than a correctness one, which is the whole of what #741 changed. Syncing first leaves
   * the frame's `markerCount` one ahead of an index that has not been told yet, so the
   * drift check reconciles at `O(M)` a fact this event already delivered at `O(1)`.
   * Placement no longer depends on it, on either axis.
   */
  onMarkerCreated(
    id: number,
    line: number,
    _kind: number,
    evictedTotal: number,
    epoch: number,
  ): void {
    // A non-finite argument is refused rather than stored, and the refusal is the loud
    // option here rather than the quiet one: a stored `NaN` still counts toward
    // {@link MarkerIndexCache.size}, so `markerCount === lines.size` holds and the drift
    // check — the one thing watching for an index that has gone wrong — never fires
    // again. Dropping the entry leaves the count one ahead, which is exactly the
    // condition that re-pulls and heals. Reachable only from an untyped host, since
    // `evictedTotal` is a required parameter; this repo has met the same shape twice on
    // the producer side (#672, #675) and the rule is the same — a value the receiving
    // type cannot mean does not get stored.
    if (!Number.isFinite(line) || !Number.isFinite(evictedTotal) || !Number.isFinite(epoch)) return;
    // **A birth is adopted only into the generation it names (#741).** The queue below is
    // not the only way a stale birth arrives: an event channel slower than the frame
    // channel delivers one *after* the pull that repaired it has already landed, and that
    // birth never touches `pendingOps` at all. Frame mode allows exactly that — core has
    // no IPC (ADR-0017), so the host owns both transports and nothing couples their
    // latencies. The same check answers both, applied at the two points a line enters:
    // here against the generation this index describes, and in the replay against the one
    // the landing snapshot adopts.
    //
    // It also refuses a birth from the generation *ahead* of this one — the host drained
    // an event core queued after a bump the frame stream has not delivered yet. That line
    // is right for a buffer this index is not describing, and the frame carrying that
    // generation is one sync away. Waiting a frame is the same trade the in-flight window
    // makes: absent beats wrong.
    //
    // Only while a generation is known. `adopted === undefined` means either a pull is out
    // — in which case the push below carries the epoch and the replay decides — or the
    // transport failed, where `lineOf` refuses anyway and the next pull clears this map.
    // (A pull is always preceded by `invalidate()`, so `inFlight` implies `adopted ===
    // undefined` and this return can never swallow a needed push.)
    if (this.adopted !== undefined && epoch !== this.adopted) return;
    this.lines.set(id, { line, basis: evictedTotal });
    if (this.inFlight) this.pendingOps.push({ add: true, id, line, basis: evictedTotal, epoch });
  }

  /** A marker died (core's `TermEvent::MarkerDisposed`). Costs no re-pull — that is why
   * disposal deliberately does not move the epoch. */
  onMarkerDisposed(id: number): void {
    this.lines.delete(id);
    if (this.inFlight) this.pendingOps.push({ add: false, id });
  }

  /**
   * The marker's absolute line *as of the last synced frame*, or `undefined` when it is
   * not held — disposed, never seen, or invalidated by an epoch the index has not caught
   * up with. A caller must treat `undefined` as "do not project", never as line 0.
   */
  lineOf(id: number): number | undefined {
    // The contents must describe the buffer the newest frame came from.
    if (this.adopted === undefined || this.adopted !== this.seen) return undefined;
    const held = this.lines.get(id);
    if (!held) return undefined;
    return held.line - (this.basis - held.basis);
  }

  /** Detach from a session: drops the contents and orphans any pull still out, so a
   * response cannot repopulate the index of a widget that has moved on. The counterpart
   * to `AccessibilityController.reactivate` for this class's async state. */
  reset(): void {
    this.lines.clear();
    this.pendingOps.length = 0;
    this.basis = 0;
    this.adopted = undefined;
    this.seen = undefined;
    this.inFlight = false;
    // Stated rather than relied on: clearing `seen` already makes the next `sync` take the
    // epoch-change path, which clears this too. It becomes load-bearing the day `seen`
    // survives a reset, and a latch a reset does not clear is how a new session inherits a
    // dead session's refusal (#746).
    this.lastAttemptFailed = false;
    this.seq++;
  }

  /** How many markers the index currently holds — for a consumer that wants to compare
   * against a frame's own count, and for tests. */
  get size(): number {
    return this.lines.size;
  }
}
