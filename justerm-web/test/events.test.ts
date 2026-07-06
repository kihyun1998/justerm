import { describe, it, expect, vi } from "vitest";
import { dispatchTermEvent } from "../src/events";

describe("dispatchTermEvent", () => {
  it("routes a title event to onTitle with the title string", () => {
    const onTitle = vi.fn();
    dispatchTermEvent({ type: "title", title: "vim — notes.md" }, { onTitle });
    expect(onTitle).toHaveBeenCalledWith("vim — notes.md");
  });

  it("routes a bell event to onBell (no argument)", () => {
    const onBell = vi.fn();
    dispatchTermEvent({ type: "bell" }, { onBell });
    expect(onBell).toHaveBeenCalledTimes(1);
    expect(onBell).toHaveBeenCalledWith();
  });

  it("routes a cwd event to onCwd with the URI", () => {
    const onCwd = vi.fn();
    dispatchTermEvent({ type: "cwd", cwd: "file://host/home/ki" }, { onCwd });
    expect(onCwd).toHaveBeenCalledWith("file://host/home/ki");
  });

  it("is a no-op when the matching handler is absent (all handlers optional)", () => {
    expect(() => dispatchTermEvent({ type: "title", title: "x" }, {})).not.toThrow();
    expect(() => dispatchTermEvent({ type: "bell" }, {})).not.toThrow();
    expect(() => dispatchTermEvent({ type: "cwd", cwd: "file://h/p" }, {})).not.toThrow();
  });

  it("ignores an unknown event type (a palette/query event on the same stream)", () => {
    const onTitle = vi.fn();
    const onBell = vi.fn();
    // A #122 colour event shares the drain stream; the notification dispatcher skips it.
    dispatchTermEvent({ type: "setBackground", spec: "rgb:00/00/00" } as never, { onTitle, onBell });
    expect(onTitle).not.toHaveBeenCalled();
    expect(onBell).not.toHaveBeenCalled();
  });
});
