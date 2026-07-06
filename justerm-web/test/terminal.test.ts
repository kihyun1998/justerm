import { describe, it, expect, vi } from "vitest";
import { Terminal, rendererNotifyingSink, routeWheel, wheelGoesToApp, wheelScrollTarget } from "../src/terminal";
import { StubFrameSource } from "../src/frame-source";
import { MouseEvents, StubInputSink } from "../src/input";
import type { Intent } from "../src/input";
import type { DecodedFrame } from "../src/types";
import type { Renderer } from "../src/renderer";

/** A test-double renderer: records what the widget hands it, no WebGL. */
class FakeRenderer implements Renderer {
  applied: DecodedFrame[] = [];
  renderCount = 0;
  applyFrame(frame: DecodedFrame): void {
    this.applied.push(frame);
  }
  render(): void {
    this.renderCount++;
  }
}

const emptyFrame = (cols: number, rows: number): DecodedFrame => ({
  cols,
  rows,
  kind: 0, // Full
  codepoints: [],
  fg: [],
  bg: [],
  flags: [],
  extra: [],
  spans: [],
  sideTable: [],
});

describe("Terminal wiring", () => {
  it("forwards a source frame to the renderer and presents it", () => {
    const source = new StubFrameSource();
    const renderer = new FakeRenderer();
    const term = new Terminal(source, renderer);
    term.mount();

    const frame = emptyFrame(80, 24);
    source.push(frame);

    expect(renderer.applied).toEqual([frame]);
    expect(renderer.renderCount).toBe(1);
  });

  it("stops rendering after dispose", () => {
    const source = new StubFrameSource();
    const renderer = new FakeRenderer();
    const term = new Terminal(source, renderer);
    term.mount();
    term.dispose();

    source.push(emptyFrame(80, 24));

    expect(renderer.applied).toEqual([]);
    expect(renderer.renderCount).toBe(0);
  });
});

// --- S16 (#133): wheel routing — app vs scrollback (the #129 mask) ---

describe("wheelGoesToApp", () => {
  // The app tracks wheel only when the WHEEL bit (2) is set on the frame's
  // mouseWantedEvents mask (#129). Then a notch reports to the app; else it
  // stays local (scrollback). Normal (?1000) and up include WHEEL.
  it("is true only when the WHEEL bit is set", () => {
    expect(wheelGoesToApp(MouseEvents.Wheel)).toBe(true);
    expect(wheelGoesToApp(MouseEvents.Down | MouseEvents.Up | MouseEvents.Wheel)).toBe(true);
  });

  it("is false with no mask, or a mask without WHEEL (X10 = DOWN only)", () => {
    expect(wheelGoesToApp(0)).toBe(false);
    expect(wheelGoesToApp(MouseEvents.Down)).toBe(false); // X10: press only, no wheel
    expect(wheelGoesToApp(undefined)).toBe(false); // frame omitted the field
  });
});

describe("wheelScrollTarget", () => {
  // WheelScroller yields lines where positive = down/newer; displayOffset is
  // lines UP from the bottom (0 = following), so scrolling newer LOWERS it.
  it("scrolling down (positive lines) lowers the offset", () => {
    expect(wheelScrollTarget(3, 10, 100)).toBe(7);
  });

  it("scrolling up (negative lines) raises the offset", () => {
    expect(wheelScrollTarget(-3, 10, 100)).toBe(13);
  });

  it("clamps to the bottom (0) and to scrollbackLen", () => {
    expect(wheelScrollTarget(5, 2, 100)).toBe(0); // 2-5 = -3 → 0 (can't pass the live edge)
    expect(wheelScrollTarget(-5, 98, 100)).toBe(100); // 98+5 = 103 → 100 (top of history)
  });

  it("returns null for a zero-line notch (nothing to request)", () => {
    expect(wheelScrollTarget(0, 10, 100)).toBeNull();
  });
});

// --- S16 (#133): routeWheel — the whole app/alt/scrollback decision (adversarial
// pass, 2-lens: xterm gates the app path AND the alt-buffer cursor-key path on the
// SAME accumulated `lines`; the local path clamps like the scrollbar drag). ---

