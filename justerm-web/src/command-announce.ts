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
 * Per-modality enablement (#167), the subset of VSCode's `EnabledState`
 * (`accessibilitySignalService.ts:272`) justerm has a source for: `on` always
 * fires, `off` never, `auto` fires iff a screen reader is active (#161). VSCode
 * resolves `auto` *inside* `playSignal` (line 274 `checkEnabledState`) by reading
 * the SR-attached observable — so justerm resolves it in the controller too,
 * NOT by wrapping the sink. A blanket sink gate (#161's `gateSignal`) cannot
 * express `on`, since the outer gate would still suppress it while SR is off.
 */
export type Enablement = "on" | "off" | "auto";

/**
 * Per-outcome policy: the aria-live `announce` and the earcon `signal` are
 * enabled independently, mirroring VSCode's per-`{sound, announcement}` config on
 * `terminalCommandSucceeded` / `terminalCommandFailed`
 * (`accessibilityConfiguration.ts`).
 *
 * Note VSCode restricts the announcement modality to `auto | off` (announcing
 * with no SR present reaches nobody); justerm keeps the uniform {@link Enablement}
 * for both so a consumer *can* opt into `announce: "on"` explicitly (e.g. it pipes
 * the live region somewhere else), at the cost of that one divergence.
 */
export interface OutcomePolicy {
  /** Speak the outcome on the aria-live region (VSCode `announcement`). */
  announce: Enablement;
  /** Play the success/fail earcon (VSCode `sound`). */
  signal: Enablement;
}

/** The enable matrix passed to {@link CommandAnnounceController}. Verbosity of
 * the announce *text* is a separate concern (tracked apart from this slice). */
export interface AnnouncePolicy {
  /** Exit 0, or no exit code reported. */
  succeeded: OutcomePolicy;
  /** Non-zero exit. */
  failed: OutcomePolicy;
}

/** VSCode's defaults: every modality `auto` (`sound`/`announcement` both default
 * `auto`). Combined with #161's default-active SR state this reproduces the
 * pre-#167 always-announce behaviour, so wiring the controller with no policy is
 * a no-op change. */
export const DEFAULT_ANNOUNCE_POLICY: AnnouncePolicy = {
  succeeded: { announce: "auto", signal: "auto" },
  failed: { announce: "auto", signal: "auto" },
};

/** Mirror of VSCode `checkEnabledState` (`accessibilitySignalService.ts:274`),
 * minus the `always` / `userGesture` / `never` states justerm has no source for. */
function enabled(state: Enablement, srActive: boolean): boolean {
  return state === "on" || (state === "auto" && srActive);
}

/**
 * Formats the spoken text for a finished command (#179). The *text* is pure
 * presentation policy (ADR-0017), so the consumer injects it wholesale — the
 * controller owns only *when* to speak (dedup + the #167 enable gate), never
 * *what*. `exit` is the non-zero code on `"failed"` and `undefined`/`0` on
 * `"succeeded"` (which never renders it). Parameterizing this instead of an enum
 * subsumes terse/verbose *and* localization in one seam. Mirrors VSCode's
 * localized `announcementMessage` (`accessibilitySignalService.ts`).
 */
export type AnnounceText = (outcome: "succeeded" | "failed", exit: number | undefined) => string;

/**
 * Default formatter = the pre-#179 verbose wording: a failure carries its exit
 * code. This is #167 F2 — an intentional enhancement over VSCode's exit-less
 * `"Command Failed"` (the code is useful to a non-sighted user who has no red
 * decoration to read) — so wiring the controller with no formatter is a no-op.
 */
export const VERBOSE_ANNOUNCE_TEXT: AnnounceText = (outcome, exit) =>
  outcome === "failed" ? `Command failed, exit ${exit}` : "Command succeeded";

/**
 * VSCode-parity formatter: the failure omits the exit code, matching
 * `accessibilitySignalService.ts`'s `announcementMessage` (`"Command Failed"`).
 * A sighted user reads the code off the red decoration, so terse mode drops the
 * redundant number. Success text is identical to {@link VERBOSE_ANNOUNCE_TEXT}
 * (success never renders an exit either way).
 */
export const TERSE_ANNOUNCE_TEXT: AnnounceText = (outcome) =>
  outcome === "failed" ? "Command failed" : "Command succeeded";

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

  /**
   * @param opts.policy per-outcome × per-modality enable matrix (#167). Defaults
   *   to {@link DEFAULT_ANNOUNCE_POLICY} (all `auto`).
   * @param opts.screenReaderActive probes SR presence for the `auto` state (#161).
   *   Wire it to the shared {@link ScreenReaderState} (`() => srState.isActive()`)
   *   and do NOT also wrap these sinks with `gateLive`/`gateSignal` — this
   *   controller now owns the gating, and double-gating would break `on`.
   *   Defaults to `() => true` (SR active), matching #161's default.
   * @param opts.announceText formats the spoken text per outcome (#179). Defaults
   *   to {@link VERBOSE_ANNOUNCE_TEXT} (failure carries its exit code); pass
   *   {@link TERSE_ANNOUNCE_TEXT} for VSCode-parity terse wording, or any custom
   *   `(outcome, exit) => string`. Orthogonal to `policy`: the enable gate decides
   *   *whether* to speak, this decides *what*.
   */
  constructor(
    private readonly live: LiveRegionSink,
    private readonly signal: SignalSink,
    private readonly opts: {
      policy?: AnnouncePolicy;
      screenReaderActive?: () => boolean;
      announceText?: AnnounceText;
    } = {},
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
    const policy = this.opts.policy ?? DEFAULT_ANNOUNCE_POLICY;
    // Read SR presence once per frame (all its marks share one instant). Resolved
    // here rather than in a sink wrapper so `on` can override an SR-off state.
    const srActive = this.opts.screenReaderActive?.() ?? true;
    const announceText = this.opts.announceText ?? VERBOSE_ANNOUNCE_TEXT;
    for (const m of finished) {
      if (this.seen.has(m.id)) continue;
      // Mark seen UNCONDITIONALLY — before any enable check — so a policy- or
      // SR-suppressed command is still deduped and never replays when a modality
      // is later enabled (#161's invariant, preserved now that gating lives here).
      this.seen.add(m.id);
      const failed = m.exit !== undefined && m.exit !== 0;
      const rule = failed ? policy.failed : policy.succeeded;
      if (enabled(rule.announce, srActive)) {
        this.live.announce(announceText(failed ? "failed" : "succeeded", m.exit));
      }
      if (enabled(rule.signal, srActive)) {
        if (failed) this.signal.commandFailed();
        else this.signal.commandSucceeded();
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
