import { describe, expect, it } from "vitest";
import {
  copySelection,
  dragScrollSpeed,
  SelectionController,
  StubSelectionPort,
} from "../src/selection";
import type { CellGeometry, MouseEventLike } from "../src/input";

// 10×20 px cells at the canvas origin — a cell column is [col*10, col*10+10).
const GEOM: CellGeometry = { originX: 0, originY: 0, cellWidth: 10, cellHeight: 20, cols: 80, rows: 24 };

// A bare-left-button DOM-ish event at pixel (clientX, clientY). `held` sets the
// `buttons` bitmask for motion events (1 = left held); a press leaves it 0.
function ev(clientX: number, clientY: number, over: Partial<MouseEventLike> = {}): MouseEventLike {
  return {
    clientX,
    clientY,
    button: 0,
    buttons: 0,
    shiftKey: false,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    ...over,
  };
}

// Pixel at the left/right half of cell `col` on row `row`, so the controller
// resolves the cell and which edge (side) the pointer is nearest.
const leftHalf = (col: number, row: number) => ev(col * 10 + 2, row * 20 + 5);
const rightHalf = (col: number, row: number) => ev(col * 10 + 8, row * 20 + 5);

function controller(port: StubSelectionPort) {
  return new SelectionController(port, () => GEOM);
}

describe("SelectionController — drag → selection commands", () => {
  // A plain single click (detail 1) anchors a char selection at the cell under
  // the pointer. Left half of the cell → Left side (cell included on a
  // rightward drag), matching core's half-open `[from, to)` Side model.
  it("a single-click press begins a char selection at the cell + nearest side", () => {
    const port = new StubSelectionPort();

    controller(port).mouseDown(leftHalf(5, 3), 1);

    expect(port.calls).toEqual([{ kind: "begin", row: 3, col: 5, side: "left", ty: "char" }]);
  });

  // Dragging after the press moves the selection's focus to the cell under the
  // pointer — the right edge here (right half of the cell), so the cell is
  // included in the run.
  it("a move during a drag extends the focus to the new cell + side", () => {
    const port = new StubSelectionPort();
    const ctrl = controller(port);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(rightHalf(8, 3));

    expect(port.calls).toEqual([
      { kind: "begin", row: 3, col: 5, side: "left", ty: "char" },
      { kind: "extend", row: 3, col: 8, side: "right" },
    ]);
  });

  // Releasing ends the drag; later motion (the bare mouse moving over the
  // terminal) must not keep extending the now-finished selection.
  it("stops extending after the button is released", () => {
    const port = new StubSelectionPort();
    const ctrl = controller(port);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseUp(leftHalf(6, 3));
    ctrl.mouseMove(rightHalf(9, 3));

    expect(port.calls.map((c) => c.kind)).toEqual(["begin"]);
  });

  // Click count selects the granularity (xterm: detail 1/2/3). A double-click
  // anchors a word selection; the engine expands it to word boundaries.
  it("a double-click begins a word selection", () => {
    const port = new StubSelectionPort();

    controller(port).mouseDown(leftHalf(5, 3), 2);

    expect(port.calls).toEqual([{ kind: "begin", row: 3, col: 5, side: "left", ty: "word" }]);
  });

  // A triple-click anchors a whole-line selection.
  it("a triple-click begins a line selection", () => {
    const port = new StubSelectionPort();

    controller(port).mouseDown(leftHalf(5, 3), 3);

    expect(port.calls).toEqual([{ kind: "begin", row: 3, col: 5, side: "left", ty: "line" }]);
  });

  // Holding Alt on a single click switches to a rectangular (block/COLUMN)
  // selection — xterm's `shouldColumnSelect`. Block applies to single clicks
  // only; double/triple keep word/line.
  it("an alt single-click begins a block selection", () => {
    const port = new StubSelectionPort();
    const altLeftHalf = ev(5 * 10 + 2, 3 * 20 + 5, { altKey: true });

    controller(port).mouseDown(altLeftHalf, 1);

    expect(port.calls).toEqual([{ kind: "begin", row: 3, col: 5, side: "left", ty: "block" }]);
  });

  // Shift+click extends the existing selection to the clicked cell instead of
  // starting a new one (xterm `_handleIncrementalClick`) — the original anchor
  // is kept, so only an `extend` reaches the engine.
  it("a shift-click extends the existing selection rather than re-anchoring", () => {
    const port = new StubSelectionPort();
    const ctrl = controller(port);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseUp(leftHalf(5, 3));
    ctrl.mouseDown(ev(10 * 10 + 8, 3 * 20 + 5, { shiftKey: true }), 1);

    expect(port.calls).toEqual([
      { kind: "begin", row: 3, col: 5, side: "left", ty: "char" },
      { kind: "extend", row: 3, col: 10, side: "right" },
    ]);
  });
});

