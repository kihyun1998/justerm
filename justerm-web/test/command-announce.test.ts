import { describe, expect, it } from "vitest";
import {
  CommandAnnounceController,
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
