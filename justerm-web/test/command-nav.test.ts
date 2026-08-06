import { describe, expect, it } from "vitest";
import {
  type CommandInfo,
  CommandNavController,
  type CommandNavPort,
  type NavView,
} from "../src/command-nav";
import type { LiveRegionSink } from "../src/accessibility";
import type { SignalSink } from "../src/command-announce";

/** A preset command list behind the query seam (mirrors StubAccessiblePort).
 * Counts asks, because *when* the list is sampled is half of #743's contract. */
class StubPort implements CommandNavPort {
  asks = 0;
  constructor(public list: CommandInfo[]) {}
  commands(): Promise<CommandInfo[]> {
    this.asks++;
    return Promise.resolve(this.list);
  }
}

class RecLive implements LiveRegionSink {
  readonly said: string[] = [];
  announce(text: string): void {
    this.said.push(text);
  }
  clear(): void {}
}

class RecSignal implements SignalSink {
  readonly signals: string[] = [];
  commandSucceeded(): void {
    this.signals.push("ok");
  }
  commandFailed(): void {
    this.signals.push("fail");
  }
}

/** A view holding a document of `lines` lines. `reveal` reports whether the line
 * exists in it — the state the nav asks for rather than tracking (#743). `close()`
 * tears the document down, exactly as `DomAccessibleView.hide` empties `lineEls`. */
class RecView implements NavView {
  readonly revealed: number[] = [];
  constructor(private lines = Number.POSITIVE_INFINITY) {}
  reveal(line: number): boolean {
    if (line >= this.lines) return false;
    this.revealed.push(line);
    return true;
  }
  close(): void {
    this.lines = 0;
  }
}

/** Three commands at document lines 0/2/5 — success, fail(1), success. */
function threeCommands(): CommandInfo[] {
  return [
    { line: 0, command: "echo one", exit: 0 },
    { line: 2, command: "false", exit: 1 },
    { line: 5, command: "echo two", exit: 0 },
  ];
}

function make(list: CommandInfo[], docLines = Number.POSITIVE_INFINITY) {
  const live = new RecLive();
  const signal = new RecSignal();
  const view = new RecView(docLines);
  const port = new StubPort(list);
  const c = new CommandNavController(port, live, signal, view);
  return { c, live, signal, view, port };
}

/** A controller whose list has been sampled, i.e. the host has summoned the view.
 * `load()` is the sampling point, so nav before it is meaningless (#743).
 * `docLines` bounds the document the view is holding; unbounded by default, so the
 * tests that are not about the pairing keep asserting only what they are about. */
async function summoned(list: CommandInfo[], docLines?: number) {
  const m = make(list, docLines);
  await m.c.load();
  return m;
}

