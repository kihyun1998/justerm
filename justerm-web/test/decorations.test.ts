import { describe, expect, it } from "vitest";
import { DecorationRegistry } from "../src/decorations";
import { MarkerKind } from "../src/markers";

/** One stride-5 marker record `(id, row, kind, exitPresent, exitBits)`. Decorations
 * attach to any marker id, so kind/exit are irrelevant here (default Plain). */
function mk(id: number, row: number, kind: MarkerKind = MarkerKind.Plain): number[] {
  return [id, row, kind, 0, 0];
}
/** A frame carrying the given flat marker records (the only field the registry
 * reads — `markerPositions`, projected to on-viewport markers by the wire). */
function frame(...records: number[][]) {
  return { markerPositions: records.flat() };
}

describe("DecorationRegistry (#120 S1)", () => {
  // The core join: a decoration registered against a marker id projects onto that
  // marker's CURRENT viewport row each frame, with its x-range/layer/colour refs.
  // Colours pass through as opaque refs (resolution is the renderer/#115).
  it("projects a registered decoration onto its marker's current row", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, x: 2, width: 3, layer: "top", bg: 0x112233 });

    expect(reg.decorationsForFrame(frame(mk(7, 4)))).toEqual([
      { row: 4, left: 2, right: 4, layer: "top", bg: 0x112233, fg: undefined },
    ]);
  });

  // Defaults mirror xterm's IDecorationOptions: x=0, width=1 (single cell),
  // layer='bottom' (under the text).
  it("defaults x=0, width=1, layer=bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1 });

    expect(reg.decorationsForFrame(frame(mk(1, 0)))).toEqual([
      { row: 0, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Disposing the handle removes the decoration — it stops projecting.
  it("disposing the handle stops projection", () => {
    const reg = new DecorationRegistry();
    const d = reg.register({ markerId: 3 });
    expect(reg.decorationsForFrame(frame(mk(3, 1)))).toHaveLength(1);

    d.dispose();

    expect(d.disposed).toBe(true);
    expect(reg.decorationsForFrame(frame(mk(3, 1)))).toEqual([]);
  });

  // Auto-dispose: a MarkerDisposed event (out-of-band, like #160's) disposes every
  // decoration anchored to that marker — xterm's `marker.onDispose(() =>
  // decoration.dispose())`. Even if the id is later reissued (RIS) and reappears
  // in a frame, the disposed decoration never projects again.
  it("onMarkerDisposed disposes every decoration on that marker", () => {
    const reg = new DecorationRegistry();
    const a = reg.register({ markerId: 5, x: 0 });
    const b = reg.register({ markerId: 5, x: 1, layer: "top" });
    reg.register({ markerId: 6 }); // a different marker, untouched

    reg.onMarkerDisposed(5);

    expect(a.disposed).toBe(true);
    expect(b.disposed).toBe(true);
    // marker 5 reappearing yields nothing; marker 6 still projects.
    expect(reg.decorationsForFrame(frame(mk(5, 2), mk(6, 3)))).toEqual([
      { row: 3, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // The projection is frame-driven: a marker scrolled off the viewport is absent
  // from `markerPositions`, so its decoration yields no rect — and reappears when
  // the marker scrolls back into view (mirrors `overlay.ts` highlight projection).
  it("yields no rect for a marker absent from the frame; it reappears on scroll-back", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 9, x: 1 });

    expect(reg.decorationsForFrame(frame(mk(2, 0)))).toEqual([]); // marker 9 off-viewport

    expect(reg.decorationsForFrame(frame(mk(9, 5)))).toEqual([
      { row: 5, left: 1, right: 1, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Registering against a marker id that never appears in a frame is a harmless
  // no-op: a live handle that simply never projects (justerm has no marker object
  // to guard on `isDisposed`, and id reuse forbids a permanent reject-set).
  it("registering against a marker never seen in a frame is a harmless no-op", () => {
    const reg = new DecorationRegistry();
    const d = reg.register({ markerId: 999 });

    expect(d.disposed).toBe(false);
    expect(reg.decorationsForFrame(frame(mk(1, 0)))).toEqual([]);
  });

  // Multiple decorations per marker (different layers/columns) and across markers
  // each project, following their marker's row — registration order within a
  // marker, frame order across markers.
  it("projects multiple decorations per marker and across markers", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, layer: "bottom" });
    reg.register({ markerId: 1, x: 5, layer: "top" });
    reg.register({ markerId: 2, x: 2 });

    expect(reg.decorationsForFrame(frame(mk(1, 3), mk(2, 8)))).toEqual([
      { row: 3, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
      { row: 3, left: 5, right: 5, layer: "top", bg: undefined, fg: undefined },
      { row: 8, left: 2, right: 2, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Dispose is idempotent — a double dispose neither throws nor corrupts the store.
  it("dispose is idempotent", () => {
    const reg = new DecorationRegistry();
    const d = reg.register({ markerId: 4 });

    d.dispose();
    d.dispose();

    expect(reg.decorationsForFrame(frame(mk(4, 0)))).toEqual([]);
  });

  // Completeness pass (lens 1): the registry is a pass-through positions model — it
  // does NOT validate x/width (mirroring xterm, which only defaults them). A width
  // of 0 yields a degenerate span (right < left) that a renderer's `col >= left &&
  // col <= right` test never matches — harmlessly invisible, not a crash.
  it("passes degenerate width through as an empty (invisible) span", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 3, width: 0 });

    expect(reg.decorationsForFrame(frame(mk(1, 2)))).toEqual([
      { row: 2, left: 3, right: 2, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // A negative x is likewise passed through (no clamping here — viewport clipping
  // is the renderer's job): left..=right can start left of column 0.
  it("passes a negative x through without clamping", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: -2, width: 3 });

    expect(reg.decorationsForFrame(frame(mk(1, 0)))).toEqual([
      { row: 0, left: -2, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Completeness pass (lens 1 + command-announce parity): disposal is purely
  // event-driven with NO permanent reject-set, so a marker id reissued by a full
  // reset (RIS) can be registered against afresh and projects normally. Mirrors
  // `CommandAnnounceController` pruning `seen` on disposal so a reused id re-fires.
  it("allows registering a new decoration on a reused marker id after disposal", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 5, x: 0 });
    reg.onMarkerDisposed(5); // marker 5 disposed; its decoration gone

    const fresh = reg.register({ markerId: 5, x: 4, layer: "top" }); // id reissued

    expect(fresh.disposed).toBe(false);
    expect(reg.decorationsForFrame(frame(mk(5, 7)))).toEqual([
      { row: 7, left: 4, right: 4, layer: "top", bg: undefined, fg: undefined },
    ]);
  });
});
