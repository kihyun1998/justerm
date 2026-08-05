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
}

export class MarkerIndexCache {
  /** `markerId → (absolute line, the `evictedTotal` it was absolute at)`. */
  private readonly lines = new Map<number, { line: number; basis: number }>();
  /** The newest basis a frame reported, which `lineOf` rebases against. */
  private basis = 0;
  /** The epoch the current contents belong to; `undefined` until the first pull lands. */
  private epoch: number | undefined;
  /** Set while a pull is outstanding. Two jobs: it makes the index report *unknown*
   * rather than stale, and it is the once-per-epoch-change cap. */
  private inFlight = false;
  /** The epoch that triggered the pull in flight, so a *further* change during it is
   * still noticed rather than swallowed by the cap. */
  private pullingFor: number | undefined;

  constructor(private readonly port: MarkerPort) {}

  /**
   * Call once per frame, before projecting. Adopts the frame's basis and, if the epoch
   * moved, invalidates the index and asks for a fresh one.
   *
   * **At most one pull is in flight.** A marker sitting below a bottom margin bumps the
   * epoch on every output line (measured: 1 000 bumps over 1 000 region scrolls), so an
   * uncapped re-pull would cost `O(M)` per frame — exactly the cost this design removes.
   * With the cap the worst case is one outstanding request, and the frames in between
   * simply report unknown.
   */
  sync(frame: MarkerBasisFrame): void {
    if (frame.evictedTotal !== undefined) this.basis = frame.evictedTotal;
    const epoch = frame.markerEpoch;
    if (epoch === undefined) return;
    if (this.epoch === epoch) return;
    // The contents no longer describe this buffer. Drop them *now* rather than serving
    // them until the answer arrives: a decoration missing for a frame is visible and
    // self-correcting, a decoration painted on a line it no longer owns is neither.
    this.lines.clear();
    this.epoch = undefined;
    if (this.inFlight) {
      // One pull at a time — the cap. A pull already out for THIS epoch needs nothing;
      // one out for an older epoch will be stale on arrival, so record the newer epoch
      // and let the completion handler ask again rather than adopt it. Both cases are
      // this one assignment: re-recording the same epoch is a no-op, which is why there
      // is no separate branch for it (an earlier draft had one, and a mutation test
      // showed it could be deleted with every test still green — a guard nothing can
      // fail is not a guard).
      this.pullingFor = epoch;
      return;
    }
    this.pull(epoch);
  }

  private pull(forEpoch: number): void {
    this.inFlight = true;
    this.pullingFor = forEpoch;
    void this.port
      .index()
      .then((snap) => {
        this.inFlight = false;
        // Adopt only if the world has not moved again while we were asking.
        if (this.pullingFor !== snap.epoch) {
          const wanted = this.pullingFor;
          if (wanted !== undefined) this.pull(wanted);
          return;
        }
        this.lines.clear();
        for (const m of snap.markers) {
          this.lines.set(m.id, { line: m.line, basis: snap.evictedTotal });
        }
        this.epoch = snap.epoch;
      })
      .catch(() => {
        // A failed transport leaves the index empty rather than stale — the same choice
        // the in-flight window makes, for the same reason.
        this.inFlight = false;
      });
  }

  /** A marker was created (core's `TermEvent::MarkerCreated`). `line` is absolute on the
   * basis current at creation, which is why it is stored with one. */
  onMarkerCreated(id: number, line: number, _kind: number): void {
    this.lines.set(id, { line, basis: this.basis });
  }

  /** A marker died (core's `TermEvent::MarkerDisposed`). Costs no re-pull — that is why
   * disposal deliberately does not move the epoch. */
  onMarkerDisposed(id: number): void {
    this.lines.delete(id);
  }

  /**
   * The marker's absolute line *as of the last synced frame*, or `undefined` when it is
   * not held — disposed, never seen, or invalidated by an epoch the index has not caught
   * up with. A caller must treat `undefined` as "do not project", never as line 0.
   */
  lineOf(id: number): number | undefined {
    const held = this.lines.get(id);
    if (!held) return undefined;
    return held.line - (this.basis - held.basis);
  }

  /** How many markers the index currently holds — for a consumer that wants to compare
   * against a frame's own count, and for tests. */
  get size(): number {
    return this.lines.size;
  }
}
