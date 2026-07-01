import { describe, expect, it } from "vitest";
import {
  type AnnouncePolicy,
  CommandAnnounceController,
  type Enablement,
  type SignalSink,
} from "../src/command-announce";
import type { LiveRegionSink } from "../src/accessibility";
import { MarkerKind } from "../src/markers";

/** Records what the controller announces (reuses #119's aria-live sink). */
class RecLive implements LiveRegionSink {
  readonly said: string[] = [];
  announce(text: string): void {
    this.said.push(text);
  }
  clear(): void {}
}

/** Records the exit-driven earcon/aria signal the consumer would play. */
class RecSignal implements SignalSink {
  readonly signals: string[] = [];
  commandSucceeded(): void {
    this.signals.push("ok");
  }
  commandFailed(): void {
    this.signals.push("fail");
  }
}

/** One stride-5 marker record `(id, row, kind, exitPresent, exitBits)`. */
function marker(
  id: number,
  kind: MarkerKind,
  exit?: number,
): [number, number, number, number, number] {
  return [id, 0, kind, exit === undefined ? 0 : 1, exit === undefined ? 0 : exit];
}

/** A frame of the given kind carrying the given flat marker records. */
function frame(kind: number, ...records: number[][]) {
  return { kind, markerPositions: records.flat() };
}
const PARTIAL = 1;
const FULL = 0;

/** A controller whose baseline is already seeded (an empty first frame), so the
 * next `onFrame` is a real post-attach event — the common case under test. */
function seeded(live: LiveRegionSink, signal: SignalSink) {
  const c = new CommandAnnounceController(live, signal);
  c.onFrame(frame(PARTIAL)); // consume the first-frame baseline seed
  return c;
}

/** Seed a controller carrying a #167 per-outcome policy and/or a screen-reader
 * probe (for the `auto` state), then consume the baseline frame. */
function seededWith(
  live: LiveRegionSink,
  signal: SignalSink,
  opts: { policy?: AnnouncePolicy; screenReaderActive?: () => boolean },
) {
  const c = new CommandAnnounceController(live, signal, opts);
  c.onFrame(frame(PARTIAL));
  return c;
}

describe("CommandAnnounceController (#160)", () => {
  // A finished command with exit 0 → success announce + success signal (VSCode
  // `terminalCommandSucceeded`). Exit 0/undefined counts as success.
  it("announces and signals a successful command", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seeded(live, signal);

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));

    expect(live.said).toEqual(["Command succeeded"]);
    expect(signal.signals).toEqual(["ok"]);
  });

  // Non-zero exit → failure announce (with the code) + failure signal, matching
  // VSCode's `if (exitCode)` → `terminalCommandFailed`.
  it("announces and signals a failed command with its exit code", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seeded(live, signal);

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 130)));

    expect(live.said).toEqual(["Command failed, exit 130"]);
    expect(signal.signals).toEqual(["fail"]);
  });

  // No exit reported (`None` on the wire) is treated as success, matching VSCode
  // (undefined exitCode → succeeded signal).
  it("treats a missing exit code as success", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seeded(live, signal);

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished)));

    expect(live.said).toEqual(["Command succeeded"]);
    expect(signal.signals).toEqual(["ok"]);
  });

  // The same finished mark visible across many frames announces exactly once —
  // marker ids dedup via a Set without re-announcing on redraw.
  it("announces each finished command only once", () => {
    const live = new RecLive();
    const c = seeded(live, new RecSignal());

    const f = frame(PARTIAL, marker(7, MarkerKind.CommandFinished, 0));
    c.onFrame(f);
    c.onFrame(f); // same mark still visible next frame

    expect(live.said).toEqual(["Command succeeded"]);
  });

  // Only CommandFinished marks announce; prompt/command/output-start marks are
  // boundaries the nav feature (#166) uses, not announcements.
  it("ignores non-finished marks", () => {
    const live = new RecLive();
    const c = seeded(live, new RecSignal());

    c.onFrame(
      frame(
        PARTIAL,
        marker(1, MarkerKind.PromptStart),
        marker(2, MarkerKind.CommandStart),
        marker(3, MarkerKind.OutputStart),
      ),
    );

    expect(live.said).toEqual([]);
  });

  // The FIRST frame is the attach baseline (pre-existing history), so its marks
  // seed `seen` without announcing — a restored session must not read out every
  // past command as if it just ran.
  it("seeds the first frame's marks as baseline without announcing", () => {
    const live = new RecLive();
    const c = new CommandAnnounceController(live, new RecSignal());

    c.onFrame(frame(FULL, marker(1, MarkerKind.CommandFinished, 0)));
    expect(live.said).toEqual([]); // baseline, not announced

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));
    expect(live.said).toEqual([]); // already seen from the baseline
  });

  // #1 regression: a command that finishes while the user is scrolled up lands
  // off-screen (absent from frames), then first surfaces on the Full repaint when
  // they scroll back. It must be announced THEN — a Full frame must not suppress
  // a genuinely-new mark.
  it("announces a command first visible on a later Full frame", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seeded(live, signal); // baseline seeded with no marks

    c.onFrame(frame(FULL, marker(5, MarkerKind.CommandFinished, 1)));

    expect(live.said).toEqual(["Command failed, exit 1"]);
    expect(signal.signals).toEqual(["fail"]);
  });

  // #2 regression: a full reset (RIS / `tput reset`) disposes every marker and
  // reissues ids from 0. Forwarding disposal prunes `seen`, so a reused id is
  // announced afresh instead of being silently dropped as a stale duplicate.
  it("re-announces after a disposed id is reissued by a reset", () => {
    const live = new RecLive();
    const c = seeded(live, new RecSignal());

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));
    expect(live.said).toEqual(["Command succeeded"]);

    c.onMarkerDisposed(1); // RIS disposes markers, reissues ids from 0
    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));

    expect(live.said).toEqual(["Command succeeded", "Command succeeded"]);
  });
});

