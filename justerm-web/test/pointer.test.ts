import { describe, expect, it } from "vitest";
import { PointerRouter, type LocalPointer, type PointerEventLike } from "../src/pointer";
import { MouseEvents, type CellGeometry, type MouseEvent } from "../src/input";

// 10×20 px cells at the origin — pixel (col*10+2, row*20+5) is inside cell (col, row).
const GEOM: CellGeometry = { originX: 0, originY: 0, cellWidth: 10, cellHeight: 20, cols: 80, rows: 24 };

const NORMAL = MouseEvents.Down | MouseEvents.Up | MouseEvents.Wheel; // ?1000
const BUTTON = NORMAL | MouseEvents.Drag; // ?1002
const ANY = BUTTON | MouseEvents.Move; // ?1003

function at(col: number, row: number, over: Partial<PointerEventLike> = {}): PointerEventLike {
  return {
    clientX: col * 10 + 2,
    clientY: row * 20 + 5,
    button: 0,
    buttons: 0,
    detail: 1,
    shiftKey: false,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    ...over,
  };
}

/** A recording local handler with the shape `SelectionController` already has. */
class RecordingLocal implements LocalPointer {
  readonly calls: string[] = [];
  mouseDown(ev: PointerEventLike, detail: number, forced?: boolean): void {
    this.calls.push(`down(${ev.button},${detail},${forced ? "forced" : "plain"})`);
  }
  mouseMove(): void {
    this.calls.push("move");
  }
  mouseUp(): void {
    this.calls.push("up");
  }
  tick(): void {
    this.calls.push("tick");
  }
}

function rig(mask: number, opts: { local?: boolean; geom?: () => CellGeometry | undefined } = {}) {
  const sent: MouseEvent[] = [];
  const ticking: boolean[] = [];
  const local = opts.local === false ? undefined : new RecordingLocal();
  const state = { mask };
  const router = new PointerRouter({
    mask: () => state.mask,
    getGeometry: opts.geom ?? (() => GEOM),
    send: (e) => sent.push(e),
    local,
    setTicking: (on) => ticking.push(on),
  });
  return { router, sent, ticking, local, state };
}

describe("PointerRouter — a press the application tracks", () => {
  it("reports the press to the app and never reaches the local handler", () => {
    const { router, sent, local } = rig(NORMAL);

    const handled = router.down(at(5, 3));

    expect(handled).toBe(true);
    expect(sent).toEqual([{ button: "left", action: "press", col: 5, row: 3, px: 52, py: 65, mods: 0 }]);
    expect(local!.calls).toEqual([]);
  });
});

describe("PointerRouter — a press that stays local", () => {
  it("goes to the local handler, unforced, and starts the tick when the app tracks nothing", () => {
    const { router, sent, local, ticking } = rig(0);

    const handled = router.down(at(5, 3, { detail: 2 }));

    expect(handled).toBe(true);
    expect(sent).toEqual([]);
    expect(local!.calls).toEqual(["down(0,2,plain)"]);
    expect(ticking).toEqual([true]);
  });

  it("Shift forces a tracked press local, and says so, instead of reporting it", () => {
    const { router, sent, local } = rig(ANY);

    router.down(at(5, 3, { shiftKey: true }));

    expect(sent).toEqual([]);
    expect(local!.calls).toEqual(["down(0,1,forced)"]);
  });
});

