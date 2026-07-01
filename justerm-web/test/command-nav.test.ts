import { describe, expect, it } from "vitest";
import {
  type CommandInfo,
  CommandNavController,
  type CommandNavPort,
  type NavView,
} from "../src/command-nav";
import type { LiveRegionSink } from "../src/accessibility";
import type { SignalSink } from "../src/command-announce";

/** A preset command list behind the query seam (mirrors StubAccessiblePort). */
class StubPort implements CommandNavPort {
  constructor(public list: CommandInfo[]) {}
  commands(): Promise<CommandInfo[]> {
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

class RecView implements NavView {
  readonly revealed: number[] = [];
  reveal(line: number): void {
    this.revealed.push(line);
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

function make(list: CommandInfo[]) {
  const live = new RecLive();
  const signal = new RecSignal();
  const view = new RecView();
  const c = new CommandNavController(new StubPort(list), live, signal, view);
  return { c, live, signal, view };
}

describe("CommandNavController (#166)", () => {
  // Mirrors VSCode navigateToCommand: from the end, Previous jumps to the last
  // command (line < cursor, max), reveals it, announces the command text, and
  // fires the exit-driven signal (#160 reuse).
  it("Previous from the end jumps to the last command", async () => {
    const { c, live, signal, view } = make(threeCommands());

    await c.previous();

    expect(view.revealed).toEqual([5]);
    expect(live.said).toEqual(["echo two"]);
    expect(signal.signals).toEqual(["ok"]);
  });

  // Repeated Previous walks upward through history (VSCode: line < cursor, max).
  it("Previous walks upward through commands", async () => {
    const { c, view, live } = make(threeCommands());

    await c.previous(); // -> line 5
    await c.previous(); // -> line 2
    await c.previous(); // -> line 0

    expect(view.revealed).toEqual([5, 2, 0]);
    expect(live.said).toEqual(["echo two", "false", "echo one"]);
  });

  // Next moves forward (line > cursor, min) after having moved up.
  it("Next moves forward from the current position", async () => {
    const { c, view } = make(threeCommands());

    await c.previous(); // 5
    await c.previous(); // 2
    await c.next(); //    5

    expect(view.revealed).toEqual([5, 2, 5]);
  });

  // A failed command fires the failure signal (exit != 0).
  it("fires the failure signal for a non-zero exit", async () => {
    const { c, signal } = make(threeCommands());

    await c.previous(); // 5 ok
    await c.previous(); // 2 false -> fail

    expect(signal.signals).toEqual(["ok", "fail"]);
  });

  // Boundary clamp: Previous at the first command is a no-op (VSCode returns when
  // the filtered list is empty) — nothing revealed/announced/signalled.
  it("clamps at the top: Previous past the first command is a no-op", async () => {
    const { c, view, live, signal } = make(threeCommands());

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
    const { c, view } = make(threeCommands());

    await c.next(); // nothing below the end cursor

    expect(view.revealed).toEqual([]);
  });

  // No commands at all → every nav is inert.
  it("is inert with no commands", async () => {
    const { c, view, live, signal } = make([]);

    await c.previous();
    await c.next();

    expect(view.revealed).toEqual([]);
    expect(live.said).toEqual([]);
    expect(signal.signals).toEqual([]);
  });

  // An empty command string (e.g. a bare Enter) still reveals + signals, but does
  // NOT announce — VSCode only alerts when `commandLine` is non-empty.
  it("reveals and signals but does not announce an empty command", async () => {
    const { c, view, live, signal } = make([{ line: 3, command: "", exit: 0 }]);

    await c.previous();

    expect(view.revealed).toEqual([3]);
    expect(live.said).toEqual([]); // empty command: no alert
    expect(signal.signals).toEqual(["ok"]);
  });

  // A missing exit is treated as success (mirrors #160 / VSCode undefined -> ok).
  it("treats a missing exit as success", async () => {
    const { c, signal } = make([{ line: 1, command: "sleep" }]);

    await c.previous();

    expect(signal.signals).toEqual(["ok"]);
  });

  // load() re-queries and resets the reading cursor to the end, so a re-summon
  // starts Previous from the last command again (not wherever it left off).
  it("load() resets the cursor to the end", async () => {
    const { c, view } = make(threeCommands());

    await c.previous(); // 5
    await c.previous(); // 2
    await c.load(); // re-summon: cursor back to end
    await c.previous(); // 5 again

    expect(view.revealed).toEqual([5, 2, 5]);
  });
});
