import { describe, expect, it, vi } from "vitest";
import {
  ClipboardController,
  StubClipboardPort,
  type ClipboardProvider,
} from "../src/clipboard";

// The bytes tmux 3.2a was measured emitting on the RHEL 9 VM (core #828,
// `justerm-core/tests/fixtures/tmux_clipboard.raw`) decode to this, on the EMPTY
// target field — which core resolves to "clipboard". The fixture is the reason
// the store cases below use this text rather than a synthesised one.
const TMUX_TEXT = "HELLOJUSTERM";

/** A provider recording both directions; each half is independently omittable,
 * which is the shape a consumer uses to honour writes and refuse reads. */
function provider(over: Partial<ClipboardProvider> = {}): ClipboardProvider {
  return over;
}

describe("ClipboardController — a store", () => {
  it("hands the decoded text to the provider, on the target that arrived", async () => {
    const writeText = vi.fn();
    const c = new ClipboardController({ provider: provider({ writeText }) });
    await c.handle({ type: "clipboardStore", target: "clipboard", text: TMUX_TEXT });
    expect(writeText).toHaveBeenCalledWith("clipboard", TMUX_TEXT);
  });

  it("carries the primary and selection targets through unresolved", async () => {
    const writeText = vi.fn();
    const c = new ClipboardController({ provider: provider({ writeText }) });
    await c.handle({ type: "clipboardStore", target: "primary", text: "p" });
    await c.handle({ type: "clipboardStore", target: "selection", text: "s" });
    expect(writeText).toHaveBeenNthCalledWith(1, "primary", "p");
    expect(writeText).toHaveBeenNthCalledWith(2, "selection", "s");
  });

  it("stores EMPTY text rather than filtering it — an empty payload is the clear idiom", async () => {
    const writeText = vi.fn();
    const c = new ClipboardController({ provider: provider({ writeText }) });
    await c.handle({ type: "clipboardStore", target: "clipboard", text: "" });
    expect(writeText).toHaveBeenCalledWith("clipboard", "");
  });

  it("is a no-op with no provider at all — the widget holds no clipboard policy", async () => {
    const c = new ClipboardController({});
    await expect(
      c.handle({ type: "clipboardStore", target: "clipboard", text: TMUX_TEXT }),
    ).resolves.toBeUndefined();
  });

  // The bare version of this ("a store with no writeText calls nothing") is
  // structurally guaranteed — `store()` names `readText` on no path, so no mutation
  // of the write guard can redden it. What is NOT guaranteed is that refusing one
  // direction leaves the other alone, so the positive control is the test.
  it("a provider that omits writeText refuses writes and still answers reads", async () => {
    const port = new StubClipboardPort();
    const readText = vi.fn(() => "still readable");
    const c = new ClipboardController({ port, provider: provider({ readText }) });

    await c.handle({ type: "clipboardStore", target: "clipboard", text: TMUX_TEXT });
    expect(readText, "a store must not reach the read half").not.toHaveBeenCalled();
    expect(port.reports, "and a store is never answered").toEqual([]);

    // POSITIVE CONTROL — the refused write did not disable the controller.
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(port.reports).toEqual([
      { target: "clipboard", text: "still readable", terminator: "st" },
    ]);
  });

  it("swallows a rejecting writeText instead of leaking an unhandled rejection", async () => {
    const writeText = vi.fn(() => Promise.reject(new Error("NotAllowedError")));
    const c = new ClipboardController({ provider: provider({ writeText }) });
    await expect(
      c.handle({ type: "clipboardStore", target: "clipboard", text: TMUX_TEXT }),
    ).resolves.toBeUndefined();
    expect(writeText).toHaveBeenCalledTimes(1);
  });
});

describe("ClipboardController — a query", () => {
  it("answers on the port with the target AND the terminator that arrived", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({
      port,
      provider: provider({ readText: () => "local clipboard" }),
    });
    await c.handle({ type: "clipboardQuery", target: "primary", terminator: "bel" });
    expect(port.reports).toEqual([
      { target: "primary", text: "local clipboard", terminator: "bel" },
    ]);
  });

  it("echoes the ST terminator too — the reply follows the question, not a default", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({ port, provider: provider({ readText: () => "x" }) });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(port.reports[0]?.terminator).toBe("st");
  });

  it("awaits an async readText and answers with the resolved text", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({
      port,
      provider: provider({ readText: () => Promise.resolve("async text") }),
    });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(port.reports[0]?.text).toBe("async text");
  });

  it("stays SILENT with no readText — a refusal sends no reply, it does not send an empty one", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({ port, provider: provider({ writeText: vi.fn() }) });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(port.reports).toEqual([]);
  });

  it("stays silent when readText returns null — a per-call refusal", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({ port, provider: provider({ readText: () => null }) });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(port.reports).toEqual([]);
  });

  it("stays silent when readText rejects — the browser denies a stream-driven read", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({
      port,
      provider: provider({ readText: () => Promise.reject(new Error("NotAllowedError")) }),
    });
    await expect(
      c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" }),
    ).resolves.toBeUndefined();
    expect(port.reports).toEqual([]);
  });

  it("does not read the clipboard at all when there is no port to answer on", async () => {
    const readText = vi.fn(() => "secret");
    const c = new ClipboardController({ provider: provider({ readText }) });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(readText).not.toHaveBeenCalled();
  });
});