describe("PointerRouter — the gesture a reported press starts", () => {
  it("reports drag and release under button-event tracking, and nothing after the last button lifts", () => {
    const { router, sent, local } = rig(BUTTON);

    router.down(at(5, 3));
    router.move(at(6, 3, { buttons: 1 }));
    router.up(at(7, 4, { buttons: 0 }));
    router.move(at(8, 4, { buttons: 1 }));

    expect(sent.map((e) => `${e.action}:${e.button}@${e.col},${e.row}`)).toEqual([
      "press:left@5,3",
      "motion:left@6,3",
      "release:left@7,4",
    ]);
    expect(local!.calls).toEqual([]);
  });

  it("reports the release but not the drag when the application tracks presses only (?1000)", () => {
    const { router, sent } = rig(NORMAL);

    router.down(at(5, 3));
    router.move(at(6, 3, { buttons: 1 }));
    router.up(at(6, 3));

    expect(sent.map((e) => e.action)).toEqual(["press", "release"]);
  });

  it("keeps the gesture while another button is still held", () => {
    const { router, sent } = rig(BUTTON);

    router.down(at(5, 3));
    router.down(at(5, 3, { button: 2, buttons: 3 }));
    router.up(at(5, 3, { button: 2, buttons: 1 }));
    router.move(at(6, 3, { buttons: 1 }));

    expect(sent.map((e) => `${e.action}:${e.button}`)).toEqual([
      "press:left",
      "press:right",
      "release:right",
      "motion:left",
    ]);
  });
});

describe("PointerRouter — the gesture a local press starts", () => {
  it("hands motion and the release to the local handler and stops the tick", () => {
    const { router, sent, local, ticking } = rig(0);

    router.down(at(5, 3));
    router.move(at(6, 3, { buttons: 1 }));
    router.up(at(6, 3));
    router.move(at(7, 3));

    expect(local!.calls).toEqual(["down(0,1,plain)", "move", "up"]);
    expect(ticking).toEqual([true, false]);
    expect(sent).toEqual([]);
  });

  it("stays local to its release even when the application starts tracking mid-drag", () => {
    const { router, sent, local, state } = rig(0);

    router.down(at(5, 3));
    state.mask = BUTTON;
    router.move(at(6, 3, { buttons: 1 }));
    router.up(at(6, 3));

    expect(sent).toEqual([]);
    expect(local!.calls).toEqual(["down(0,1,plain)", "move", "up"]);
  });

  it("gives a non-primary press to the local handler without arming a drag", () => {
    const { router, local, ticking } = rig(0);

    router.down(at(5, 3, { button: 2 }));
    router.up(at(5, 3, { button: 2 }));

    expect(local!.calls).toEqual(["down(2,1,plain)"]);
    expect(ticking).toEqual([]);
  });
});

describe("PointerRouter — bare motion", () => {
  it("reports motion with no button held only under any-event tracking (?1003)", () => {
    const any = rig(ANY);
    const button = rig(BUTTON);

    any.router.hover(at(5, 3));
    button.router.hover(at(5, 3));

    expect(any.sent).toEqual([{ button: null, action: "motion", col: 5, row: 3, px: 52, py: 65, mods: 0 }]);
    expect(button.sent).toEqual([]);
  });

  it("does not report a held-button hover — a drag that began elsewhere has no press to follow", () => {
    const { router, sent } = rig(ANY);

    router.hover(at(5, 3, { buttons: 1 }));

    expect(sent).toEqual([]);
  });

  it("leaves hover to the gesture while one is live, so a drag is not reported twice", () => {
    const { router, sent } = rig(ANY);

    router.down(at(5, 3));
    router.hover(at(6, 3));
    router.move(at(6, 3, { buttons: 1 }));

    expect(sent.map((e) => e.action)).toEqual(["press", "motion"]);
  });
});