describe("dragScrollSpeed — distance → scroll lines", () => {
  // No auto-scroll while the pointer is over the terminal: the whole height,
  // edges inclusive, returns 0 (xterm `0 <= offset <= terminalHeight`).
  it("returns 0 while the pointer is inside the viewport", () => {
    const HEIGHT = 24 * 20; // 24 rows × 20px

    expect(dragScrollSpeed(0, HEIGHT)).toBe(0);
    expect(dragScrollSpeed(HEIGHT / 2, HEIGHT)).toBe(0);
    expect(dragScrollSpeed(HEIGHT, HEIGHT)).toBe(0);
  });

  // Below the bottom edge scrolls down (positive), proportional to how far out
  // the pointer is: 1 line just past the edge, ramping to the 15-line cap at
  // 50px out and beyond (xterm DRAG_SCROLL_MAX_THRESHOLD/SPEED).
  it("scrolls down proportionally below the bottom, min 1 max 15", () => {
    const H = 480;

    expect(dragScrollSpeed(H + 1, H)).toBe(1); // just past the edge → min step
    expect(dragScrollSpeed(H + 25, H)).toBe(8); // halfway → ~middle speed
    expect(dragScrollSpeed(H + 50, H)).toBe(15); // at the threshold → cap
    expect(dragScrollSpeed(H + 1000, H)).toBe(15); // beyond → clamped to cap
  });

  // Above the top edge scrolls up (negative), symmetric to the downward ramp.
  it("scrolls up symmetrically above the top", () => {
    const H = 480;

    expect(dragScrollSpeed(-1, H)).toBe(-1);
    expect(dragScrollSpeed(-25, H)).toBe(-8);
    expect(dragScrollSpeed(-50, H)).toBe(-15);
    expect(dragScrollSpeed(-1000, H)).toBe(-15);
  });
});

describe("SelectionController — drag-scroll via tick()", () => {
  function autoScrollController(port: StubSelectionPort, scrolls: number[]) {
    return new SelectionController(port, () => GEOM, {
      onScroll: (n) => scrolls.push(n),
      getRows: () => 24, // viewport 24 rows × 20px = 480px tall
    });
  }

  // While the pointer is dragged below the viewport, each tick scrolls the
  // viewport down by the distance-proportional amount and pins the selection
  // focus to the bottom edge row (xterm `_dragScroll`). The move itself, being
  // out of bounds, emits no normal extend — the tick owns the edge.
  it("ticks a downward scroll and extends to the bottom edge", () => {
    const port = new StubSelectionPort();
    const scrolls: number[] = [];
    const ctrl = autoScrollController(port, scrolls);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(ev(7 * 10 + 2, 24 * 20 + 30)); // 30px below bottom, col 7
    ctrl.tick();

    expect(scrolls).toEqual([9]); // 30px out → speed 9
    expect(port.calls).toEqual([
      { kind: "begin", row: 3, col: 5, side: "left", ty: "char" },
      { kind: "extend", row: 23, col: 7, side: "left" }, // bottom edge row
    ]);
  });

  // A tick while the pointer is inside the viewport must not scroll — auto-scroll
  // is strictly an out-of-bounds affordance.
  it("does not scroll while the pointer is inside the viewport", () => {
    const port = new StubSelectionPort();
    const scrolls: number[] = [];
    const ctrl = autoScrollController(port, scrolls);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(rightHalf(8, 3)); // well inside the 480px viewport
    ctrl.tick();

    expect(scrolls).toEqual([]);
    expect(port.calls.map((c) => c.kind)).toEqual(["begin", "extend"]); // no tick extend
  });

  // No drag in progress → a stray timer tick is a no-op (the timer may outlive
  // the mouseup by one interval).
  it("is a no-op when no drag is active", () => {
    const port = new StubSelectionPort();
    const scrolls: number[] = [];
    const ctrl = autoScrollController(port, scrolls);

    ctrl.tick();

    expect(scrolls).toEqual([]);
    expect(port.calls).toEqual([]);
  });

  // Dragging above the top scrolls up (negative) and pins the focus to row 0 —
  // the mirror branch of the bottom-edge case.
  it("ticks an upward scroll and extends to the top edge", () => {
    const port = new StubSelectionPort();
    const scrolls: number[] = [];
    const ctrl = autoScrollController(port, scrolls);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(ev(6 * 10 + 2, -40)); // 40px above the top, col 6
    ctrl.tick();

    expect(scrolls).toEqual([-12]); // 40px out → speed -12
    expect(port.calls.at(-1)).toEqual({ kind: "extend", row: 0, col: 6, side: "left" });
  });
});

