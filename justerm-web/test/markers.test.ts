import { describe, expect, it } from "vitest";
import { MarkerKind, readMarkers } from "../src/markers";

describe("readMarkers (#159/#160)", () => {
  // DoD ④ real-wire agreement: this is the EXACT flat array the wasm boundary
  // test `marker_position_view_crosses_the_boundary` (justerm-wasm-decode/
  // tests/web.rs) asserts `markerPositions()` produces from a *real* core
  // encode → decode of a PromptStart marker + a CommandFinished(Some(-1)). If the
  // web-side stride/lane order drifts from the real wire, this breaks — closing
  // the gap that hand-built controller fixtures would otherwise leave open.
  it("decodes the real wasm-boundary lane layout, incl. signed exit", () => {
    // (id, row, kind, exitPresent, exitBits) × 2 — exitBits 0xFFFFFFFF = -1.
    const wire = [5, 3, 1, 0, 0, 99, 0, 4, 1, 0xffffffff];

    expect(readMarkers(wire)).toEqual([
      { id: 5, row: 3, kind: MarkerKind.PromptStart, exit: undefined },
      { id: 99, row: 0, kind: MarkerKind.CommandFinished, exit: -1 },
    ]);
  });

  // A missing array (a frame with no markers) is an empty list, not a throw.
  it("returns [] for an absent markerPositions", () => {
    expect(readMarkers(undefined)).toEqual([]);
  });

  // exitPresent 0 → exit is undefined regardless of the (padding) exitBits lane.
  it("treats exitPresent 0 as no exit even with nonzero padding bits", () => {
    const wire = [1, 0, 4, 0, 12345]; // CommandFinished, present=0, junk bits
    expect(readMarkers(wire)[0]!.exit).toBeUndefined();
  });
});