describe("ClipboardController — the shared event stream", () => {
  it("ignores a notification event travelling the same channel", async () => {
    const port = new StubClipboardPort();
    const writeText = vi.fn();
    const c = new ClipboardController({ port, provider: provider({ writeText }) });
    await c.handle({ type: "title", title: "vim — notes.md" });
    await c.handle({ type: "bell" });
    expect(writeText).not.toHaveBeenCalled();
    expect(port.reports).toEqual([]);
  });
});

describe("ClipboardController — an EMPTY clipboard is an answer, not a refusal", () => {
  // The mirror of the store side's clear idiom, and the one predicate that
  // separates the two: `if (!text) return` passes every other test in this file
  // while hanging an application over an ordinary empty clipboard.
  it('answers with "" when the clipboard is empty — the app is blocked waiting for a reply', async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({ port, provider: provider({ readText: () => "" }) });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "bel" });
    expect(port.reports).toEqual([{ target: "clipboard", text: "", terminator: "bel" }]);
  });
});

describe("ClipboardController — a query does not overtake the store it follows", () => {
  // A store and a query arrive back-to-back in ONE drained batch and `Terminal`
  // floats each, so without serialisation the read observes the pre-store clipboard.
  it("waits for an in-flight write to the same target before reading", async () => {
    const port = new StubClipboardPort();
    let clipboard = "OLD";
    let releaseWrite!: () => void;
    const c = new ClipboardController({
      port,
      provider: provider({
        writeText: (_t, text) =>
          new Promise<void>((resolve) => {
            releaseWrite = () => {
              clipboard = text; // the platform commits only when the write settles
              resolve();
            };
          }),
        readText: () => clipboard,
      }),
    });

    // Exactly the batch shape: both floated, in arrival order, neither awaited.
    const store = c.handle({ type: "clipboardStore", target: "clipboard", text: "NEW" });
    const query = c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });

    // THE WINDOW EXISTS: the write has not settled, and the query has not answered.
    await Promise.resolve();
    expect(port.reports, "nothing may be answered while the write is in flight").toEqual([]);

    releaseWrite();
    await Promise.all([store, query]);
    expect(port.reports).toEqual([{ target: "clipboard", text: "NEW", terminator: "st" }]);
  });

  it("does not let a write to one target stall a read of another", async () => {
    const port = new StubClipboardPort();
    const c = new ClipboardController({
      port,
      provider: provider({
        writeText: () => new Promise<void>(() => {}), // never settles — the measured browser shape
        readText: (t) => `contents of ${t}`,
      }),
    });
    void c.handle({ type: "clipboardStore", target: "primary", text: "hangs forever" });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(port.reports).toEqual([
      { target: "clipboard", text: "contents of clipboard", terminator: "st" },
    ]);
  });
});

describe("ClipboardController — dispose latches the landing", () => {
  // Unsubscribing from the event channel cannot help: an in-flight read is already
  // past it. A browser read was measured pending indefinitely on the permission
  // prompt, so "the host unmounts, then the user clicks Allow" is the ordinary
  // sequence rather than a race to be hand-waved.
  it("a read that settles AFTER dispose reports nothing", async () => {
    const port = new StubClipboardPort();
    let allow!: (text: string) => void;
    const c = new ClipboardController({
      port,
      provider: provider({ readText: () => new Promise<string>((r) => (allow = r)) }),
    });

    const inFlight = c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "bel" });
    c.dispose();
    allow("the user's clipboard"); // the prompt is answered after teardown
    await inFlight;

    expect(port.reports, "a disposed widget answers nothing").toEqual([]);
  });

  it("ignores events that arrive after dispose", async () => {
    const port = new StubClipboardPort();
    const writeText = vi.fn();
    const c = new ClipboardController({ port, provider: provider({ writeText, readText: () => "x" }) });
    c.dispose();
    await c.handle({ type: "clipboardStore", target: "clipboard", text: TMUX_TEXT });
    await c.handle({ type: "clipboardQuery", target: "clipboard", terminator: "st" });
    expect(writeText).not.toHaveBeenCalled();
    expect(port.reports).toEqual([]);
  });
});
