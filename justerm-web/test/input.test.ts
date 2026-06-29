import { describe, expect, it } from "vitest";
import { keyFromDom, Mod, mouseFromDom, StubInputSink, wheelMouseFromDom } from "../src/input";
import type { KeyboardEventLike, MouseEventLike } from "../src/input";

function key(over: Partial<KeyboardEventLike> & { key: string }): KeyboardEventLike {
  return { shiftKey: false, altKey: false, ctrlKey: false, metaKey: false, ...over };
}

describe("keyFromDom", () => {
  // A printable key becomes Key::Char with no modifiers (core input.rs Key::Char).
  it("maps a printable key to a Char intent", () => {
    const ev = keyFromDom(key({ key: "a" }));

    expect(ev).toEqual({
      key: { type: "char", char: "a" },
      mods: Mod.None,
      action: "press",
    });
  });

  // Arrow keys map to their named Key variants (core Key::Up/Down/Left/Right).
  // The DECCKM (app-cursor) byte choice is the backend's; the web only names it.
  it("maps arrow keys to named intents", () => {
    expect(keyFromDom(key({ key: "ArrowUp" })).key).toEqual({ type: "up" });
    expect(keyFromDom(key({ key: "ArrowDown" })).key).toEqual({ type: "down" });
    expect(keyFromDom(key({ key: "ArrowLeft" })).key).toEqual({ type: "left" });
    expect(keyFromDom(key({ key: "ArrowRight" })).key).toEqual({ type: "right" });
  });

  // The editing / navigation keys core Key names as unit variants.
  it("maps editing and navigation keys to named intents", () => {
    const t = (k: string): unknown => keyFromDom(key({ key: k })).key;
    expect(t("Enter")).toEqual({ type: "enter" });
    expect(t("Tab")).toEqual({ type: "tab" });
    expect(t("Backspace")).toEqual({ type: "backspace" });
    expect(t("Escape")).toEqual({ type: "escape" });
    expect(t("Delete")).toEqual({ type: "delete" });
    expect(t("Insert")).toEqual({ type: "insert" });
    expect(t("Home")).toEqual({ type: "home" });
    expect(t("End")).toEqual({ type: "end" });
    expect(t("PageUp")).toEqual({ type: "pageup" });
    expect(t("PageDown")).toEqual({ type: "pagedown" });
  });

  // Function keys carry their number (core Key::F(n), n in 1..=12).
  it("maps function keys to F(n) intents", () => {
    expect(keyFromDom(key({ key: "F1" })).key).toEqual({ type: "f", n: 1 });
    expect(keyFromDom(key({ key: "F12" })).key).toEqual({ type: "f", n: 12 });
  });

  // DOM modifier booleans → the kitty bitmask core uses. metaKey (Cmd/Win) is
  // Super in the kitty scheme.
  it("extracts modifier bits", () => {
    expect(keyFromDom(key({ key: "a", ctrlKey: true, shiftKey: true })).mods).toBe(Mod.Ctrl | Mod.Shift);
    expect(keyFromDom(key({ key: "a", altKey: true })).mods).toBe(Mod.Alt);
    expect(keyFromDom(key({ key: "a", metaKey: true })).mods).toBe(Mod.Super);
  });
});

function mouse(over: Partial<MouseEventLike> & { clientX: number; clientY: number }): MouseEventLike {
  return { button: 0, buttons: 0, shiftKey: false, altKey: false, ctrlKey: false, metaKey: false, ...over };
}

// 10×20 px cells, terminal at the origin.
const geom = { originX: 0, originY: 0, cellWidth: 10, cellHeight: 20 };

describe("mouseFromDom", () => {
  // Pixel coords → 0-based cell (col, row); DOM button 0/1/2 → Left/Middle/Right.
  // The encoding shifts to 1-based on the wire — that's the backend's job.
  it("maps pixel coords to a cell and the button to a named intent", () => {
    const ev = mouseFromDom(mouse({ clientX: 105, clientY: 45, button: 0 }), "press", geom);

    expect(ev).toEqual({
      button: "left",
      action: "press",
      col: 10, // floor(105 / 10)
      row: 2, // floor(45 / 20)
      px: 105,
      py: 45,
      mods: Mod.None,
    });
  });

  // On motion the trigger `button` is meaningless; the held button comes from the
  // `buttons` bitmask (1 = left, 2 = right, 4 = middle). A drag reports it; bare
  // motion (no button held) reports null (core MouseEvent.button = None).
  it("uses the held button on drag and null on bare motion", () => {
    expect(mouseFromDom(mouse({ clientX: 5, clientY: 5, buttons: 1 }), "motion", geom).button).toBe("left");
    expect(mouseFromDom(mouse({ clientX: 5, clientY: 5, buttons: 2 }), "motion", geom).button).toBe("right");
    expect(mouseFromDom(mouse({ clientX: 5, clientY: 5, buttons: 0 }), "motion", geom).button).toBeNull();
  });

  // When the app requests wheel reporting, a wheel notch is a button press
  // (xterm's 64-base wheel group). deltaY < 0 scrolls up.
  it("maps a wheel notch to a wheel-button press", () => {
    const up = wheelMouseFromDom(mouse({ clientX: 5, clientY: 25 }), -1, geom);
    expect({ button: up.button, action: up.action, row: up.row }).toEqual({
      button: "wheelUp",
      action: "press",
      row: 1, // floor(25 / 20)
    });
    expect(wheelMouseFromDom(mouse({ clientX: 5, clientY: 25 }), 1, geom).button).toBe("wheelDown");
  });
});

describe("InputSink", () => {
  // The outbound seam: normalised intents are pushed to a sink the backend wires
  // to core's encoders. StubInputSink collects them for tests/demos (the analog
  // of StubFrameSource on the inbound side).
  it("collects sent intents in order", () => {
    const sink = new StubInputSink();

    sink.send({ kind: "key", event: keyFromDom({ key: "a", shiftKey: false, altKey: false, ctrlKey: false, metaKey: false }) });
    sink.send({ kind: "paste", text: "hi" });
    sink.send({ kind: "focus", focused: true });

    expect(sink.sent).toEqual([
      { kind: "key", event: { key: { type: "char", char: "a" }, mods: Mod.None, action: "press" } },
      { kind: "paste", text: "hi" },
      { kind: "focus", focused: true },
    ]);
  });
});
