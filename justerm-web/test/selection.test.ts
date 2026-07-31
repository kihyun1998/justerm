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

// A pointer leaves the grid whenever a drag does, and `fit.ts`'s `Math.floor`
// guarantees a remainder strip below the last row on any container whose height
// is not an exact multiple of the cell — so an out-of-grid coordinate is
// ordinary input here, not an edge case. All three references clamp it in this
// converter rather than relying on the engine: xterm.js `getCoords`
// (`Mouse.ts:40-47`, one shared converter for mouse reporting *and* selection),
// alacritty `Mouse::point` (`event.rs:1811-1815`), ghostty `Coordinate.convert`
// (`renderer/size.zig:142-147`, "We need our grid to clamp"). justerm's own
// mouse-reporting converter already does (`input.ts:253-256`, #266); this is the
// sibling that did not. (#667)
describe("SelectionController — the pointer is clamped to the grid", () => {
  // Below the last row: `fit.ts` floors `rows`, so the leftover strip is a
  // structural part of every non-exact container, not an exotic drag.
  it("clamps a press in the remainder strip below the last row", () => {
    const port = new StubSelectionPort();

    controller(port).mouseDown(ev(5 * 10 + 2, 24 * 20 + 3), 1); // row 24 of 24

    expect(port.calls).toEqual([{ kind: "begin", row: 23, col: 5, side: "left", ty: "char" }]);
  });

  // Above / left of the canvas the raw arithmetic is *negative* — a different
  // failure from overshooting the far edge, because `Math.floor(-3 / 10)` is `-1`.
  // The engine clamps both axes as a backstop (#660 row, #671 column), but a
  // negative crossing an unsigned seam is not something a clamp downstream can
  // undo, and the alt-click path never reaches the engine at all.
  it("clamps a press above and to the left of the canvas to (0, 0)", () => {
    const port = new StubSelectionPort();

    controller(port).mouseDown(ev(-3, -7), 1);

    expect(port.calls).toEqual([{ kind: "begin", row: 0, col: 0, side: "left", ty: "char" }]);
  });

  // Past the right edge the clamp must land on the last column's RIGHT side —
  // that boundary is the end of the row, and losing it would silently shorten
  // every drag that overshoots. xterm buys the same endpoint with an extra
  // column (`colCount + 1` when `isSelection`); justerm's `Side` already
  // expresses it, so `cols - 1` is the right bound here and copying xterm's
  // constant would be off by one.
  it("clamps a press past the right edge to the last column's right side", () => {
    const port = new StubSelectionPort();

    controller(port).mouseDown(ev(80 * 10 + 5, 3 * 20 + 5), 1); // col 80 of 80

    expect(port.calls).toEqual([{ kind: "begin", row: 3, col: 79, side: "right", ty: "char" }]);
  });

  // The drag path's vertical axis is already covered — but by `dragScrollSpeed`,
  // not by the converter, and that gate reads `clientY` only. A drag that leaves
  // the canvas sideways while staying vertically inside reaches `extend` with a
  // negative column.
  it("clamps a sideways drag that stays vertically inside the viewport", () => {
    const port = new StubSelectionPort();
    const ctrl = controller(port);

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(ev(-12, 3 * 20 + 5, { buttons: 1 }));

    expect(port.calls).toEqual([
      { kind: "begin", row: 3, col: 5, side: "left", ty: "char" },
      { kind: "extend", row: 3, col: 0, side: "left" },
    ]);
  });

  // `tick()` re-anchors the focus at `lastCol`, recorded by the *move*. An
  // unclamped column therefore survives the edge-row pin and is replayed on
  // every tick of the auto-scroll.
  it("does not carry an out-of-grid column into the auto-scroll tick", () => {
    const port = new StubSelectionPort();
    const ctrl = new SelectionController(port, () => GEOM, {
      onScroll: () => {},
      getRows: () => 24,
    });

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(ev(80 * 10 + 40, 24 * 20 + 30, { buttons: 1 })); // out on both axes
    ctrl.tick();

    expect(port.calls).toEqual([
      { kind: "begin", row: 3, col: 5, side: "left", ty: "char" },
      { kind: "extend", row: 23, col: 79, side: "right" },
    ]);
  });

  // The alt-click cursor move leaves through `onMoveCursor`, not `SelectionPort`
  // — so the engine's own clamp cannot cover it at all, whatever core does. This
  // is the call site that makes "leave it, core clamps" false rather than merely
  // fragile.
  it("clamps the alt-click cursor-move cell, which never reaches the engine", () => {
    const port = new StubSelectionPort();
    const moves: { row: number; col: number }[] = [];
    const ctrl = new SelectionController(port, () => GEOM, {
      onMoveCursor: (c) => moves.push(c),
    });
    const out = (t: number) => ev(-20, 24 * 20 + 40, { altKey: true, timeStamp: t });

    ctrl.mouseDown(out(1000), 1);
    ctrl.mouseUp(out(1200));

    expect(moves).toEqual([{ row: 23, col: 0 }]);
  });

  // `tick()` is the one row-producer in the file that does not go through
  // `cellAndSide`, so the converter's clamp cannot cover it — the count comes
  // from the consumer's `getRows`, not from `geom`. A widget that fits its own
  // grid cannot report 0 (`fit.ts` MINIMUM_ROWS), but `SelectionController` is
  // published and takes whatever its constructor is handed. xterm bounds both
  // branches of `_dragScroll` (`SelectionService.ts:707,711`). (#667, found by
  // the completeness pass.)
  it("does not produce a negative edge row when the consumer reports no rows", () => {
    const port = new StubSelectionPort();
    const ctrl = new SelectionController(port, () => GEOM, {
      onScroll: () => {},
      getRows: () => 0, // a 0-row viewport is 0px tall, so any pointer is "below" it
    });

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(ev(5 * 10 + 2, 40, { buttons: 1 }));
    ctrl.tick();

    expect(port.calls.at(-1)).toEqual({ kind: "extend", row: 0, col: 5, side: "left" });
  });

  // A degenerate grid (nothing measured yet) must not produce a negative cell
  // from `cols - 1` / `rows - 1`. `input.ts`'s `clampTo` floors its own bound for
  // exactly this reason; the selection converter must agree.
  //
  // `side` comes out `right`, and that is the general rule holding rather than a
  // special case: with no columns the pointer is past the (empty) grid's right
  // edge, which is the same situation as the overshoot test above. Pinned as
  // measured — forcing `left` here would need a degenerate-grid branch that
  // contradicts the rule everywhere else.
  it("resolves a zero-sized grid to (0, 0) rather than a negative cell", () => {
    const port = new StubSelectionPort();
    const zero: CellGeometry = { ...GEOM, cols: 0, rows: 0 };

    new SelectionController(port, () => zero).mouseDown(ev(30, 60), 1);

    expect(port.calls).toEqual([{ kind: "begin", row: 0, col: 0, side: "right", ty: "char" }]);
  });
});

// The vertical half of the drag path is guarded today, but by `dragScrollSpeed`
// rather than by the converter — an accidental coupling worth pinning, since a
// change to the auto-scroll policy would silently remove it. (#667)
describe("SelectionController — the drag-scroll gate owns the vertical drag bound", () => {
  it("routes a drag below the last row to the tick, never to a bare extend", () => {
    const port = new StubSelectionPort();
    const scrolls: number[] = [];
    const ctrl = new SelectionController(port, () => GEOM, {
      onScroll: (n) => scrolls.push(n),
      getRows: () => 24,
    });

    ctrl.mouseDown(leftHalf(5, 3), 1);
    ctrl.mouseMove(ev(7 * 10 + 2, 24 * 20 + 5, { buttons: 1 })); // 5px into the strip

    expect(port.calls.map((c) => c.kind)).toEqual(["begin"]); // no extend from the move
    expect(scrolls).toEqual([]); // and nothing scrolled until a tick runs
  });
});
