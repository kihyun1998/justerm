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

  it("is a no-op with a provider that omits writeText — writes refused, reads untouched", async () => {
    const readText = vi.fn(() => "should not be reached");
    const c = new ClipboardController({ provider: provider({ readText }) });
    await c.handle({ type: "clipboardStore", target: "clipboard", text: TMUX_TEXT });
    expect(readText).not.toHaveBeenCalled();
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