describe("CommandAnnounceController per-outcome policy (#167)", () => {
  // Mirrors VSCode's independent `{sound, announcement}` per outcome
  // (accessibilityConfiguration.ts terminalCommandSucceeded/Failed). Each of the
  // two modalities (announce / signal) is `on | off | auto`; `auto` fires iff a
  // screen reader is active (accessibilitySignalService.ts:274 checkEnabledState).

  const ALL_AUTO: AnnouncePolicy = {
    succeeded: { announce: "auto", signal: "auto" },
    failed: { announce: "auto", signal: "auto" },
  };

  // `off` on a modality suppresses just that modality, even with SR active — the
  // other modality still fires ("succeeded: earcon only, announce off").
  it("off on one modality suppresses only that modality", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seededWith(live, signal, {
      policy: { ...ALL_AUTO, succeeded: { announce: "off", signal: "auto" } },
    });

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));

    expect(live.said).toEqual([]); // announce off
    expect(signal.signals).toEqual(["ok"]); // signal still auto+SR-on
  });

  // The failed outcome is policed independently of succeeded: fail-announce off
  // but fail-signal on → the earcon plays, nothing is spoken.
  it("polices the failed outcome independently of succeeded", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seededWith(live, signal, {
      policy: {
        succeeded: { announce: "auto", signal: "auto" },
        failed: { announce: "off", signal: "on" },
      },
    });

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 3)));

    expect(live.said).toEqual([]);
    expect(signal.signals).toEqual(["fail"]);
  });

  // `auto` with NO screen reader → nothing fires (the point of `auto`: don't
  // announce/earcon to an absent SR). This is #161's gate, now resolved inside
  // the controller so `on` can still override it (next test).
  it("auto suppresses both modalities when the screen reader is inactive", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seededWith(live, signal, {
      policy: ALL_AUTO,
      screenReaderActive: () => false,
    });

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));

    expect(live.said).toEqual([]);
    expect(signal.signals).toEqual([]);
  });

  // `on` fires REGARDLESS of SR state — this is exactly what a blanket sink gate
  // (#161's gateSignal wrapping) cannot express, and why `auto` had to move into
  // the controller.
  it("on fires even when the screen reader is inactive", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seededWith(live, signal, {
      policy: { ...ALL_AUTO, succeeded: { announce: "on", signal: "on" } },
      screenReaderActive: () => false,
    });

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0)));

    expect(live.said).toEqual(["Command succeeded"]);
    expect(signal.signals).toEqual(["ok"]);
  });

  // #161 invariant preserved: a command suppressed by policy (auto + SR off) is
  // still marked seen, so flipping SR on later does NOT replay it. The gating
  // moved into the controller, but `seen.add` stays unconditional.
  it("does not replay a policy-suppressed command when SR later turns on", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    let sr = false;
    const c = seededWith(live, signal, {
      policy: ALL_AUTO,
      screenReaderActive: () => sr,
    });

    const f = frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 0));
    c.onFrame(f); // auto + SR off → suppressed, but marked seen
    expect(live.said).toEqual([]);

    sr = true;
    c.onFrame(f); // same mark, SR now on → still seen, must NOT re-announce
    expect(live.said).toEqual([]);
    expect(signal.signals).toEqual([]);
  });

  // Exhaustive matrix: outcome × state × SR-active, both modalities coupled to
  // the state under test. `enabled(state, sr)` = `on || (auto && sr)` is the
  // spec oracle (VSCode checkEnabledState, accessibilitySignalService.ts:274);
  // this asserts the controller against it for every cell so no combination is
  // covered only implicitly.
  const STATES: Enablement[] = ["on", "off", "auto"];
  const OUTCOMES = [
    { name: "succeeded", exit: 0, say: "Command succeeded", sig: "ok" },
    { name: "failed", exit: 5, say: "Command failed, exit 5", sig: "fail" },
  ] as const;
  for (const o of OUTCOMES) {
    for (const state of STATES) {
      for (const sr of [true, false]) {
        const fires = state === "on" || (state === "auto" && sr);
        it(`${o.name}: ${state} + SR ${sr ? "on" : "off"} → ${fires ? "fires" : "silent"}`, () => {
          const live = new RecLive();
          const signal = new RecSignal();
          // Couple both modalities to the state under test; the *other* outcome
          // is off and never reached (only one mark, of this outcome, is fed).
          const rule = { announce: state, signal: state };
          const other = { announce: "off", signal: "off" } as const;
          const policy: AnnouncePolicy =
            o.name === "succeeded"
              ? { succeeded: rule, failed: other }
              : { succeeded: other, failed: rule };
          const c = seededWith(live, signal, { policy, screenReaderActive: () => sr });

          c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, o.exit)));

          expect(live.said).toEqual(fires ? [o.say] : []);
          expect(signal.signals).toEqual(fires ? [o.sig] : []);
        });
      }
    }
  }

  // The default (no opts) is every modality `auto`; with #161's default-active SR
  // that reproduces the pre-#167 always-on behaviour (the existing suite above
  // exercises this via the 2-arg constructor).
  it("defaults to all-auto, preserving pre-#167 behaviour with SR active", () => {
    const live = new RecLive();
    const signal = new RecSignal();
    const c = seededWith(live, signal, {}); // no policy, no SR probe → default true

    c.onFrame(frame(PARTIAL, marker(1, MarkerKind.CommandFinished, 7)));

    expect(live.said).toEqual(["Command failed, exit 7"]);
    expect(signal.signals).toEqual(["fail"]);
  });
});