describe("copySelection — engine text → clipboard", () => {
  // Copy asks the engine for the selection text (core `selection_text`, run on
  // the backend across scrollback) and writes it to the clipboard. Returns true
  // when something was copied.
  it("writes the engine's selection text to the clipboard", async () => {
    const port = new StubSelectionPort();
    port.textValue = "hello world";
    const written: string[] = [];

    const ok = await copySelection(port, async (t) => {
      written.push(t);
    });

    expect(written).toEqual(["hello world"]);
    expect(ok).toBe(true);
  });

  // Copy normalizes non-breaking spaces (U+00A0) to regular spaces so pasted
  // text doesn't carry invisible NBSPs (xterm does the same on copy). justerm
  // never emits NBSP as padding, so any here is real content — the conversion
  // is a deliberate web-side copy policy, not done in core's selection_text.
  it("normalizes non-breaking spaces to regular spaces", async () => {
    const port = new StubSelectionPort();
    const nbsp = String.fromCharCode(0xa0);
    port.textValue = "a" + nbsp + nbsp + "b"; // two NBSPs between the words
    let written = "";

    await copySelection(port, async (t) => {
      written = t;
    });

    expect(written).toBe("a  b");
  });

  // No selection (null) or an empty/collapsed one must not overwrite the
  // clipboard — a bare click shouldn't wipe whatever the user copied earlier.
  it("does not touch the clipboard when nothing is selected", async () => {
    const writes: string[] = [];
    const write = async (t: string) => {
      writes.push(t);
    };

    const portNull = new StubSelectionPort(); // textValue stays null
    const portEmpty = new StubSelectionPort();
    portEmpty.textValue = "";

    expect(await copySelection(portNull, write)).toBe(false);
    expect(await copySelection(portEmpty, write)).toBe(false);
    expect(writes).toEqual([]);
  });
});