describe("PointerRouter — presses with nothing to report", () => {
  it("does not report a button the intent cannot name (back/forward), nor arm a gesture", () => {
    const { router, sent, local } = rig(BUTTON);

    const handled = router.down(at(5, 3, { button: 3 }));
    router.up(at(5, 3, { button: 3 }));

    expect(handled).toBe(false);
    expect(sent).toEqual([]);
    expect(local!.calls).toEqual([]);
  });

  it("requests nothing and arms nothing when the box cannot be measured (#819)", () => {
    const { router, sent, local } = rig(BUTTON, { geom: () => undefined });

    const handled = router.down(at(5, 3));
    router.up(at(5, 3));

    expect(handled).toBe(false);
    expect(sent).toEqual([]);
    expect(local!.calls).toEqual([]);
  });

  it("reports only the press under press-only tracking (X10): no drag, no release", () => {
    const { router, sent } = rig(MouseEvents.Down);

    router.down(at(5, 3));
    router.move(at(6, 3, { buttons: 1 }));
    router.up(at(6, 3));

    expect(sent.map((e) => e.action)).toEqual(["press"]);
  });

  it("drops the release when the application stops tracking mid-gesture, and routes the next press afresh", () => {
    const { router, sent, local, state } = rig(BUTTON);

    router.down(at(5, 3));
    state.mask = 0;
    router.up(at(5, 3));
    router.down(at(6, 3));

    expect(sent.map((e) => e.action)).toEqual(["press"]);
    expect(local!.calls).toEqual(["down(0,1,plain)"]);
  });

  it("is inert without a local handler when the application tracks nothing", () => {
    const { router, sent, ticking } = rig(0, { local: false });

    expect(router.down(at(5, 3))).toBe(false);
    expect(sent).toEqual([]);
    expect(ticking).toEqual([]);
  });
});

describe("PointerRouter — the windows where a nearby predicate would disagree", () => {
  it("leaves a Shift press unforced when the application tracks nothing, so it can still extend", () => {
    const { router, local } = rig(0);

    router.down(at(5, 3, { shiftKey: true }));

    expect(local!.calls).toEqual(["down(0,1,plain)"]);
  });

  it("does not report a buttonless move during a gesture as a drag (its release was lost)", () => {
    const { router, sent } = rig(BUTTON);

    router.down(at(5, 3));
    router.move(at(6, 3, { buttons: 0 }));

    expect(sent.map((e) => e.action)).toEqual(["press"]);
  });

  it("ends a reported gesture whose release was lost, so the next press is routed afresh", () => {
    const { router, sent, local } = rig(ANY);

    router.down(at(5, 3));
    router.move(at(6, 3, { buttons: 0 }));
    router.hover(at(7, 3));
    router.down(at(8, 3, { shiftKey: true, buttons: 1 }));

    expect(sent.map((e) => `${e.action}:${e.button}`)).toEqual(["press:left", "motion:null"]);
    expect(local!.calls).toEqual(["down(0,1,forced)"]);
  });

  it("routes a press afresh when no other button is held, even if the last release never arrived", () => {
    const { router, sent, local } = rig(BUTTON);

    router.down(at(5, 3));
    router.down(at(6, 3, { shiftKey: true, buttons: 1 }));

    expect(sent.map((e) => `${e.action}:${e.button}`)).toEqual(["press:left"]);
    expect(local!.calls).toEqual(["down(0,1,forced)"]);

    // The same for a right press, whose `buttons` bit (2) is not `1 << button` (4).
    const right = rig(BUTTON);
    right.router.down(at(5, 3, { button: 2, buttons: 2 }));
    right.router.down(at(6, 3, { button: 2, buttons: 2, shiftKey: true }));

    expect(right.sent.map((e) => `${e.action}:${e.button}`)).toEqual(["press:right"]);
    expect(right.local!.calls).toEqual(["down(2,1,forced)"]);
  });

  it("does not report the release of a button the intent cannot name, inside a live gesture", () => {
    const { router, sent } = rig(BUTTON);

    router.down(at(5, 3));
    router.up(at(5, 3, { button: 3, buttons: 1 }));

    expect(sent.map((e) => e.action)).toEqual(["press"]);
  });

  it("does not report a release whose press found no box, even if the box is back by then", () => {
    let box: CellGeometry | undefined;
    const { router, sent } = rig(BUTTON, { geom: () => box });

    router.down(at(5, 3));
    box = GEOM;
    router.up(at(5, 3));

    expect(sent).toEqual([]);
  });
});