describe("routeWheel", () => {
  it("a zero-line notch is `none` regardless of mode (accumulator gates first)", () => {
    expect(routeWheel(MouseEvents.Wheel, 0, false, 10, 100)).toEqual({ kind: "none" });
    expect(routeWheel(0, 0, true, 10, 100)).toEqual({ kind: "none" });
  });

  it("routes to the app when it tracks the wheel — direction only, by line sign", () => {
    expect(routeWheel(MouseEvents.Wheel, 3, false, 10, 100)).toEqual({ kind: "app", direction: "down" });
    expect(routeWheel(MouseEvents.Wheel, -2, false, 10, 100)).toEqual({ kind: "app", direction: "up" });
  });

  it("the app path wins over the alt-buffer path (a wheel-tracking TUI on alt)", () => {
    expect(routeWheel(MouseEvents.Wheel, -2, true, 10, 100)).toEqual({ kind: "app", direction: "up" });
  });

  it("on the alt screen (no scrollback), a non-tracking app gets cursor keys (xterm)", () => {
    expect(routeWheel(0, -2, true, 10, 100)).toEqual({ kind: "altKeys", direction: "up" });
    expect(routeWheel(0, 4, true, 10, 100)).toEqual({ kind: "altKeys", direction: "down" });
  });

  it("normal buffer, no tracking → a clamped local scrollback request", () => {
    expect(routeWheel(0, 3, false, 10, 100)).toEqual({ kind: "scroll", displayOffset: 7 });
    expect(routeWheel(0, -3, false, 10, 100)).toEqual({ kind: "scroll", displayOffset: 13 });
    expect(routeWheel(0, 5, false, 2, 100)).toEqual({ kind: "scroll", displayOffset: 0 }); // clamp bottom
    expect(routeWheel(0, -5, false, 98, 100)).toEqual({ kind: "scroll", displayOffset: 100 }); // clamp top
  });
});

// --- S16 (#133): the renderer-notifying sink — typing restarts the blink (the S5
// #107 deferral), focus/blur drives the renderer's focus state (blink + selection
// tint gating, the sibling-lens gap: setFocused was built but unwired). ---

describe("rendererNotifyingSink", () => {
  const fakeRenderer = () => ({
    applyFrame: vi.fn(),
    render: vi.fn(),
    restartCursorBlink: vi.fn(),
    setFocused: vi.fn(),
  });

  it("restarts the cursor blink on a key intent, then forwards it", () => {
    const inner = new StubInputSink();
    const r = fakeRenderer();
    const sink = rendererNotifyingSink(inner, r);

    const key: Intent = {
      kind: "key",
      event: { key: { type: "char", char: "a" }, mods: 0, action: "press" },
    };
    sink.send(key);

    expect(r.restartCursorBlink).toHaveBeenCalledTimes(1);
    expect(r.setFocused).not.toHaveBeenCalled();
    expect(inner.sent).toEqual([key]);
  });

  it("drives renderer focus state on a focus intent, then forwards it", () => {
    const inner = new StubInputSink();
    const r = fakeRenderer();
    const sink = rendererNotifyingSink(inner, r);

    const blur: Intent = { kind: "focus", focused: false };
    sink.send(blur);

    expect(r.setFocused).toHaveBeenCalledWith(false);
    expect(r.restartCursorBlink).not.toHaveBeenCalled();
    expect(inner.sent).toEqual([blur]);
  });

  it("forwards paste untouched (no renderer notification)", () => {
    const inner = new StubInputSink();
    const r = fakeRenderer();
    const sink = rendererNotifyingSink(inner, r);

    const paste: Intent = { kind: "paste", text: "hi" };
    sink.send(paste);

    expect(r.restartCursorBlink).not.toHaveBeenCalled();
    expect(r.setFocused).not.toHaveBeenCalled();
    expect(inner.sent).toEqual([paste]);
  });

  it("tolerates a renderer that omits the optional hooks (no throw)", () => {
    const inner = new StubInputSink();
    const sink = rendererNotifyingSink(inner, { applyFrame: vi.fn(), render: vi.fn() });

    expect(() =>
      sink.send({ kind: "key", event: { key: { type: "enter" }, mods: 0, action: "press" } }),
    ).not.toThrow();
    expect(() => sink.send({ kind: "focus", focused: true })).not.toThrow();
  });
});
