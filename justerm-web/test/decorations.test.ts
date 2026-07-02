import { describe, expect, it } from "vitest";
import { DecorationRegistry } from "../src/decorations";
import { MarkerKind } from "../src/markers";

/** A frame stripped to what the ruler join reads: all-marker absolute lines
 * (stride-2 `id, line`) plus the scroll geometry for the `line / total` ratio. */
function rulerFrame(markerLines: number[], scrollbackLen: number, rows: number) {
  return { markerLines, scrollbackLen, rows };
}

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

/** A frame with viewport geometry (cols/rows) — needed for right-anchored columns
 * and multi-row (`height`) clipping. */
function frameGeom(cols: number, rows: number, ...records: number[][]) {
  return { cols, rows, markerPositions: records.flat() };
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

  // #202: a multi-row decoration (`height` > 1) projects one single-row rect per
  // row it spans, starting at the marker's row and extending DOWN (xterm `top =
  // marker.line`, `height` rows). The rect shape stays single-row so the S2 render
  // is untouched.
  it("projects one rect per row for a multi-row height", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 2, height: 3, bg: 0x111111 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 4)))).toEqual([
      { row: 4, left: 0, right: 1, layer: "bottom", bg: 0x111111, fg: undefined },
      { row: 5, left: 0, right: 1, layer: "bottom", bg: 0x111111, fg: undefined },
      { row: 6, left: 0, right: 1, layer: "bottom", bg: 0x111111, fg: undefined },
    ]);
  });

  // A multi-row decoration whose span runs past the viewport bottom is clipped to
  // the visible rows (no rects for rows that don't exist).
  it("clips a multi-row height to the viewport bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, height: 5 }); // marker at row 3, only rows 3..4 visible

    expect(reg.decorationsForFrame(frameGeom(20, 5, mk(1, 3)))).toEqual([
      { row: 3, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
      { row: 4, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // #202: a right-anchored decoration counts columns from the RIGHT edge. x=0,
  // width=1 → the rightmost cell (cols-1); the span extends leftward by width.
  it("anchors columns to the right edge", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, anchor: "right", x: 0, width: 1 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 2)))).toEqual([
      { row: 2, left: 19, right: 19, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Right anchor with an x offset and width: x cells in from the right, extending
  // leftward. cols=20, x=1, width=3 → right = 20-1-1 = 18, left = 20-1-3 = 16.
  it("offsets a right-anchored span inward by x", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, anchor: "right", x: 1, width: 3 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([
      { row: 0, left: 16, right: 18, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // A right-anchored span wider than the screen overflows the LEFT edge: columns
  // pass through un-clamped (like the negative-x case), so `left` goes negative.
  // The renderer's `decorationAt` intersects with the visible cells, painting only
  // [0, right] — matching xterm, which clips an on-screen-anchored overflow (and
  // hides only when the anchor x itself is off-screen, where right < 0 here → no
  // cell matches). Locks in the completeness-pass edge.
  it("passes a right-anchored overflow through un-clamped (renderer clips)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, anchor: "right", x: 0, width: 25 }); // wider than 20 cols

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([
      { row: 0, left: -5, right: 19, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Defaults are unchanged (height 1, anchor left) — a plain decoration still
  // projects exactly one left-anchored rect (guards against the additive fields
  // shifting existing behaviour).
  it("defaults to height 1, anchor left (single left rect)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 2, width: 3 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 1)))).toEqual([
      { row: 1, left: 2, right: 4, layer: "bottom", bg: undefined, fg: undefined },
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
});

describe("DecorationRegistry.rulerMarksForFrame (#120 S3)", () => {
  // A ruler mark sits at the marker's buffer-relative position: line / (scrollback
  // + rows). Here total = 90 + 10 = 100, line 25 → topRatio 0.25.
  it("places a ruler mark at the marker's buffer-relative position", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([7, 25], 90, 10))).toEqual([
      { topRatio: 0.25, color: 0xff0000, position: "full" },
    ]);
  });

  // The whole point of the ruler: an OFF-viewport marker (absent from the viewport
  // marker group, present in the all-marker `markerLines`) still gets a mark, so a
  // user sees anchors they'd have to scroll to reach.
  it("shows a mark for a marker off the current viewport", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 3, overviewRulerOptions: { color: 0x00ff00 } });

    // marker at buffer line 4, viewport far below — markerLines still carries it.
    expect(reg.rulerMarksForFrame(rulerFrame([3, 4], 196, 4))).toEqual([
      { topRatio: 0.02, color: 0x00ff00, position: "full" },
    ]);
  });

  // A decoration with no overviewRulerOptions is a cell-only decoration (#198) —
  // it never contributes a ruler mark.
  it("ignores decorations without overviewRulerOptions", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, bg: 0x111111 }); // cell decoration, no ruler

    expect(reg.rulerMarksForFrame(rulerFrame([7, 25], 90, 10))).toEqual([]);
  });

  // A ruler decoration whose marker isn't in `markerLines` (disposed, or a stale
  // id) yields no mark — the join is inner.
  it("ignores a ruler decoration whose marker is absent from markerLines", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 9, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([1, 5], 90, 10))).toEqual([]);
  });

  // The position option rides through (default "full"); an explicit position is
  // honoured so a consumer can put marks in a gutter column.
  it("honours the position option", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 2, overviewRulerOptions: { color: 0x0000ff, position: "right" } });

    expect(reg.rulerMarksForFrame(rulerFrame([2, 50], 90, 10))).toEqual([
      { topRatio: 0.5, color: 0x0000ff, position: "right" },
    ]);
  });

  // No content (total 0) → no marks, no divide-by-zero.
  it("yields no marks when there is no content", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([1, 0], 0, 0))).toEqual([]);
  });

  // The ruler is a scrollback navigator, so it's suppressed on the alt screen
  // (xterm hides its ruler canvas on the alt buffer) — even when markerLines and
  // content are present.
  it("yields no marks on the alt screen", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    expect(
      reg.rulerMarksForFrame({ markerLines: [7, 25], scrollbackLen: 90, rows: 10, altScreen: true }),
    ).toEqual([]);
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

// #189 (S3): the user-visible payoff of the per-buffer marker refactor (#186/#187).
// core now carries alt-scoped markers on the alt frame's `marker_positions` and
// disposes them on alt-leave; the registry is buffer-agnostic (it reads
// `markerPositions` with NO alt gate — unlike `rulerMarksForFrame`, which suppresses
// on alt), so those alt markers' decorations already project on the alt screen's
// visible rows. There is no new registry logic in this slice — these tests LOCK the
// existing correct behavior against a future alt-gate regression, mirroring core's
// `alt_markers.rs` on the web side.
describe("DecorationRegistry — alt-screen decorations (#189)", () => {
  /** An alt frame: alt-scoped marker records plus `altScreen: true`. The registry
   * does NOT read `altScreen` (that's the point — decorations are not suppressed on
   * alt); it's set so the scenario reads as the alt buffer. */
  const altFrame = (...records: number[][]) => ({
    markerPositions: records.flat(),
    altScreen: true,
  });
  /** An alt frame WITH viewport geometry — real alt frames carry `cols`/`rows`, so
   * pin the geometry-dependent paths (multi-row `height` clip, right-anchor columns)
   * on the alt buffer too, not just via the primary `frameGeom` tests. */
  const altFrameGeom = (cols: number, rows: number, ...records: number[][]) => ({
    cols,
    rows,
    markerPositions: records.flat(),
    altScreen: true,
  });

  // Alt-frame markers drive decoration rendering on the alt screen's visible rows —
  // highlighting a line inside a full-screen TUI (vim/htop). The alt screen has no
  // scrollback, so an alt marker is always on-viewport when carried.
  it("projects a decoration whose marker rides an alt frame (no alt suppression)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 42, x: 0, width: 4, bg: 0x008f00 });

    expect(reg.decorationsForFrame(altFrame(mk(42, 3)))).toEqual([
      { row: 3, left: 0, right: 3, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });

  // No cross-buffer bleed: core omits primary markers from the alt frame (and marker
  // ids are engine-global — a single `next_marker_id` counter — so a primary id never
  // collides with an alt id). A decoration on a primary-only marker, absent from the
  // alt frame's `markerPositions`, yields nothing on alt; it reappears on the primary
  // frame that carries it. The join is purely marker-id, so isolation is structural.
  it("does not bleed a primary-only decoration onto the alt screen", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 100, x: 0, width: 4, bg: 0x008f00 }); // primary anchor

    // On alt, only the alt marker (7) is carried; 100 is absent → no rect for it.
    expect(reg.decorationsForFrame(altFrame(mk(7, 1)))).toEqual([]);

    // Back on a primary frame carrying marker 100, the primary decoration projects.
    expect(reg.decorationsForFrame(frame(mk(100, 2)))).toEqual([
      { row: 2, left: 0, right: 3, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });

  // Clear on alt-leave: core disposes the alt-scoped marker on `?1049l` and fires
  // `MarkerDisposed`, which the consumer forwards to `onMarkerDisposed`. The alt
  // decoration then stops projecting (even if its id is later reused), while a
  // primary decoration on a still-live marker is untouched — no cross-buffer teardown.
  it("clears alt decorations on alt-leave (MarkerDisposed) without touching primary", () => {
    const reg = new DecorationRegistry();
    const primary = reg.register({ markerId: 100, bg: 0x001122 });
    const alt = reg.register({ markerId: 7, bg: 0x008f00 });

    reg.onMarkerDisposed(7); // alt-leave disposes the alt-scoped marker

    expect(alt.disposed).toBe(true);
    expect(primary.disposed).toBe(false);
    // The alt id reappearing (id reuse) yields nothing; the primary still projects.
    expect(reg.decorationsForFrame(frame(mk(7, 0), mk(100, 2)))).toEqual([
      { row: 2, left: 0, right: 0, layer: "bottom", bg: 0x001122, fg: undefined },
    ]);
  });

  // Geometry on alt (2-lens completeness, sibling lens): a multi-row `height`
  // decoration clips to the alt viewport bottom using the alt frame's `rows`. The
  // alt screen has no scrollback, so nothing exists below the viewport to spill onto
  // — the demo paints exactly this (a height-3 highlight inside the alt buffer).
  it("clips a multi-row (height) decoration at the alt viewport bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, x: 0, width: 2, height: 4, bg: 0x008f00 });

    // Alt viewport = 5 rows (0..4); the marker sits at row 3, so a height-4 span
    // (rows 3..6) clips to the two on-screen rows 3 and 4.
    expect(reg.decorationsForFrame(altFrameGeom(6, 5, mk(7, 3)))).toEqual([
      { row: 3, left: 0, right: 1, layer: "bottom", bg: 0x008f00, fg: undefined },
      { row: 4, left: 0, right: 1, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });

  // Geometry on alt (2-lens completeness, sibling lens): a right-anchored span is
  // measured from the alt frame's `cols`, exactly as on the primary — the column
  // math has no buffer-specific path, but pin it on alt so a future regression in
  // alt `cols` handling can't slip past.
  it("right-anchors columns against the alt frame's cols", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, x: 0, width: 3, anchor: "right", bg: 0x008f00 });

    // cols=10, right anchor x=0 width=3 → [10-0-3, 10-1-0] = [7, 9].
    expect(reg.decorationsForFrame(altFrameGeom(10, 5, mk(7, 2)))).toEqual([
      { row: 2, left: 7, right: 9, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });
});