describe("SelectionController — alt-click cursor move", () => {
  // A short alt-click that never dragged is not a block selection — it asks the
  // shell to move its cursor to that cell (xterm `altClickMovesCursor`). The
  // controller emits the intent; the consumer synthesises the arrow-key bytes.
  // The empty block selection begun on mousedown is cleared.
  function altClickController(
    port: StubSelectionPort,
    moves: { row: number; col: number }[],
    opts: { isAtBottom?: () => boolean } = {},
  ) {
    return new SelectionController(port, () => GEOM, {
      onMoveCursor: (c) => moves.push(c),
      isAtBottom: opts.isAtBottom,
    });
  }

  const altAt = (col: number, row: number, timeStamp: number) =>
    ev(col * 10 + 2, row * 20 + 5, { altKey: true, timeStamp });

  it("moves the cursor to the cell on a quick alt-click with no drag", () => {
    const port = new StubSelectionPort();
    const moves: { row: number; col: number }[] = [];
    const ctrl = altClickController(port, moves);

    ctrl.mouseDown(altAt(5, 3, 1000), 1);
    ctrl.mouseUp(altAt(5, 3, 1200)); // 200ms elapsed < 500

    expect(moves).toEqual([{ row: 3, col: 5 }]);
    expect(port.calls.map((c) => c.kind)).toEqual(["begin", "clear"]);
  });

  // An alt-drag that moved is a real block selection, not a cursor move — the
  // selection is kept and no move intent fires.
  it("does not move the cursor when the alt-click dragged into a block selection", () => {
    const port = new StubSelectionPort();
    const moves: { row: number; col: number }[] = [];
    const ctrl = altClickController(port, moves);

    ctrl.mouseDown(altAt(5, 3, 1000), 1);
    ctrl.mouseMove(ev(8 * 10 + 2, 3 * 20 + 5, { altKey: true })); // drag → extend
    ctrl.mouseUp(altAt(8, 3, 1100));

    expect(moves).toEqual([]);
    expect(port.calls.map((c) => c.kind)).toEqual(["begin", "extend"]); // no clear
  });

  // A slow alt-click (held past the 500ms threshold) is a deliberate click, not
  // a cursor move.
  it("does not move the cursor when the alt-click is slow", () => {
    const port = new StubSelectionPort();
    const moves: { row: number; col: number }[] = [];
    const ctrl = altClickController(port, moves);

    ctrl.mouseDown(altAt(5, 3, 1000), 1);
    ctrl.mouseUp(altAt(5, 3, 1600)); // 600ms ≥ 500

    expect(moves).toEqual([]);
  });

  // Moving the prompt cursor only makes sense at the live prompt — when scrolled
  // back into history, an alt-click does nothing.
  it("does not move the cursor when scrolled back in history", () => {
    const port = new StubSelectionPort();
    const moves: { row: number; col: number }[] = [];
    const ctrl = altClickController(port, moves, { isAtBottom: () => false });

    ctrl.mouseDown(altAt(5, 3, 1000), 1);
    ctrl.mouseUp(altAt(5, 3, 1100));

    expect(moves).toEqual([]);
  });
});

describe("SelectionController — middle-click paste & primary selection", () => {
  // Middle-click pastes (X11 primary convention) — it is not a selection
  // gesture, so no selection command is issued. The consumer reads the primary
  // buffer and sends the bytes; the controller only signals the intent.
  it("a middle-click requests a paste and starts no selection", () => {
    const port = new StubSelectionPort();
    const pastes: number[] = [];
    const ctrl = new SelectionController(port, () => GEOM, { onPaste: () => pastes.push(1) });

    ctrl.mouseDown(ev(5 * 10 + 2, 3 * 20 + 5, { button: 1, buttons: 4 }), 1);

    expect(pastes).toEqual([1]);
    expect(port.calls).toEqual([]); // no begin/extend
  });
});

describe("SelectionController — primary selection on drag complete", () => {
  const flush = () => new Promise((r) => setTimeout(r, 0));

  // On a completed drag selection the controller offers the text for the X11
  // primary buffer (xterm `onLinuxMouseSelection`). It reuses the copy path, so
  // the text is NBSP-normalized and an empty selection is skipped. The consumer
  // (only on Linux) writes it to the primary buffer.
  it("offers the selected text for the primary buffer when a drag completes", async () => {
    const port = new StubSelectionPort();
    const primary: string[] = [];
    const ctrl = new SelectionController(port, () => GEOM, {
      onPrimarySelection: (t) => primary.push(t),
    });

    ctrl.mouseDown(leftHalf(2, 1), 1);
    ctrl.mouseMove(rightHalf(6, 1)); // a real drag
    port.textValue = "picked text";
    ctrl.mouseUp(rightHalf(6, 1));
    await flush();

    expect(primary).toEqual(["picked text"]);
  });

  // A bare click (no drag) is not a selection — nothing is offered to primary,
  // so a stray click never clobbers the primary buffer.
  it("does not offer anything to primary on a bare click", async () => {
    const port = new StubSelectionPort();
    const primary: string[] = [];
    const ctrl = new SelectionController(port, () => GEOM, {
      onPrimarySelection: (t) => primary.push(t),
    });

    ctrl.mouseDown(leftHalf(2, 1), 1);
    port.textValue = "should not leak";
    ctrl.mouseUp(leftHalf(2, 1));
    await flush();

    expect(primary).toEqual([]);
  });
});
