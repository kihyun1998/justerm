import { describe, expect, it } from "vitest";
import { AccessibleViewController } from "../src/accessible-view";
import type { AccessiblePort, AccessibleView } from "../src/accessible-view";

/** A recording {@link AccessiblePort} — resolves the full-buffer text tests set
 * (mirrors `StubSelectionPort`). In frame mode this round-trips to the backend's
 * `Engine::accessible_text`. */
class StubPort implements AccessiblePort {
  value = "";
  calls = 0;
  text(): Promise<string> {
    this.calls++;
    return Promise.resolve(this.value);
  }
}

/** A recording {@link AccessibleView}. */
class StubView implements AccessibleView {
  readonly shown: string[] = [];
  hidden = 0;
  show(text: string): void {
    this.shown.push(text);
  }
  hide(): void {
    this.hidden++;
  }
}

describe("AccessibleViewController (#150)", () => {
  // Summoning queries the backend for the whole-buffer text and hands it to the
  // view (VSCode `provideContent` → show). The view is now open.
  it("summon fetches the buffer text and shows it", async () => {
    const port = new StubPort();
    port.value = "line0\nline1";
    const view = new StubView();
    const ctrl = new AccessibleViewController(port, view);

    await ctrl.summon();

    expect(port.calls).toBe(1);
    expect(view.shown).toEqual(["line0\nline1"]);
    expect(ctrl.isOpen()).toBe(true);
  });

  // Closing tears the document down and returns focus to the terminal widget —
  // the load-bearing part of the contract (VSCode `onClose` → `instance.focus`),
  // so the user isn't stranded in a hidden element.
  it("close hides the view and restores focus", async () => {
    const port = new StubPort();
    const view = new StubView();
    const focused: string[] = [];
    const ctrl = new AccessibleViewController(port, view, {
      restoreFocus: () => focused.push("widget"),
    });
    await ctrl.summon();

    ctrl.close();

    expect(view.hidden).toBe(1);
    expect(ctrl.isOpen()).toBe(false);
    expect(focused).toEqual(["widget"]);
  });

  // A rejected query (the port round-trips to the backend over IPC, which can
  // fail) leaves the view closed and untouched — the caller handles the error.
  it("summon that rejects leaves the view closed", async () => {
    const view = new StubView();
    const port: AccessiblePort = { text: () => Promise.reject(new Error("ipc")) };
    const ctrl = new AccessibleViewController(port, view);

    await expect(ctrl.summon()).rejects.toThrow("ipc");
    expect(ctrl.isOpen()).toBe(false);
    expect(view.shown).toEqual([]);
  });

  // Re-summoning while open refreshes the document with the latest text (VSCode's
  // accessible-buffer refresh) — it re-queries and re-shows.
  it("re-summon refreshes the document", async () => {
    const port = new StubPort();
    const view = new StubView();
    const ctrl = new AccessibleViewController(port, view);

    port.value = "a";
    await ctrl.summon();
    port.value = "b";
    await ctrl.summon();

    expect(view.shown).toEqual(["a", "b"]);
    expect(ctrl.isOpen()).toBe(true);
  });

  // Closing when nothing is open is inert — no teardown, no focus yank.
  it("close is inert when not open", () => {
    const view = new StubView();
    const focused: string[] = [];
    const ctrl = new AccessibleViewController(new StubPort(), view, {
      restoreFocus: () => focused.push("widget"),
    });

    ctrl.close();

    expect(view.hidden).toBe(0);
    expect(focused).toEqual([]);
  });
});
