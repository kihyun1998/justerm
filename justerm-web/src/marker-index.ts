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
  /** How many markers the engine holds (wire v16). Optional: absent on an older decoder,
   * and absent must not read as "zero markers". */
  readonly markerCount?: number;
}

type Op =
  | { readonly add: true; readonly id: number; readonly line: number; readonly basis: number }
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
  /** Events that landed while a pull was out, replayed onto the snapshot it returns. */
  private readonly pendingOps: Op[] = [];

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

    if (epoch === this.seen) {
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
        if (!this.inFlight) this.pull();
      }
      return;
    }

    this.seen = epoch;
    // What we hold describes a buffer that has moved non-uniformly. Drop it now rather
    // than serving it: a decoration missing for a frame is visible and self-correcting,
    // one painted on a line it no longer owns is neither.
    this.invalidate();
    // The cap. The `epoch === this.seen` branch above swallows a repeated epoch, so a
    // rejecting transport costs one request per epoch *change*, not one per frame — an
    // explicit "this epoch already failed" flag was tried and deleted: nothing could make
    // a test fail with it removed, because this line and `seen` already covered every
    // path to it.
    if (this.inFlight) return;
    this.pull();
  }

  /** Drop the contents and mark them unusable, leaving the flight and the frame-side
   * bookkeeping (`basis`, `seen`) alone. */
  private invalidate(): void {
    this.lines.clear();
    this.adopted = undefined;
    this.pendingOps.length = 0;
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
        for (const op of this.pendingOps) {
          if (op.add) this.lines.set(op.id, { line: op.line, basis: op.basis });
          else this.lines.delete(op.id);
        }
        this.pendingOps.length = 0;
        this.adopted = snap.epoch;
        // The frame stream may have moved on while we were asking. `lineOf` already
        // refuses to answer for a non-current epoch; ask again so it can resume.
        if (this.seen !== undefined && this.seen !== snap.epoch) this.pull();
      },
      () => {
        // A failed transport leaves the index empty rather than stale — the same choice
        // the in-flight window makes.
        this.inFlight = false;
      },
    );
  }

  /** A marker was created (core's `TermEvent::MarkerCreated`). `line` is absolute on the
   * basis current at creation, which is why it is stored with one. */
  onMarkerCreated(id: number, line: number, _kind: number): void {
    this.lines.set(id, { line, basis: this.basis });
    if (this.inFlight) this.pendingOps.push({ add: true, id, line, basis: this.basis });
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
    this.seq++;
  }

  /** How many markers the index currently holds — for a consumer that wants to compare
   * against a frame's own count, and for tests. */
  get size(): number {
    return this.lines.size;
  }
}
