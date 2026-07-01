/**
 * Command/exit announce + success/fail signal (#160), the frame-mode analog of
 * VSCode's terminal command a11y. VSCode fires this automatically on *every*
 * command finish (`decorationAddon.ts` `onCommandFinished` → `playSignal`, which
 * both speaks the outcome via `status()` and plays an earcon) — not only on
 * navigation. When an OSC-133 `CommandFinished` mark (#158/#159) first becomes
 * visible on a frame, this announces the outcome to the screen reader and fires
 * an exit-driven signal, once per command.
 *
 * Pure logic — the aria-live sink ({@link LiveRegionSink}) and the earcon/aria
 * {@link SignalSink} are injected (ADR-0017: the marks are core's, the announce/
 * signal *policy* is the consumer's). Prompt-to-prompt navigation is a separate
 * slice (#166); a per-outcome enable/verbosity policy is #167.
 */

import type { LiveRegionSink } from "./accessibility";
import { MarkerKind, readMarkers } from "./markers";

/**
 * The exit-driven outcome signal — a success/failure earcon or aria cue the
 * consumer plays (the a11y counterpart to a green/red prompt a sighted user
 * sees). Web policy; a thin adapter over the Web Audio API / a live region
 * satisfies it. Mirrors VSCode `terminalCommandSucceeded` / `terminalCommandFailed`.
 */
export interface SignalSink {
  /** The command exited 0 (or reported no code). */
  commandSucceeded(): void;
  /** The command exited non-zero. */
  commandFailed(): void;
}

/**
 * Announces finished commands exactly once and fires their success/fail signal.
 * Drive it with every decoded frame; it reads the frame's `markerPositions`, and
 * you must forward marker disposal via {@link onMarkerDisposed}.
 *
 * **Wire the `live` sink to a SEPARATE, *polite* aria-live region** — not the
 * one #119's {@link AccessibilityController} uses for output. Sharing one region
 * lets an output flush (debounced) or an `onKey`/`onBlur` clear clobber a command
 * announce; and VSCode speaks the outcome on a polite `status()` channel (which
 * doesn't interrupt ongoing speech), so an assertive region would over-interrupt.
 */
export class CommandAnnounceController {
  /** Finished-mark ids already handled. Marker ids are monotonic *within an
   * engine instance* but are reissued from 0 by a full reset (RIS / `tput
   * reset`), so ids are pruned on disposal ({@link onMarkerDisposed}) rather than
   * assumed unique forever — otherwise a reused id would be silently dropped. */
  private readonly seen = new Set<number>();

  /** Whether the baseline has been seeded. The first frame is the state at
   * attach — its marks are pre-existing history, not commands that just ran — so
   * it seeds `seen` without announcing (mirrors #119's first-frame baseline).
   * Every later frame announces genuinely-new marks, Full or Partial alike. */
  private seeded = false;

  constructor(
    private readonly live: LiveRegionSink,
    private readonly signal: SignalSink,
  ) {}

  /**
   * Process one frame. A `CommandFinished` mark not seen before is announced +
   * signalled — except on the first frame, which only seeds the baseline (so
   * pre-existing commands aren't announced as if they just ran).
   *
   * A mark is announced the first time it becomes *visible*, whatever the frame
   * kind: a command that finishes while the user is scrolled up lands off-screen
   * and only surfaces (on a Full repaint) when they scroll back — it must still
   * be announced then, so this does NOT skip Full frames.
   */
  onFrame(frame: { kind: number; markerPositions?: ArrayLike<number> }): void {
    const finished = readMarkers(frame.markerPositions).filter(
      (m) => m.kind === MarkerKind.CommandFinished,
    );
    if (!this.seeded) {
      this.seeded = true;
      for (const m of finished) this.seen.add(m.id);
      return;
    }
    for (const m of finished) {
      if (this.seen.has(m.id)) continue;
      this.seen.add(m.id);
      const failed = m.exit !== undefined && m.exit !== 0;
      if (failed) {
        this.live.announce(`Command failed, exit ${m.exit}`);
        this.signal.commandFailed();
      } else {
        this.live.announce("Command succeeded");
        this.signal.commandSucceeded();
      }
    }
  }

  /**
   * Forward a marker's disposal (the backend's `MarkerDisposed` event, out-of-band
   * from frames). Pruning the id keeps `seen` bounded over a long session and —
   * critically — lets a full reset work: RIS disposes every marker and reissues
   * ids from 0, so without this a reused id would collide with a stale `seen`
   * entry and its command would never be announced.
   */
  onMarkerDisposed(id: number): void {
    this.seen.delete(id);
  }
}