describe("CommandNavController (#166)", () => {
  // Mirrors VSCode navigateToCommand: from the end, Previous jumps to the last
  // command (line < cursor, max), reveals it, announces the command text, and
  // fires the exit-driven signal (#160 reuse).
  it("Previous from the end jumps to the last command", async () => {
    const { c, live, signal, view } = await summoned(threeCommands());

    await c.previous();

    expect(view.revealed).toEqual([5]);
    expect(live.said).toEqual(["echo two"]);
    expect(signal.signals).toEqual(["ok"]);
  });

  // Repeated Previous walks upward through history (VSCode: line < cursor, max).
  it("Previous walks upward through commands", async () => {
    const { c, view, live } = await summoned(threeCommands());

    await c.previous(); // -> line 5
    await c.previous(); // -> line 2
    await c.previous(); // -> line 0

    expect(view.revealed).toEqual([5, 2, 0]);
    expect(live.said).toEqual(["echo two", "false", "echo one"]);
  });

  // Next moves forward (line > cursor, min) after having moved up.
  it("Next moves forward from the current position", async () => {
    const { c, view } = await summoned(threeCommands());

    await c.previous(); // 5
    await c.previous(); // 2
    await c.next(); //    5

    expect(view.revealed).toEqual([5, 2, 5]);
  });

  // A failed command fires the failure signal (exit != 0).
  it("fires the failure signal for a non-zero exit", async () => {
    const { c, signal } = await summoned(threeCommands());

    await c.previous(); // 5 ok
    await c.previous(); // 2 false -> fail

    expect(signal.signals).toEqual(["ok", "fail"]);
  });

  // Boundary clamp: Previous at the first command is a no-op (VSCode returns when
  // the filtered list is empty) — nothing revealed/announced/signalled.
  it("clamps at the top: Previous past the first command is a no-op", async () => {
    const { c, view, live, signal } = await summoned(threeCommands());

    await c.previous(); // 5
    await c.previous(); // 2
    await c.previous(); // 0
    await c.previous(); // clamp

    expect(view.revealed).toEqual([5, 2, 0]);
    expect(live.said).toEqual(["echo two", "false", "echo one"]);
    expect(signal.signals).toEqual(["ok", "fail", "ok"]);
  });

  // Boundary clamp at the bottom: Next from the end is a no-op.
  it("clamps at the bottom: Next from the end is a no-op", async () => {
    const { c, view } = await summoned(threeCommands());

    await c.next(); // nothing below the end cursor

    expect(view.revealed).toEqual([]);
  });

  // No commands at all → every nav is inert.
  it("is inert with no commands", async () => {
    const { c, view, live, signal } = await summoned([]);

    await c.previous();
    await c.next();

    expect(view.revealed).toEqual([]);
    expect(live.said).toEqual([]);
    expect(signal.signals).toEqual([]);
  });

  // An empty command string (e.g. a bare Enter) still reveals + signals, but does
  // NOT announce — VSCode only alerts when `commandLine` is non-empty.
  it("reveals and signals but does not announce an empty command", async () => {
    const { c, view, live, signal } = await summoned([{ line: 3, command: "", exit: 0 }]);

    await c.previous();

    expect(view.revealed).toEqual([3]);
    expect(live.said).toEqual([]); // empty command: no alert
    expect(signal.signals).toEqual(["ok"]);
  });

  // A missing exit is treated as success (mirrors #160 / VSCode undefined -> ok).
  it("treats a missing exit as success", async () => {
    const { c, signal } = await summoned([{ line: 1, command: "sleep" }]);

    await c.previous();

    expect(signal.signals).toEqual(["ok"]);
  });

  // load() re-queries and resets the reading cursor to the end, so a re-summon
  // starts Previous from the last command again (not wherever it left off).
  it("load() resets the cursor to the end", async () => {
    const { c, view } = await summoned(threeCommands());

    await c.previous(); // 5
    await c.previous(); // 2
    await c.load(); // re-summon: cursor back to end
    await c.previous(); // 5 again

    expect(view.revealed).toEqual([5, 2, 5]);
  });

  // #743 — a command line is a *document* line: it only means anything against the
  // document the accessible view is holding, and `load()` is where both are sampled.
  // Navigating before that used to sample the list on its own, at an instant no
  // document was taken at, and then hold it for the rest of the session.
  describe("the list is sampled with the document it indexes (#743)", () => {
    it("does nothing before the view has been summoned", async () => {
      const { c, view, live, signal, port } = make(threeCommands());

      await c.previous();
      await c.next();

      expect(view.revealed).toEqual([]);
      expect(live.said).toEqual([]);
      expect(signal.signals).toEqual([]);
      expect(port.asks).toBe(0); // and it did not sample a list on its own
    });

    // The other half, and the reason this is not fixed by re-asking per jump: the
    // lines index the document `summon()` captured. A list newer than that document
    // is wrong in exactly the same way a list older than it is — off by however many
    // rows moved, silently, because a stale document line is a plausible one.
    it("does not re-ask while the view stays open", async () => {
      const { c, port } = await summoned(threeCommands());
      expect(port.asks).toBe(1);

      await c.previous();
      await c.previous();
      await c.next();

      expect(port.asks).toBe(1);
    });

    // Re-summoning re-samples: that is the re-ask discharge (ADR-0029 D3), and the
    // only thing that makes holding the answer legitimate in between.
    it("re-asks when the view is summoned again", async () => {
      const { c, port } = await summoned(threeCommands());

      await c.load();

      expect(port.asks).toBe(2);
    });

    // `loaded` is an EDGE — set once, cleared nowhere — so on its own it says the
    // list was sampled, not that the document it indexes is still open. The view owns
    // that state and is asked for it (the shape #746/#747 reached for the sibling
    // coordinate cache). Without this, close-then-Previous announced a command and
    // played its earcon while the reading cursor stayed put, because `reveal` no-ops
    // silently on a torn-down document.
    it("announces nothing once the document it indexes is gone", async () => {
      const { c, view, live, signal } = await summoned(threeCommands());
      await c.previous();
      view.close();

      await c.previous();

      expect(view.revealed).toEqual([5]); // the pre-close jump, and nothing after it
      expect(live.said).toEqual(["echo two"]);
      expect(signal.signals).toEqual(["ok"]);
    });

    // The general form, and the one that also covers the alt screen: a line the held
    // document does not contain is a jump that did not happen. Nothing is announced,
    // and the reading cursor does not move — so a retry is not silently swallowed.
    it("does not announce or move the cursor for a line the document lacks", async () => {
      // A 3-line document while the commands sit at 0, 2 and 5 — the shape a list
      // sampled against a different instant produces, and the shape the alt screen
      // produces (a short document, primary-document lines).
      const { c, view, live, signal } = await summoned(threeCommands(), 3);

      await c.previous(); // targets line 5, absent from a 3-line document
      await c.previous(); // and retries 5 — the cursor did not move past it

      expect(view.revealed).toEqual([]);
      expect(live.said).toEqual([]);
      expect(signal.signals).toEqual([]);
    });
  });
});
